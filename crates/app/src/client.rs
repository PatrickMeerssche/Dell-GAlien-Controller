//! Blocking client for the root helper socket, plus the multi-step power logic.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use dell5530_proto::{Fan, Governor, Reply, Request, SOCKET_PATH};

use crate::settings::PowerMode;

pub fn call(req: Request) -> Result<Vec<u64>, String> {
    let mut stream = UnixStream::connect(SOCKET_PATH).map_err(|e| format!("helper not reachable: {e}"))?;
    let t = Some(Duration::from_secs(3));
    let _ = stream.set_read_timeout(t);
    let _ = stream.set_write_timeout(t);
    writeln!(stream, "{}", req.encode()).map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).map_err(|e| e.to_string())?;
    match Reply::decode(&line) {
        Reply::Ok(v) => Ok(v),
        Reply::Err(e) => Err(e),
    }
}

fn first(req: Request) -> Result<u64, String> {
    call(req)?.first().copied().ok_or_else(|| "empty reply".to_string())
}

#[derive(Clone, Debug)]
pub struct Probe {
    pub supported: bool,
    pub model: String,
}

pub fn probe() -> Result<Probe, String> {
    let sig = first(Request::Model)?;
    // Signature 0 identifies the Dell G15 5530.
    if sig == 0 {
        Ok(Probe { supported: true, model: "Dell G15 5530".into() })
    } else {
        Ok(Probe { supported: false, model: format!("Unsupported signature {sig:#x}") })
    }
}

pub fn set_power_mode(mode: PowerMode) -> Result<String, String> {
    let code = u64::from(mode.code());
    call(Request::SetPowerMode(mode.code()))?;
    let mut msg = match first(Request::GetPowerMode)? {
        v if v == code => format!("Power mode set to {}.", mode.label()),
        v => format!("Error: firmware reports {v:#x}, expected {code:#x}."),
    };
    let g_on = first(Request::GetGMode)? == 1;
    if (mode == PowerMode::GMode) != g_on {
        let toggled = first(Request::ToggleGMode)?;
        let want = u64::from(mode == PowerMode::GMode);
        if toggled != want {
            msg.push_str(&format!(" G Mode expected {want} but read {toggled}."));
        }
    }
    Ok(msg)
}

pub fn set_governor(g: Governor) -> Result<(), String> {
    call(Request::SetGovernor(g)).map(|_| ())
}

pub fn set_nvidia_powerd(on: bool) -> Result<(), String> {
    call(Request::SetNvidiaPowerd(on)).map(|_| ())
}

pub fn set_fan_boost(fan: Fan, value: u8) -> Result<String, String> {
    let pct = |v: u64| v as f32 / 255.0 * 100.0;
    let before = first(Request::GetFanBoost(fan))?;
    call(Request::SetFanBoost(fan, value))?;
    let after = first(Request::GetFanBoost(fan))?;
    let name = if fan == Fan::Cpu { "CPU" } else { "GPU" };
    Ok(format!("{name} fan boost: {:.0}% to {:.0}%", pct(before), pct(after)))
}
