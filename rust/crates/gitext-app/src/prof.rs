//! Opt-in frame profiling: set `GITEXT_PROFILE=1` to log slow frames with a per-section breakdown.

use std::cell::RefCell;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

thread_local! {
    static SECTIONS: RefCell<Vec<(&'static str, Duration)>> = const { RefCell::new(Vec::new()) };
}

pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("GITEXT_PROFILE").is_some())
}

/// Runs `f` and records its duration under `name` when profiling is enabled.
pub fn scope<R>(name: &'static str, f: impl FnOnce() -> R) -> R {
    if !enabled() {
        return f();
    }
    let start = Instant::now();
    let r = f();
    let d = start.elapsed();
    SECTIONS.with(|s| s.borrow_mut().push((name, d)));
    r
}

/// Prints the frame breakdown if the frame took longer than 16 ms.
pub fn end_frame(total: Duration) {
    if !enabled() {
        return;
    }
    let sections = SECTIONS.with(|s| std::mem::take(&mut *s.borrow_mut()));
    if total > Duration::from_millis(16) {
        let parts: Vec<String> = sections.iter().map(|(n, d)| format!("{n}={:.1}", d.as_secs_f64() * 1000.0)).collect();
        eprintln!("slow frame {:.1} ms: {}", total.as_secs_f64() * 1000.0, parts.join(" "));
    }
}

/// Calls [`end_frame`] when dropped.
pub struct FrameTimer(Instant);

impl FrameTimer {
    pub fn start() -> Self {
        FrameTimer(Instant::now())
    }
}

impl Drop for FrameTimer {
    fn drop(&mut self) {
        end_frame(self.0.elapsed());
    }
}
