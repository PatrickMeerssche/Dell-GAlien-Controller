#!/bin/sh
# Removes everything install.sh added (run with sudo).
set -eu

if [ "$(id -u)" -ne 0 ]; then
    echo "Run with sudo: sudo ./uninstall.sh" >&2
    exit 1
fi

systemctl disable --now dell5530-helper.service 2>/dev/null || true
rm -f /etc/systemd/system/dell5530-helper.service \
      /usr/local/bin/dell5530-controller \
      /usr/local/bin/dell5530-helper \
      /usr/share/applications/dell5530-controller.desktop \
      /usr/share/icons/hicolor/256x256/apps/dell5530-controller.png \
      /etc/udev/rules.d/00-aw-elc.rules \
      /etc/modules-load.d/acpi-call.conf
rm -rf /etc/dell5530
systemctl daemon-reload
udevadm control --reload-rules
echo "Removed."
