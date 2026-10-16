//! Port of `FormPull`: pull (merge / rebase) or fetch.

use egui::Ui;
use gitext_core::settings::PullAction;
use gitext_core::GitArgs;

use super::{branch_combo, ok_cancel, Cx, Dialog, GitRun};
use crate::i18n::{tr, trf};
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
            PullAction::Merge | PullAction::Rebase => (
                trf("Pull {0}", &[&remote]),
                m.pull_args(remote, self.remote_branch.trim(), self.action == PullAction::Rebase, self.tags, self.auto_stash, self.prune),
            ),
            PullAction::Fetch => (trf("Fetch {0}", &[&remote]), m.fetch_args(remote, self.remote_branch.trim(), "", self.tags, self.prune, self.unshallow)),
            PullAction::FetchAll => (tr("Fetch all").into(), m.fetch_args("", "", "", self.tags, self.prune, self.unshallow)),
            PullAction::FetchPruneAll => (tr("Fetch and prune all").into(), m.fetch_args("", "", "", self.tags, true, self.unshallow)),
        })
    }
}

impl Dialog for PullDialog {
    fn title(&self) -> String {
        tr("Pull").into()
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
            ui.label(tr("Remote"));
            branch_combo(ui, "pull_remote", &mut self.remote, &self.remotes, 340.0);
            ui.end_row();
            ui.label(tr("Remote branch"));
            let branches: Vec<String> =
                cx.data.map(|d| d.remote_branches().filter(|r| r.remote == self.remote).map(|r| r.local_name()).collect()).unwrap_or_default();
            branch_combo(ui, "pull_branch", &mut self.remote_branch, &branches, 340.0);
            ui.end_row();
        });
        ui.label(egui::RichText::new(tr("Leave the branch empty to pull the tracked branch.")).small());
        ui.add_space(4.0);
        ui.label(tr("Action"));
        ui.radio_value(&mut self.action, PullAction::Merge, tr("Merge remote branch into current branch"));
        ui.radio_value(&mut self.action, PullAction::Rebase, tr("Rebase current branch on top of remote branch"));
        ui.radio_value(&mut self.action, PullAction::Fetch, tr("Do not merge, only fetch remote changes"));
        ui.add_space(4.0);
        ui.checkbox(&mut self.auto_stash, tr("Auto stash"));
        ui.checkbox(&mut self.prune, tr("Prune remote branches"));
        ui.checkbox(&mut self.unshallow, tr("Unshallow (fetch full history)"));
        ui.horizontal(|ui| {
            ui.label(tr("Tags:"));
            ui.radio_value(&mut self.tags, None, tr("Default"));
            ui.radio_value(&mut self.tags, Some(true), tr("All"));
            ui.radio_value(&mut self.tags, Some(false), tr("None"));
        });
        let (ok, cancel) = ok_cancel(ui, if matches!(self.action, PullAction::Merge | PullAction::Rebase) { tr("⬇ Pull") } else { tr("⬇ Fetch") }, true);
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
