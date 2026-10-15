//! Supported laptop models. The model is detected once by the installer and stored in
//! `/etc/dell5530/model.conf`; nothing else probes the hardware at run time.

use std::fs;

pub const CONFIG_DIR: &str = "/etc/dell5530";
pub const CONFIG_PATH: &str = "/etc/dell5530/model.conf";

/// Firmware power-mode codes (the values sent with `set_power_mode`).
pub const BALANCED: u8 = 0xa0;
pub const PERFORMANCE: u8 = 0xa1;
pub const COOL: u8 = 0xa2;
pub const QUIET: u8 = 0xa3;
pub const FULL_SPEED: u8 = 0xa4;
pub const BATTERY_SAVER: u8 = 0xa5;
pub const G_MODE: u8 = 0xab;
pub const MANUAL: u8 = 0x00;

/// ACPI object that implements the WMAX method: Intel models use `AMWW`, AMD models `AMW3`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wmi {
    Amww,
    Amw3,
}

impl Wmi {
    pub const ALL: [Wmi; 2] = [Wmi::Amww, Wmi::Amw3];

    pub fn namespace(self) -> &'static str {
        match self {
            Wmi::Amww => "AMWW",
            Wmi::Amw3 => "AMW3",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Wmi::Amww => "amww",
            Wmi::Amw3 => "amw3",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Wmi::ALL.into_iter().find(|w| w.id() == s)
    }
}

pub struct Model {
    pub id: &'static str,
    pub name: &'static str,
    /// Lower-case fragments of the DMI product name that identify this model.
    pub dmi: &'static [&'static str],
    /// Namespace used when the firmware probe finds none.
    pub wmi: Wmi,
    /// Firmware signatures (namespace, `get_laptop_model` result) that identify this model.
    pub signatures: &'static [(Wmi, u32)],
    /// Power modes the firmware accepts.
    pub modes: &'static [u8],
    pub power: bool,
    pub keyboard: bool,
}

const MODES_STANDARD: &[u8] = &[BALANCED, PERFORMANCE, QUIET, BATTERY_SAVER, G_MODE, MANUAL];
const MODES_FULL: &[u8] = &[BALANCED, PERFORMANCE, QUIET, FULL_SPEED, BATTERY_SAVER, G_MODE, MANUAL];

pub static MODELS: [Model; 10] = [
    Model {
        id: "g15_5530",
        name: "Dell G15 5530",
        dmi: &["g15 5530"],
        wmi: Wmi::Amww,
        signatures: &[(Wmi::Amww, 0x0)],
        modes: MODES_STANDARD,
        power: true,
        keyboard: true,
    },
    Model {
        id: "g15_5525",
        name: "Dell G15 5525",
        dmi: &["g15 5525"],
        wmi: Wmi::Amw3,
        signatures: &[(Wmi::Amw3, 0x12c0)],
        modes: MODES_FULL,
        power: true,
        keyboard: true,
    },
    Model {
        id: "g15_5520",
        name: "Dell G15 5520",
        dmi: &["g15 5520"],
        wmi: Wmi::Amww,
        signatures: &[(Wmi::Amww, 0x12c0)],
        modes: MODES_STANDARD,
        power: true,
        keyboard: true,
    },
    Model {
        id: "g15_5515",
        name: "Dell G15 5515",
        dmi: &["g15 5515"],
        wmi: Wmi::Amw3,
        signatures: &[(Wmi::Amw3, 0xc80)],
        modes: &[G_MODE, MANUAL],
        power: true,
        keyboard: true,
    },
    Model {
        id: "g15_5511",
        name: "Dell G15 5511",
        dmi: &["g15 5511"],
        wmi: Wmi::Amww,
        signatures: &[(Wmi::Amww, 0xc80)],
        modes: &[BALANCED, PERFORMANCE, COOL, QUIET, G_MODE, MANUAL],
        power: true,
        keyboard: true,
    },
    Model {
        id: "g16_7620",
        name: "Dell G16 7620",
        dmi: &["g16 7620"],
        wmi: Wmi::Amww,
        signatures: &[],
        modes: MODES_FULL,
        power: true,
        keyboard: true,
    },
    Model {
        id: "g16_7630",
        name: "Dell G16 7630",
        dmi: &["g16 7630"],
        wmi: Wmi::Amww,
        signatures: &[],
        modes: MODES_STANDARD,
        power: true,
        keyboard: true,
    },
    Model {
        id: "alienware_m16_r1",
        name: "Alienware M16 R1",
        dmi: &["m16 r1"],
        wmi: Wmi::Amww,
        signatures: &[],
        modes: MODES_FULL,
        power: true,
        keyboard: true,
    },
    Model {
        id: "generic",
        name: "Unrecognized Dell G-series / Alienware",
        dmi: &[],
        wmi: Wmi::Amw3,
        signatures: &[],
        modes: MODES_FULL,
        power: true,
        keyboard: true,
    },
    Model {
        id: "none",
        name: "Unrecognized laptop (keyboard lighting only)",
        dmi: &[],
        wmi: Wmi::Amww,
        signatures: &[],
        modes: &[],
        power: false,
        keyboard: true,
    },
];

pub fn find(id: &str) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.id == id)
}

/// Matches the DMI product name (e.g. `Dell G15 5530`).
pub fn by_dmi(product: &str) -> Option<&'static Model> {
    let product = product.to_lowercase();
    MODELS.iter().find(|m| m.dmi.iter().any(|d| product.contains(d)))
}

/// Matches the `get_laptop_model` firmware signature; used when the DMI name is unknown.
pub fn by_signature(wmi: Wmi, signature: u32) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.signatures.contains(&(wmi, signature)))
}

/// The result of the install-time detection.
#[derive(Clone, Copy)]
pub struct Installed {
    pub model: &'static Model,
    pub wmi: Wmi,
}

impl Installed {
    pub fn to_config(self) -> String {
        format!("model={}\nwmi={}\n", self.model.id, self.wmi.id())
    }

    pub fn parse(text: &str) -> Option<Self> {
        let mut model = None;
        let mut wmi = None;
        for line in text.lines() {
            match line.split_once('=') {
                Some(("model", v)) => model = find(v.trim()),
                Some(("wmi", v)) => wmi = Wmi::parse(v.trim()),
                _ => {}
            }
        }
        let model = model?;
        Some(Self { model, wmi: wmi.unwrap_or(model.wmi) })
    }
}

/// Reads the model chosen at install time, or `None` when the installer has not run.
pub fn load() -> Option<Installed> {
    Installed::parse(&fs::read_to_string(CONFIG_PATH).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_dmi_name() {
        assert_eq!(by_dmi("Dell G15 5530").map(|m| m.id), Some("g15_5530"));
        assert_eq!(by_dmi("Dell G16 7630").map(|m| m.id), Some("g16_7630"));
        assert_eq!(by_dmi("Alienware m16 R1").map(|m| m.id), Some("alienware_m16_r1"));
        assert!(by_dmi("Some Other Laptop").is_none());
    }

    #[test]
    fn detects_by_signature() {
        assert_eq!(by_signature(Wmi::Amww, 0x12c0).map(|m| m.id), Some("g15_5520"));
        assert_eq!(by_signature(Wmi::Amw3, 0x12c0).map(|m| m.id), Some("g15_5525"));
        assert_eq!(by_signature(Wmi::Amw3, 0xc80).map(|m| m.id), Some("g15_5515"));
        assert!(by_signature(Wmi::Amww, 0x1234).is_none());
    }

    #[test]
    fn config_roundtrip() {
        let inst = Installed { model: find("g15_5525").unwrap(), wmi: Wmi::Amw3 };
        let back = Installed::parse(&inst.to_config()).unwrap();
        assert_eq!(back.model.id, "g15_5525");
        assert_eq!(back.wmi, Wmi::Amw3);
        assert!(Installed::parse("model=nonsense").is_none());
    }

    #[test]
    fn ids_are_unique_and_modes_consistent() {
        for (i, m) in MODELS.iter().enumerate() {
            assert!(MODELS[i + 1..].iter().all(|o| o.id != m.id));
            assert_eq!(m.power, !m.modes.is_empty());
        }
    }
}
