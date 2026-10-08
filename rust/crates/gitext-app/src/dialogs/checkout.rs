//! Ports of `FormCheckoutBranch` and `FormCheckoutRevision`.

use egui::Ui;
use gitext_core::commands::{self, CheckoutNewBranchMode, LocalChangesAction};
use gitext_core::{GitArgs, ObjectId};

use super::{branch_combo, ok_cancel, Cx, Dialog, GitRun};
use crate::repo::RepoData;

pub struct CheckoutBranchDialog {
    branch: String,
    remote: bool,
    locals: Vec<String>,
    remotes: Vec<String>,
    local_changes: LocalChangesAction,
    new_branch_mode: CheckoutNewBranchMode,
    new_branch_name: String,
    has_changes: bool,
}

impl CheckoutBranchDialog {
    pub fn new(data: &RepoData, preselect: Option<String>, remote: bool) -> Self {
        let locals: Vec<String> = data.local_branches().map(|r| r.name.clone()).collect();
        let remotes: Vec<String> = data.remote_branches().map(|r| r.name.clone()).collect();
        let branch = preselect.unwrap_or_default();
        let new_name = if remote { local_name(&branch, data) } else { String::new() };
        let exists = locals.contains(&new_name);
        CheckoutBranchDialog {
            branch,
            remote,
            locals,
            remotes,
            local_changes: LocalChangesAction::DontChange,
            new_branch_mode: if remote { if exists { CheckoutNewBranchMode::Reset } else { CheckoutNewBranchMode::Create } } else { CheckoutNewBranchMode::DontCreate },
            new_branch_name: new_name,
            has_changes: data.status.iter().any(|s| s.is_tracked),
        }
    }
}

fn local_name(remote_branch: &str, data: &RepoData) -> String {
    for r in &data.remotes {
        if let Some(n) = remote_branch.strip_prefix(&format!("{}/", r.name)) {
            return n.to_string();
        }
    }
    remote_branch.split_once('/').map(|(_, b)| b.to_string()).unwrap_or_else(|| remote_branch.to_string())
}

impl Dialog for CheckoutBranchDialog {
    fn title(&self) -> String {
        "Checkout branch".into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.remote, false, "Local branch");
            ui.radio_value(&mut self.remote, true, "Remote branch");
        });
        let items = if self.remote { self.remotes.clone() } else { self.locals.clone() };
        let before = self.branch.clone();
        branch_combo(ui, "checkout_branch", &mut self.branch, &items, 380.0);
        if self.remote && before != self.branch {
            self.new_branch_name = cx.data.map(|d| local_name(&self.branch, d)).unwrap_or_default();
            self.new_branch_mode = if self.locals.contains(&self.new_branch_name) { CheckoutNewBranchMode::Reset } else { CheckoutNewBranchMode::Create };
        }
        if self.remote {
            ui.add_space(4.0);
            ui.radio_value(&mut self.new_branch_mode, CheckoutNewBranchMode::Create, "Create local branch with the name:");
            ui.add_enabled(self.new_branch_mode != CheckoutNewBranchMode::DontCreate, egui::TextEdit::singleline(&mut self.new_branch_name).desired_width(380.0));
            ui.radio_value(&mut self.new_branch_mode, CheckoutNewBranchMode::Reset, "Reset local branch with the same name (-B)");
            ui.radio_value(&mut self.new_branch_mode, CheckoutNewBranchMode::DontCreate, "Checkout remote branch (detached HEAD)");
        }
        ui.add_space(4.0);
        ui.label(if self.has_changes { "Local changes:" } else { "Local changes (none):" });
        ui.horizontal_wrapped(|ui| {
            ui.radio_value(&mut self.local_changes, LocalChangesAction::DontChange, "Don't change");
            ui.radio_value(&mut self.local_changes, LocalChangesAction::Merge, "Merge");
            ui.radio_value(&mut self.local_changes, LocalChangesAction::Stash, "Stash");
            ui.radio_value(&mut self.local_changes, LocalChangesAction::Reset, "Reset");
        });
        let (ok, cancel) = ok_cancel(ui, "Checkout", !self.branch.trim().is_empty());
        if ok {
            let branch = self.branch.trim().to_string();
            let mut cmds = Vec::new();
            if self.local_changes == LocalChangesAction::Stash && self.has_changes {
                cmds.push(commands::stash_save(false, false, Some(&format!("Auto stash before checkout of {branch}")), &[]));
            }
            cmds.push(commands::checkout_branch(&branch, self.remote, self.local_changes, self.new_branch_mode, Some(self.new_branch_name.trim())));
            if self.local_changes == LocalChangesAction::Stash && self.has_changes {
                cmds.push(GitArgs::new("stash").arg("pop"));
            }
            let mut run = GitRun::many(format!("Checkout {branch}"), cmds).conflicts();
            if cx.settings.update_submodules_on_checkout {
                run.commands.push(commands::submodule_update(&[], false));
            }
            cx.run(run);
            return false;
        }
        !cancel
    }
}

pub struct CheckoutRevisionDialog {
    revision: String,
    force: bool,
}

impl CheckoutRevisionDialog {
    pub fn new(id: ObjectId) -> Self {
        CheckoutRevisionDialog { revision: if id.is_zero() { String::new() } else { id.to_string() }, force: false }
    }
}

impl Dialog for CheckoutRevisionDialog {
    fn title(&self) -> String {
        "Checkout revision".into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.label("Revision to checkout (detached HEAD):");
        ui.add(egui::TextEdit::singleline(&mut self.revision).desired_width(400.0).font(egui::TextStyle::Monospace));
        ui.checkbox(&mut self.force, "Force (discard local changes)");
        let (ok, cancel) = ok_cancel(ui, "Checkout", !self.revision.trim().is_empty());
        if ok {
            let action = if self.force { LocalChangesAction::Reset } else { LocalChangesAction::DontChange };
            cx.run(GitRun::new("Checkout revision", commands::checkout(self.revision.trim(), action)));
            return false;
        }
        !cancel
    }
}
