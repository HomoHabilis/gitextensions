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

/// Port of `FormResetAnotherBranch`: moves a local branch (not the current one) to a commit
/// with `git update-ref`, only fast-forward unless forced.
pub struct ResetAnotherBranchDialog {
    id: ObjectId,
    subject: String,
    /// (name, complete name, commit) of the local branches that can be reset.
    branches: Vec<(String, String, ObjectId)>,
    branch: String,
    force: bool,
    /// Whether the chosen branch is an ancestor of the commit (checked in the background).
    fast_forward: crate::tasks::Loader<String, bool>,
}

impl ResetAnotherBranchDialog {
    pub fn new(data: &RepoData, rev: &gitext_core::GitRevision) -> Self {
        let current = data.current_branch.as_deref();
        // local branches tracking a remote branch at the commit first, as upstream
        let remotes: Vec<&gitext_core::GitRef> = rev.refs.iter().filter(|r| r.is_remote()).collect();
        let tracks = |b: &gitext_core::GitRef| remotes.iter().any(|r| r.name == format!("{}/{}", b.tracking_remote, b.merge_with) || r.name.split_once('/').is_some_and(|(_, n)| n == b.name));
        let mut locals: Vec<&gitext_core::GitRef> = data.local_branches().filter(|b| Some(b.name.as_str()) != current && b.object_id != rev.object_id).collect();
        locals.sort_by_key(|b| !tracks(b));
        let candidates: Vec<&&gitext_core::GitRef> = locals.iter().filter(|b| tracks(b)).collect();
        let branch = if remotes.len() == 1 && candidates.len() == 1 { candidates[0].name.clone() } else { String::new() };
        ResetAnotherBranchDialog {
            id: rev.object_id,
            subject: rev.subject.clone(),
            branches: locals.iter().map(|b| (b.name.clone(), b.complete_name.clone(), b.object_id)).collect(),
            branch,
            force: false,
            fast_forward: Default::default(),
        }
    }
}

impl Dialog for ResetAnotherBranchDialog {
    fn title(&self) -> String {
        "Reset branch".into()
    }

    fn kind(&self) -> super::DialogKind {
        super::DialogKind::Modal(560.0)
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let palette = Palette::for_ui(ui);
        ui.label(format!("Reset to {}  {}", self.id.to_short_string(), self.subject));
        ui.add_space(4.0);
        let names: Vec<String> = self.branches.iter().map(|(n, ..)| n.clone()).collect();
        ui.horizontal(|ui| {
            ui.label("Reset local branch:");
            super::branch_combo(ui, "reset_another_branch", &mut self.branch, &names, 300.0);
        });
        let selected = self.branches.iter().find(|(n, ..)| *n == self.branch.trim()).cloned();
        let fast_forward = match (&selected, cx.module) {
            (Some((name, _, at)), Some(m)) => {
                let (m, id, at) = (m.clone(), self.id, *at);
                self.fast_forward.request(cx.ctx, name.clone(), move || m.is_ancestor(at, id)).copied()
            }
            _ => None,
        };
        ui.checkbox(&mut self.force, "Force reset for a non-fast-forward reset");
        ui.label(
            RichText::new("You can only reset a branch safely if there is a direct path from it to the selected revision.\nForcing a branch to reset if it has not been merged might leave some commits unreachable.")
                .small()
                .color(palette.muted),
        );
        if selected.is_none() && !self.branch.trim().is_empty() {
            ui.label(RichText::new(format!("'{}' is not the name of an existing local branch.", self.branch.trim())).color(palette.error));
        } else if fast_forward == Some(false) && !self.force {
            ui.label(RichText::new("This is not a fast-forward: check \"Force\" to reset anyway.").color(palette.warning));
        }
        ui.checkbox(&mut cx.settings.checkout_other_branch_after_reset, "Checkout branch after reset");
        let enabled = selected.is_some() && (self.force || fast_forward == Some(true));
        let (ok, cancel) = ok_cancel(ui, "Reset", enabled);
        if ok {
            let Some((name, complete, _)) = selected else { return true };
            let mut run = GitRun::new(format!("Reset {name}"), commands::update_ref(&complete, self.id));
            if cx.settings.checkout_other_branch_after_reset {
                if let Some(d) = cx.data {
                    run = run.then(Action::OpenDialog(Box::new(super::checkout::CheckoutBranchDialog::new(d, Some(name), false))));
                }
            }
            cx.push(Action::SaveSettings);
            cx.run(run);
            return false;
        }
        !cancel
    }
}
