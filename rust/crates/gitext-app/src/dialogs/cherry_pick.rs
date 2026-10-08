//! Ports of `FormCherryPick` and `FormRevertCommit`.

use egui::{RichText, Ui};
use gitext_core::{commands, GitRevision};

use super::{ok_cancel, Cx, Dialog, GitRun};

pub struct CherryPickDialog {
    revision: Option<GitRevision>,
    revision_text: String,
    revert: bool,
    commit: bool,
    append_source: bool,
    parent: u32,
}

impl CherryPickDialog {
    pub fn new(revision: Option<GitRevision>, revert: bool) -> Self {
        let text = revision.as_ref().map(|r| r.guid()).unwrap_or_default();
        CherryPickDialog { revision, revision_text: text, revert, commit: true, append_source: false, parent: 1 }
    }
}

impl Dialog for CherryPickDialog {
    fn title(&self) -> String {
        if self.revert { "Revert commit".into() } else { "Cherry pick commit".into() }
    }

    fn kind(&self) -> super::DialogKind {
        super::DialogKind::Modal(480.0)
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.label("Commit:");
        ui.add(egui::TextEdit::singleline(&mut self.revision_text).font(egui::TextStyle::Monospace).desired_width(440.0));
        if let Some(r) = &self.revision {
            if r.guid() == self.revision_text {
                ui.label(RichText::new(format!("{} — {}", r.author, r.subject)).italics());
            }
        }
        let parents = self.revision.as_ref().map(|r| r.parents().len()).unwrap_or(0);
        if parents > 1 {
            ui.horizontal(|ui| {
                ui.label("This is a merge commit. Mainline parent:");
                ui.add(egui::DragValue::new(&mut self.parent).range(1..=parents as u32));
            });
        }
        ui.checkbox(&mut self.commit, "Automatically create a commit");
        if !self.revert {
            ui.checkbox(&mut self.append_source, "Add commit reference to message (-x)");
        }
        let (ok, cancel) = ok_cancel(ui, if self.revert { "Revert" } else { "Cherry pick" }, !self.revision_text.trim().is_empty());
        if ok {
            let Some(m) = cx.module else { return false };
            let id = m.rev_parse(self.revision_text.trim());
            if id.is_zero() {
                cx.error(&self.title(), "Revision not found");
                return true;
            }
            let parent = if parents > 1 { self.parent } else { 0 };
            let args = if self.revert {
                commands::revert(id, self.commit, parent)
            } else {
                let mut extra: Vec<String> = Vec::new();
                if self.append_source {
                    extra.push("-x".into());
                }
                if parent > 0 {
                    extra.push("-m".into());
                    extra.push(parent.to_string());
                }
                let extra: Vec<&str> = extra.iter().map(String::as_str).collect();
                commands::cherry_pick(id, self.commit, &extra)
            };
            cx.run(GitRun::new(self.title(), args).conflicts());
            return false;
        }
        !cancel
    }
}

