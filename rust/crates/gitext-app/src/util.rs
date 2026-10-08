//! Small UI helpers.

use chrono::{DateTime, Local, TimeZone};

/// Absolute date (`yyyy-MM-dd HH:mm:ss`).
pub fn format_date(unix: i64) -> String {
    if unix == 0 {
        return String::new();
    }
    match Local.timestamp_opt(unix, 0).single() {
        Some(d) => d.format("%Y-%m-%d %H:%M:%S").to_string(),
        None => String::new(),
    }
}

/// Relative date like the revision grid ("5 minutes ago").
pub fn relative_date(unix: i64, now: DateTime<Local>) -> String {
    if unix == 0 {
        return String::new();
    }
    let Some(d) = Local.timestamp_opt(unix, 0).single() else { return String::new() };
    let secs = (now - d).num_seconds();
    if secs < 0 {
        return format_date(unix);
    }
    let (n, unit) = if secs < 60 {
        (secs, "second")
    } else if secs < 3600 {
        (secs / 60, "minute")
    } else if secs < 86_400 {
        (secs / 3600, "hour")
    } else if secs < 7 * 86_400 {
        (secs / 86_400, "day")
    } else if secs < 31 * 86_400 {
        (secs / (7 * 86_400), "week")
    } else if secs < 365 * 86_400 {
        (secs / (30 * 86_400), "month")
    } else {
        (secs / (365 * 86_400), "year")
    };
    if n == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{n} {unit}s ago")
    }
}

pub fn short_date(unix: i64) -> String {
    relative_date(unix, Local::now())
}

/// Copies text to the clipboard.
pub fn copy_to_clipboard(ctx: &egui::Context, text: impl Into<String>) {
    ctx.copy_text(text.into());
}

/// Opens a path or URL with the system handler.
pub fn open_with_system(path: &str) {
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("cmd").args(["/C", "start", "", path]).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(path).spawn();
    let _ = r;
}

/// Opens a terminal in `dir` (port of "Git bash").
pub fn open_terminal(dir: &std::path::Path, configured: &str) {
    if !configured.trim().is_empty() {
        let mut parts = configured.split_whitespace();
        if let Some(cmd) = parts.next() {
            let _ = std::process::Command::new(cmd).args(parts).current_dir(dir).spawn();
        }
        return;
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd").args(["/C", "start", "cmd"]).current_dir(dir).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").args(["-a", "Terminal"]).arg(dir).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for t in ["x-terminal-emulator", "gnome-terminal", "konsole", "xfce4-terminal", "xterm"] {
            if std::process::Command::new(t).current_dir(dir).spawn().is_ok() {
                break;
            }
        }
    }
}

/// Opens a file in the configured editor or the system default.
pub fn open_in_editor(path: &std::path::Path, editor: &str) {
    if editor.trim().is_empty() {
        open_with_system(&path.display().to_string());
    } else {
        let mut parts = editor.split_whitespace();
        if let Some(cmd) = parts.next() {
            let _ = std::process::Command::new(cmd).args(parts).arg(path).spawn();
        }
    }
}

/// Whether the last file dialog could not be shown (no xdg-desktop-portal, zenity or kdialog,
/// e.g. on WSL): the dialogs then ask to type the path.
static FILE_DIALOG_UNAVAILABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn file_dialog_unavailable() -> bool {
    FILE_DIALOG_UNAVAILABLE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Linux: the dialog of zenity (GNOME) or kdialog (KDE) when installed. They work without the
/// xdg-desktop-portal that the built-in dialog needs (which WSL and minimal desktops lack).
/// `None`: no helper; `Some(None)`: cancelled.
#[cfg(all(unix, not(target_os = "macos")))]
fn helper_dialog(mode: DialogMode, start: Option<&str>) -> Option<Option<String>> {
    use gitext_core::diff_tools::find_in_path;
    let dir = start.filter(|s| std::path::Path::new(s).is_dir()).map(|s| format!("{}/", s.trim_end_matches('/')));
    let initial = match mode {
        DialogMode::Save(name) => format!("{}{name}", dir.clone().unwrap_or_default()),
        _ => dir.clone().unwrap_or_default(),
    };
    let mut cmd = if find_in_path("zenity").is_some() {
        let mut c = std::process::Command::new("zenity");
        c.arg("--file-selection");
        match mode {
            DialogMode::Folder => {
                c.arg("--directory");
            }
            DialogMode::Save(_) => {
                c.arg("--save").arg("--confirm-overwrite");
            }
            DialogMode::Open => {}
        }
        if !initial.is_empty() {
            c.arg(format!("--filename={initial}"));
        }
        c
    } else if find_in_path("kdialog").is_some() {
        let mut c = std::process::Command::new("kdialog");
        c.arg(match mode {
            DialogMode::Folder => "--getexistingdirectory",
            DialogMode::Open => "--getopenfilename",
            DialogMode::Save(_) => "--getsavefilename",
        });
        c.arg(if initial.is_empty() { "." } else { initial.as_str() });
        c
    } else {
        return None;
    };
    let out = cmd.stderr(std::process::Stdio::null()).output().ok()?;
    match out.status.code() {
        Some(0) => {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            Some((!p.is_empty()).then_some(p))
        }
        Some(1) => Some(None),
        _ => None,
    }
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn helper_dialog(_mode: DialogMode, _start: Option<&str>) -> Option<Option<String>> {
    None
}

#[derive(Clone, Copy)]
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
enum DialogMode<'a> {
    Folder,
    Open,
    Save(&'a str),
}

/// Runs the built-in (portal) dialog; a "cancel" faster than a person can click means that no
/// dialog was shown.
fn native_dialog(f: impl FnOnce() -> Option<std::path::PathBuf>) -> Option<String> {
    let started = std::time::Instant::now();
    let r = f().map(|p| p.display().to_string());
    let unavailable = cfg!(all(unix, not(target_os = "macos"))) && r.is_none() && started.elapsed() < std::time::Duration::from_millis(250);
    FILE_DIALOG_UNAVAILABLE.store(unavailable, std::sync::atomic::Ordering::Relaxed);
    r
}

/// Picks a folder with the native dialog.
pub fn pick_folder(start: Option<&str>) -> Option<String> {
    if let Some(r) = helper_dialog(DialogMode::Folder, start) {
        FILE_DIALOG_UNAVAILABLE.store(false, std::sync::atomic::Ordering::Relaxed);
        return r;
    }
    let mut d = rfd::FileDialog::new();
    if let Some(s) = start.filter(|s| std::path::Path::new(s).is_dir()) {
        d = d.set_directory(s);
    }
    native_dialog(|| d.pick_folder())
}

pub fn pick_file(start: Option<&str>, filter: Option<(&str, &[&str])>) -> Option<String> {
    if let Some(r) = helper_dialog(DialogMode::Open, start) {
        FILE_DIALOG_UNAVAILABLE.store(false, std::sync::atomic::Ordering::Relaxed);
        return r;
    }
    let mut d = rfd::FileDialog::new();
    if let Some(s) = start.filter(|s| std::path::Path::new(s).is_dir()) {
        d = d.set_directory(s);
    }
    if let Some((name, ext)) = filter {
        d = d.add_filter(name, ext);
    }
    native_dialog(|| d.pick_file())
}

pub fn save_file(start: Option<&str>, name: &str) -> Option<String> {
    if let Some(r) = helper_dialog(DialogMode::Save(name), start) {
        FILE_DIALOG_UNAVAILABLE.store(false, std::sync::atomic::Ordering::Relaxed);
        return r;
    }
    let mut d = rfd::FileDialog::new().set_file_name(name);
    if let Some(s) = start.filter(|s| std::path::Path::new(s).is_dir()) {
        d = d.set_directory(s);
    }
    native_dialog(|| d.save_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_dates() {
        let now = Local::now();
        let t = now.timestamp();
        assert_eq!(relative_date(t - 30, now), "30 seconds ago");
        assert_eq!(relative_date(t - 60, now), "1 minute ago");
        assert_eq!(relative_date(t - 3 * 3600, now), "3 hours ago");
        assert_eq!(relative_date(t - 2 * 86_400, now), "2 days ago");
        assert_eq!(relative_date(t - 400 * 86_400, now), "1 year ago");
        assert_eq!(relative_date(0, now), "");
    }
}
