#!/bin/sh
# One-time install (run with sudo after `cargo build --release`).
# Installs the GUI, the root ACPI helper (systemd), the udev rule and the desktop entry,
# and detects the laptop model once (stored in /etc/dell-galien/model.conf).
#
# Options:  --model <id>        skip detection and use this model (see --list-models)
#           --assume-generic    for unrecognized laptops, use the generic G-series controls
#           --list-models       print the supported model ids and exit
set -eu

DIR=$(cd "$(dirname "$0")" && pwd)
MODEL_ARGS=""
while [ $# -gt 0 ]; do
    case "$1" in
        --model) [ $# -ge 2 ] || { echo "--model needs an id" >&2; exit 2; }; MODEL_ARGS="$MODEL_ARGS --model $2"; shift 2 ;;
        --assume-generic) MODEL_ARGS="$MODEL_ARGS --assume-generic"; shift ;;
        --list-models)
            [ -x "$DIR/target/release/dell-galien-helper" ] || { echo "Run: cargo build --release" >&2; exit 1; }
            exec "$DIR/target/release/dell-galien-helper" --list-models ;;
        *) echo "Unknown option: $1" >&2; exit 2 ;;
    esac
done

if [ "$(id -u)" -ne 0 ]; then
    echo "Run with sudo: sudo ./install.sh" >&2
    exit 1
fi

USER_NAME=${SUDO_USER:-}
if [ -z "$USER_NAME" ] || [ "$USER_NAME" = root ]; then
    echo "Run through sudo from your normal user account." >&2
    exit 1
fi
GID=$(id -g "$USER_NAME")

# Remove an install made under the old "dell5530" name, if there is one.
systemctl disable --now dell5530-helper.service 2>/dev/null || true
rm -f /etc/systemd/system/dell5530-helper.service /usr/local/bin/dell5530-controller \
      /usr/local/bin/dell5530-helper /usr/share/applications/dell5530-controller.desktop \
      /usr/share/icons/hicolor/256x256/apps/dell5530-controller.png
rm -rf /etc/dell5530

for bin in dell-galien-controller dell-galien-helper; do
    if [ ! -x "$DIR/target/release/$bin" ]; then
        echo "Missing target/release/$bin. Run: cargo build --release" >&2
        exit 1
    fi
done

install -Dm755 "$DIR/target/release/dell-galien-controller" /usr/local/bin/dell-galien-controller
install -Dm755 "$DIR/target/release/dell-galien-helper" /usr/local/bin/dell-galien-helper
install -Dm644 "$DIR/assets/images/alien-square.png" /usr/share/icons/hicolor/256x256/apps/dell-galien-controller.png
install -Dm644 "$DIR/packaging/dell-galien-controller.desktop" /usr/share/applications/dell-galien-controller.desktop
install -Dm644 "$DIR/00-aw-elc.rules" /etc/udev/rules.d/00-aw-elc.rules
echo acpi_call > /etc/modules-load.d/acpi-call.conf
modprobe acpi_call 2>/dev/null || echo "Warning: could not load the acpi_call module; power controls need it." >&2

echo "Detecting the laptop model (only done once)..."
# shellcheck disable=SC2086
"$DIR/target/release/dell-galien-helper" --detect $MODEL_ARGS || exit 1

sed "s/@GID@/$GID/" "$DIR/packaging/dell-galien-helper.service" > /etc/systemd/system/dell-galien-helper.service

udevadm control --reload-rules
udevadm trigger
systemctl daemon-reload
systemctl enable dell-galien-helper.service
systemctl restart dell-galien-helper.service
gtk-update-icon-cache -q /usr/share/icons/hicolor 2>/dev/null || true

echo "Installed. Launch 'Dell GAlien Controller' from the application menu."
