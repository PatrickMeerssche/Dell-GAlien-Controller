use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LedMode {
    Static,
    Morph,
    Off,
}

impl LedMode {
    pub const ALL: [LedMode; 3] = [LedMode::Static, LedMode::Morph, LedMode::Off];
    pub fn label(self) -> &'static str {
        match self {
            LedMode::Static => "Static",
            LedMode::Morph => "Morph",
            LedMode::Off => "Off",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Brightness {
    Low,
    Medium,
    High,
}

impl Brightness {
    pub const ALL: [Brightness; 3] = [Brightness::Low, Brightness::Medium, Brightness::High];
    pub fn label(self) -> &'static str {
        match self {
            Brightness::Low => "Low",
            Brightness::Medium => "Medium",
            Brightness::High => "High",
        }
    }
    /// The controller's dim scale is inverse: 0 is brightest.
    pub fn dim(self) -> u8 {
        match self {
            Brightness::Low => 70,
            Brightness::Medium => 35,
            Brightness::High => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PowerMode {
    Balanced,
    Performance,
    Cool,
    Quiet,
    FullSpeed,
    BatterySaver,
    GMode,
    Manual,
}

impl PowerMode {
    pub const ALL: [PowerMode; 8] = [
        PowerMode::Balanced,
        PowerMode::Performance,
        PowerMode::Cool,
        PowerMode::Quiet,
        PowerMode::FullSpeed,
        PowerMode::BatterySaver,
        PowerMode::GMode,
        PowerMode::Manual,
    ];
    pub fn label(self) -> &'static str {
        match self {
            PowerMode::Balanced => "Balanced",
            PowerMode::Performance => "Performance",
            PowerMode::Cool => "Cool",
            PowerMode::Quiet => "Quiet",
            PowerMode::FullSpeed => "Full Speed",
            PowerMode::BatterySaver => "Battery Saver",
            PowerMode::GMode => "G Mode",
            PowerMode::Manual => "Manual",
        }
    }
    pub fn blurb(self) -> &'static str {
        match self {
            PowerMode::Balanced => "Default thermal profile",
            PowerMode::Performance => "Higher sustained clocks",
            PowerMode::Cool => "Lower temperatures",
            PowerMode::Quiet => "Lower fan noise",
            PowerMode::FullSpeed => "Fans at full speed",
            PowerMode::BatterySaver => "Reduced power draw",
            PowerMode::GMode => "Maximum cooling and power",
            PowerMode::Manual => "You control fan boost",
        }
    }
    /// Value sent to the firmware.
    pub fn code(self) -> u8 {
        use dell_galien_proto::model as m;
        match self {
            PowerMode::Balanced => m::BALANCED,
            PowerMode::Performance => m::PERFORMANCE,
            PowerMode::Cool => m::COOL,
            PowerMode::Quiet => m::QUIET,
            PowerMode::FullSpeed => m::FULL_SPEED,
            PowerMode::BatterySaver => m::BATTERY_SAVER,
            PowerMode::GMode => m::G_MODE,
            PowerMode::Manual => m::MANUAL,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Accent {
    Red,
    Green,
    Purple,
    Pink,
    Blue,
    Yellow,
    Orange,
}

impl Accent {
    pub const ALL: [Accent; 7] = [
        Accent::Red,
        Accent::Green,
        Accent::Purple,
        Accent::Pink,
        Accent::Blue,
        Accent::Yellow,
        Accent::Orange,
    ];
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Accent::Red => [0xff, 0x4b, 0x4b],
            Accent::Green => [0x4b, 0xb6, 0x64],
            Accent::Purple => [0x8a, 0x4d, 0xff],
            Accent::Pink => [0xfc, 0x3c, 0x92],
            Accent::Blue => [0x00, 0xaa, 0xff],
            Accent::Yellow => [0xff, 0xd2, 0x4d],
            Accent::Orange => [0xff, 0x66, 0x00],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub led_mode: LedMode,
    /// Last non-Off mode, restored by the tray toggle.
    pub last_led_mode: LedMode,
    pub rgb: [u8; 3],
    pub brightness: Brightness,
    pub duration: u16,
    pub led_on: bool,
    pub power: PowerMode,
    pub fan_cpu: u8,
    pub fan_gpu: u8,
    pub live_fan: bool,
    pub close_to_tray: bool,
    pub accent: Accent,
    pub auto_refresh: bool,
    /// Last PowerMizer mode chosen; reapplied at startup because the driver resets it.
    pub powermizer: Option<crate::tuning::PowerMizer>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            led_mode: LedMode::Static,
            last_led_mode: LedMode::Static,
            rgb: [122, 122, 122],
            brightness: Brightness::High,
            duration: 255,
            led_on: false,
            power: PowerMode::Balanced,
            fan_cpu: 0,
            fan_gpu: 0,
            live_fan: false,
            close_to_tray: false,
            accent: Accent::Blue,
            auto_refresh: true,
            powermizer: None,
        }
    }
}

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("dell-galien-controller").join("settings.toml"))
}

impl Settings {
    pub fn load() -> Self {
        path()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(s) = toml::to_string_pretty(self) {
            let _ = fs::write(p, s);
        }
    }
}
