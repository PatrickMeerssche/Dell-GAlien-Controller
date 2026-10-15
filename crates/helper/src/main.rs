//! Root helper: executes a fixed whitelist of ACPI calls for the unprivileged GUI.
//! Usage: dell5530-helper --gid <gid>   (socket is chowned root:<gid>, mode 0660)
//!        dell5530-helper --detect [--model <id>] [--assume-generic]   (install-time)
//!        dell5530-helper --list-models

mod detect;

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{chown, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::thread;

use dell5530_proto::model::{self, Installed};
use dell5530_proto::{parse_acpi_result, Reply, Request, SOCKET_DIR, SOCKET_PATH};

const ACPI_CALL: &str = "/proc/acpi/call";

static ACPI_LOCK: Mutex<()> = Mutex::new(());
/// Model chosen by the installer; read once at start-up.
static MODEL: OnceLock<Option<Installed>> = OnceLock::new();

pub(crate) fn acpi(call: &str) -> Result<u32, String> {
    let _guard = ACPI_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = File::options()
        .write(true)
        .open(ACPI_CALL)
        .map_err(|e| format!("cannot open {ACPI_CALL}: {e} (is acpi-call loaded?)"))?;
    f.write_all(call.as_bytes()).map_err(|e| format!("acpi write failed: {e}"))?;
    drop(f);
    let raw = fs::read(ACPI_CALL).map_err(|e| format!("acpi read failed: {e}"))?;
    parse_acpi_result(&String::from_utf8_lossy(&raw))
}

fn read_u64(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Package-level RAPL domain, preferring `package-*` names. Returns the energy_uj path.
fn find_rapl() -> Option<PathBuf> {
    let mut best: Option<(u8, PathBuf)> = None;
    for entry in fs::read_dir("/sys/class/powercap").ok()?.flatten() {
        let dir = entry.path();
        let energy = dir.join("energy_uj");
        if !energy.exists() {
            continue;
        }
        let name = fs::read_to_string(dir.join("name")).unwrap_or_default();
        let score = if name.trim().starts_with("package") { 10 } else { 0 };
        if best.as_ref().map_or(true, |(s, _)| score > *s) {
            best = Some((score, energy));
        }
    }
    best.map(|(_, p)| p)
}

fn rapl_energy() -> (u64, u64) {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    let Some(path) = PATH.get_or_init(find_rapl) else { return (0, 0) };
    let energy = read_u64(path).unwrap_or(0);
    let max = path.parent().and_then(|d| read_u64(&d.join("max_energy_range_uj"))).unwrap_or(0);
    (energy, max)
}

fn set_governor(name: &str) -> Reply {
    let Ok(dir) = fs::read_dir("/sys/devices/system/cpu") else {
        return Reply::Err("cpufreq not available".into());
    };
    let mut count = 0;
    for entry in dir.flatten() {
        let file = entry.file_name();
        let file = file.to_string_lossy();
        let is_cpu = file.strip_prefix("cpu").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if !is_cpu {
            continue;
        }
        let path = entry.path().join("cpufreq/scaling_governor");
        if path.exists() {
            if let Err(e) = fs::write(&path, name) {
                return Reply::Err(format!("{}: {e}", path.display()));
            }
            count += 1;
        }
    }
    if count == 0 {
        Reply::Err("cpufreq not available".into())
    } else {
        Reply::Ok(vec![count])
    }
}

fn set_nvidia_powerd(on: bool) -> Reply {
    let action = if on { "start" } else { "stop" };
    match Command::new("/usr/bin/systemctl").args([action, "nvidia-powerd.service"]).output() {
        Ok(o) if o.status.success() => Reply::Ok(vec![u64::from(on)]),
        Ok(o) => Reply::Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Reply::Err(format!("systemctl failed: {e}")),
    }
}

fn handle_request(req: Request) -> Reply {
    match req {
        Request::Ping => return Reply::Ok(Vec::new()),
        Request::SetGovernor(g) => return set_governor(g.name()),
        Request::SetNvidiaPowerd(on) => return set_nvidia_powerd(on),
        _ => {}
    }
    let Some(installed) = MODEL.get().copied().flatten() else {
        return Reply::Err("no laptop model configured; run sudo ./install.sh".into());
    };
    if !installed.model.power {
        return Reply::Err(format!("{} has no supported power controls", installed.model.name));
    }
    if let Request::SetPowerMode(m) = req {
        if !installed.model.modes.contains(&m) {
            return Reply::Err(format!("power mode {m:#04x} is not supported by {}", installed.model.name));
        }
    }
    let mut out = Vec::new();
    for call in req.acpi_calls() {
        match acpi(&call.to_acpi_string(installed.wmi)) {
            Ok(v) => out.push(u64::from(v)),
            Err(e) => return Reply::Err(e),
        }
    }
    if req == Request::Snapshot {
        let (energy, max) = rapl_energy();
        out.push(energy);
        out.push(max);
    }
    Reply::Ok(out)
}

fn serve(stream: UnixStream) {
    let mut writer = match stream.try_clone() {
        Ok(w) => w,
        Err(_) => return,
    };
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        let reply = match Request::decode(&line) {
            Some(req) => handle_request(req),
            None => Reply::Err("unknown request".into()),
        };
        if writeln!(writer, "{reply}").is_err() {
            return;
        }
    }
}

fn parse_gid(args: &[String]) -> Result<u32, String> {
    match (args.first().map(String::as_str), args.get(1)) {
        (Some("--gid"), Some(g)) => g.parse().map_err(|_| "invalid gid".to_string()),
        _ => Err("usage: dell5530-helper --gid <gid> | --detect [--model <id>] [--assume-generic] | --list-models".into()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--list-models") => {
            detect::list_models();
            return;
        }
        Some("--detect") => {
            let forced = args.iter().position(|a| a == "--model").and_then(|i| args.get(i + 1)).cloned();
            let assume_generic = args.iter().any(|a| a == "--assume-generic");
            std::process::exit(detect::run(detect::Options { forced, assume_generic }));
        }
        _ => {}
    }

    let gid = match parse_gid(&args) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    let installed = model::load();
    match installed {
        Some(i) => eprintln!("model: {} via {}", i.model.name, i.wmi.namespace()),
        None => eprintln!("no model configured; run sudo ./install.sh"),
    }
    let _ = MODEL.set(installed);

    fs::create_dir_all(SOCKET_DIR).expect("create socket dir");
    let _ = fs::remove_file(SOCKET_PATH);
    let listener = UnixListener::bind(SOCKET_PATH).expect("bind socket");
    chown(SOCKET_DIR, Some(0), Some(gid)).expect("chown dir");
    fs::set_permissions(SOCKET_DIR, fs::Permissions::from_mode(0o750)).expect("chmod dir");
    chown(SOCKET_PATH, Some(0), Some(gid)).expect("chown socket");
    fs::set_permissions(SOCKET_PATH, fs::Permissions::from_mode(0o660)).expect("chmod socket");

    eprintln!("dell5530-helper listening on {SOCKET_PATH}");
    for stream in listener.incoming().flatten() {
        thread::spawn(move || serve(stream));
    }
}
