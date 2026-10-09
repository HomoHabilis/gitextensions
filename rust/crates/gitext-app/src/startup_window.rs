//! Keeps the main window out of sight until its first frames are painted (Windows).
//!
//! eframe creates the window hidden and shows it after the first frame, but on Windows the
//! window still flashed white at startup:
//! - a window restored maximized is shown by winit right when it is created
//!   (`ShowWindow(SW_MAXIMIZE)` ignores the hidden state), long before OpenGL is set up and
//!   the repository is loaded, so an empty white window stays on screen meanwhile;
//! - when the window is finally shown, DWM can present it before the first OpenGL frame.
//!
//! The window is therefore created restored and cloaked (`DWMWA_CLOAK`: laid out and painted
//! but not displayed), maximized during the first frame, and uncloaked once a frame of its
//! final size has been presented.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Frames painted before the window is uncloaked: the first one, plus one at the maximized
/// size (the maximize command of the first frame is applied after it is painted).
#[cfg(windows)]
const HIDDEN_FRAMES: u32 = 2;

/// Adds the hook that creates the window restored instead of maximized; returns whether it
/// should be maximized once hidden (set when eframe builds the window).
pub fn defer_maximize(options: &mut eframe::NativeOptions) -> Arc<AtomicBool> {
    let maximize = Arc::new(AtomicBool::new(false));
    if cfg!(windows) {
        let flag = maximize.clone();
        options.window_builder = Some(Box::new(move |mut builder: egui::ViewportBuilder| {
            if builder.maximized == Some(true) {
                flag.store(true, Ordering::Relaxed);
                builder.maximized = None;
            }
            builder
        }));
    }
    maximize
}

/// Reveals the window after its first frames (see the module documentation).
pub struct StartupWindow {
    #[cfg(windows)]
    hwnd: Option<isize>,
    #[cfg(windows)]
    maximize: bool,
    #[cfg(windows)]
    frames: u32,
}

impl StartupWindow {
    /// Cloaks the (still hidden) window.
    pub fn new(cc: &eframe::CreationContext<'_>, maximize: &AtomicBool) -> Self {
        #[cfg(windows)]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let hwnd = match cc.window_handle().map(|h| h.as_raw()) {
                Ok(RawWindowHandle::Win32(h)) => Some(h.hwnd.get()),
                _ => None,
            };
            if let Some(hwnd) = hwnd {
                dwm::set_cloaked(hwnd, true);
            }
            StartupWindow { hwnd, maximize: maximize.load(Ordering::Relaxed), frames: 0 }
        }
        #[cfg(not(windows))]
        {
            let _ = (cc, maximize);
            StartupWindow {}
        }
    }

    /// Called at the start of every frame until it returns `true` (window revealed).
    pub fn on_frame(&mut self, ctx: &egui::Context) -> bool {
        #[cfg(windows)]
        {
            if self.frames == 0 && self.maximize {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
            if self.frames < HIDDEN_FRAMES {
                self.frames += 1;
                ctx.request_repaint();
                return false;
            }
            if let Some(hwnd) = self.hwnd {
                dwm::set_cloaked(hwnd, false);
            }
            true
        }
        #[cfg(not(windows))]
        {
            let _ = ctx;
            true
        }
    }
}

#[cfg(windows)]
mod dwm {
    use std::ffi::c_void;

    const DWMWA_CLOAK: u32 = 13;

    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(hwnd: isize, attribute: u32, value: *const c_void, size: u32) -> i32;
    }

    /// Hides (or shows again) a window without changing its visibility state. Fails silently
    /// (before Windows 8): the window is then shown as before.
    pub fn set_cloaked(hwnd: isize, cloaked: bool) {
        let value: i32 = cloaked.into();
        // SAFETY: valid window handle of this process; `value` outlives the call.
        unsafe {
            DwmSetWindowAttribute(hwnd, DWMWA_CLOAK, (&value as *const i32).cast(), std::mem::size_of::<i32>() as u32);
        }
    }
}
