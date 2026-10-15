//! Wire protocol between the GUI and the root helper. One text line per request/reply.

pub mod model;

use std::fmt;

use model::Wmi;

pub const SOCKET_DIR: &str = "/run/dell5530";
pub const SOCKET_PATH: &str = "/run/dell5530/helper.sock";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fan {
    Cpu,
    Gpu,
}

impl Fan {
    fn id(self) -> u8 {
        match self {
            Fan::Cpu => 0x32,
            Fan::Gpu => 0x33,
        }
    }
    fn temp_id(self) -> u8 {
        match self {
            Fan::Cpu => 0x01,
            Fan::Gpu => 0x06,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Fan::Cpu => "cpu",
            Fan::Gpu => "gpu",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        match s {
            "cpu" => Some(Fan::Cpu),
            "gpu" => Some(Fan::Gpu),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Governor {
    Powersave,
    Performance,
}

impl Governor {
    pub fn name(self) -> &'static str {
        match self {
            Governor::Powersave => "powersave",
            Governor::Performance => "performance",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        match s {
            "powersave" => Some(Governor::Powersave),
            "performance" => Some(Governor::Performance),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// Replies `ok` when the helper is running.
    Ping,
    GetPowerMode,
    SetPowerMode(u8),
    GetGMode,
    ToggleGMode,
    GetFanBoost(Fan),
    SetFanBoost(Fan, u8),
    /// Replies with `[cpu_temp, gpu_temp, cpu_rpm, gpu_rpm, energy_uj, energy_max_uj]`;
    /// the energy values are 0 when RAPL is unavailable.
    Snapshot,
    /// Sets the cpufreq governor on every CPU.
    SetGovernor(Governor),
    /// Starts or stops `nvidia-powerd.service`.
    SetNvidiaPowerd(bool),
}

/// One WMAX call: `\_SB.<AMWW|AMW3>.WMAX 0 <cmd> {a, b, c, 0x00}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AcpiCall {
    pub cmd: u8,
    pub args: [u8; 3],
}

/// Asks the firmware for the laptop model signature (used once, by the installer).
pub const MODEL_PROBE: AcpiCall = AcpiCall::new(0x1a, 0x02, 0x02, 0x00);

impl AcpiCall {
    const fn new(cmd: u8, a: u8, b: u8, c: u8) -> Self {
        Self { cmd, args: [a, b, c] }
    }

    pub fn to_acpi_string(self, wmi: Wmi) -> String {
        format!(
            "\\_SB.{}.WMAX 0 0x{:02x} {{0x{:02x}, 0x{:02x}, 0x{:02x}, 0x00}}",
            wmi.namespace(),
            self.cmd,
            self.args[0],
            self.args[1],
            self.args[2]
        )
    }
}

impl Request {
    /// ACPI calls needed to answer this request, in order.
    pub fn acpi_calls(self) -> Vec<AcpiCall> {
        match self {
            Request::Ping | Request::SetGovernor(_) | Request::SetNvidiaPowerd(_) => Vec::new(),
            Request::GetPowerMode => vec![AcpiCall::new(0x14, 0x0b, 0x00, 0x00)],
            Request::SetPowerMode(m) => vec![AcpiCall::new(0x15, 0x01, m, 0x00)],
            Request::GetGMode => vec![AcpiCall::new(0x25, 0x02, 0x00, 0x00)],
            Request::ToggleGMode => vec![AcpiCall::new(0x25, 0x01, 0x00, 0x00)],
            Request::GetFanBoost(f) => vec![AcpiCall::new(0x14, 0x0c, f.id(), 0x00)],
            Request::SetFanBoost(f, v) => vec![AcpiCall::new(0x15, 0x02, f.id(), v)],
            Request::Snapshot => vec![
                AcpiCall::new(0x14, 0x04, Fan::Cpu.temp_id(), 0x00),
                AcpiCall::new(0x14, 0x04, Fan::Gpu.temp_id(), 0x00),
                AcpiCall::new(0x14, 0x05, Fan::Cpu.id(), 0x00),
                AcpiCall::new(0x14, 0x05, Fan::Gpu.id(), 0x00),
            ],
        }
    }

    pub fn encode(self) -> String {
        match self {
            Request::Ping => "ping".into(),
            Request::GetPowerMode => "get_power_mode".into(),
            Request::SetPowerMode(m) => format!("set_power_mode {m}"),
            Request::GetGMode => "get_g_mode".into(),
            Request::ToggleGMode => "toggle_g_mode".into(),
            Request::GetFanBoost(f) => format!("get_fan_boost {}", f.name()),
            Request::SetFanBoost(f, v) => format!("set_fan_boost {} {v}", f.name()),
            Request::Snapshot => "snapshot".into(),
            Request::SetGovernor(g) => format!("set_governor {}", g.name()),
            Request::SetNvidiaPowerd(on) => format!("set_nvidia_powerd {}", u8::from(on)),
        }
    }

    pub fn decode(line: &str) -> Option<Self> {
        let mut it = line.split_whitespace();
        let req = match it.next()? {
            "ping" => Request::Ping,
            "get_power_mode" => Request::GetPowerMode,
            "set_power_mode" => Request::SetPowerMode(it.next()?.parse().ok()?),
            "get_g_mode" => Request::GetGMode,
            "toggle_g_mode" => Request::ToggleGMode,
            "get_fan_boost" => Request::GetFanBoost(Fan::parse(it.next()?)?),
            "set_fan_boost" => {
                Request::SetFanBoost(Fan::parse(it.next()?)?, it.next()?.parse().ok()?)
            }
            "snapshot" => Request::Snapshot,
            "set_governor" => Request::SetGovernor(Governor::parse(it.next()?)?),
            "set_nvidia_powerd" => Request::SetNvidiaPowerd(match it.next()? {
                "1" => true,
                "0" => false,
                _ => return None,
            }),
            _ => return None,
        };
        it.next().is_none().then_some(req)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Ok(Vec<u64>),
    Err(String),
}

impl fmt::Display for Reply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reply::Ok(v) => {
                write!(f, "ok")?;
                for x in v {
                    write!(f, " {x}")?;
                }
                Ok(())
            }
            Reply::Err(e) => write!(f, "err {}", e.replace('\n', " ")),
        }
    }
}

impl Reply {
    pub fn decode(line: &str) -> Self {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("ok") {
            let vals: Option<Vec<u64>> = rest.split_whitespace().map(|s| s.parse().ok()).collect();
            match vals {
                Some(v) => Reply::Ok(v),
                None => Reply::Err("malformed reply".into()),
            }
        } else if let Some(rest) = line.strip_prefix("err") {
            Reply::Err(rest.trim().to_string())
        } else {
            Reply::Err("malformed reply".into())
        }
    }
}

/// Parses an `acpi_call` result such as `0x1a\0`.
pub fn parse_acpi_result(raw: &str) -> Result<u32, String> {
    let s = raw.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    match s.strip_prefix("0x") {
        Some(h) => u32::from_str_radix(h, 16).map_err(|_| format!("bad ACPI result: {s}")),
        None => Err(format!("ACPI error: {s}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for r in [
            Request::Ping,
            Request::SetPowerMode(0xa1),
            Request::SetFanBoost(Fan::Gpu, 200),
            Request::GetFanBoost(Fan::Cpu),
            Request::Snapshot,
            Request::SetGovernor(Governor::Powersave),
            Request::SetNvidiaPowerd(false),
        ] {
            assert_eq!(Request::decode(&r.encode()), Some(r));
        }
        assert_eq!(Request::decode("rm -rf /"), None);
    }

    #[test]
    fn acpi_string_matches_python_template() {
        let c = Request::SetFanBoost(Fan::Cpu, 0xff).acpi_calls()[0];
        assert_eq!(c.to_acpi_string(Wmi::Amww), "\\_SB.AMWW.WMAX 0 0x15 {0x02, 0x32, 0xff, 0x00}");
        assert_eq!(MODEL_PROBE.to_acpi_string(Wmi::Amw3), "\\_SB.AMW3.WMAX 0 0x1a {0x02, 0x02, 0x00, 0x00}");
    }

    #[test]
    fn parse_results() {
        assert_eq!(parse_acpi_result("0x1a\0"), Ok(0x1a));
        assert!(parse_acpi_result("Error: AE_NOT_FOUND").is_err());
    }
}
