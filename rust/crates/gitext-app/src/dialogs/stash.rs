//! Port of `FormStash`: list, show, apply, pop, drop and create stashes.

use egui::{RichText, Ui, Vec2};
use gitext_core::{commands, GitArgs, ObjectId};

use super::{ok_cancel, Action, Confirm, Cx, Dialog, DialogKind, GitRun};
use crate::theme::Palette;
use crate::views::revision_diff::RevisionDiffView;

#[derive(Default)]
pub struct StashDialog {
    selected: Option<usize>,
    diff: RevisionDiffView,
    include_untracked: bool,
    keep_index: bool,
    message: String,
}

impl Dialog for StashDialog {
    fn title(&self) -> String {
        "Stash".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(1000.0, 640.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let palette = Palette::for_ui(ui);
        let Some(m) = cx.module.cloned() else { return false };
        let stashes = cx.data.map(|d| d.stashes.clone()).unwrap_or_default();
        let changes = cx.data.map(|d| d.work_tree_changes() + d.index_changes()).unwrap_or(0);
        let mut keep = true;
        egui::TopBottomPanel::top("stash_top").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.message).hint_text("Stash message (optional)").desired_width(260.0));
                ui.checkbox(&mut self.include_untracked, "Include untracked");
                ui.checkbox(&mut self.keep_index, "Keep index");
                if ui.add_enabled(changes > 0, egui::Button::new("☰ Stash changes")).clicked() {
                    cx.run(GitRun::new("Stash", commands::stash_save(self.include_untracked, self.keep_index, Some(&self.message), &[])));
                    self.message.clear();
                    self.selected = None;
                }
                ui.label(RichText::new(format!("{changes} local change(s)")).color(palette.muted));
            });
        });
        egui::SidePanel::left("stash_list").resizable(true).default_width(320.0).show_inside(ui, |ui| {
            ui.label(RichText::new(format!("Stashes ({})", stashes.len())).strong());
            egui::ScrollArea::vertical().max_height(ui.available_height() - 80.0).show(ui, |ui| {
                for (i, s) in stashes.iter().enumerate() {
                    let name = s.reflog_selector.clone().unwrap_or_default().trim_start_matches("refs/").to_string();
                    if ui.selectable_label(self.selected == Some(i), format!("{name}: {}", s.subject)).clicked() {
                        self.selected = Some(i);
                        self.diff.list.clear();
                    }
                }
                if stashes.is_empty() {
                    ui.label(RichText::new("There are no stashes.").italics().color(palette.muted));
                }
            });
            ui.separator();
            let sel = self.selected.and_then(|i| stashes.get(i));
            let name = sel.map(|s| s.reflog_selector.clone().unwrap_or_default().trim_start_matches("refs/").to_string());
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(sel.is_some(), |ui| {
                    if ui.button("Apply").clicked() {
                        cx.run(GitRun::new("Apply stash", GitArgs::new("stash").arg("apply").arg(name.clone().unwrap())).conflicts());
                    }
                    if ui.button("Pop").clicked() {
                        cx.run(GitRun::new("Pop stash", GitArgs::new("stash").arg("pop").arg(name.clone().unwrap())).conflicts());
                        self.selected = None;
                    }
                    if ui.button("Drop…").clicked() {
                        let n = name.clone().unwrap();
                        cx.open(Confirm::new("Drop stash", format!("Drop {n}?"), "Drop", move |cx| cx.run(GitRun::new("Drop stash", GitArgs::new("stash").arg("drop").arg(n)))));
                        self.selected = None;
                    }
                    if ui.button("Create branch…").clicked() {
                        cx.open(StashBranchDialog { stash: name.clone().unwrap(), branch: String::new() });
                    }
                });
                if ui.add_enabled(!stashes.is_empty(), egui::Button::new("Clear all…")).clicked() {
                    cx.open(Confirm::new("Clear stashes", "Drop ALL stashes? This cannot be undone.", "Clear", |cx| cx.run(GitRun::new("Clear stashes", GitArgs::new("stash").arg("clear")))));
                }
            });
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            match self.selected.and_then(|i| stashes.get(i)) {
                Some(s) => {
                    let parent = s.parents().first().copied();
                    if let Some(c) = self.diff.ui(ui, &m, parent, Some(s.object_id), &[], cx.settings, "stash_diff") {
                        let _ = c;
                    }
                }
                None => {
                    ui.centered_and_justified(|ui| ui.label(RichText::new("Select a stash to see its changes").color(palette.muted)));
                }
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            keep = false;
        }
        let _ = ObjectId::ZERO;
        keep
    }
}

struct StashBranchDialog {
    stash: String,
    branch: String,
}

impl Dialog for StashBranchDialog {
    fn title(&self) -> String {
        "Create branch from stash".into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.label(format!("Create a branch from {} (the stash is dropped on success).", self.stash));
        ui.add(egui::TextEdit::singleline(&mut self.branch).hint_text("Branch name"));
        let (ok, cancel) = ok_cancel(ui, "Create", !self.branch.trim().is_empty());
        if ok {
            cx.run(GitRun::new("Stash branch", GitArgs::new("stash").arg("branch").arg(self.branch.trim()).arg(&self.stash)));
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
        "Stash changes".into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.label("Message");
        ui.add(egui::TextEdit::singleline(&mut self.message).desired_width(360.0)).request_focus();
        ui.checkbox(&mut self.include_untracked, "Include untracked files");
        ui.checkbox(&mut self.keep_index, "Keep index");
        ui.checkbox(&mut self.staged_only, "Stash only staged changes");
        let (ok, cancel) = ok_cancel(ui, "Stash", true);
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
            cx.run(GitRun::new("Stash", args));
            cx.push(Action::RefreshStatus);
            return false;
        }
        !cancel
    }
}
