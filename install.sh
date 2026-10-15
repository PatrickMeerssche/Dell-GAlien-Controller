#!/bin/sh
# One-time install (run with sudo after `cargo build --release`).
# Installs the GUI, the root ACPI helper (systemd), the udev rule and the desktop entry.
set -eu

if [ "$(id -u)" -ne 0 ]; then
    echo "Run with sudo: sudo ./install.sh" >&2
    exit 1
fi

DIR=$(cd "$(dirname "$0")" && pwd)
USER_NAME=${SUDO_USER:-}
if [ -z "$USER_NAME" ] || [ "$USER_NAME" = root ]; then
    echo "Run through sudo from your normal user account." >&2
    exit 1
fi
GID=$(id -g "$USER_NAME")

for bin in dell5530-controller dell5530-helper; do
    if [ ! -x "$DIR/target/release/$bin" ]; then
        echo "Missing target/release/$bin. Run: cargo build --release" >&2
        exit 1
    fi
done

install -Dm755 "$DIR/target/release/dell5530-controller" /usr/local/bin/dell5530-controller
install -Dm755 "$DIR/target/release/dell5530-helper" /usr/local/bin/dell5530-helper
install -Dm644 "$DIR/assets/images/alien-square.png" /usr/share/icons/hicolor/256x256/apps/dell5530-controller.png
install -Dm644 "$DIR/packaging/dell5530-controller.desktop" /usr/share/applications/dell5530-controller.desktop
install -Dm644 "$DIR/00-aw-elc.rules" /etc/udev/rules.d/00-aw-elc.rules
echo acpi_call > /etc/modules-load.d/acpi-call.conf

sed "s/@GID@/$GID/" "$DIR/packaging/dell5530-helper.service" > /etc/systemd/system/dell5530-helper.service

udevadm control --reload-rules
udevadm trigger
systemctl daemon-reload
systemctl enable dell5530-helper.service
systemctl restart dell5530-helper.service
gtk-update-icon-cache -q /usr/share/icons/hicolor 2>/dev/null || true

echo "Installed. Launch 'Dell G15 5530 Controller' from the application menu."
