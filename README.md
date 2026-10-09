![Git Extensions logo](assets/logo/git-extensions-logo.svg)

# Git Extensions (Rust port)

Git Extensions is a graphical user interface for git. This repository is a cross-platform
port of it to Rust and [egui](https://github.com/emilk/egui) that runs on Windows, macOS and
Linux. It keeps the features, the window layout and the revision graph of the original
Windows application, and adds a light and a dark theme.

The original C# / WinForms application is maintained at
[gitextensions/gitextensions](https://github.com/gitextensions/gitextensions). It was
removed from this repository; it is still in the git history before the commit that removed it.

[![rust](https://github.com/HomoHabilis/gitextensions/actions/workflows/rust.yml/badge.svg?branch=master)](https://github.com/HomoHabilis/gitextensions/actions/workflows/rust.yml)

## Download and install

**[Download the latest release](https://github.com/HomoHabilis/gitextensions/releases/latest).**
Git must be installed: it is not bundled.

| Platform | File |
|---|---|
| Windows 10/11 (x64) | `GitExtensions-<version>-setup.exe` (installer) or `…-windows-x86_64-portable.zip` |
| macOS 11+ (Apple silicon and Intel) | `…-macos-universal.dmg` (app bundle) or `…-macos-universal.tar.gz` (binary) |
| Linux x86_64 (Ubuntu 22.04+, glibc 2.35+) | `…-linux-x86_64.tar.gz` |

- **Windows:** run the installer. It installs for the current user, without administrator
  rights, and adds the *Git Extensions* menu to Explorer (on Windows 11 under *Show more
  options*). The files are not code-signed, so SmartScreen may ask for confirmation
  (*More info → Run anyway*). The portable zip needs no installation.
- **macOS:** open the disk image and drag *Git Extensions* to *Applications*. The app is not
  notarized: the first time, right-click it and choose *Open*, or run
  `xattr -dr com.apple.quarantine "/Applications/Git Extensions.app"`.
- **Linux:** extract the archive and run `./install.sh`. It installs to `~/.local`, or
  system-wide with `PREFIX=/usr/local sudo ./install.sh`, and `./install.sh --uninstall`
  removes it. It needs OpenGL and `libxkbcommon-x11` (X11) or Wayland. zenity or kdialog are
  used for the file dialogs when installed, and [Meld](https://meldmerge.org) is the
  recommended diff and merge tool.

The program is called `gitext`. `gitext` opens the repository in the current directory, and
`gitext --help` lists the command-line verbs (`gitext commit`, `gitext blame <file>`, …).

## Build from source

Install a [Rust toolchain](https://rustup.rs) (1.85 or newer), then:

```sh
cd rust
cargo run --release -p gitext-app   # build and run
cargo test --workspace              # run the tests
```

[rust/README.md](rust/README.md) describes the code layout, the features and how they map to
the original application, the settings, the tests, packaging and the release process.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and the [code of conduct](CODE_OF_CONDUCT.md).

## License and credits

Git Extensions is licensed under the [GNU General Public License v3](LICENSE.md). The port is
based on the work of the [Git Extensions contributors](https://github.com/gitextensions/gitextensions/graphs/contributors).
