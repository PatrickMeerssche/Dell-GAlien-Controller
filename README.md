# Dell G-Series Controller

A small, fast Linux app for **Dell G15, G16 and Alienware M16** laptops (Rust + [iced](https://iced.rs)).

## Supported models

| Laptop           | Power modes and fans | Keyboard lighting |
| ---------------- | -------------------- | ----------------- |
| G15 5530         | yes (tested)         | yes (tested)      |
| G15 5525         | yes                  | yes               |
| G15 5520         | yes                  | yes               |
| G15 5515         | G Mode and Manual only | yes             |
| G15 5511         | yes                  | yes               |
| G16 7620         | yes                  | untested          |
| G16 7630         | yes                  | untested          |
| Alienware M16 R1 | yes                  | untested          |

The model support comes from the original project (see Credits); only the G15 5530 is tested by this fork's author.
The installer detects your laptop **once**, during `sudo ./install.sh`, and remembers it. The app never probes the hardware to find the model again.
Other laptops with a `187c:0550` or `187c:0551` keyboard controller get keyboard lighting only, unless you choose to try the generic power controls during install.

## What it does

| Page                              | Features                                                                                                                                                      |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Lighting**                | Keyboard colour (static or colour-cycling "morph"), brightness, off                                                                                           |
| **Performance → Overview** | Power mode (the modes your model supports: Balanced, Performance, Cool, Quiet, Full Speed, Battery Saver, G Mode, Manual), manual fan boost, live CPU/GPU temperature, usage, power, fan RPM and memory graphs |
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

Run it as your normal user (with `sudo`), not as root. It is the only step that needs a password. It:

- detects your laptop model once and stores it in `/etc/dell5530/model.conf` (you will see the result printed)
- installs the app (`/usr/local/bin/dell5530-controller`) and its menu entry
- installs a small background service (`dell5530-helper`) that is the only part running as root. It can only do a fixed list of things (power modes, fans, CPU governor, `nvidia-powerd`), and only your user can talk to it
- installs the udev rule that lets you control the keyboard lighting without root

If your laptop is not recognized, the installer asks whether to try the generic Dell G-series power controls. These may not suit other models, so only answer `y` if you accept that risk.
You can also pick a model yourself with `sudo ./install.sh --model g15_5525` (list the ids with `./install.sh --list-models`).
If you ever change the model (for example after moving the SSD to another laptop), just run `sudo ./install.sh` again.

### 5. Start the app

Open **Dell G-Series Controller** from your application menu, or run `dell5530-controller`.

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
Run `lsusb | grep -i 187c`. If nothing is listed, this laptop has no supported keyboard controller (`187c:0550` or `187c:0551`), or it needs a reboot. Check that `/etc/udev/rules.d/00-aw-elc.rules` exists, then unplug the charger and reboot.

**The installer says "keyboard lighting only" or the power modes are missing**
Your laptop was not recognized. Run `./install.sh --list-models`, then install again with `sudo ./install.sh --model <id>`, or `sudo ./install.sh --assume-generic` to try the generic controls. Please also open an issue with the output of `cat /sys/class/dmi/id/product_name`.

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
crates/proto/               GUI <-> helper protocol, ACPI call table and the supported-model table (model.rs)
crates/helper/              root helper (ACPI via /proc/acpi/call, RAPL energy, governor, nvidia-powerd) and the install-time model detection (--detect)
crates/app/                 iced GUI: hid.rs, client.rs, sensors.rs, tuning.rs, tray.rs, view.rs
```

The previous Python/Qt version is on the `beta-version` branch.

## Credits

Based on the original project by cemkaya-mpi:
[https://github.com/cemkaya-mpi/Dell-G-Series-Controller](https://github.com/cemkaya-mpi/Dell-G-Series-Controller)
