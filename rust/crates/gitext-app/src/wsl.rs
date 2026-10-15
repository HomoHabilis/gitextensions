//! Workarounds for WSL and WSLg (the Wayland/X server of WSL).

#[cfg(target_os = "linux")]
fn running_in_wsl() -> bool {
    std::env::var_os("WSL_DISTRO_NAME").is_some()
        || std::env::var_os("WSL_INTEROP").is_some()
        || std::fs::read_to_string("/proc/sys/kernel/osrelease").is_ok_and(|r| r.to_ascii_lowercase().contains("microsoft"))
}

/// Under WSL, makes the application and the tools it starts (meld, gitk...) use X11 instead
/// of Wayland.
///
/// WSLg's Wayland compositor drops the application's connection while its window opens
/// ("Connection reset by peer"), and GTK tools on it can fail to create cursors, which makes
/// meld abort and can leave the mouse pointer invisible. Through XWayland both work, and the
/// window gets the title bar of WSLg. GTK still connects to the default Wayland socket
/// without `WAYLAND_DISPLAY`, so it is told to use X11 explicitly (unless the user set
/// `GDK_BACKEND`). Must run before other threads are started (it changes
/// the process environment).
pub fn prefer_x11() {
    #[cfg(target_os = "linux")]
    if running_in_wsl() && std::env::var_os("DISPLAY").is_some() {
        std::env::remove_var("WAYLAND_DISPLAY");
        if std::env::var_os("GDK_BACKEND").is_none() {
            std::env::set_var("GDK_BACKEND", "x11");
        }
    }
}

/// What `explorer.exe` opens for `target`: a URL as is, a path of the distro as a Windows path
/// (`\\wsl$\<distro>\home\…`, or `C:\…` for `/mnt/c/…`).
#[cfg_attr(not(target_os = "linux"), allow(dead_code, reason = "only used on Linux, tested everywhere"))]
fn windows_target(target: &str, distro: &str) -> String {
    if target.contains("://") {
        target.to_string()
    } else {
        gitext_core::wsl::windows_path(target, distro)
    }
}

/// Under WSL, opens a folder, a file or a URL with Windows (Explorer, the default browser or
/// application). The distro usually has no file manager nor browser, so `xdg-open` fails
/// there; egui's links already open on Windows (the `webbrowser` crate does the same).
/// Returns false when not in WSL or when Windows programs cannot be started from it.
pub fn open_on_windows(target: &str) -> bool {
    #[cfg(target_os = "linux")]
    {
        let Some(distro) = std::env::var("WSL_DISTRO_NAME").ok().filter(|d| !d.is_empty()) else {
            return false;
        };
        // interop can be disabled (`[interop] enabled=false` in wsl.conf)
        let interop = ["WSLInterop", "WSLInterop-late"]
            .iter()
            .any(|f| std::fs::read_to_string(format!("/proc/sys/fs/binfmt_misc/{f}")).is_ok_and(|s| s.contains("enabled")));
        interop && crate::util::spawn_detached(std::process::Command::new("explorer.exe").arg(windows_target(target, &distro)))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = target;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_targets() {
        assert_eq!(windows_target("https://git-extensions-documentation.readthedocs.io/", "Ubuntu"), "https://git-extensions-documentation.readthedocs.io/");
        assert_eq!(windows_target("/home/jack/repo", "Ubuntu-22.04"), r"\\wsl$\Ubuntu-22.04\home\jack\repo");
        assert_eq!(windows_target("/home/jack/dépôt/a b.txt", "Ubuntu"), r"\\wsl$\Ubuntu\home\jack\dépôt\a b.txt");
        assert_eq!(windows_target("/mnt/c/Users/jack", "Ubuntu"), r"C:\Users\jack");
    }
}
