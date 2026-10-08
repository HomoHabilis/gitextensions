//! Port of `FormReflog`.

use egui::{RichText, Ui, Vec2};
use gitext_core::module::RefLogItem;

use super::{Action, Cx, Dialog, DialogKind};
use crate::tasks::Loader;

#[derive(Default)]
pub struct ReflogDialog {
    git_ref: String,
    items: Loader<String, Vec<RefLogItem>>,
}

impl Dialog for ReflogDialog {
    fn title(&self) -> String {
        "Reflog".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(860.0, 560.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let Some(m) = cx.module.cloned() else { return false };
        if self.git_ref.is_empty() {
            self.git_ref = "HEAD".into();
        }
        let refs: Vec<String> = std::iter::once("HEAD".to_string()).chain(cx.data.map(|d| d.branch_names(true)).unwrap_or_default()).collect();
        ui.horizontal(|ui| {
            ui.label("Reference");
            egui::ComboBox::from_id_salt("reflog_ref").selected_text(&self.git_ref).width(260.0).height(400.0).show_ui(ui, |ui| {
                for r in &refs {
                    ui.selectable_value(&mut self.git_ref, r.clone(), r);
                }
            });
        });
        ui.separator();
        let key = self.git_ref.clone();
        let items = self.items.request(cx.ctx, key.clone(), move || m.get_reflog(&key)).cloned();
        match items {
            None => {
                ui.spinner();
            }
            Some(items) => {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    egui::Grid::new("reflog").striped(true).num_columns(3).show(ui, |ui| {
                        for it in &items {
                            let r = ui.add(egui::Label::new(RichText::new(it.object_id.to_short_string()).monospace()).sense(egui::Sense::click()));
                            if r.clicked() {
                                cx.push(Action::SelectRevision(it.object_id));
                            }
                            r.context_menu(|ui| {
                                if ui.button("Select in revision grid").clicked() {
                                    cx.push(Action::SelectRevision(it.object_id));
                                    ui.close_kind(egui::UiKind::Menu);
                                }
                                if ui.button("Create branch here…").clicked() {
                                    cx.open(super::branch::CreateBranchDialog::new(it.object_id));
                                    ui.close_kind(egui::UiKind::Menu);
                                }
                                if ui.button("Reset current branch here…").clicked() {
                                    if let Some(d) = cx.data {
                                        cx.open(super::reset::ResetBranchDialog::new(d, it.object_id));
                                    }
                                    ui.close_kind(egui::UiKind::Menu);
                                }
                                if ui.button("Copy hash").clicked() {
                                    cx.push(Action::Copy(it.object_id.to_string()));
                                    ui.close_kind(egui::UiKind::Menu);
                                }
                            });
                            ui.label(RichText::new(&it.selector).monospace());
                            ui.label(&it.subject);
                            ui.end_row();
                        }
                    });
                });
            }
        }
        !ui.input(|i| i.key_pressed(egui::Key::Escape))
    }
}
