//! Ports of `FormResetCurrentBranch` and `FormResetChanges`.

use egui::{RichText, Ui};
use gitext_core::commands::{self, ResetMode};
use gitext_core::status::StagedStatus;
use gitext_core::{GitArgs, ObjectId};

use super::{ok_cancel, Action, Cx, Dialog, GitRun};
use crate::repo::RepoData;
use crate::theme::Palette;

pub struct ResetBranchDialog {
    id: ObjectId,
    branch: String,
    mode: ResetMode,
}

impl ResetBranchDialog {
    pub fn new(data: &RepoData, id: ObjectId) -> Self {
        ResetBranchDialog { id, branch: data.current_branch.clone().unwrap_or_else(|| "HEAD".into()), mode: ResetMode::Mixed }
    }
}

impl Dialog for ResetBranchDialog {
    fn title(&self) -> String {
        "Reset current branch".into()
    }

    fn kind(&self) -> super::DialogKind {
        super::DialogKind::Modal(520.0)
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let palette = Palette::for_ui(ui);
        ui.label(format!("Reset branch '{}' to {}", self.branch, self.id.to_short_string()));
        ui.add_space(4.0);
        ui.radio_value(&mut self.mode, ResetMode::Soft, "Soft: leave working directory and index untouched");
        ui.radio_value(&mut self.mode, ResetMode::Mixed, "Mixed: leave working directory untouched, reset index");
        ui.radio_value(&mut self.mode, ResetMode::Keep, "Keep: update working directory to the commit, keep local changes");
        ui.radio_value(&mut self.mode, ResetMode::Merge, "Merge: reset index, update files that differ, keep local changes");
        ui.radio_value(&mut self.mode, ResetMode::Hard, "Hard: reset working directory and index (discard ALL local changes)");
        if self.mode == ResetMode::Hard {
            ui.label(RichText::new("⚠ All uncommitted changes will be lost!").color(palette.error));
        }
        let (ok, cancel) = ok_cancel(ui, "Reset", true);
        if ok {
            if let Ok(args) = commands::reset(self.mode, Some(&self.id.to_string()), None, false) {
                cx.run(GitRun::new("Reset current branch", args));
            }
            return false;
        }
        !cancel
    }
}

pub struct ResetChangesDialog {
    files: Option<Vec<String>>,
    include_untracked: bool,
}

impl ResetChangesDialog {
    /// Reset all changes of the working directory.
    pub fn all() -> Self {
        ResetChangesDialog { files: None, include_untracked: false }
    }

    /// Reset changes of the given files.
    pub fn files(files: Vec<String>) -> Self {
        ResetChangesDialog { files: Some(files), include_untracked: true }
    }
}

impl Dialog for ResetChangesDialog {
    fn title(&self) -> String {
        "Reset changes".into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let palette = Palette::for_ui(ui);
        match &self.files {
            None => ui.label("Reset all changes in the working directory and the index?"),
            Some(f) => ui.label(format!("Reset changes of {} file(s)?\n{}", f.len(), f.iter().take(10).cloned().collect::<Vec<_>>().join("\n"))),
        };
        ui.checkbox(&mut self.include_untracked, "Also delete new (untracked) files");
        ui.label(RichText::new("⚠ This cannot be undone.").color(palette.error));
        let (ok, cancel) = ok_cancel(ui, "Reset", true);
        if ok {
            let Some(m) = cx.module else { return false };
            match &self.files {
                None => {
                    let mut cmds = vec![commands::reset(ResetMode::Hard, Some("HEAD"), None, false).unwrap()];
                    if self.include_untracked {
                        cmds.push(commands::clean(commands::CleanMode::OnlyNonIgnored, false, true, &[], &[]));
                    }
                    cx.run(GitRun::many("Reset changes", cmds));
                }
                Some(files) => {
                    let status = m.get_status(commands::UntrackedFilesMode::All, false).unwrap_or_default();
                    let (untracked, tracked): (Vec<&String>, Vec<&String>) =
                        files.iter().partition(|f| status.iter().any(|s| &s.name == *f && s.is_new && s.staged == StagedStatus::WorkTree && !s.is_tracked));
                    let added: Vec<&str> = files
                        .iter()
                        .filter(|f| status.iter().any(|s| &s.name == *f && s.is_new && s.staged == StagedStatus::Index))
                        .map(String::as_str)
                        .collect();
                    if !added.is_empty() {
                        let _ = m.unstage_files(&added);
                    }
                    let tracked: Vec<&str> = tracked.iter().map(|s| s.as_str()).filter(|f| !added.contains(f)).collect();
                    if !tracked.is_empty() {
                        let args = GitArgs::new("checkout").arg("HEAD").arg("--").args(tracked.iter().copied());
                        if let Err(e) = m.run_checked(&args) {
                            // file might not exist in HEAD; fall back to the index
                            if m.reset_files(&tracked).is_err() {
                                cx.error("Reset changes", e.to_string());
                            }
                        }
                    }
                    if self.include_untracked {
                        let refs: Vec<&str> = untracked.iter().map(|s| s.as_str()).chain(added.iter().copied()).collect();
                        if let Err(e) = m.delete_untracked(&refs) {
                            cx.error("Reset changes", e.to_string());
                        }
                    }
                    cx.push(Action::Refresh);
                }
            }
            return false;
        }
        !cancel
    }
}
