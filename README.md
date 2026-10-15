# Dell G15 5530 Controller

A small, fast Linux app for the **Dell G15 5530** (Rust + [iced](https://iced.rs)). It does not support other models for now.

## What it does

| Page                              | Features                                                                                                                                                      |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Lighting**                | Keyboard colour (static or colour-cycling "morph"), brightness, off                                                                                           |
| **Performance → Overview** | Power mode (Balanced, Performance, Quiet, Battery Saver, G Mode, Manual), manual fan boost, live CPU/GPU temperature, usage, power, fan RPM and memory graphs |
| **Performance → Nvidia**   | PowerMizer mode (Auto, Normal, Performance), `nvidia-powerd` on/off                                                                                         |
| **Performance → CoreCtrl** | CPU governor (Powersave or Performance); also switches the Ubuntu power mode                                                                                  |
| **Settings**                | Accent colour, close-to-tray, diagnostics                                                                                                                     |
| **Tray icon**               | Click it to turn the keyboard lighting on or off                                                                                                              |

## Install

Tested on Ubuntu 22.04 (GNOME). Other Debian/Ubuntu-based systems should work the same way.
Copy and paste each block into a terminal, one after the other.

### 1. Install the required packages

```bash
sudo apt update
sudo apt install git curl build-essential pkg-config libusb-1.0-0-dev acpi-call-dkms libxnvctrl0
```

- `acpi-call-dkms` is the kernel module used for power modes and fans. If you use Secure Boot, the installer may ask you to set a password and enroll a key on the next reboot.
- `libxnvctrl0` is only needed for the PowerMizer setting. It is installed together with `nvidia-settings`.

### 2. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Press **Enter** to accept the defaults, then **close the terminal and open a new one** so `cargo` is found.
Check that it works:

```bash
cargo --version
```

If this prints an old version or an error about a "lock file", you are using the old system `cargo`. Remove it with `sudo apt remove cargo rustc`, open a new terminal and try again.

### 3. Download and build

```bash
git clone https://github.com/PatrickMeerssche/Dell-5530-Controller.git
cd Dell-5530-Controller
cargo build --release
```

The first build takes a few minutes.

### 4. Install

```bash
sudo ./install.sh
```

Run it as your normal user (with `sudo`), not as root. It is the only step that needs a password. It installs:

- the app (`/usr/local/bin/dell5530-controller`) and its menu entry
- a small background service (`dell5530-helper`) that is the only part running as root. It can only do a fixed list of things (power modes, fans, CPU governor, `nvidia-powerd`), and only your user can talk to it
- the udev rule that lets you control the keyboard lighting without root

### 5. Start the app

Open **Dell G15 5530 Controller** from your application menu, or run `dell5530-controller`.

## Update

```bash
cd Dell-5530-Controller
git pull
cargo build --release
sudo ./install.sh
```

## Uninstall

```bash
sudo ./uninstall.sh
```

Your settings stay in `~/.config/dell5530-controller/`; delete that folder to remove them too.

## Troubleshooting

**"No supported device was found" (keyboard lighting)**
Run `lsusb | grep -i 187c`. If nothing is listed, this is not a supported 5530, or the keyboard controller needs a reboot. Check that `/etc/udev/rules.d/00-aw-elc.rules` exists, then unplug the charger and reboot.

**"Power helper is not running" (power modes and fans don't work)**

```bash
systemctl status dell5530-helper
lsmod | grep acpi_call
```

If `acpi_call` is not listed, run `sudo modprobe acpi_call`. If that fails, the module is not installed (step 1) or Secure Boot is blocking it. Then run `sudo systemctl restart dell5530-helper`.

**The Nvidia PowerMizer buttons show an error**
PowerMizer needs an X11 session (not Wayland), the NVIDIA driver and `libxnvctrl0`. The mode only stays set while the app is running, and the app reapplies your last choice when it starts.

**No tray icon**
GNOME needs the AppIndicator extension: `sudo apt install gnome-shell-extension-appindicator`, then log out and in. Without a tray the close button always quits the app.

**The build fails with a missing library**
Install the `-dev` package for the library named in the error (for example `sudo apt install libxkbcommon-dev`) and run `cargo build --release` again.

## For developers

```
Cargo.toml                  workspace
install.sh / uninstall.sh   one-time sudo install / removal
00-aw-elc.rules             udev rule for keyboard USB access
packaging/                  systemd unit and desktop entry
crates/proto/               GUI <-> helper protocol and ACPI call table
crates/helper/              root helper (ACPI via /proc/acpi/call, RAPL energy, governor, nvidia-powerd)
crates/app/                 iced GUI: hid.rs, client.rs, sensors.rs, tuning.rs, tray.rs, view.rs
```

The previous Python/Qt version is on the `beta-version` branch.

## Credits

Based on the original project by cemkaya-mpi:
[https://github.com/cemkaya-mpi/Dell-G-Series-Controller](https://github.com/cemkaya-mpi/Dell-G-Series-Controller)
