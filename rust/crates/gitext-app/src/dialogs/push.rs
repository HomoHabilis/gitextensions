//! Port of `FormPush`: push a branch, tags or multiple branches.

use egui::Ui;
use gitext_core::commands::{self, ForcePushOptions, GitPushAction};
use gitext_core::settings::AppSettings;

use super::{branch_combo, ok_cancel, Cx, Dialog, GitRun};
use crate::i18n::{tr, trf};
use crate::repo::RepoData;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Branch,
    Tags,
    Multiple,
}

pub struct PushDialog {
    mode: Mode,
    remote: String,
    remotes: Vec<String>,
    local: String,
    locals: Vec<String>,
    remote_branch: String,
    force: ForcePushOptions,
    set_upstream: bool,
    push_all_branches: bool,
    with_tags: bool,
    tag: String,
    tags: Vec<String>,
    all_tags: bool,
    recurse: u8,
    multiple: Vec<(String, String, bool, bool, bool)>,
}

impl PushDialog {
    /// Preselects the force option (commit & push after an amend).
    pub fn with_force(mut self, force: ForcePushOptions) -> Self {
        self.force = force;
        self
    }

    pub fn new(data: &RepoData, settings: &AppSettings, branch: Option<String>) -> Self {
        let local = branch.or_else(|| data.current_branch.clone()).unwrap_or_default();
        let r = data.local_branches().find(|r| r.name == local);
        let remote = r.map(|r| r.tracking_remote.clone()).filter(|s| !s.is_empty()).or_else(|| data.current_remote()).unwrap_or_default();
        let remote_branch = r.map(|r| r.merge_with.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| local.clone());
        let set_upstream = r.is_none_or(|r| r.merge_with.is_empty());
        PushDialog {
            mode: Mode::Branch,
            remote,
            remotes: data.remote_names(),
            local,
            locals: data.local_branches().map(|r| r.name.clone()).collect(),
            remote_branch,
            force: ForcePushOptions::DoNotForce,
            set_upstream,
            push_all_branches: false,
            with_tags: false,
            tag: String::new(),
            tags: data.tags().map(|t| t.name.clone()).collect(),
            all_tags: false,
            recurse: settings.recurse_submodules_on_push,
            multiple: data
                .local_branches()
                .map(|r| (r.name.clone(), if r.merge_with.is_empty() { r.name.clone() } else { r.merge_with.clone() }, false, false, false))
                .collect(),
        }
    }

    pub fn tag(data: &RepoData, settings: &AppSettings, tag: &str) -> Self {
        let mut d = Self::new(data, settings, None);
        d.mode = Mode::Tags;
        d.tag = tag.to_string();
        d
    }
}

impl Dialog for PushDialog {
    fn title(&self) -> String {
        tr("Push").into()
    }

    fn kind(&self) -> super::DialogKind {
        super::DialogKind::Modal(560.0)
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.horizontal(|ui| {
            ui.label(tr("Remote"));
            branch_combo(ui, "push_remote", &mut self.remote, &self.remotes, 300.0);
            if ui.button(tr("Manage remotes…")).clicked() {
                cx.open(super::remotes::RemotesDialog::default());
            }
        });
        if let Some(url) = cx.data.and_then(|d| d.remotes.iter().find(|r| r.name == self.remote)).and_then(|r| r.push_urls.first()) {
            ui.label(egui::RichText::new(url).small());
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.mode, Mode::Branch, tr("Push branch"));
            ui.selectable_value(&mut self.mode, Mode::Tags, tr("Push tags"));
            ui.selectable_value(&mut self.mode, Mode::Multiple, tr("Push multiple branches"));
        });
        ui.add_space(4.0);
        match self.mode {
            Mode::Branch => {
                ui.checkbox(&mut self.push_all_branches, tr("Push all branches"));
                ui.add_enabled_ui(!self.push_all_branches, |ui| {
                    egui::Grid::new("push_branch").num_columns(2).show(ui, |ui| {
                        ui.label(tr("Branch to push"));
                        branch_combo(ui, "push_local", &mut self.local, &self.locals, 320.0);
                        ui.end_row();
                        ui.label(tr("To remote branch"));
                        ui.add(egui::TextEdit::singleline(&mut self.remote_branch).desired_width(320.0));
                        ui.end_row();
                    });
                });
                ui.checkbox(&mut self.set_upstream, tr("Set upstream (track the remote branch)"));
                ui.checkbox(&mut self.with_tags, tr("Push tags as well (--follow-tags)"));
            }
            Mode::Tags => {
                ui.checkbox(&mut self.all_tags, tr("Push all tags"));
                ui.add_enabled_ui(!self.all_tags, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr("Tag"));
                        branch_combo(ui, "push_tag", &mut self.tag, &self.tags, 320.0);
                    });
                });
            }
            Mode::Multiple => {
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    egui::Grid::new("push_multi").striped(true).num_columns(5).show(ui, |ui| {
                        ui.strong(tr("Local"));
                        ui.strong(tr("Remote"));
                        ui.strong(tr("Push"));
                        ui.strong(tr("Force"));
                        ui.strong(tr("Delete"));
                        ui.end_row();
                        for (l, r, push, force, delete) in &mut self.multiple {
                            ui.label(l.as_str());
                            ui.add(egui::TextEdit::singleline(r).desired_width(160.0));
                            ui.checkbox(push, "");
                            ui.checkbox(force, "");
                            ui.checkbox(delete, "");
                            ui.end_row();
                        }
                    });
                });
            }
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(tr("Force:"));
            ui.radio_value(&mut self.force, ForcePushOptions::DoNotForce, tr("No"));
            ui.radio_value(&mut self.force, ForcePushOptions::ForceWithLease, tr("With lease"));
            ui.radio_value(&mut self.force, ForcePushOptions::Force, tr("Force"));
        });
        ui.horizontal(|ui| {
            ui.label(tr("Submodules:"));
            ui.radio_value(&mut self.recurse, 0, tr("Don't check"));
            ui.radio_value(&mut self.recurse, 1, tr("Check"));
            ui.radio_value(&mut self.recurse, 2, tr("On demand"));
        });
        let (ok, cancel) = ok_cancel(ui, tr("⬆ Push"), !self.remote.trim().is_empty());
        if ok {
            let remote = self.remote.trim();
            let args = match self.mode {
                Mode::Branch if self.push_all_branches => commands::push_all(remote, self.force, self.set_upstream, self.recurse),
                Mode::Branch => {
                    let mut a = commands::push(remote, &self.local, Some(self.remote_branch.trim()), self.force, self.set_upstream, self.recurse);
                    if self.with_tags {
                        a.add("--follow-tags");
                    }
                    a
                }
                Mode::Tags => commands::push_tag(remote, &self.tag, self.all_tags, self.force),
                Mode::Multiple => {
                    let actions: Vec<GitPushAction> = self
                        .multiple
                        .iter()
                        .filter(|m| m.2 || m.4)
                        .map(|(l, r, _, force, delete)| GitPushAction { local: l.clone(), remote: r.clone(), force: *force, delete: *delete })
                        .collect();
                    commands::push_multiple(remote, &actions)
                }
            };
            if args.is_empty() {
                cx.error(tr("Push"), tr("Nothing to push"));
                return true;
            }
            cx.run(GitRun::new(trf("Push to {0}", &[&remote]), args));
            return false;
        }
        !cancel
    }
}
