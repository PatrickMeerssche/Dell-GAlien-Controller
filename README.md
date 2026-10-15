# Dell G15 5530 Controller

A **Dell G15 5530**-focused **Linux** controller app, written in Rust with [iced](https://iced.rs):

- **Keyboard lighting**: static colour, morph animation, off, and brightness control
- **Power profiles**: balanced, performance, quiet, battery saver, G Mode and manual
- **Manual fan boost**: per-fan boost for the CPU and GPU fans
- **Live sensors**: CPU/GPU temperature, fan RPM and power with sparklines
- **System tray**: click the tray icon to toggle the keyboard LEDs
- **Diagnostics**: helper state, detected model, LED firmware and last results

This fork intentionally targets the **Dell G15 5530** only.

## What Is Different From Upstream

- rewritten in Rust (the Python/Qt version lives on the `main` branch)
- device support narrowed to the 5530 RGB controller (`187c:0551`)
- the GUI runs as a normal user; only a small systemd helper runs as root, and it can only issue a fixed set of ACPI calls
- sensors are polled off the UI thread

## Credits

Original project by cemkaya-mpi:
<https://github.com/cemkaya-mpi/Dell-G-Series-Controller>

## Requirements

Ubuntu/Debian package names; adjust for your distro.

- Rust toolchain (stable): <https://rustup.rs>
- `build-essential`, `pkg-config` and `libusb-1.0-0-dev` (build time)
- `acpi-call-dkms` (kernel module, loaded automatically by the installer)
- `nvidia-smi` (optional, GPU power readings)
- A StatusNotifier tray host for the tray icon (GNOME: the AppIndicator extension, preinstalled on Ubuntu)

```bash
sudo apt install build-essential pkg-config libusb-1.0-0-dev acpi-call-dkms
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## Installation

```bash
git clone https://github.com/PatrickMeerssche/Dell-5530-Controller.git
cd Dell-5530-Controller
git checkout v2-rust
cargo build --release
sudo ./install.sh
```

`install.sh` is the only step that needs sudo. It installs:

- `/usr/local/bin/dell5530-controller` (GUI) and `/usr/local/bin/dell5530-helper` (root helper)
- `dell5530-helper.service`, which loads `acpi_call` and exposes `/run/dell5530/helper.sock`, usable only by your user's group
- the udev rule that gives your session access to the RGB USB controller
- the application menu entry

Launch **Dell G15 5530 Controller** from the application menu, or run `dell5530-controller`.
To remove everything: `sudo ./uninstall.sh`.

Settings are stored in `~/.config/dell5530-controller/settings.toml`.

## Troubleshooting

### `No supported device was found. Expected RGB controller 187c:0551`

```bash
lsusb | grep -i 187c
```

If nothing appears, make sure this is a supported 5530 variant, replug power or reboot, and check that the udev rule `00-aw-elc.rules` is installed.

### "Power helper is not running"

```bash
systemctl status dell5530-helper
lsmod | grep acpi_call
```

If the module is missing, install `acpi-call-dkms` (Secure Boot must allow DKMS modules), then run `sudo systemctl restart dell5530-helper`.

### No tray icon

GNOME needs the AppIndicator extension (`gnome-shell-extension-appindicator`). Without a tray host, the close button always quits.

## Project Layout

```
Cargo.toml                  workspace
install.sh / uninstall.sh   one-time sudo install / removal
00-aw-elc.rules             udev rule for USB access
packaging/                  systemd unit and desktop entry
crates/proto/               GUI <-> helper protocol and ACPI call table
crates/helper/              root helper (ACPI via /proc/acpi/call, RAPL energy)
crates/app/                 iced GUI: hid.rs, client.rs, sensors.rs, tray.rs, view.rs
```
