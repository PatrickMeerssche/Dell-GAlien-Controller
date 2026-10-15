//! NVIDIA PowerMizer, nvidia-powerd and CPU governor state (read as the user; writes of
//! privileged items go through the helper).

use std::fs;
use std::process::Command;

use dell5530_proto::Governor;
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

const ATTR: &str = "[gpu:0]/GPUPowerMizerMode";

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

/// The driver's readback of this attribute stays at 0 on this hardware, so the chosen mode
/// is remembered by the app instead of being queried.
pub fn powermizer_set(mode: PowerMizer) -> Result<(), String> {
    nvidia_settings(&["-a", &format!("{ATTR}={}", mode.code())]).map(|_| ())
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
    pub powerd: Option<bool>,
    pub governor: Option<Governor>,
    pub epp: Option<String>,
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
        scaling_driver: cpu0("scaling_driver"),
    }
}
