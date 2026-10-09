//! Workarounds for WSLg (the Wayland/X server of WSL).
//!
//! Driver noise: under WSL the OpenGL driver (Mesa's d3d12 backend over dxcore) prints lines such as
//! `Dropped Escape call with ulEscapeCode : 0x03007703` straight to stdout/stderr from
//! native code, so they cannot be silenced through any API. While the window is open,
//! stdout and stderr are routed through pipes and every line is forwarded except those.

#[cfg(target_os = "linux")]
mod imp {
    use std::fs::File;
    use std::io::{BufRead, BufReader, Write};
    use std::os::fd::{FromRawFd, RawFd};
    use std::thread::JoinHandle;

    /// Lines starting with one of these are dropped.
    const NOISE: &[&str] = &["Dropped Escape call with ulEscapeCode"];

    struct Redirect {
        fd: RawFd,
        saved: RawFd,
        thread: JoinHandle<()>,
    }

    /// Restores the original stdout/stderr when dropped, after the remaining output was forwarded.
    pub struct Guard(Vec<Redirect>);

    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = std::io::stdout().flush();
            for r in self.0.drain(..) {
                // closes the pipe's write end: the forwarding thread sees EOF and finishes, unless
                // a child process still holds it, so it is not waited for indefinitely
                unsafe { libc::dup2(r.saved, r.fd) };
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(200);
                while !r.thread.is_finished() && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                unsafe { libc::close(r.saved) };
            }
        }
    }

    pub fn running_in_wsl() -> bool {
        std::env::var_os("WSL_DISTRO_NAME").is_some()
            || std::env::var_os("WSL_INTEROP").is_some()
            || std::fs::read_to_string("/proc/sys/kernel/osrelease").is_ok_and(|r| r.to_ascii_lowercase().contains("microsoft"))
    }

    fn redirect(fd: RawFd) -> Option<Redirect> {
        let mut pipe = [0; 2];
        unsafe {
            if libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC) != 0 {
                return None;
            }
            let saved = libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0);
            if saved < 0 || libc::dup2(pipe[1], fd) < 0 {
                libc::close(pipe[0]);
                libc::close(pipe[1]);
                if saved >= 0 {
                    libc::close(saved);
                }
                return None;
            }
            libc::close(pipe[1]);
            let reader = File::from_raw_fd(pipe[0]);
            let out_fd = libc::fcntl(saved, libc::F_DUPFD_CLOEXEC, 0);
            if out_fd < 0 {
                libc::dup2(saved, fd);
                libc::close(saved);
                return None;
            }
            let mut out = File::from_raw_fd(out_fd);
            let thread = std::thread::Builder::new()
                .name("output-filter".into())
                .spawn(move || {
                    let mut reader = BufReader::new(reader);
                    let mut line = Vec::new();
                    while reader.read_until(b'\n', &mut line).is_ok_and(|n| n > 0) {
                        if !NOISE.iter().any(|n| line.starts_with(n.as_bytes())) {
                            let _ = out.write_all(&line);
                        }
                        line.clear();
                    }
                })
                .ok();
            match thread {
                Some(thread) => Some(Redirect { fd, saved, thread }),
                None => {
                    libc::dup2(saved, fd);
                    libc::close(saved);
                    None
                }
            }
        }
    }

    pub fn filter() -> Option<Guard> {
        if !running_in_wsl() {
            return None;
        }
        Some(Guard([libc::STDOUT_FILENO, libc::STDERR_FILENO].into_iter().filter_map(redirect).collect()))
    }
}

#[cfg(target_os = "linux")]
pub use imp::filter;

/// Nothing to filter on this platform.
#[cfg(not(target_os = "linux"))]
pub fn filter() -> Option<()> {
    None
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
    if imp::running_in_wsl() && std::env::var_os("GDK_BACKEND").is_none() && std::env::var_os("DISPLAY").is_some() {
        std::env::set_var("GDK_BACKEND", "x11");
    }
}
