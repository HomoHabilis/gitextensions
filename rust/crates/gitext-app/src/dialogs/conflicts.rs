//! Port of `FormResolveConflicts`.

use egui::{RichText, Ui, Vec2};
use gitext_core::{commands, GitArgs};

use super::{Action, Cx, Dialog, DialogKind, GitRun};
use crate::tasks::Loader;
use crate::theme::Palette;

#[derive(Default)]
pub struct ConflictsDialog {
    generation: u64,
    files: Loader<u64, Vec<String>>,
    selected: Option<String>,
}

impl Dialog for ConflictsDialog {
    fn title(&self) -> String {
        "Resolve merge conflicts".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(760.0, 460.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let palette = Palette::for_ui(ui);
        let Some(m) = cx.module.cloned() else { return false };
        let mm = m.clone();
        let files = self.files.request(cx.ctx, self.generation, move || mm.get_unmerged_files()).cloned().unwrap_or_default();
        let state = m.state();
        if let Some(d) = state.description() {
            ui.label(RichText::new(d).strong().color(palette.warning));
        }
        ui.horizontal(|ui| {
            egui::ScrollArea::vertical().id_salt("conflict_files").max_width(360.0).min_scrolled_height(300.0).show(ui, |ui| {
                ui.set_min_width(340.0);
                for f in &files {
                    if ui.selectable_label(self.selected.as_deref() == Some(f.as_str()), f).clicked() {
                        self.selected = Some(f.clone());
                    }
                }
                if files.is_empty() {
                    ui.label(RichText::new("All conflicts are resolved.").color(palette.success));
                }
            });
            ui.vertical(|ui| {
                let sel = self.selected.clone().filter(|s| files.contains(s));
                ui.add_enabled_ui(sel.is_some(), |ui| {
                    let f = sel.clone().unwrap_or_default();
                    let mut errors: Vec<String> = Vec::new();
                    let mut run = |args: Vec<GitArgs>, refresh: &mut u64| {
                        for a in args {
                            if let Err(e) = m.run_checked(&a) {
                                errors.push(e.to_string());
                                break;
                            }
                        }
                        *refresh += 1;
                    };
                    let mut merge_tool = None;
                    if ui.add_sized(Vec2::new(220.0, 26.0), egui::Button::new("Open in merge tool")).clicked() {
                        let mut a = GitArgs::new("mergetool").arg("--no-prompt");
                        if !cx.settings.merge_tool.is_empty() {
                            a.add(format!("--tool={}", cx.settings.merge_tool));
                        }
                        a.add("--");
                        a.add(f.clone());
                        merge_tool = Some(a);
                    }
                    if ui.add_sized(Vec2::new(220.0, 26.0), egui::Button::new("Choose local (ours)")).clicked() {
                        run(vec![GitArgs::new("checkout").arg("--ours").arg("--").arg(&f), GitArgs::new("add").arg("--").arg(&f)], &mut self.generation);
                    }
                    if ui.add_sized(Vec2::new(220.0, 26.0), egui::Button::new("Choose remote (theirs)")).clicked() {
                        run(vec![GitArgs::new("checkout").arg("--theirs").arg("--").arg(&f), GitArgs::new("add").arg("--").arg(&f)], &mut self.generation);
                    }
                    if ui.add_sized(Vec2::new(220.0, 26.0), egui::Button::new("Open in editor")).clicked() {
                        crate::util::open_in_editor(&m.work_dir().join(&f), &cx.settings.editor);
                    }
                    if ui.add_sized(Vec2::new(220.0, 26.0), egui::Button::new("Mark as solved")).clicked() {
                        run(vec![GitArgs::new("add").arg("--").arg(&f)], &mut self.generation);
                    }
                    if ui.add_sized(Vec2::new(220.0, 26.0), egui::Button::new("Delete file (resolve)")).clicked() {
                        run(vec![GitArgs::new("rm").arg("--").arg(&f)], &mut self.generation);
                    }
                    drop(run);
                    for e in errors {
                        cx.error("Resolve", e);
                    }
                    if let Some(a) = merge_tool {
                        cx.run(GitRun::new("Merge tool", a).keep_open());
                        self.generation += 1;
                    }
                });
                ui.add_space(12.0);
                if ui.button("⟳ Refresh").clicked() {
                    self.generation += 1;
                }
            });
        });
        ui.separator();
        let mut keep = true;
        ui.horizontal(|ui| {
            if files.is_empty() {
                if state.rebasing && ui.button("Continue rebase").clicked() {
                    cx.run(GitRun::new("Continue rebase", commands::continue_rebase()).conflicts());
                    keep = false;
                }
                if state.merging && ui.button("Commit merge…").clicked() {
                    cx.push(Action::OpenCommit);
                    keep = false;
                }
                if state.cherry_picking && ui.button("Continue cherry-pick").clicked() {
                    cx.run(GitRun::new("Continue", GitArgs::new("cherry-pick").arg("--continue")).conflicts());
                    keep = false;
                }
                if state.reverting && ui.button("Continue revert").clicked() {
                    cx.run(GitRun::new("Continue", GitArgs::new("revert").arg("--continue")).conflicts());
                    keep = false;
                }
                if state.applying_patch && ui.button("Continue applying patches").clicked() {
                    cx.run(GitRun::new("Continue", commands::resolved_mailbox()).conflicts());
                    keep = false;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    keep = false;
                }
                if state.merging && ui.button("Abort merge").clicked() {
                    cx.run(GitRun::new("Abort merge", commands::abort_merge()));
                    keep = false;
                }
                if state.rebasing && ui.button("Abort rebase").clicked() {
                    cx.run(GitRun::new("Abort rebase", commands::abort_rebase()));
                    keep = false;
                }
            });
        });
        if !keep {
            cx.push(Action::RefreshStatus);
        }
        keep
    }
}
