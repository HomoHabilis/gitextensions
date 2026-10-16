//! Port of `FormStash`: list, show, apply, pop, drop and create stashes. With no stash selected
//! ("Current working directory changes") it lists the local changes, so that chosen files can be
//! stashed ("Stash selected changes").

use egui::{Key, RichText, Ui, Vec2};
use gitext_core::status::{GitItemStatus, StagedStatus};
use gitext_core::{commands, GitArgs, GitModule, ObjectId};

use super::{ok_cancel, Action, Confirm, Cx, Dialog, DialogKind, GitRun};
use crate::i18n::{tr, trf};
use crate::tasks::Loader;
use crate::theme::Palette;
use crate::views::diff_viewer::{content_from_bytes, DiffViewer, ViewerContent};
use crate::views::file_actions::{self, Selection};
use crate::views::file_list::FileList;
use crate::views::revision_diff::RevisionDiffView;
use crate::views::{plain_key, shortcut};

#[derive(Default)]
pub struct StashDialog {
    /// The selected stash; `None` shows the working directory changes.
    selected: Option<usize>,
    diff: RevisionDiffView,
    include_untracked: bool,
    keep_index: bool,
    message: String,
    /// The working directory changes (`Stashed` list when the first entry is selected).
    work_tree: FileList,
    index: FileList,
    staged_side: bool,
    /// The status shown last, to reload the diff when it changes.
    status: Vec<GitItemStatus>,
    generation: u64,
    file_diff: Loader<String, ViewerContent>,
    viewer: DiffViewer,
}

/// The `stash@{n}` name of a stash.
fn stash_name(s: &gitext_core::revision::GitRevision) -> String {
    s.reflog_selector.clone().unwrap_or_default().trim_start_matches("refs/").to_string()
}

impl Dialog for StashDialog {
    fn title(&self) -> String {
        tr("Stash").into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(1100.0, 700.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let palette = Palette::for_ui(ui);
        let Some(m) = cx.module.cloned() else { return false };
        let stashes = cx.data.map(|d| d.stashes.clone()).unwrap_or_default();
        if self.selected.is_some_and(|i| i >= stashes.len()) {
            self.selected = None;
        }
        let mut keep = true;
        // the panels do not claim the window's width: keep the size given to the window
        ui.set_min_width(ui.available_width());
        ui.set_max_height(ui.available_height());
        // Ctrl+Up / Ctrl+Down: previous / next stash (newest first, the working directory above them)
        if shortcut(ui.ctx(), egui::Modifiers::COMMAND, Key::ArrowDown) {
            self.select(Some(self.selected.map_or(0, |i| (i + 1).min(stashes.len().saturating_sub(1)))).filter(|_| !stashes.is_empty()).or(self.selected));
        }
        if shortcut(ui.ctx(), egui::Modifiers::COMMAND, Key::ArrowUp) {
            self.select(self.selected.and_then(|i| i.checked_sub(1)));
        }
        if plain_key(ui.ctx(), Key::F5) {
            cx.push(Action::RefreshStatus);
        }
        egui::SidePanel::left("stash_list").resizable(true).default_width(300.0).show_inside(ui, |ui| {
            ui.label(RichText::new(tr("Show")).strong());
            egui::ScrollArea::vertical().max_height(ui.available_height() - 80.0).show(ui, |ui| {
                if ui.selectable_label(self.selected.is_none(), tr("Current working directory changes")).clicked() {
                    self.select(None);
                }
                for (i, s) in stashes.iter().enumerate() {
                    if ui.selectable_label(self.selected == Some(i), format!("{}: {}", stash_name(s), s.subject)).clicked() {
                        self.select(Some(i));
                    }
                }
                if stashes.is_empty() {
                    ui.label(RichText::new(tr("There are no stashes.")).italics().color(palette.muted));
                }
            });
            ui.separator();
            let name = self.selected.and_then(|i| stashes.get(i)).map(stash_name);
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(name.is_some(), |ui| {
                    if ui.button(tr("Apply")).clicked() {
                        cx.run(GitRun::new(tr("Apply stash"), GitArgs::new("stash").arg("apply").arg(name.clone().unwrap())).conflicts());
                    }
                    if ui.button(tr("Pop")).clicked() {
                        cx.run(GitRun::new(tr("Pop stash"), GitArgs::new("stash").arg("pop").arg(name.clone().unwrap())).conflicts());
                        self.select(None);
                    }
                    if ui.button(tr("Drop selected stash…")).clicked() {
                        let n = name.clone().unwrap();
                        cx.open(Confirm::new(tr("Drop stash"), trf("Drop {0}? This cannot be undone.", &[&n]), tr("Drop"), move |cx| cx.run(GitRun::new(tr("Drop stash"), GitArgs::new("stash").arg("drop").arg(n)))));
                        self.select(None);
                    }
                    if ui.button(tr("Create branch…")).clicked() {
                        cx.open(StashBranchDialog { stash: name.clone().unwrap(), branch: String::new() });
                    }
                });
                if ui.add_enabled(!stashes.is_empty(), egui::Button::new(tr("Clear all…"))).clicked() {
                    cx.open(Confirm::new(tr("Clear stashes"), tr("Drop ALL stashes? This cannot be undone."), tr("Clear"), |cx| cx.run(GitRun::new(tr("Clear stashes"), GitArgs::new("stash").arg("clear")))));
                }
            });
        });
        match self.selected.and_then(|i| stashes.get(i)) {
            Some(s) => {
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    let parent = s.parents().first().copied();
                    let _ = self.diff.ui(ui, &m, parent, Some(s.object_id), &[], cx.settings, "stash_diff");
                });
            }
            None => self.work_tree_ui(ui, cx, &m),
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            keep = false;
        }
        keep
    }
}

impl StashDialog {
    fn select(&mut self, selected: Option<usize>) {
        if selected != self.selected {
            self.selected = selected;
            self.diff.list.clear();
        }
    }

    /// The working directory changes with the stash options ("Current working directory changes").
    fn work_tree_ui(&mut self, ui: &mut Ui, cx: &mut Cx, m: &GitModule) {
        let palette = Palette::for_ui(ui);
        // the status of the browse window: no git call of its own (refresh reloads it)
        let status = cx.data.map(|d| d.status.clone()).unwrap_or_default();
        if status != self.status {
            self.status = status.clone();
            self.generation += 1;
            self.file_diff.invalidate();
        }
        let work_tree: Vec<GitItemStatus> = status.iter().filter(|s| s.staged == StagedStatus::WorkTree).cloned().collect();
        let index: Vec<GitItemStatus> = status.iter().filter(|s| s.staged == StagedStatus::Index).cloned().collect();
        let selected: Vec<&GitItemStatus> = if self.staged_side { self.index.selected_items(&index) } else { self.work_tree.selected_items(&work_tree) };
        let selected_names: Vec<String> = selected.iter().map(|f| f.name.clone()).collect();
        let selected_untracked = selected.iter().any(|f| f.is_new && !f.is_tracked);
        let mut action = None;
        egui::SidePanel::left("stash_files").resizable(true).default_width(360.0).width_range(220.0..=700.0).show_inside(ui, |ui| {
            // the stash options at the bottom, the two lists share the height above them
            egui::TopBottomPanel::bottom("stash_options").show_inside(ui, |ui| {
                ui.label(tr("Message:"));
                ui.add(egui::TextEdit::multiline(&mut self.message).desired_rows(3).desired_width(f32::INFINITY).hint_text(tr("Stash message (optional)")));
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.keep_index, tr("Keep index"));
                    ui.checkbox(&mut self.include_untracked, tr("Include untracked"));
                });
                let changes = !status.is_empty();
                ui.vertical_centered_justified(|ui| {
                    if ui.add_enabled(changes, egui::Button::new(tr("Stash all changes"))).clicked() {
                        cx.run(GitRun::new(tr("Stash"), commands::stash_save(self.include_untracked, self.keep_index, Some(self.message.trim()), &[])));
                        self.message.clear();
                    }
                    let tip = tr("Stash the files selected above (with the untracked ones among them)");
                    if ui.add_enabled(!selected_names.is_empty(), egui::Button::new(tr("Stash selected changes"))).on_hover_text(tip).clicked() {
                        let refs: Vec<&str> = selected_names.iter().map(String::as_str).collect();
                        // a selected untracked file is only stashed with -u
                        let untracked = self.include_untracked || selected_untracked;
                        cx.run(GitRun::new(tr("Stash"), commands::stash_save(untracked, self.keep_index, Some(self.message.trim()), &refs)));
                        self.message.clear();
                        self.work_tree.clear();
                        self.index.clear();
                    }
                });
            });
            ui.horizontal(|ui| {
                if ui.button("⟳").on_hover_text(format!("{} (F5)", tr("Refresh"))).clicked() {
                    cx.push(Action::RefreshStatus);
                }
                self.work_tree.toolbar(ui);
            });
            // what is left once both headers are shown
            let lists_h = (ui.available_height() - 2.0 * (ui.spacing().interact_size.y + ui.spacing().item_spacing.y) - 8.0).max(80.0);
            ui.label(RichText::new(trf("({0}) Working directory", &[&work_tree.len()])).strong());
            ui.allocate_ui(Vec2::new(ui.available_width(), lists_h * 0.6), |ui| {
                let r = self.work_tree.ui(ui, "stash_work_tree", &work_tree, |ui, sel| {
                    let items = sel.iter().filter_map(|&i| work_tree.get(i)).collect();
                    if let Some(a) = file_actions::menu(ui, &Selection { items, first: Some(ObjectId::INDEX), second: ObjectId::WORK_TREE }) {
                        action = Some(a);
                    }
                });
                if r.selection_changed && !self.work_tree.selected.is_empty() {
                    self.staged_side = false;
                    self.index.clear();
                }
            });
            ui.label(RichText::new(trf("({0}) Commit index", &[&index.len()])).strong());
            ui.allocate_ui(Vec2::new(ui.available_width(), ui.available_height()), |ui| {
                let r = self.index.ui(ui, "stash_index", &index, |ui, sel| {
                    let items = sel.iter().filter_map(|&i| index.get(i)).collect();
                    if let Some(a) = file_actions::menu(ui, &Selection { items, first: None, second: ObjectId::INDEX }) {
                        action = Some(a);
                    }
                });
                if r.selection_changed && !self.index.selected.is_empty() {
                    self.staged_side = true;
                    self.work_tree.clear();
                }
            });
        });
        if let Some(a) = action {
            file_actions::run(a, m, cx.actions);
        }
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let (list, items) = if self.staged_side { (&self.index, &index) } else { (&self.work_tree, &work_tree) };
            let file = list.selected.last().and_then(|&i| items.get(i)).cloned();
            let content = match &file {
                None if status.is_empty() => ViewerContent::Empty(tr("There are no local changes").into()),
                None => ViewerContent::Empty(tr("Select a file to see its changes").into()),
                Some(f) => {
                    let opts = crate::views::revision_diff::diff_options(cx.settings);
                    let staged = self.staged_side;
                    let key = format!("{staged}|{}|{opts:?}|{}", f.name, self.generation);
                    let (mm, f) = (m.clone(), f.clone());
                    self.file_diff
                        .request(cx.ctx, key, move || {
                            if f.is_new && !f.is_tracked {
                                return mm.get_file_bytes(ObjectId::WORK_TREE, &f.name).map(|b| content_from_bytes(&b)).unwrap_or(ViewerContent::Empty(tr("Cannot read file").into()));
                            }
                            let (first, second) = if staged { (None, ObjectId::INDEX) } else { (Some(ObjectId::INDEX), ObjectId::WORK_TREE) };
                            match mm.get_file_diff(first, second, &f.name, f.old_name.as_deref(), &opts) {
                                Ok(d) if d.trim().is_empty() => ViewerContent::Empty(tr("No differences").into()),
                                Ok(d) => ViewerContent::Diff(d),
                                Err(e) => ViewerContent::Empty(e.to_string()),
                            }
                        })
                        .cloned()
                        .unwrap_or(ViewerContent::Empty(tr("Loading…").into()))
                }
            };
            if let Some(f) = &file {
                ui.label(RichText::new(&f.name).strong());
            } else {
                ui.label(RichText::new(" ").color(palette.muted));
            }
            if self.viewer.toolbar(ui, &content, cx.settings) {
                cx.push(Action::SaveSettings);
            }
            let _ = self.viewer.ui(ui, &content, cx.settings.show_line_numbers, &[]);
        });
    }
}

struct StashBranchDialog {
    stash: String,
    branch: String,
}

impl Dialog for StashBranchDialog {
    fn title(&self) -> String {
        tr("Create branch from stash").into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.label(trf("Create a branch from {0} (the stash is dropped on success).", &[&self.stash]));
        ui.add(egui::TextEdit::singleline(&mut self.branch).hint_text(tr("Branch name")));
        let (ok, cancel) = ok_cancel(ui, tr("Create"), !self.branch.trim().is_empty());
        if ok {
            cx.run(GitRun::new(tr("Stash branch"), GitArgs::new("stash").arg("branch").arg(self.branch.trim()).arg(&self.stash)));
            return false;
        }
        !cancel
    }
}

#[derive(Default)]
pub struct CreateStashDialog {
    message: String,
    include_untracked: bool,
    keep_index: bool,
    staged_only: bool,
}

impl Dialog for CreateStashDialog {
    fn title(&self) -> String {
        tr("Stash changes").into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.label(tr("Message"));
        ui.add(egui::TextEdit::singleline(&mut self.message).desired_width(360.0)).request_focus();
        ui.checkbox(&mut self.include_untracked, tr("Include untracked files"));
        ui.checkbox(&mut self.keep_index, tr("Keep index"));
        ui.checkbox(&mut self.staged_only, tr("Stash only staged changes"));
        let (ok, cancel) = ok_cancel(ui, tr("Stash"), true);
        if ok {
            let args = if self.staged_only {
                let mut a = GitArgs::new("stash").arg("push").arg("--staged");
                if !self.message.trim().is_empty() {
                    a.add("-m");
                    a.add(self.message.trim());
                }
                a
            } else {
                commands::stash_save(self.include_untracked, self.keep_index, Some(&self.message), &[])
            };
            cx.run(GitRun::new(tr("Stash"), args));
            cx.push(Action::RefreshStatus);
            return false;
        }
        !cancel
    }
}
