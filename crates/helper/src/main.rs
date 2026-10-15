//! Root helper: executes a fixed whitelist of ACPI calls for the unprivileged GUI.
//! Usage: dell5530-helper --gid <gid>   (socket is chowned root:<gid>, mode 0660)

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{chown, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;

use dell5530_proto::{parse_acpi_result, Reply, Request, SOCKET_DIR, SOCKET_PATH};

const ACPI_CALL: &str = "/proc/acpi/call";

static ACPI_LOCK: Mutex<()> = Mutex::new(());

fn acpi(call: &str) -> Result<u32, String> {
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

fn handle_request(req: Request) -> Reply {
    let mut out = Vec::new();
    for call in req.acpi_calls() {
        match acpi(&call.to_acpi_string()) {
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

fn parse_gid() -> Result<u32, String> {
    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next()) {
        (Some("--gid"), Some(g)) => g.parse().map_err(|_| "invalid gid".to_string()),
        _ => Err("usage: dell5530-helper --gid <gid>".into()),
    }
}

fn main() {
    let gid = match parse_gid() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

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
