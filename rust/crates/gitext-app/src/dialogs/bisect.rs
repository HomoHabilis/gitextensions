//! Port of `FormBisect`.

use egui::Ui;
use gitext_core::commands::{self, GitBisectOption};

use super::{Cx, Dialog, GitRun};
use crate::i18n::{tr, trf};

pub struct BisectDialog;

impl Dialog for BisectDialog {
    fn title(&self) -> String {
        tr("Bisect").into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let bisecting = cx.module.is_some_and(|m| m.state().bisecting);
        let mut keep = true;
        if !bisecting {
            ui.label(tr("Start a bisect session to find the commit that introduced a bug."));
            if ui.button(tr("Start bisect")).clicked() {
                cx.run(GitRun::new(tr("Bisect start"), commands::start_bisect()));
            }
        } else {
            let sel = cx.selected.to_vec();
            ui.label(trf("Mark the selected revision(s) ({0}):", &[&sel.len()]));
            ui.horizontal(|ui| {
                if ui.button(tr("Good")).clicked() {
                    cx.run(GitRun::new(tr("Bisect good"), commands::continue_bisect(GitBisectOption::Good, &sel)).keep_open());
                }
                if ui.button(tr("Bad")).clicked() {
                    cx.run(GitRun::new(tr("Bisect bad"), commands::continue_bisect(GitBisectOption::Bad, &sel)).keep_open());
                }
                if ui.button(tr("Skip")).clicked() {
                    cx.run(GitRun::new(tr("Bisect skip"), commands::continue_bisect(GitBisectOption::Skip, &sel)).keep_open());
                }
            });
            ui.separator();
            if ui.button(tr("Stop bisect")).clicked() {
                cx.run(GitRun::new(tr("Bisect reset"), commands::stop_bisect()));
                keep = false;
            }
        }
        ui.add_space(6.0);
        if ui.button(tr("Close")).clicked() {
            keep = false;
        }
        keep
    }
}
