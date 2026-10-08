#!/usr/bin/env bash
# Writes the release notes of <version> to stdout (run in the git checkout, at the release commit):
#   release-notes.sh <version> [tag]
# The description is the message of the annotated tag <tag> if there is one, else the
# "## <version>" section of CHANGELOG.md. Then come the commits since the previous release tag
# that changed the Rust port, and the downloads and installation instructions.
set -euo pipefail

version="$1"
tag="${2:-v$version}"
root="$(cd "$(dirname "$0")/.." && pwd)"
repo="${GITHUB_REPOSITORY:-homohabilis/gitextensions}"

# the section "## <version>" (or "## [<version>]") up to the next "## "
section="$(awk -v v="$version" '
    /^## / { if (found) exit; title = $0; sub(/^## +\[?/, "", title); sub(/\].*$/, "", title); sub(/ .*$/, "", title); found = (title == v); next }
    found { print }
' "$root/CHANGELOG.md")"

previous="$(git describe --tags --abbrev=0 --match 'v[0-9]*' --exclude "$tag" HEAD 2>/dev/null || true)"
range="${previous:+$previous..}HEAD"
commits="$(git log "$range" --no-merges --format='- %s (%h)' -- "$root" "$root/../.github/workflows/rust.yml" "$root/../.github/workflows/rust-release.yml")"

if [ "$(git cat-file -t "refs/tags/$tag" 2>/dev/null)" = tag ]; then
    message="$(git for-each-ref --format='%(contents)' "refs/tags/$tag")"
    signature="$(git for-each-ref --format='%(contents:signature)' "refs/tags/$tag")"
    message="${message%"$signature"}"
    printf '%s\n\n' "$message"
elif [ -n "$section" ]; then
    printf '%s\n\n' "$(echo "$section" | sed -e '/./,$!d')"
fi

echo "## Changes"
echo
if [ -n "$commits" ]; then
    echo "$commits"
else
    echo "- No changes to the application."
fi
if [ -n "$previous" ]; then
    echo
    echo "**Full changelog:** https://github.com/$repo/compare/$previous...$tag"
fi

cat <<NOTES

## Downloads

| Platform | File | |
|---|---|---|
| Windows 10/11 (x64) | \`GitExtensions-$version-setup.exe\` | Installer, with the Explorer context menu |
| Windows 10/11 (x64) | \`gitextensions-$version-windows-x86_64-portable.zip\` | Portable, no installation |
| macOS 11+ (Apple silicon and Intel) | \`gitextensions-$version-macos-universal.dmg\` | Application bundle |
| macOS 11+ (Apple silicon and Intel) | \`gitextensions-$version-macos-universal.tar.gz\` | Command-line binary |
| Linux x86_64 (Ubuntu 22.04+, glibc 2.35+) | \`gitextensions-$version-linux-x86_64.tar.gz\` | Binary, desktop entry and install script |

\`SHA256SUMS.txt\` lists the checksums of the files.

## Installation

Git must be installed: it is not bundled.

- **Windows:** run the installer. It installs for the current user (no administrator rights)
  and adds the *Git Extensions* menu to Explorer; on Windows 11 it is under *Show more
  options*. The files are not code-signed, so SmartScreen may ask for confirmation
  (*More info → Run anyway*).
- **macOS:** open the disk image and drag *Git Extensions* to *Applications*. The app is not
  notarized: the first time, right-click it and choose *Open*, or run
  \`xattr -dr com.apple.quarantine "/Applications/Git Extensions.app"\`.
- **Linux:** extract the archive and run \`./install.sh\` (installs to \`~/.local\`, or
  \`PREFIX=/usr/local sudo ./install.sh\`). It needs OpenGL and \`libxkbcommon-x11\` (X11) or
  Wayland; zenity or kdialog are used for the file dialogs when installed.
NOTES
