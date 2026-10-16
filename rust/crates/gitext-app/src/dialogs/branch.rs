//! Ports of `FormCreateBranch`, `FormRenameBranch`, `FormDeleteBranch`, `FormDeleteRemoteBranch`
//! and the "set upstream" action.

use egui::Ui;
use gitext_core::branch_name::{normalise, GitBranchNameOptions};
use gitext_core::commands;
use gitext_core::{GitArgs, ObjectId};

use super::{branch_combo, form_row, ok_cancel, Cx, Dialog, GitRun};
use crate::i18n::{tr, trf};
use crate::repo::RepoData;

pub struct CreateBranchDialog {
    id: ObjectId,
    revision: String,
    name: String,
    checkout: bool,
    orphan: bool,
    clear_orphan: bool,
    /// Whether the name has been prefilled from a ref at the start commit (done on the first frame,
    /// where the repository data is available).
    prefilled: bool,
}

impl CreateBranchDialog {
    pub fn new(id: ObjectId) -> Self {
        CreateBranchDialog {
            id,
            revision: if id.is_zero() { "HEAD".into() } else { id.to_string() },
            name: String::new(),
            checkout: true,
            orphan: false,
            clear_orphan: true,
            prefilled: false,
        }
    }
}

impl CreateBranchDialog {
    /// A branch created from a ref resolved when the branch is created (e.g. a remote branch
    /// being fetched), named after it without the remote.
    pub fn from_ref(name: &str) -> Self {
        let mut d = CreateBranchDialog::new(ObjectId::ZERO);
        d.revision = name.to_string();
        d.name = name.split_once('/').map_or(name, |(_, b)| b).to_string();
        d.prefilled = true;
        d
    }
}

/// The name `FormCreateBranch` suggests for a branch at `id`: the first local branch there, else
/// the first remote branch without its remote prefix, else the first tag.
fn suggested_name(data: &RepoData, id: ObjectId) -> Option<String> {
    let id = if id.is_zero() { data.head } else { id };
    let refs = data.refs_by_commit.get(&id)?;
    let remote = |r: &&gitext_core::GitRef| r.is_remote() && !gitext_core::git_ref::ref_name::is_remote_head(&r.complete_name);
    refs.iter().find(|r| r.is_head()).or_else(|| refs.iter().find(remote)).or_else(|| refs.iter().find(|r| r.is_tag())).map(|r| r.local_name())
}

impl Dialog for CreateBranchDialog {
    fn title(&self) -> String {
        tr("Create branch").into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let normalised = normalise(&self.name, &GitBranchNameOptions::new("_"));
        egui::Grid::new("create_branch").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            let mut prefilled = false;
            if !self.prefilled {
                self.prefilled = true;
                if let Some(name) = cx.data.and_then(|d| suggested_name(d, self.id)) {
                    self.name = name;
                    prefilled = true;
                }
            }
            form_row(ui, tr("Branch name"), |ui| {
                let mut out = egui::TextEdit::singleline(&mut self.name).desired_width(300.0).show(ui);
                if prefilled {
                    // caret after the suggested name, so it can be extended
                    let end = egui::text::CCursor::new(self.name.chars().count());
                    out.state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                    out.state.store(ui.ctx(), out.response.id);
                }
                let r = out.response;
                if r.lost_focus() {
                    self.name = normalise(&self.name, &GitBranchNameOptions::new("_"));
                }
                r.request_focus();
            });
            form_row(ui, tr("Create from"), |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.revision).desired_width(300.0));
            });
        });
        if !self.name.is_empty() && normalised != self.name {
            ui.label(egui::RichText::new(trf("Will be created as '{0}'", &[&normalised])).small());
        }
        ui.checkbox(&mut self.checkout, tr("Checkout after create"));
        ui.checkbox(&mut self.orphan, tr("Create orphan branch (no parents)"));
        if self.orphan {
            ui.checkbox(&mut self.clear_orphan, tr("Clear working directory and index"));
        }
        let (ok, cancel) = ok_cancel(ui, tr("Create branch"), !normalised.is_empty());
        if ok || (ui.input(|i| i.key_pressed(egui::Key::Enter)) && !normalised.is_empty()) {
            let start = cx.module.map(|m| m.rev_parse(&self.revision)).unwrap_or(self.id);
            if self.orphan {
                let mut cmds = vec![commands::create_orphan(&normalised, start)];
                if self.clear_orphan {
                    cmds.push(GitArgs::new("rm").args(["-r", "--cached", "--quiet", "--ignore-unmatch", "."]));
                    cmds.push(GitArgs::new("clean").args(["-f", "-d"]));
                }
                cx.run(GitRun::many(tr("Create orphan branch"), cmds));
            } else {
                cx.run(GitRun::new(trf("Create branch {0}", &[&normalised]), commands::branch(&normalised, start, self.checkout)));
            }
            return false;
        }
        !cancel
    }
}

pub struct RenameBranchDialog {
    old: String,
    name: String,
}

impl RenameBranchDialog {
    pub fn new(old: &str) -> Self {
        RenameBranchDialog { old: old.to_string(), name: old.to_string() }
    }
}

impl Dialog for RenameBranchDialog {
    fn title(&self) -> String {
        trf("Rename branch {0}", &[&self.old])
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.horizontal(|ui| {
            ui.label(tr("New name"));
            ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(300.0)).request_focus();
        });
        let n = normalise(&self.name, &GitBranchNameOptions::new("_"));
        let (ok, cancel) = ok_cancel(ui, tr("Rename"), !n.is_empty() && n != self.old);
        if ok {
            cx.run(GitRun::new(tr("Rename branch"), commands::rename_branch(&self.old, &n)));
            return false;
        }
        !cancel
    }
}

pub struct DeleteBranchDialog {
    branches: Vec<(String, String, bool)>,
    force: bool,
    filter: String,
}

impl DeleteBranchDialog {
    /// `preselect` is the complete ref name.
    pub fn new(data: &RepoData, preselect: Option<String>) -> Self {
        let current = data.current_branch.clone();
        let mut branches: Vec<(String, String, bool)> = data
            .local_branches()
            .filter(|r| Some(&r.name) != current.as_ref())
            .chain(data.remote_branches())
            .map(|r| (r.complete_name.clone(), r.name.clone(), preselect.as_deref() == Some(r.complete_name.as_str())))
            .collect();
        branches.sort_by(|a, b| a.0.cmp(&b.0));
        DeleteBranchDialog { branches, force: false, filter: String::new() }
    }
}

impl Dialog for DeleteBranchDialog {
    fn title(&self) -> String {
        tr("Delete branch").into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.label(tr("Select branches to delete:"));
        ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text(tr("Filter")).desired_width(f32::INFINITY));
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for (full, name, checked) in &mut self.branches {
                if self.filter.is_empty() || name.contains(&self.filter) {
                    let label = if full.starts_with("refs/remotes/") { trf("{0} (remote tracking)", &[&name]) } else { name.clone() };
                    ui.checkbox(checked, label);
                }
            }
        });
        ui.checkbox(&mut self.force, tr("Force delete (also unmerged branches)"));
        let selected: Vec<&(String, String, bool)> = self.branches.iter().filter(|b| b.2).collect();
        let (ok, cancel) = ok_cancel(ui, tr("Delete"), !selected.is_empty());
        if ok {
            let refs: Vec<gitext_core::GitRef> = selected.iter().map(|b| gitext_core::GitRef::from_complete_name(ObjectId::ZERO, b.0.clone())).collect();
            let refs: Vec<&gitext_core::GitRef> = refs.iter().collect();
            match commands::delete_branch(&refs, self.force) {
                Ok(args) => cx.run(GitRun::new(tr("Delete branch"), args)),
                Err(e) => cx.error(tr("Delete branch"), e),
            }
            return false;
        }
        !cancel
    }
}

pub struct DeleteRemoteBranchDialog {
    name: String,
    remote: String,
    branch: String,
    delete_tracking_local: bool,
}

impl DeleteRemoteBranchDialog {
    pub fn new(data: &RepoData, name: &str) -> Self {
        let remote = gitext_core::git_ref::ref_name::get_remote_name_from(name, data.remotes.iter().map(|r| r.name.as_str()));
        let branch = name.strip_prefix(&format!("{remote}/")).unwrap_or(name).to_string();
        DeleteRemoteBranchDialog { name: name.to_string(), remote, branch, delete_tracking_local: false }
    }
}

impl Dialog for DeleteRemoteBranchDialog {
    fn title(&self) -> String {
        tr("Delete remote branch").into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.label(trf("Delete branch '{0}' on remote '{1}'?", &[&self.branch, &self.remote]));
        ui.label(egui::RichText::new(tr("This deletes the branch on the server.")).small());
        let local_tracking =
            cx.data.and_then(|d| d.local_branches().find(|r| r.tracking_remote == self.remote && r.merge_with == self.branch).map(|r| r.name.clone()));
        if let Some(l) = &local_tracking {
            ui.checkbox(&mut self.delete_tracking_local, trf("Also delete local branch '{0}'", &[l]));
        }
        let (ok, cancel) = ok_cancel(ui, tr("Delete"), true);
        if ok {
            let mut cmds = vec![commands::delete_remote_branches(&self.remote, &[&self.branch])];
            if self.delete_tracking_local {
                if let Some(l) = local_tracking {
                    cmds.push(GitArgs::new("branch").arg("-D").arg(l));
                }
            }
            cx.run(GitRun::many(trf("Delete {0}", &[&self.name]), cmds));
            return false;
        }
        !cancel
    }
}

pub struct SetUpstreamDialog {
    branch: String,
    upstream: String,
    items: Vec<String>,
}

impl SetUpstreamDialog {
    pub fn new(data: &RepoData, branch: &str) -> Self {
        let current =
            data.local_branches().find(|r| r.name == branch).filter(|r| !r.merge_with.is_empty()).map(|r| format!("{}/{}", r.tracking_remote, r.merge_with));
        SetUpstreamDialog { branch: branch.to_string(), upstream: current.unwrap_or_default(), items: data.remote_branches().map(|r| r.name.clone()).collect() }
    }
}

impl Dialog for SetUpstreamDialog {
    fn title(&self) -> String {
        trf("Set upstream of {0}", &[&self.branch])
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.label(tr("Upstream (remote) branch:"));
        branch_combo(ui, "upstream", &mut self.upstream, &self.items, 340.0);
        ui.label(egui::RichText::new(tr("Leave empty to remove the upstream.")).small());
        let (ok, cancel) = ok_cancel(ui, tr("OK"), true);
        if ok {
            let args = if self.upstream.trim().is_empty() {
                GitArgs::new("branch").arg("--unset-upstream").arg(&self.branch)
            } else {
                GitArgs::new("branch").arg(format!("--set-upstream-to={}", self.upstream.trim())).arg(&self.branch)
            };
            cx.run(GitRun::new(tr("Set upstream"), args));
            return false;
        }
        !cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitext_core::GitRef;

    fn data(id: ObjectId, refs: &[(&str, &str)]) -> RepoData {
        let mut d = RepoData { head: id, ..Default::default() };
        d.refs_by_commit.insert(id, refs.iter().map(|(name, remote)| GitRef::new(id, *name, *remote)).collect());
        d
    }

    #[test]
    fn suggests_local_then_remote_then_tag() {
        let id = ObjectId::parse("1111111111111111111111111111111111111111").unwrap();
        let all = [("refs/tags/v1", ""), ("refs/remotes/origin/HEAD", "origin"), ("refs/remotes/origin/feature/x", "origin"), ("refs/heads/main", "")];
        assert_eq!(suggested_name(&data(id, &all), id).as_deref(), Some("main"));
        assert_eq!(suggested_name(&data(id, &all[..3]), id).as_deref(), Some("feature/x"));
        assert_eq!(suggested_name(&data(id, &all[..2]), id).as_deref(), Some("v1"));
        assert_eq!(suggested_name(&data(id, &all[..1]), ObjectId::default()).as_deref(), Some("v1"));
        assert_eq!(suggested_name(&data(id, &[]), id), None);
    }
}
