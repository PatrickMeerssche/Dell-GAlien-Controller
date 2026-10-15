//! One-time model detection, run by `install.sh`. Writes `/etc/dell5530/model.conf`.

use std::fs;
use std::io::{self, IsTerminal, Write};

use dell5530_proto::model::{self, Installed, Model, Wmi, CONFIG_DIR, CONFIG_PATH};
use dell5530_proto::MODEL_PROBE;

pub struct Options {
    pub forced: Option<String>,
    pub assume_generic: bool,
}

/// First WMAX namespace that answers the model probe, with the firmware signature.
fn probe() -> Option<(Wmi, u32)> {
    Wmi::ALL
        .into_iter()
        .find_map(|wmi| crate::acpi(&MODEL_PROBE.to_acpi_string(wmi)).ok().map(|sig| (wmi, sig)))
}

fn ask(question: &str) -> bool {
    if !io::stdin().is_terminal() {
        return false;
    }
    print!("{question} [y/N] ");
    let _ = io::stdout().flush();
    let mut line = String::new();
    io::stdin().read_line(&mut line).is_ok() && matches!(line.trim(), "y" | "Y" | "yes")
}

pub fn list_models() {
    for m in model::MODELS.iter() {
        println!("{:<18} {}", m.id, m.name);
    }
}

pub fn run(opts: Options) -> i32 {
    let product = fs::read_to_string("/sys/class/dmi/id/product_name").unwrap_or_default();
    let product = product.trim();
    let probed = probe();

    println!("Laptop:   {}", if product.is_empty() { "unknown" } else { product });
    match probed {
        Some((w, sig)) => println!("Firmware: {} answered {sig:#x}", w.namespace()),
        None => println!("Firmware: no WMAX answer (is the acpi_call module loaded?)"),
    }

    let chosen: &Model = if let Some(id) = &opts.forced {
        match model::find(id) {
            Some(m) => m,
            None => {
                eprintln!("Unknown model '{id}'. Use --list-models to see the valid ids.");
                return 2;
            }
        }
    } else if let Some(m) = model::by_dmi(product) {
        m
    } else if let Some(m) = probed.and_then(|(w, sig)| model::by_signature(w, sig)) {
        m
    } else if probed.is_some()
        && (opts.assume_generic
            || ask("This laptop is not on the supported list. Try the generic Dell G-series power controls anyway? They might misbehave on other models."))
    {
        model::find("generic").expect("generic model")
    } else {
        model::find("none").expect("none model")
    };

    let wmi = probed.map_or(chosen.wmi, |(w, _)| w);
    if let Err(e) = fs::create_dir_all(CONFIG_DIR).and_then(|_| fs::write(CONFIG_PATH, Installed { model: chosen, wmi }.to_config())) {
        eprintln!("Cannot write {CONFIG_PATH}: {e}");
        return 1;
    }

    println!("Model:    {} ({})", chosen.name, chosen.id);
    if chosen.power {
        println!("Power:    enabled via {}", wmi.namespace());
    } else {
        println!("Power:    disabled (only keyboard lighting will work)");
    }
    0
}
