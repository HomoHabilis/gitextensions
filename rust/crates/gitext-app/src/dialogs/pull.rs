//! Port of `FormPull`: pull (merge / rebase) or fetch.

use egui::Ui;
use gitext_core::settings::PullAction;
use gitext_core::GitArgs;

use super::{branch_combo, ok_cancel, Cx, Dialog, GitRun};
use crate::repo::RepoData;

pub struct PullDialog {
    remote: String,
    remotes: Vec<String>,
    remote_branch: String,
    action: PullAction,
    auto_stash: bool,
    prune: bool,
    tags: Option<bool>,
    unshallow: bool,
    /// Run directly without showing the dialog (toolbar quick actions).
    quick: Option<PullAction>,
    initialized: bool,
}

impl PullDialog {
    pub fn new(data: &RepoData, quick: Option<PullAction>) -> Self {
        let mut remotes = vec!["[ All ]".to_string()];
        remotes.extend(data.remote_names());
        PullDialog {
            remote: data.current_remote().unwrap_or_default(),
            remotes,
            remote_branch: String::new(),
            action: PullAction::Merge,
            auto_stash: false,
            prune: false,
            tags: None,
            unshallow: false,
            quick,
            initialized: false,
        }
    }

    fn command(&self, cx: &Cx) -> Option<(String, GitArgs)> {
        let m = cx.module?;
        let remote = if self.remote.starts_with("[ All") { "" } else { self.remote.trim() };
        Some(match self.action {
            PullAction::Merge | PullAction::Rebase => {
                (format!("Pull {}", remote), m.pull_args(remote, self.remote_branch.trim(), self.action == PullAction::Rebase, self.tags, self.auto_stash, self.prune))
            }
            PullAction::Fetch => (format!("Fetch {}", remote), m.fetch_args(remote, self.remote_branch.trim(), "", self.tags, self.prune, self.unshallow)),
            PullAction::FetchAll => ("Fetch all".into(), m.fetch_args("", "", "", self.tags, self.prune, self.unshallow)),
            PullAction::FetchPruneAll => ("Fetch and prune all".into(), m.fetch_args("", "", "", self.tags, true, self.unshallow)),
        })
    }
}

impl Dialog for PullDialog {
    fn title(&self) -> String {
        "Pull".into()
    }

    fn kind(&self) -> super::DialogKind {
        super::DialogKind::Modal(520.0)
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        if !self.initialized {
            self.initialized = true;
            self.action = cx.settings.pull_action;
            self.auto_stash = cx.settings.auto_stash_on_pull;
            self.prune = cx.settings.prune_on_fetch;
            if let Some(q) = self.quick {
                self.action = q;
                if let Some((title, args)) = self.command(cx) {
                    cx.run(GitRun::new(title, args).conflicts());
                }
                return false;
            }
        }
        egui::Grid::new("pull").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Remote");
            branch_combo(ui, "pull_remote", &mut self.remote, &self.remotes, 340.0);
            ui.end_row();
            ui.label("Remote branch");
            let branches: Vec<String> = cx
                .data
                .map(|d| d.remote_branches().filter(|r| r.remote == self.remote).map(|r| r.local_name()).collect())
                .unwrap_or_default();
            branch_combo(ui, "pull_branch", &mut self.remote_branch, &branches, 340.0);
            ui.end_row();
        });
        ui.label(egui::RichText::new("Leave the branch empty to pull the tracked branch.").small());
        ui.add_space(4.0);
        ui.label("Action");
        ui.radio_value(&mut self.action, PullAction::Merge, "Merge remote branch into current branch");
        ui.radio_value(&mut self.action, PullAction::Rebase, "Rebase current branch on top of remote branch");
        ui.radio_value(&mut self.action, PullAction::Fetch, "Do not merge, only fetch remote changes");
        ui.add_space(4.0);
        ui.checkbox(&mut self.auto_stash, "Auto stash");
        ui.checkbox(&mut self.prune, "Prune remote branches");
        ui.checkbox(&mut self.unshallow, "Unshallow (fetch full history)");
        ui.horizontal(|ui| {
            ui.label("Tags:");
            ui.radio_value(&mut self.tags, None, "Default");
            ui.radio_value(&mut self.tags, Some(true), "All");
            ui.radio_value(&mut self.tags, Some(false), "None");
        });
        let (ok, cancel) = ok_cancel(ui, if matches!(self.action, PullAction::Merge | PullAction::Rebase) { "⬇ Pull" } else { "⬇ Fetch" }, true);
        if ok {
            cx.settings.pull_action = self.action;
            cx.settings.auto_stash_on_pull = self.auto_stash;
            if let Some((title, args)) = self.command(cx) {
                cx.run(GitRun::new(title, args).conflicts());
            }
            cx.push(super::Action::SaveSettings);
            return false;
        }
        !cancel
    }
}
