//! Port of `FormCleanupRepository`.

use egui::{RichText, Ui};
use gitext_core::commands::{self, CleanMode};

use super::{ok_cancel, Cx, Dialog, GitRun};
use crate::tasks::Task;

#[derive(Default)]
pub struct CleanupDialog {
    mode: Option<CleanMode>,
    directories: bool,
    paths: String,
    excludes: String,
    submodules: bool,
    preview: Option<Task<String>>,
    preview_text: String,
}

impl Dialog for CleanupDialog {
    fn title(&self) -> String {
        "Clean working directory".into()
    }

    fn kind(&self) -> super::DialogKind {
        super::DialogKind::Modal(560.0)
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let mode = self.mode.get_or_insert(CleanMode::OnlyNonIgnored);
        ui.radio_value(mode, CleanMode::OnlyNonIgnored, "Remove untracked files that are not ignored");
        ui.radio_value(mode, CleanMode::OnlyIgnored, "Remove only ignored files");
        ui.radio_value(mode, CleanMode::All, "Remove untracked and ignored files");
        ui.checkbox(&mut self.directories, "Remove untracked directories too");
        ui.checkbox(&mut self.submodules, "Also clean submodules");
        ui.horizontal(|ui| {
            ui.label("Only paths");
            ui.add(egui::TextEdit::singleline(&mut self.paths).hint_text("space separated").desired_width(360.0));
        });
        ui.horizontal(|ui| {
            ui.label("Exclude");
            ui.add(egui::TextEdit::singleline(&mut self.excludes).hint_text("patterns").desired_width(370.0));
        });
        let mode = *mode;
        let paths: Vec<String> = self.paths.split_whitespace().map(str::to_string).collect();
        let excludes: Vec<String> = self.excludes.split_whitespace().map(|e| format!("--exclude={e}")).collect();
        let pr: Vec<&str> = paths.iter().map(String::as_str).collect();
        let er: Vec<&str> = excludes.iter().map(String::as_str).collect();
        if let Some(t) = &mut self.preview {
            if let Some(r) = t.try_take() {
                self.preview_text = r;
                self.preview = None;
            }
        }
        if ui.button("Preview").clicked() {
            if let Some(m) = cx.module {
                let m = m.clone();
                let args = commands::clean(mode, true, self.directories, &pr, &er);
                self.preview = Some(Task::spawn(cx.ctx, move || m.run(&args).map(|r| r.all_output()).unwrap_or_else(|e| e.to_string())));
            }
        }
        if !self.preview_text.is_empty() {
            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                ui.label(RichText::new(&self.preview_text).monospace());
            });
        }
        let (ok, cancel) = ok_cancel(ui, "Clean", true);
        if ok {
            let mut cmds = vec![commands::clean(mode, false, self.directories, &pr, &er)];
            if self.submodules {
                cmds.push(commands::clean_submodules(mode, false, self.directories, &pr));
            }
            cx.run(GitRun::many("Clean", cmds).keep_open());
            return false;
        }
        !cancel
    }
}
