//! NVIDIA PowerMizer, nvidia-powerd and CPU governor state (read as the user; writes of
//! privileged items go through the helper).

use std::fs;
use std::process::Command;

use dell5530_proto::Governor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

    fn from_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(PowerMizer::Normal),
            1 => Some(PowerMizer::Performance),
            2 => Some(PowerMizer::Auto),
            _ => None,
        }
    }
}

const ATTR: &str = "[gpu:0]/GpuPowerMizerMode";

fn nvidia_settings(args: &[&str]) -> Result<String, String> {
    let out = Command::new("nvidia-settings")
        .args(args)
        .output()
        .map_err(|e| format!("nvidia-settings not available: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err.lines().find(|l| !l.trim().is_empty()).unwrap_or("nvidia-settings failed").trim().to_string())
    }
}

pub fn powermizer_get() -> Result<PowerMizer, String> {
    let v = nvidia_settings(&["-q", ATTR, "-t"])?;
    v.parse::<u8>().ok().and_then(PowerMizer::from_code).ok_or_else(|| format!("unexpected value: {v}"))
}

pub fn powermizer_set(mode: PowerMizer) -> Result<PowerMizer, String> {
    nvidia_settings(&["-a", &format!("{ATTR}={}", mode.code())])?;
    powermizer_get()
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
    pub powermizer: Option<Result<PowerMizer, String>>,
    pub powerd: Option<bool>,
    pub governor: Option<Governor>,
    pub scaling_driver: Option<String>,
}

pub fn snapshot() -> Snapshot {
    Snapshot {
        powermizer: Some(powermizer_get()),
        powerd: powerd_active(),
        governor: governor_get(),
        scaling_driver: fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_driver")
            .ok()
            .map(|s| s.trim().to_string()),
    }
}
