//! Workarounds for WSLg (the Wayland/X server of WSL).

#[cfg(target_os = "linux")]
fn running_in_wsl() -> bool {
    std::env::var_os("WSL_DISTRO_NAME").is_some()
        || std::env::var_os("WSL_INTEROP").is_some()
        || std::fs::read_to_string("/proc/sys/kernel/osrelease").is_ok_and(|r| r.to_ascii_lowercase().contains("microsoft"))
}

/// Makes the GTK tools started from the application (meld, gitk...) use X11 under WSL.
///
/// On WSLg's Wayland compositor GTK can fail to create cursors, which makes meld abort while
/// mapping its window ("Gdk.Cursor.new_for_display returned NULL"); the abort can leave the
/// compositor with an invisible mouse pointer for every window. Through XWayland GTK falls back
/// to the X core cursors instead. A `GDK_BACKEND` set by the user is kept.
/// Must run before other threads are started (it changes the process environment).
pub fn prefer_x11_for_gtk_tools() {
    #[cfg(target_os = "linux")]
    if running_in_wsl() && std::env::var_os("GDK_BACKEND").is_none() && std::env::var_os("DISPLAY").is_some() {
        std::env::set_var("GDK_BACKEND", "x11");
    }
}
