//! Ports of `FormCheckoutBranch` and `FormCheckoutRevision`.

use egui::Ui;
use gitext_core::commands::{self, CheckoutNewBranchMode, LocalChangesAction};
use gitext_core::{GitArgs, ObjectId};

use super::{branch_combo, ok_cancel, Cx, Dialog, GitRun};
use crate::i18n::{tr, trf};
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
    /// The local-changes choice was set from the settings.
    initialized: bool,
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
            initialized: false,
        }
    }
}

/// The commands of a checkout: `checkout` wrapped in a stash and stash pop when the local
/// changes are stashed, then the submodule update when the settings ask for it.
pub fn checkout_run(branch: &str, checkout: GitArgs, local_changes: LocalChangesAction, has_changes: bool, update_submodules: bool) -> GitRun {
    let stash = local_changes == LocalChangesAction::Stash && has_changes;
    let mut cmds = Vec::new();
    if stash {
        cmds.push(commands::stash_save(false, false, Some(&format!("Auto stash before checkout of {branch}")), &[]));
    }
    cmds.push(checkout);
    if stash {
        cmds.push(GitArgs::new("stash").arg("pop"));
    }
    if update_submodules {
        cmds.push(commands::submodule_update(&[], false));
    }
    GitRun::many(trf("Checkout {0}", &[&branch]), cmds).conflicts()
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
        tr("Checkout branch").into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        if !self.initialized {
            self.initialized = true;
            self.local_changes = cx.settings.default_local_changes_action;
        }
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.remote, false, tr("Local branch"));
            ui.radio_value(&mut self.remote, true, tr("Remote branch"));
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
            ui.radio_value(&mut self.new_branch_mode, CheckoutNewBranchMode::Create, tr("Create local branch with the name:"));
            ui.add_enabled(self.new_branch_mode != CheckoutNewBranchMode::DontCreate, egui::TextEdit::singleline(&mut self.new_branch_name).desired_width(380.0));
            ui.radio_value(&mut self.new_branch_mode, CheckoutNewBranchMode::Reset, tr("Reset local branch with the same name (-B)"));
            ui.radio_value(&mut self.new_branch_mode, CheckoutNewBranchMode::DontCreate, tr("Checkout remote branch (detached HEAD)"));
        }
        ui.add_space(4.0);
        ui.label(if self.has_changes { tr("Local changes:") } else { tr("Local changes (none):") });
        ui.horizontal_wrapped(|ui| {
            ui.radio_value(&mut self.local_changes, LocalChangesAction::DontChange, tr("Don't change"));
            ui.radio_value(&mut self.local_changes, LocalChangesAction::Merge, tr("Merge"));
            ui.radio_value(&mut self.local_changes, LocalChangesAction::Stash, tr("Stash"));
            ui.radio_value(&mut self.local_changes, LocalChangesAction::Reset, tr("Reset"));
        });
        let (ok, cancel) = ok_cancel(ui, tr("Checkout"), !self.branch.trim().is_empty());
        if ok {
            let branch = self.branch.trim().to_string();
            let checkout = commands::checkout_branch(&branch, self.remote, self.local_changes, self.new_branch_mode, Some(self.new_branch_name.trim()));
            cx.run(checkout_run(&branch, checkout, self.local_changes, self.has_changes, cx.settings.update_submodules_on_checkout));
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
        tr("Checkout revision").into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.label(tr("Revision to checkout (detached HEAD):"));
        ui.add(egui::TextEdit::singleline(&mut self.revision).desired_width(400.0).font(egui::TextStyle::Monospace));
        ui.checkbox(&mut self.force, tr("Force (discard local changes)"));
        let (ok, cancel) = ok_cancel(ui, tr("Checkout"), !self.revision.trim().is_empty());
        if ok {
            let action = if self.force { LocalChangesAction::Reset } else { LocalChangesAction::DontChange };
            cx.run(GitRun::new(tr("Checkout revision"), commands::checkout(self.revision.trim(), action)));
            return false;
        }
        !cancel
    }
}
