//! Background work: replaces the `JoinableTaskFactory` / `AsyncLoader` patterns of the C# UI.
//! Work runs on a thread and the UI polls the result each frame.

use std::sync::{Arc, Mutex};

/// The result of a computation running on a background thread.
pub struct Task<T> {
    slot: Arc<Mutex<Option<T>>>,
    /// Start time and duration of the work (profiling).
    started: std::time::Instant,
    worked: Arc<Mutex<Option<std::time::Duration>>>,
}

impl<T: Send + 'static> Task<T> {
    /// Runs `f` on a new thread and requests a repaint when done.
    pub fn spawn(ctx: &egui::Context, f: impl FnOnce() -> T + Send + 'static) -> Self {
        let slot = Arc::new(Mutex::new(None));
        let s = Arc::clone(&slot);
        let worked = Arc::new(Mutex::new(None));
        let w = Arc::clone(&worked);
        let ctx = ctx.clone();
        let started = std::time::Instant::now();
        std::thread::spawn(move || {
            let r = f();
            *w.lock().unwrap() = Some(started.elapsed());
            *s.lock().unwrap() = Some(r);
            ctx.request_repaint();
        });
        Task { slot, started, worked }
    }

    /// Takes the result if it is ready.
    pub fn try_take(&mut self) -> Option<T> {
        self.slot.lock().unwrap().take()
    }
}

/// A value loaded in the background, keyed by the request that produced it, so that
/// stale results (for a previous selection) are discarded.
pub struct Loader<K: PartialEq + Clone, T> {
    pub key: Option<K>,
    task: Option<(K, Task<T>)>,
    pub value: Option<T>,
}

impl<K: PartialEq + Clone, T: Send + 'static> Default for Loader<K, T> {
    fn default() -> Self {
        Loader { key: None, task: None, value: None }
    }
}

impl<K: PartialEq + Clone, T: Send + 'static> Loader<K, T> {
    /// Ensures the value for `key` is (being) loaded; returns the current value if it matches.
    pub fn request(&mut self, ctx: &egui::Context, key: K, f: impl FnOnce() -> T + Send + 'static) -> Option<&T> {
        self.poll();
        let loading_this = self.task.as_ref().is_some_and(|(k, _)| *k == key);
        if self.key.as_ref() != Some(&key) && !loading_this {
            self.task = Some((key.clone(), Task::spawn(ctx, f)));
        }
        if self.key.as_ref() == Some(&key) {
            self.value.as_ref()
        } else {
            None
        }
    }

    pub fn poll(&mut self) {
        if let Some((k, t)) = self.task.as_mut() {
            if let Some(v) = t.try_take() {
                if crate::prof::enabled() {
                    let worked = t.worked.lock().unwrap().unwrap_or_default();
                    eprintln!("load {}: work {:.1} ms, shown after {:.1} ms", std::any::type_name::<T>(), worked.as_secs_f64() * 1000.0, t.started.elapsed().as_secs_f64() * 1000.0);
                }
                self.key = Some(k.clone());
                self.value = Some(v);
                self.task = None;
            }
        }
    }

    /// Forgets the cached value so the next request reloads.
    pub fn invalidate(&mut self) {
        self.key = None;
        self.value = None;
    }
}
