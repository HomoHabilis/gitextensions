#!/usr/bin/env bash
# Packages a release build of gitext into ../dist (run from any directory):
#   package.sh <version> linux|macos|windows [binary]
# linux:   gitextensions-<version>-linux-x86_64.tar.gz (binary, desktop file, icon, install.sh)
# macos:   gitextensions-<version>-macos-universal.dmg (Git Extensions.app) and .tar.gz (binary)
# windows: GitExtensions-<version>-setup.exe (NSIS installer) and a portable .zip
# The binary defaults to target/release/gitext[.exe]; for macOS pass the universal binary.
set -euo pipefail

version="$1"
platform="$2"
root="$(cd "$(dirname "$0")/.." && pwd)"
here="$root/packaging"
dist="$root/dist"
mkdir -p "$dist"
docs=("$root/README.md" "$root/../LICENSE.md")
abs() { echo "$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"; }

case "$platform" in
linux)
    bin="${3:-$root/target/release/gitext}"
    name="gitextensions-$version-linux-$(uname -m)"
    stage="$(mktemp -d)/$name"
    mkdir -p "$stage"
    install -m755 "$bin" "$stage/gitext"
    cp "${docs[@]}" "$here/linux/gitextensions.desktop" "$here/linux/gitextensions.png" "$here/linux/install.sh" "$stage/"
    tar -C "$(dirname "$stage")" -czf "$dist/$name.tar.gz" "$name"
    ;;
macos)
    bin="${3:-$root/target/release/gitext}"
    name="gitextensions-$version-macos-universal"
    work="$(mktemp -d)"
    app="$work/dmg/Git Extensions.app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    install -m755 "$bin" "$app/Contents/MacOS/gitext"
    sed "s/@VERSION@/$version/g" "$here/macos/Info.plist" > "$app/Contents/Info.plist"
    iconset="$work/gitext.iconset"
    mkdir -p "$iconset"
    for size in 16 32 128 256 512; do
        sips -z $size $size "$here/macos/icon-512.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
        double=$((size * 2))
        if [ $double -le 512 ]; then
            sips -z $double $double "$here/macos/icon-512.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
        fi
    done
    iconutil -c icns "$iconset" -o "$app/Contents/Resources/gitext.icns"
    # unsigned: an ad-hoc signature is needed to run on Apple silicon
    codesign --force --deep --sign - "$app"
    ln -s /Applications "$work/dmg/Applications"
    cp "${docs[@]}" "$work/dmg/"
    # hdiutil sometimes fails with "Resource busy" on CI machines: retry
    for attempt in 1 2 3 4 5; do
        hdiutil create -volname "Git Extensions $version" -srcfolder "$work/dmg" -format UDZO -ov "$dist/$name.dmg" && break
        [ $attempt -eq 5 ] && exit 1
        sleep $((attempt * 5))
    done
    tar_stage="$work/$name"
    mkdir -p "$tar_stage"
    install -m755 "$bin" "$tar_stage/gitext"
    cp "${docs[@]}" "$tar_stage/"
    tar -C "$work" -czf "$dist/$name.tar.gz" "$name"
    ;;
windows)
    bin="$(abs "${3:-$root/target/release/gitext.exe}")"
    numeric="${version%%-*}"
    makensis="makensis"
    if ! command -v makensis >/dev/null 2>&1; then
        makensis="/c/Program Files (x86)/NSIS/makensis.exe"
    fi
    # makensis and 7z are Windows programs: give them Windows paths (Git Bash on Windows)
    native() { if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else echo "$1"; fi; }
    # "-D" instead of "/D": Git Bash would convert "/D..." to a path
    MSYS2_ARG_CONV_EXCL='*' "$makensis" -V2 "-DVERSION=$version" "-DNUMVERSION=$numeric" "-DEXE=$(native "$bin")" \
        "-DOUTFILE=$(native "$dist/GitExtensions-$version-setup.exe")" "$(native "$here/windows/gitext.nsi")"
    name="gitextensions-$version-windows-x86_64-portable"
    stage="$(mktemp -d)/$name"
    mkdir -p "$stage"
    cp "$bin" "$stage/gitext.exe"
    cp "${docs[@]}" "$stage/"
    if command -v 7z >/dev/null 2>&1; then
        (cd "$(dirname "$stage")" && 7z a -tzip -bd "$(native "$dist/$name.zip")" "$name" >/dev/null)
    else
        (cd "$(dirname "$stage")" && zip -qr "$dist/$name.zip" "$name")
    fi
    ;;
*)
    echo "unknown platform '$platform'" >&2
    exit 2
    ;;
esac
ls -l "$dist"
