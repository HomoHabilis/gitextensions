#!/bin/sh
# Installs Git Extensions for the current user (~/.local), or system-wide with
# PREFIX=/usr/local sudo ./install.sh. Remove with ./install.sh --uninstall.
set -e
cd "$(dirname "$0")"
PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin/gitext"
DESKTOP="$PREFIX/share/applications/gitextensions.desktop"
ICON="$PREFIX/share/icons/hicolor/256x256/apps/gitextensions.png"
if [ "$1" = "--uninstall" ]; then
    rm -f "$BIN" "$DESKTOP" "$ICON"
    echo "Removed Git Extensions from $PREFIX."
    exit 0
fi
install -Dm755 gitext "$BIN"
mkdir -p "$(dirname "$DESKTOP")"
# the menu entry runs the installed binary, even when its directory is not on PATH
sed "s|^Exec=gitext|Exec=$BIN|" gitextensions.desktop > "$DESKTOP"
chmod 644 "$DESKTOP"
install -Dm644 gitextensions.png "$ICON"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$PREFIX/share/applications" || true
echo "Installed $BIN."
case ":$PATH:" in *":$PREFIX/bin:"*) ;; *) echo "Add $PREFIX/bin to PATH to run 'gitext' from a terminal." ;; esac
