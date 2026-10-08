//! Port of `FormMergeBranch`.

use egui::Ui;
use gitext_core::commands::{self, MergeOptions};

use super::{branch_combo, ok_cancel, Cx, Dialog, GitRun};
use crate::repo::RepoData;

pub struct MergeDialog {
    branch: String,
    items: Vec<String>,
    fast_forward: bool,
    squash: bool,
    no_commit: bool,
    allow_unrelated: bool,
    strategy: String,
    add_log: bool,
    log_count: i32,
    custom_message: bool,
    message: String,
}

impl MergeDialog {
    pub fn new(data: &RepoData, branch: &str) -> Self {
        MergeDialog {
            branch: branch.to_string(),
            items: data.branch_names(true),
            fast_forward: true,
            squash: false,
            no_commit: false,
            allow_unrelated: false,
            strategy: String::new(),
            add_log: false,
            log_count: 20,
            custom_message: false,
            message: String::new(),
        }
    }
}

impl Dialog for MergeDialog {
    fn title(&self) -> String {
        "Merge branches".into()
    }

    fn kind(&self) -> super::DialogKind {
        super::DialogKind::Modal(480.0)
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let current = cx.current_branch().unwrap_or_else(|| "HEAD".into());
        ui.label(format!("Merge into current branch: {current}"));
        ui.label("Merge with:");
        branch_combo(ui, "merge_branch", &mut self.branch, &self.items, 440.0);
        ui.add_space(4.0);
        ui.radio_value(&mut self.fast_forward, true, "Keep a single branch line if possible (fast forward)");
        ui.radio_value(&mut self.fast_forward, false, "Always create a new merge commit (--no-ff)");
        ui.checkbox(&mut self.squash, "Squash commits");
        ui.checkbox(&mut self.no_commit, "Do not commit");
        ui.checkbox(&mut self.allow_unrelated, "Allow unrelated histories");
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.add_log, "Add log messages");
            ui.add_enabled(self.add_log, egui::DragValue::new(&mut self.log_count).range(1..=1000));
        });
        ui.horizontal(|ui| {
            ui.label("Strategy");
            egui::ComboBox::from_id_salt("merge_strategy").selected_text(if self.strategy.is_empty() { "default" } else { &self.strategy }).show_ui(ui, |ui| {
                for s in ["", "ort", "recursive", "resolve", "octopus", "ours", "subtree"] {
                    ui.selectable_value(&mut self.strategy, s.to_string(), if s.is_empty() { "default" } else { s });
                }
            });
        });
        ui.checkbox(&mut self.custom_message, "Specify merge commit message");
        if self.custom_message {
            ui.add(egui::TextEdit::multiline(&mut self.message).desired_rows(3).desired_width(f32::INFINITY));
        }
        let (ok, cancel) = ok_cancel(ui, "Merge", !self.branch.trim().is_empty());
        if ok {
            let msg_file = if self.custom_message && !self.message.trim().is_empty() {
                cx.module.map(|m| {
                    let p = m.git_dir().join("GitExtensions.MergeMessage");
                    let _ = std::fs::write(&p, &self.message);
                    p.display().to_string()
                })
            } else {
                None
            };
            let o = MergeOptions {
                allow_fast_forward: self.fast_forward,
                squash: self.squash,
                no_commit: self.no_commit,
                strategy: self.strategy.clone(),
                allow_unrelated_histories: self.allow_unrelated,
                merge_commit_file_path: msg_file,
                log: self.add_log.then_some(self.log_count),
            };
            cx.run(GitRun::new(format!("Merge {}", self.branch.trim()), commands::merge_branch(self.branch.trim(), &o)).conflicts());
            return false;
        }
        !cancel
    }
}
