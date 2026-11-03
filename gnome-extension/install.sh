#!/usr/bin/env bash
# Install the Linux Wallpaper Engine GNOME Shell extension.
#
# The extension is copied into the user extension directory; it can be
# enabled with "gnome-extensions enable <uuid>" afterwards.

set -euo pipefail

UUID="linux-wallpaper-engine@wqLouis.github.io"
SOURCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/src"
TARGET_DIR="${HOME}/.local/share/gnome-shell/extensions/${UUID}"

if [ ! -d "${SOURCE_DIR}" ]; then
    echo "error: cannot find the extension sources in ${SOURCE_DIR}" >&2
    exit 1
fi

mkdir -p "${TARGET_DIR}"
cp -f "${SOURCE_DIR}"/*.js "${SOURCE_DIR}"/*.json "${SOURCE_DIR}"/*.css "${TARGET_DIR}/"

echo "Installed the extension into ${TARGET_DIR}"

if command -v gnome-extensions >/dev/null 2>&1; then
    gnome-extensions enable "${UUID}" || true
    echo "Enabled the extension."
else
    echo "The 'gnome-extensions' tool is not available; enable the extension"
    echo "manually (GNOME Extensions app or 'gnome-extensions enable ${UUID}')."
fi

if [ "${XDG_SESSION_TYPE:-}" = "x11" ]; then
    echo
    echo "Log out and back in (or press Alt+F2 and enter 'r' on X11) to load the extension."
else
    echo
    echo "Log out and back in to load the extension (Wayland does not allow"
    echo "reloading the shell)."
fi
