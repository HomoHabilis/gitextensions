//! Port of `FormInit`.

use egui::Ui;
use gitext_core::GitModule;

use super::{ok_cancel, Action, Cx, Dialog};
use crate::i18n::tr;

pub struct InitDialog {
    directory: String,
    bare: bool,
    shared: bool,
    error: Option<String>,
}

impl InitDialog {
    pub fn new(directory: Option<String>) -> Self {
        InitDialog { directory: directory.unwrap_or_default(), bare: false, shared: false, error: None }
    }
}

impl Dialog for InitDialog {
    fn title(&self) -> String {
        tr("Create new repository").into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.label(tr("Directory"));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.directory).desired_width(330.0));
            if ui.button("…").clicked() {
                if let Some(p) = crate::util::pick_folder(None) {
                    self.directory = p;
                }
            }
        });
        ui.radio_value(&mut self.bare, false, tr("Personal repository (with working directory)"));
        ui.radio_value(&mut self.bare, true, tr("Central repository, no working directory (--bare)"));
        ui.checkbox(&mut self.shared, tr("Shared (group writable)"));
        if let Some(e) = &self.error {
            ui.colored_label(egui::Color32::RED, e);
        }
        let (ok, cancel) = ok_cancel(ui, tr("Create"), !self.directory.trim().is_empty());
        if ok {
            match GitModule::init(self.directory.trim(), self.bare, self.shared) {
                Ok(_) => {
                    cx.push(Action::OpenRepo(self.directory.trim().into()));
                    return false;
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        }
        !cancel
    }
}
