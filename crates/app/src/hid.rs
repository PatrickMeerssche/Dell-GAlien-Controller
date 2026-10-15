//! Keyboard RGB controller (187c:0551) over HID control transfers; port of hw/elc.py + hw/awelc.py.

use std::time::Duration;

use rusb::{Context, DeviceHandle, UsbContext};

use crate::settings::{Brightness, LedMode};

const VID: u16 = 0x187c;
const PIDS: [u16; 2] = [0x0551, 0x0550];
const TIMEOUT: Duration = Duration::from_secs(1);
const REPORT_LEN: usize = 33;

const ELC_QUERY: u8 = 0x20;
const USER_ANIMATION: u8 = 0x21;
const POWER_ANIMATION: u8 = 0x22;
const START_SERIES: u8 = 0x23;
const ADD_ACTION: u8 = 0x24;
const DIMMING: u8 = 0x26;
const GET_VERSION: u8 = 0x00;
const GET_ANIMATION_COUNT: u8 = 0x03;
const START_NEW: u16 = 0x01;
const FINISH_SAVE: u16 = 0x02;
const REMOVE: u16 = 0x04;
const SET_DEFAULT: u16 = 0x06;

const EFFECT_COLOR: u8 = 0x00;
const EFFECT_MORPH: u8 = 0x02;

const AC_SLEEP: u16 = 0x5b;
const AC_CHARGED: u16 = 0x5c;
const AC_CHARGING: u16 = 0x5d;
const DC_SLEEP: u16 = 0x5e;
const DC_ON: u16 = 0x5f;
const DC_LOW: u16 = 0x60;

const DURATION_MAX: u16 = 0xffff;
const DURATION_BATTERY_LOW: u16 = 0xff;
const TEMPO_MIN: u16 = 0x01;
const ZONES: [u8; 4] = [0, 1, 2, 3];

pub const NOT_FOUND: &str = "No supported device was found. Expected RGB controller 187c:0550 or 187c:0551";

#[derive(Clone, Copy)]
struct Action {
    effect: u8,
    duration: u16,
    tempo: u16,
    rgb: [u8; 3],
}

impl Action {
    fn bytes(self) -> [u8; 8] {
        let d = self.duration.to_be_bytes();
        let t = self.tempo.to_be_bytes();
        [self.effect, d[0], d[1], t[0], t[1], self.rgb[0], self.rgb[1], self.rgb[2]]
    }
}

struct Elc {
    handle: DeviceHandle<Context>,
}

type R<T> = Result<T, String>;

fn usb_err(e: rusb::Error) -> String {
    format!("USB error: {e}")
}

impl Elc {
    fn open() -> R<Self> {
        let ctx = Context::new().map_err(usb_err)?;
        let handle = PIDS
            .iter()
            .find_map(|&pid| ctx.open_device_with_vid_pid(VID, pid))
            .ok_or_else(|| NOT_FOUND.to_string())?;
        let _ = handle.reset();
        if handle.kernel_driver_active(0).unwrap_or(false) {
            handle.detach_kernel_driver(0).map_err(usb_err)?;
        }
        Ok(Self { handle })
    }

    fn run(&self, fragment: &[u8]) -> R<[u8; REPORT_LEN]> {
        let mut out = [0u8; REPORT_LEN];
        out[0] = 0x03;
        out[1..=fragment.len()].copy_from_slice(fragment);
        self.handle
            .write_control(0x21, 9, 0x200, 0, &out, TIMEOUT)
            .map_err(usb_err)?;
        let mut reply = [0u8; REPORT_LEN];
        self.handle
            .read_control(0xA1, 1, 0x100, 0, &mut reply, TIMEOUT)
            .map_err(usb_err)?;
        Ok(reply)
    }

    fn animation(&self, sub: u16, id: u16) -> R<()> {
        let cmd = if (0x5b..=0x60).contains(&id) { POWER_ANIMATION } else { USER_ANIMATION };
        let (s, i) = (sub.to_be_bytes(), id.to_be_bytes());
        self.run(&[cmd, s[0], s[1], i[0], i[1]]).map(|_| ())
    }

    fn start_series(&self, zones: &[u8]) -> R<()> {
        let mut f = vec![START_SERIES, 1];
        f.extend_from_slice(&(zones.len() as u16).to_be_bytes());
        f.extend_from_slice(zones);
        self.run(&f).map(|_| ())
    }

    fn add_action(&self, actions: &[Action]) -> R<()> {
        let mut f = vec![ADD_ACTION];
        for a in actions {
            f.extend_from_slice(&a.bytes());
        }
        self.run(&f).map(|_| ())
    }

    fn dim(&self, zones: &[u8], level: u8) -> R<()> {
        let mut f = vec![DIMMING, level];
        f.extend_from_slice(&(zones.len() as u16).to_be_bytes());
        f.extend_from_slice(zones);
        self.run(&f).map(|_| ())
    }

    fn version(&self) -> R<(u8, u8, u8)> {
        let r = self.run(&[ELC_QUERY, GET_VERSION])?;
        Ok((r[3], r[4], r[5]))
    }

    fn animation_count(&self) -> R<(u16, u16)> {
        let r = self.run(&[ELC_QUERY, GET_ANIMATION_COUNT])?;
        Ok((u16::from_be_bytes([r[3], r[4]]), u16::from_be_bytes([r[5], r[6]])))
    }

    fn save_animation(&self, id: u16, zones: &[u8], actions: &[Action]) -> R<()> {
        self.animation(REMOVE, id)?;
        self.animation(START_NEW, id)?;
        self.start_series(zones)?;
        self.add_action(actions)?;
        self.animation(FINISH_SAVE, id)?;
        self.animation(SET_DEFAULT, id)
    }

    fn apply_effect(&self, id: u16, rgb: [u8; 3], duration: u16, morph: bool) -> R<()> {
        let [r, g, b] = rgb;
        let mk = |effect, c: [u8; 3]| Action { effect, duration, tempo: TEMPO_MIN, rgb: c };
        if morph {
            self.save_animation(
                id,
                &ZONES,
                &[mk(EFFECT_MORPH, [r, g, b]), mk(EFFECT_MORPH, [g, b, r]), mk(EFFECT_MORPH, [b, r, g])],
            )
        } else {
            self.save_animation(id, &ZONES, &[mk(EFFECT_COLOR, [r, g, b])])
        }
    }

    fn battery_flashing(&self) -> R<()> {
        let mk = |c| Action {
            effect: EFFECT_COLOR,
            duration: DURATION_BATTERY_LOW,
            tempo: TEMPO_MIN,
            rgb: c,
        };
        self.save_animation(DC_LOW, &ZONES, &[mk([255, 0, 0]), mk([0, 0, 0])])
    }

    fn program(&self, rgb: [u8; 3], duration: u16, morph: bool) -> R<()> {
        let half = rgb.map(|c| c / 2);
        self.apply_effect(AC_SLEEP, [0; 3], DURATION_MAX, false)?;
        self.apply_effect(AC_CHARGED, rgb, duration, morph)?;
        self.apply_effect(AC_CHARGING, rgb, duration, morph)?;
        self.apply_effect(DC_SLEEP, [0; 3], DURATION_MAX, false)?;
        self.apply_effect(DC_ON, half, duration, morph)?;
        self.battery_flashing()
    }
}

impl Drop for Elc {
    fn drop(&mut self) {
        let _ = self.handle.reset();
    }
}

pub fn version() -> R<String> {
    let (a, b, c) = Elc::open()?.version()?;
    Ok(format!("{a}.{b}.{c}"))
}

pub fn set_dim(level: u8) -> R<()> {
    Elc::open()?.dim(&ZONES, level)
}

pub fn set_static(rgb: [u8; 3]) -> R<()> {
    Elc::open()?.program(rgb, DURATION_MAX, false)
}

pub fn set_morph(rgb: [u8; 3], duration: u16) -> R<()> {
    Elc::open()?.program(rgb, duration, true)
}

pub fn remove_animations() -> R<()> {
    set_dim(100)?;
    let elc = Elc::open()?;
    for id in [AC_SLEEP, AC_CHARGED, AC_CHARGING, DC_SLEEP, DC_ON, DC_LOW] {
        elc.animation(REMOVE, id)?;
    }
    for _ in 0..64 {
        let count = elc.animation_count()?;
        if count == (0, 0) {
            break;
        }
        elc.animation(REMOVE, count.1)?;
    }
    Ok(())
}

/// Programs the effect first and dims afterwards so brightness persists.
pub fn apply(mode: LedMode, rgb: [u8; 3], duration: u16, brightness: Brightness) -> R<()> {
    match mode {
        LedMode::Static => {
            set_static(rgb)?;
            set_dim(brightness.dim())
        }
        LedMode::Morph => {
            set_morph([255, 0, 0], duration)?;
            set_dim(brightness.dim())
        }
        LedMode::Off => remove_animations(),
    }
}
