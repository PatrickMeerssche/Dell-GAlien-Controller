//! Sensor sampling: ACPI temps/RPM via the helper, usage from /proc, power from sysfs and nvidia-smi.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
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
    pub cpu_usage: Option<f32>,
    pub gpu_usage: Option<f32>,
    pub mem_used_gb: f32,
    pub mem_total_gb: f32,
    pub error: Option<String>,
}

impl Sample {
    pub fn mem_percent(&self) -> f32 {
        if self.mem_total_gb > 0.0 {
            self.mem_used_gb / self.mem_total_gb * 100.0
        } else {
            0.0
        }
    }
}

#[derive(Clone, Copy, Default)]
struct SmiValues {
    watts: Option<f32>,
    usage: Option<f32>,
}

/// A long-lived nvidia-smi process; killed on drop so the dGPU can sleep when not sampled.
struct Smi {
    child: Child,
    values: Arc<Mutex<SmiValues>>,
}

impl Smi {
    fn spawn() -> Option<Self> {
        let mut child = Command::new("nvidia-smi")
            .args(["--query-gpu=power.draw,utilization.gpu", "--format=csv,noheader,nounits", "-lms", "1000"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let out = child.stdout.take()?;
        let values = Arc::new(Mutex::new(SmiValues::default()));
        let writer = Arc::clone(&values);
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let mut f = line.split(',').map(|s| s.trim().parse::<f32>().ok());
                let v = SmiValues { watts: f.next().flatten(), usage: f.next().flatten() };
                *writer.lock().unwrap_or_else(|e| e.into_inner()) = v;
            }
        });
        Some(Self { child, values })
    }

    fn get(&self) -> SmiValues {
        *self.values.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drop for Smi {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct Reader {
    last_energy: Option<(u64, Instant)>,
    last_cpu_w: Option<f32>,
    last_cpu_stat: Option<(u64, u64)>,
    direct_energy: Option<PathBuf>,
    gpu_hwmon: Option<PathBuf>,
    smi: Option<Smi>,
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

/// Returns `(busy, total)` jiffies from the aggregate CPU line.
fn read_cpu_stat() -> Option<(u64, u64)> {
    let stat = fs::read_to_string("/proc/stat").ok()?;
    let nums: Vec<u64> = stat.lines().next()?.split_whitespace().skip(1).filter_map(|s| s.parse().ok()).collect();
    if nums.len() < 8 {
        return None;
    }
    let total: u64 = nums[..8].iter().sum();
    let idle = nums[3] + nums[4];
    Some((total - idle, total))
}

fn read_meminfo() -> (f32, f32) {
    let Ok(info) = fs::read_to_string("/proc/meminfo") else { return (0.0, 0.0) };
    let field = |name: &str| -> f32 {
        info.lines()
            .find_map(|l| l.strip_prefix(name)?.trim_start_matches(':').split_whitespace().next()?.parse::<f32>().ok())
            .unwrap_or(0.0)
    };
    let (total, avail) = (field("MemTotal"), field("MemAvailable"));
    const KB_PER_GB: f32 = 1024.0 * 1024.0;
    ((total - avail) / KB_PER_GB, total / KB_PER_GB)
}

impl Reader {
    pub fn new() -> Self {
        let gpu_hwmon = find_hwmon_power(&["nvidia", "amdgpu", "i915", "xe", "intel_gpu"]);
        let direct_energy = ["intel-rapl:0", "intel-rapl:1"]
            .iter()
            .map(|d| Path::new("/sys/class/powercap").join(d).join("energy_uj"))
            .find(|p| read_u64(p).is_some());
        Self {
            last_energy: None,
            last_cpu_w: None,
            last_cpu_stat: None,
            direct_energy,
            gpu_hwmon,
            smi: Smi::spawn(),
        }
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

    fn cpu_usage(&mut self) -> Option<f32> {
        let (busy, total) = read_cpu_stat()?;
        let (pb, pt) = self.last_cpu_stat.replace((busy, total))?;
        let dt = total.checked_sub(pt).filter(|d| *d > 0)?;
        Some(busy.saturating_sub(pb) as f32 / dt as f32 * 100.0)
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
        let smi = self.smi.as_ref().map(Smi::get).unwrap_or_default();
        s.cpu_watts = self.cpu_watts(energy.0, energy.1);
        s.gpu_watts = match &self.gpu_hwmon {
            Some(p) => read_u64(p).map(|v| v as f32 / 1_000_000.0),
            None => smi.watts,
        };
        s.gpu_usage = smi.usage;
        s.cpu_usage = self.cpu_usage();
        (s.mem_used_gb, s.mem_total_gb) = read_meminfo();
        s
    }
}
