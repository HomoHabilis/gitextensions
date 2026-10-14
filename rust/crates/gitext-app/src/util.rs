//! Small UI helpers.

use chrono::{DateTime, Local, TimeZone};

use crate::i18n::{n_, trf};

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
    let n = |d: i64| secs / d;
    let (n, one, many) = if secs < 60 {
        (secs, n_("{0} second ago"), n_("{0} seconds ago"))
    } else if secs < 3600 {
        (n(60), n_("{0} minute ago"), n_("{0} minutes ago"))
    } else if secs < 86_400 {
        (n(3600), n_("{0} hour ago"), n_("{0} hours ago"))
    } else if secs < 7 * 86_400 {
        (n(86_400), n_("{0} day ago"), n_("{0} days ago"))
    } else if secs < 31 * 86_400 {
        (n(7 * 86_400), n_("{0} week ago"), n_("{0} weeks ago"))
    } else if secs < 365 * 86_400 {
        (n(30 * 86_400), n_("{0} month ago"), n_("{0} months ago"))
    } else {
        (n(365 * 86_400), n_("{0} year ago"), n_("{0} years ago"))
    };
    trf(if n == 1 { one } else { many }, &[&n])
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
    if crate::wsl::open_on_windows(path) {
        return;
    }
    #[cfg(target_os = "windows")]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut c = std::process::Command::new("cmd");
        // `start` opens the target itself: no console window flashing up for `cmd`
        c.args(["/C", "start", ""]).creation_flags(CREATE_NO_WINDOW);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = std::process::Command::new("xdg-open");
    spawn_detached(cmd.arg(path));
}

/// Opens a folder in the configured file manager (`configured`: a command, arguments allowed),
/// or with the system handler when none is configured.
pub fn open_folder(path: &str, configured: &str) {
    let configured = configured.trim();
    if configured.is_empty() {
        return open_with_system(path);
    }
    // A plain executable path may contain spaces ("C:\Program Files\…"); only split
    // the setting into a command and arguments when it is not one.
    let unquoted = configured.trim_matches('"');
    if std::path::Path::new(unquoted).is_file() {
        spawn_detached(std::process::Command::new(unquoted).arg(path));
        return;
    }
    let mut parts = configured.split_whitespace();
    if let Some(cmd) = parts.next() {
        spawn_detached(std::process::Command::new(cmd).args(parts).arg(path));
    }
}

/// Splits a command line into arguments: whitespace separates them, and double or single
/// quotes group words (the quotes are removed).
pub fn split_args(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut in_arg = false;
    let mut quote: Option<char> = None;
    for c in line.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                in_arg = true;
            }
            None if c.is_whitespace() => {
                if in_arg {
                    args.push(std::mem::take(&mut cur));
                    in_arg = false;
                }
            }
            None => {
                cur.push(c);
                in_arg = true;
            }
        }
    }
    if in_arg {
        args.push(cur);
    }
    args
}

/// Starts a short-lived launcher (`xdg-open`, `explorer.exe`…) without waiting for it, and
/// reaps it when it exits so that it does not stay a zombie process for the life of the app.
pub fn spawn_detached(cmd: &mut std::process::Command) -> bool {
    match cmd.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
            true
        }
        Err(_) => false,
    }
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
    fn script_arguments_keep_quoted_words() {
        assert_eq!(split_args(r#"-c 'echo a b' x"#), ["-c", "echo a b", "x"]);
        assert_eq!(split_args(r#"log --format="%h %s"  -1"#), ["log", "--format=%h %s", "-1"]);
        assert_eq!(split_args(r#"show "" end"#), ["show", "", "end"]);
        assert!(split_args("   ").is_empty());
    }

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
