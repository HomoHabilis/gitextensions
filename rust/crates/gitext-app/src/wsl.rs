//! Workarounds for WSLg (the Wayland/X server of WSL).

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
/// window gets the title bar of WSLg. Must run before other threads are started (it changes
/// the process environment).
pub fn prefer_x11() {
    #[cfg(target_os = "linux")]
    if running_in_wsl() && std::env::var_os("DISPLAY").is_some() {
        std::env::remove_var("WAYLAND_DISPLAY");
    }
}
