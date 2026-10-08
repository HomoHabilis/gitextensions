//! Port of `FormVerify`: recover lost objects (dangling commits, blobs).

use egui::{RichText, Ui, Vec2};
use gitext_core::{GitArgs, ObjectId};

use super::{Action, Cx, Dialog, DialogKind, GitRun};
use crate::tasks::Loader;

#[derive(Default)]
pub struct VerifyDialog {
    objects: Loader<u8, Vec<(ObjectId, String, String)>>,
    only_commits: bool,
}

impl Dialog for VerifyDialog {
    fn title(&self) -> String {
        "Recover lost objects".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(860.0, 520.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let Some(m) = cx.module.cloned() else { return false };
        let objects = self
            .objects
            .request(cx.ctx, 0, move || {
                m.lost_objects()
                    .into_iter()
                    .map(|(id, kind)| {
                        let subject = if kind == "commit" { m.get_revision(&id.to_string(), false).ok().flatten().map(|r| format!("{} ({})", r.subject, r.author)).unwrap_or_default() } else { String::new() };
                        (id, kind, subject)
                    })
                    .collect()
            })
            .cloned();
        ui.checkbox(&mut self.only_commits, "Show only commits");
        ui.separator();
        match objects {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Running git fsck…");
                });
            }
            Some(objs) => {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    egui::Grid::new("lost").striped(true).num_columns(4).show(ui, |ui| {
                        for (id, kind, subject) in objs.iter().filter(|o| !self.only_commits || o.1 == "commit") {
                            ui.label(RichText::new(id.to_short_string()).monospace());
                            ui.label(kind);
                            ui.label(subject);
                            ui.horizontal(|ui| {
                                if kind == "commit" {
                                    if ui.small_button("Create tag").clicked() {
                                        let name = format!("LOST_FOUND_{}", id.to_short_string());
                                        cx.run(GitRun::new("Create tag", GitArgs::new("tag").arg(name).arg(id.to_string())));
                                    }
                                    if ui.small_button("Create branch…").clicked() {
                                        cx.open(super::branch::CreateBranchDialog::new(*id));
                                    }
                                }
                                if ui.small_button("Copy hash").clicked() {
                                    cx.push(Action::Copy(id.to_string()));
                                }
                            });
                            ui.end_row();
                        }
                    });
                    if objs.is_empty() {
                        ui.label("No lost objects found.");
                    }
                });
            }
        }
        !ui.input(|i| i.key_pressed(egui::Key::Escape))
    }
}
