//! Port of `FormProcess` / `FormRemoteProcess`: runs git commands, streaming the output.

use std::time::Instant;

use egui::{RichText, Ui, Vec2};
use gitext_core::exec::{Executable, OutputEvent, RunningProcess};

use super::{Action, Cx, Dialog, DialogKind, GitRun};
use crate::theme::Palette;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

pub struct ProcessDialog {
    run: GitRun,
    index: usize,
    process: Option<RunningProcess>,
    output: String,
    progress: Option<String>,
    state: State,
    started: Instant,
    id: u64,
    then_done: bool,
}

impl ProcessDialog {
    pub fn new(run: GitRun) -> Self {
        ProcessDialog {
            run,
            index: 0,
            process: None,
            output: String::new(),
            progress: None,
            state: State::Running,
            started: Instant::now(),
            id: fastrand_id(),
            then_done: false,
        }
    }

    fn start_next(&mut self, cx: &Cx) {
        let Some(args) = self.run.commands.get(self.index) else {
            self.state = State::Succeeded;
            return;
        };
        let dir = self
            .run
            .working_dir
            .clone()
            .or_else(|| cx.module.map(|m| m.work_dir().to_path_buf()))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        self.output.push_str(&format!("$ git {args}\n"));
        match Executable::git(dir).spawn_streaming(args) {
            Ok(p) => self.process = Some(p),
            Err(e) => {
                self.output.push_str(&format!("{e}\n"));
                self.state = State::Failed;
            }
        }
    }

    fn poll(&mut self, cx: &Cx) {
        if self.state != State::Running {
            return;
        }
        if self.process.is_none() {
            self.start_next(cx);
        }
        let Some(p) = &self.process else { return };
        let mut exited = None;
        while let Ok(ev) = p.events.try_recv() {
            match ev {
                OutputEvent::Stdout(l) | OutputEvent::Stderr(l) => {
                    if let Some(prog) = self.progress.take() {
                        self.output.push_str(&prog);
                        self.output.push('\n');
                    }
                    self.output.push_str(&l);
                    self.output.push('\n');
                }
                OutputEvent::Progress(l) => self.progress = Some(l),
                OutputEvent::Exited(code) => exited = Some(code),
            }
        }
        if let Some(code) = exited {
            if let Some(prog) = self.progress.take() {
                self.output.push_str(&prog);
                self.output.push('\n');
            }
            let cancelled = p.is_cancelled();
            self.process = None;
            if cancelled {
                self.state = State::Cancelled;
                self.output.push_str("\nAborted.\n");
            } else if code != 0 {
                self.state = State::Failed;
                self.output.push_str(&format!("\nDone (exit code {code}) after {:.1}s\n", self.started.elapsed().as_secs_f32()));
            } else {
                self.index += 1;
                if self.index >= self.run.commands.len() {
                    self.state = State::Succeeded;
                    self.output.push_str(&format!("\nDone after {:.1}s\n", self.started.elapsed().as_secs_f32()));
                }
            }
        } else {
            cx.ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    fn has_conflicts(&self) -> bool {
        let o = self.output.to_lowercase();
        o.contains("conflict") || o.contains("unmerged") || o.contains("could not apply")
    }
}

fn fastrand_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

impl Dialog for ProcessDialog {
    fn title(&self) -> String {
        self.run.title.clone()
    }

    fn id(&self) -> String {
        format!("process-{}", self.id)
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(680.0, 380.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        self.poll(cx);
        let palette = Palette::for_ui(ui);
        let mut keep = true;

        if self.state != State::Running && !self.then_done {
            self.then_done = true;
            if self.run.refresh {
                cx.push(Action::Refresh);
            }
            if self.state == State::Succeeded {
                for a in self.run.then.drain(..) {
                    cx.push(a);
                }
                if cx.settings.close_process_dialog_on_success && !self.run.keep_open {
                    return false;
                }
            }
        }

        ui.horizontal(|ui| {
            match self.state {
                State::Running => {
                    ui.spinner();
                    ui.label(RichText::new(self.progress.clone().unwrap_or_else(|| "Running…".into())).monospace());
                }
                State::Succeeded => {
                    ui.label(RichText::new("✔ Done").color(palette.success).strong());
                }
                State::Failed => {
                    ui.label(RichText::new("✖ Failed").color(palette.error).strong());
                }
                State::Cancelled => {
                    ui.label(RichText::new("Aborted").color(palette.warning).strong());
                }
            }
        });
        ui.separator();
        let avail = ui.available_height() - 40.0;
        egui::ScrollArea::vertical().max_height(avail.max(80.0)).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(&self.output).monospace()).wrap());
        });
        ui.separator();
        ui.horizontal(|ui| {
            if self.state == State::Failed && self.run.check_conflicts && self.has_conflicts() && ui.button("Solve conflicts…").clicked() {
                cx.open(super::conflicts::ConflictsDialog::default());
                keep = false;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.state == State::Running {
                    if ui.button("Abort").clicked() {
                        if let Some(p) = &self.process {
                            p.cancel();
                        }
                    }
                } else if ui.button("Close").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    keep = false;
                }
                if ui.button("Copy output").clicked() {
                    crate::util::copy_to_clipboard(cx.ctx, self.output.clone());
                }
            });
        });
        keep
    }
}
