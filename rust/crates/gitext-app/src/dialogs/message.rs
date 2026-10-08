//! Message box (port of `MessageBoxes`).

use egui::{RichText, Ui};

use super::{Cx, Dialog, DialogKind};
use crate::theme::Palette;

pub struct MessageDialog {
    pub title: String,
    pub text: String,
    pub error: bool,
}

impl Dialog for MessageDialog {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn id(&self) -> String {
        format!("msg:{}:{}", self.title, self.text)
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Modal(460.0)
    }

    fn ui(&mut self, ui: &mut Ui, _cx: &mut Cx) -> bool {
        let palette = Palette::for_ui(ui);
        ui.horizontal_top(|ui| {
            ui.label(RichText::new(if self.error { "⚠" } else { "ℹ" }).size(26.0).color(if self.error { palette.error } else { palette.lanes[0] }));
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                ui.add(egui::Label::new(&self.text).wrap());
            });
        });
        ui.add_space(6.0);
        let mut keep = true;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("  OK  ").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape)) {
                keep = false;
            }
        });
        keep
    }
}
