//! Ports of `FormSubmodules` and `FormAddSubmodule`.

use egui::{RichText, Ui, Vec2};
use gitext_core::{commands, GitArgs};

use super::{ok_cancel, Action, Confirm, Cx, Dialog, DialogKind, GitRun};
use crate::theme::Palette;

#[derive(Default)]
pub struct SubmodulesDialog {
    selected: Option<usize>,
}

impl Dialog for SubmodulesDialog {
    fn title(&self) -> String {
        "Submodules".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(720.0, 420.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let palette = Palette::for_ui(ui);
        let subs = cx.data.map(|d| d.submodules.clone()).unwrap_or_default();
        ui.horizontal(|ui| {
            if ui.button("✚ Add submodule…").clicked() {
                cx.open(AddSubmoduleDialog::default());
            }
            if ui.button("Update all").clicked() {
                cx.run(GitRun::new("Update submodules", commands::submodule_update(&[], false)));
            }
            if ui.button("Synchronize all").clicked() {
                cx.run(GitRun::new("Sync submodules", commands::submodule_sync(None)));
            }
        });
        ui.separator();
        egui::Grid::new("subs").striped(true).num_columns(4).show(ui, |ui| {
            ui.strong("Path");
            ui.strong("Commit");
            ui.strong("Status");
            ui.strong("");
            ui.end_row();
            for (i, s) in subs.iter().enumerate() {
                if ui.selectable_label(self.selected == Some(i), &s.path).clicked() {
                    self.selected = Some(i);
                }
                ui.label(RichText::new(s.commit.to_short_string()).monospace());
                ui.label(match s.status {
                    '-' => "not initialized",
                    '+' => "different commit checked out",
                    'U' => "merge conflicts",
                    _ => "up to date",
                });
                ui.horizontal(|ui| {
                    if ui.small_button("Open").clicked() {
                        if let Some(m) = cx.module {
                            cx.push(Action::OpenRepo(m.work_dir().join(&s.path)));
                        }
                    }
                    if ui.small_button("Update").clicked() {
                        cx.run(GitRun::new("Update submodule", commands::submodule_update(&[&s.path], false)));
                    }
                    if ui.small_button("Remove…").clicked() {
                        let p = s.path.clone();
                        cx.open(Confirm::new("Remove submodule", format!("Remove submodule '{p}'?"), "Remove", move |cx| {
                            cx.run(GitRun::many(
                                "Remove submodule",
                                vec![GitArgs::new("submodule").arg("deinit").arg("-f").arg("--").arg(&p), GitArgs::new("rm").arg("-f").arg(&p)],
                            ));
                        }));
                    }
                });
                ui.end_row();
            }
        });
        if subs.is_empty() {
            ui.label(RichText::new("This repository has no submodules.").italics().color(palette.muted));
        }
        !ui.input(|i| i.key_pressed(egui::Key::Escape))
    }
}

#[derive(Default)]
pub struct AddSubmoduleDialog {
    url: String,
    path: String,
    branch: String,
    force: bool,
}

impl Dialog for AddSubmoduleDialog {
    fn title(&self) -> String {
        "Add submodule".into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        egui::Grid::new("addsub").num_columns(2).show(ui, |ui| {
            ui.label("Repository url");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.url).desired_width(300.0));
                if ui.button("…").clicked() {
                    if let Some(p) = crate::util::pick_folder(None) {
                        self.url = p;
                    }
                }
            });
            ui.end_row();
            ui.label("Local path");
            ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(300.0));
            ui.end_row();
            ui.label("Branch");
            ui.add(egui::TextEdit::singleline(&mut self.branch).desired_width(300.0));
            ui.end_row();
        });
        if self.path.is_empty() && !self.url.is_empty() {
            self.path = gitext_core::url_util::clone_directory_name(&self.url);
        }
        ui.checkbox(&mut self.force, "Force");
        let (ok, cancel) = ok_cancel(ui, "Add", !self.url.trim().is_empty() && !self.path.trim().is_empty());
        if ok {
            let local_file = std::path::Path::new(self.url.trim()).exists();
            let b = self.branch.trim();
            cx.run(GitRun::new("Add submodule", commands::add_submodule(self.url.trim(), self.path.trim(), (!b.is_empty()).then_some(b), self.force, local_file)));
            return false;
        }
        !cancel
    }
}
