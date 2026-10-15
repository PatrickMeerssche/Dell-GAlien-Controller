//! Sensor sampling: ACPI temps/RPM via the helper, CPU/GPU power from sysfs and nvidia-smi.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use dell5530_proto::Request;

use crate::client;

#[derive(Clone, Debug, Default)]
pub struct Sample {
    pub cpu_temp: u32,
    pub gpu_temp: u32,
    pub cpu_rpm: u32,
    pub gpu_rpm: u32,
    pub cpu_watts: Option<f32>,
    pub gpu_watts: Option<f32>,
    pub error: Option<String>,
}

pub struct Reader {
    last_energy: Option<(u64, Instant)>,
    last_cpu_w: Option<f32>,
    direct_energy: Option<PathBuf>,
    gpu_hwmon: Option<PathBuf>,
    gpu_smi: Option<Arc<Mutex<Option<f32>>>>,
}

fn read_u64(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn find_hwmon_power(keys: &[&str]) -> Option<PathBuf> {
    let mut dirs: Vec<_> = fs::read_dir("/sys/class/hwmon").ok()?.flatten().map(|e| e.path()).collect();
    dirs.sort();
    for dir in dirs {
        let name = fs::read_to_string(dir.join("name")).unwrap_or_default().to_lowercase();
        if !keys.iter().any(|k| name.contains(k)) {
            continue;
        }
        for file in ["power1_average", "power1_input"] {
            let p = dir.join(file);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// One long-lived nvidia-smi process streaming power draw, instead of spawning per sample.
fn spawn_nvidia_smi() -> Option<Arc<Mutex<Option<f32>>>> {
    let mut child = Command::new("nvidia-smi")
        .args(["--query-gpu=power.draw", "--format=csv,noheader,nounits", "-lms", "1000"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let out = child.stdout.take()?;
    let shared = Arc::new(Mutex::new(None));
    let writer = Arc::clone(&shared);
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Ok(w) = line.trim().parse::<f32>() {
                *writer.lock().unwrap_or_else(|e| e.into_inner()) = Some(w);
            }
        }
        let _ = child.wait();
    });
    Some(shared)
}

impl Reader {
    pub fn new() -> Self {
        let gpu_hwmon = find_hwmon_power(&["nvidia", "amdgpu", "i915", "xe", "intel_gpu"]);
        let gpu_smi = if gpu_hwmon.is_none() { spawn_nvidia_smi() } else { None };
        let direct_energy = ["intel-rapl:0", "intel-rapl:1"]
            .iter()
            .map(|d| Path::new("/sys/class/powercap").join(d).join("energy_uj"))
            .find(|p| read_u64(p).is_some());
        Self { last_energy: None, last_cpu_w: None, direct_energy, gpu_hwmon, gpu_smi }
    }

    fn cpu_watts(&mut self, energy: u64, max: u64) -> Option<f32> {
        let (energy, max) = match (energy, &self.direct_energy) {
            (0, Some(p)) => (read_u64(p)?, 0),
            (0, None) => return None,
            _ => (energy, max),
        };
        let now = Instant::now();
        let Some((prev, ts)) = self.last_energy.replace((energy, now)) else {
            return self.last_cpu_w;
        };
        let secs = now.duration_since(ts).as_secs_f32();
        let delta = if energy >= prev { energy - prev } else { energy + max.saturating_sub(prev) };
        if delta == 0 || secs <= 0.0 {
            return self.last_cpu_w;
        }
        self.last_cpu_w = Some(delta as f32 / secs / 1_000_000.0);
        self.last_cpu_w
    }

    fn gpu_watts(&self) -> Option<f32> {
        if let Some(p) = &self.gpu_hwmon {
            return read_u64(p).map(|v| v as f32 / 1_000_000.0);
        }
        self.gpu_smi.as_ref().and_then(|s| *s.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn sample(&mut self, helper_ready: bool) -> Sample {
        let mut s = Sample::default();
        let mut energy = (0, 0);
        if helper_ready {
            match client::call(Request::Snapshot) {
                Ok(v) if v.len() >= 6 => {
                    s.cpu_temp = v[0] as u32;
                    s.gpu_temp = v[1] as u32;
                    s.cpu_rpm = v[2] as u32;
                    s.gpu_rpm = v[3] as u32;
                    energy = (v[4], v[5]);
                }
                Ok(_) => s.error = Some("short reply".into()),
                Err(e) => s.error = Some(e),
            }
        }
        s.cpu_watts = self.cpu_watts(energy.0, energy.1);
        s.gpu_watts = self.gpu_watts();
        s
    }
}
