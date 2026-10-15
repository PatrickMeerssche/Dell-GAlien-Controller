//! NVIDIA PowerMizer, nvidia-powerd and CPU governor state (read as the user; writes of
//! privileged items go through the helper).

use std::ffi::{c_char, c_int, c_uint, c_void};
use std::fs;
use std::process::Command;
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use dell_galien_proto::Governor;
use libloading::Library;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PowerMizer {
    Auto,
    Normal,
    Performance,
}

impl PowerMizer {
    pub const ALL: [PowerMizer; 3] = [PowerMizer::Auto, PowerMizer::Normal, PowerMizer::Performance];

    pub fn label(self) -> &'static str {
        match self {
            PowerMizer::Auto => "Auto",
            PowerMizer::Normal => "Normal",
            PowerMizer::Performance => "Performance",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            PowerMizer::Auto => "Driver decides",
            PowerMizer::Normal => "Adaptive clocks",
            PowerMizer::Performance => "Prefer maximum performance",
        }
    }

    /// Value of the `GpuPowerMizerMode` attribute.
    fn code(self) -> u8 {
        match self {
            PowerMizer::Normal => 0,
            PowerMizer::Performance => 1,
            PowerMizer::Auto => 2,
        }
    }
}

const NV_CTRL_TARGET_TYPE_GPU: c_int = 1;
const NV_CTRL_GPU_POWER_MIZER_MODE: c_uint = 334;

type Reply = mpsc::Sender<Result<(), String>>;

static NVCTRL: OnceLock<Mutex<mpsc::Sender<(u8, Reply)>>> = OnceLock::new();

extern "C" fn ignore_x_error(_: *mut c_void, _: *mut c_void) -> c_int {
    0
}

type OpenDisplay = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type SetErrorHandler = unsafe extern "C" fn(extern "C" fn(*mut c_void, *mut c_void) -> c_int) -> *mut c_void;
type XSyncFn = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type SetAttr = unsafe extern "C" fn(*mut c_void, c_int, c_int, c_uint, c_uint, c_int) -> c_int;

/// The driver reverts PowerMizer when the X client that set it disconnects, so one worker
/// thread keeps its display connection open for the app's lifetime.
fn nvctrl_worker(rx: mpsc::Receiver<(u8, Reply)>) {
    let setup = || -> Result<(*mut c_void, XSyncFn, SetAttr, Library, Library), String> {
        unsafe {
            let x11 = Library::new("libX11.so.6").map_err(|e| format!("libX11 not found: {e}"))?;
            let nv = Library::new("libXNVCtrl.so.0")
                .map_err(|e| format!("libXNVCtrl not found (install libxnvctrl0): {e}"))?;
            let open: OpenDisplay = *x11.get(b"XOpenDisplay\0").map_err(|e| e.to_string())?;
            let handler: SetErrorHandler = *x11.get(b"XSetErrorHandler\0").map_err(|e| e.to_string())?;
            let sync: XSyncFn = *x11.get(b"XSync\0").map_err(|e| e.to_string())?;
            let set: SetAttr = *nv.get(b"XNVCTRLSetTargetAttributeAndGetStatus\0").map_err(|e| e.to_string())?;
            handler(ignore_x_error);
            let dpy = open(std::ptr::null());
            if dpy.is_null() {
                return Err("cannot open the X display (PowerMizer needs an X11 session)".into());
            }
            Ok((dpy, sync, set, x11, nv))
        }
    };
    let state = setup();
    for (value, reply) in rx {
        let result = match &state {
            Err(e) => Err(e.clone()),
            Ok((dpy, sync, set, _, _)) => unsafe {
                let ok = set(*dpy, NV_CTRL_TARGET_TYPE_GPU, 0, 0, NV_CTRL_GPU_POWER_MIZER_MODE, c_int::from(value));
                sync(*dpy, 0);
                if ok != 0 { Ok(()) } else { Err("the driver rejected the PowerMizer change".into()) }
            },
        };
        let _ = reply.send(result);
    }
}

pub fn powermizer_set(mode: PowerMizer) -> Result<(), String> {
    let tx = NVCTRL
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || nvctrl_worker(rx));
            Mutex::new(tx)
        })
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send((mode.code(), reply_tx)).map_err(|_| "NV-CONTROL worker stopped".to_string())?;
    reply_rx.recv_timeout(Duration::from_secs(5)).map_err(|_| "NV-CONTROL did not answer".to_string())?
}

pub fn powerd_active() -> Option<bool> {
    let out = Command::new("systemctl").args(["is-active", "nvidia-powerd.service"]).output().ok()?;
    let state = String::from_utf8_lossy(&out.stdout);
    match state.trim() {
        "active" | "activating" => Some(true),
        "inactive" | "failed" | "deactivating" => Some(false),
        _ => None,
    }
}

/// Name of the matching power-profiles-daemon profile (the Ubuntu power mode).
fn ppd_profile(g: Governor) -> &'static str {
    match g {
        Governor::Powersave => "power-saver",
        Governor::Performance => "performance",
    }
}

pub fn power_profile_get() -> Option<String> {
    let out = Command::new("powerprofilesctl").arg("get").output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn power_profile_set(g: Governor) -> Result<(), String> {
    let out = Command::new("powerprofilesctl")
        .args(["set", ppd_profile(g)])
        .output()
        .map_err(|e| format!("powerprofilesctl not available: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Sets the Ubuntu power profile first so the governor written afterwards is the one that sticks.
pub fn apply_governor(g: Governor) -> Result<(), String> {
    let profile = power_profile_set(g);
    crate::client::set_governor(g)?;
    profile.map_err(|e| format!("governor set, but the Ubuntu power profile failed: {e}"))
}

pub fn governor_get() -> Option<Governor> {
    let g = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor").ok()?;
    match g.trim() {
        "powersave" => Some(Governor::Powersave),
        "performance" => Some(Governor::Performance),
        _ => None,
    }
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub powerd: Option<bool>,
    pub governor: Option<Governor>,
    pub epp: Option<String>,
    pub power_profile: Option<String>,
    pub scaling_driver: Option<String>,
}

pub fn snapshot() -> Snapshot {
    let cpu0 = |f: &str| {
        fs::read_to_string(format!("/sys/devices/system/cpu/cpu0/cpufreq/{f}")).ok().map(|s| s.trim().to_string())
    };
    Snapshot {
        powerd: powerd_active(),
        governor: governor_get(),
        epp: cpu0("energy_performance_preference"),
        power_profile: power_profile_get(),
        scaling_driver: cpu0("scaling_driver"),
    }
}
