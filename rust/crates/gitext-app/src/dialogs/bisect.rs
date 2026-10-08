//! Port of `FormBisect`.

use egui::Ui;
use gitext_core::commands::{self, GitBisectOption};

use super::{Cx, Dialog, GitRun};

pub struct BisectDialog;

impl Dialog for BisectDialog {
    fn title(&self) -> String {
        "Bisect".into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let bisecting = cx.module.is_some_and(|m| m.state().bisecting);
        let mut keep = true;
        if !bisecting {
            ui.label("Start a bisect session to find the commit that introduced a bug.");
            if ui.button("Start bisect").clicked() {
                cx.run(GitRun::new("Bisect start", commands::start_bisect()));
            }
        } else {
            let sel = cx.selected.to_vec();
            ui.label(format!("Mark the selected revision(s) ({}):", sel.len()));
            ui.horizontal(|ui| {
                if ui.button("Good").clicked() {
                    cx.run(GitRun::new("Bisect good", commands::continue_bisect(GitBisectOption::Good, &sel)).keep_open());
                }
                if ui.button("Bad").clicked() {
                    cx.run(GitRun::new("Bisect bad", commands::continue_bisect(GitBisectOption::Bad, &sel)).keep_open());
                }
                if ui.button("Skip").clicked() {
                    cx.run(GitRun::new("Bisect skip", commands::continue_bisect(GitBisectOption::Skip, &sel)).keep_open());
                }
            });
            ui.separator();
            if ui.button("Stop bisect").clicked() {
                cx.run(GitRun::new("Bisect reset", commands::stop_bisect()));
                keep = false;
            }
        }
        ui.add_space(6.0);
        if ui.button("Close").clicked() {
            keep = false;
        }
        keep
    }
}
