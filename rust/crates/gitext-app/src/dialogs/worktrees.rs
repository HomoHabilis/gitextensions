//! Ports of `FormManageWorktree` and `FormCreateWorktree`.

use egui::{RichText, Ui, Vec2};
use gitext_core::GitArgs;

use super::{ok_cancel, Action, Confirm, Cx, Dialog, DialogKind, GitRun};
use crate::theme::Palette;

#[derive(Default)]
pub struct WorktreesDialog;

impl Dialog for WorktreesDialog {
    fn title(&self) -> String {
        "Worktrees".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(760.0, 380.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let palette = Palette::for_ui(ui);
        let wts = cx.data.map(|d| d.worktrees.clone()).unwrap_or_default();
        ui.horizontal(|ui| {
            if ui.button("✚ Create worktree…").clicked() {
                cx.open(CreateWorktreeDialog::default());
            }
            if ui.button("Prune").clicked() {
                cx.run(GitRun::new("Prune worktrees", GitArgs::new("worktree").arg("prune")));
            }
        });
        ui.separator();
        egui::Grid::new("wts").striped(true).num_columns(4).show(ui, |ui| {
            ui.strong("Path");
            ui.strong("HEAD");
            ui.strong("Branch");
            ui.strong("");
            ui.end_row();
            for w in &wts {
                ui.label(RichText::new(&w.path).color(if w.is_deleted { palette.error } else { ui.visuals().text_color() }));
                ui.label(RichText::new(w.sha1.as_deref().map(|s| &s[..s.len().min(8)]).unwrap_or("-")).monospace());
                ui.label(w.branch.clone().unwrap_or_else(|| format!("{:?}", w.head_type)));
                ui.horizontal(|ui| {
                    if !w.is_deleted && ui.small_button("Open").clicked() {
                        cx.push(Action::OpenRepo(w.path.clone().into()));
                    }
                    if !w.is_main && ui.small_button("Remove…").clicked() {
                        let p = w.path.clone();
                        cx.open(Confirm::new("Remove worktree", format!("Remove worktree '{p}'?"), "Remove", move |cx| {
                            cx.run(GitRun::new("Remove worktree", GitArgs::new("worktree").arg("remove").arg("--force").arg(p)));
                        }));
                    }
                });
                ui.end_row();
            }
        });
        !ui.input(|i| i.key_pressed(egui::Key::Escape))
    }
}

#[derive(Default)]
pub struct CreateWorktreeDialog {
    path: String,
    branch: String,
    new_branch: bool,
    open_after: bool,
}

impl Dialog for CreateWorktreeDialog {
    fn title(&self) -> String {
        "Create worktree".into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let branches = cx.data.map(|d| d.branch_names(false)).unwrap_or_default();
        ui.checkbox(&mut self.new_branch, "Create a new branch");
        ui.label(if self.new_branch { "New branch name" } else { "Existing branch" });
        super::branch_combo(ui, "wt_branch", &mut self.branch, if self.new_branch { &[] } else { &branches }, 360.0);
        ui.label("Worktree directory");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(300.0));
            if ui.button("…").clicked() {
                if let Some(p) = crate::util::pick_folder(None) {
                    self.path = p;
                }
            }
        });
        if self.path.is_empty() && !self.branch.is_empty() {
            if let Some(m) = cx.module {
                if let Some(parent) = m.work_dir().parent() {
                    self.path = parent.join(format!("{}_{}", m.name(), self.branch.replace('/', "_"))).display().to_string();
                }
            }
        }
        ui.checkbox(&mut self.open_after, "Open the new worktree");
        let (ok, cancel) = ok_cancel(ui, "Create", !self.path.trim().is_empty() && !self.branch.trim().is_empty());
        if ok {
            let mut args = GitArgs::new("worktree").arg("add");
            if self.new_branch {
                args.add("-b");
                args.add(self.branch.trim());
                args.add(self.path.trim());
            } else {
                args.add(self.path.trim());
                args.add(self.branch.trim());
            }
            let mut run = GitRun::new("Create worktree", args);
            if self.open_after {
                run = run.then(Action::OpenRepo(self.path.trim().into()));
            }
            cx.run(run);
            return false;
        }
        !cancel
    }
}
