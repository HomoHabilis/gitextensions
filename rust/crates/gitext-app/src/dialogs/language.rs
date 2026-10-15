//! Port of `FormChooseTranslation`: shown at the first start to pick the language of the UI.

use egui::{RichText, Ui};

use super::{Action, Cx, Dialog, DialogKind};
use crate::i18n::{self, tr, LANGUAGES};

pub struct LanguageDialog;

impl Dialog for LanguageDialog {
    fn title(&self) -> String {
        tr("Choose language").into()
    }

    fn id(&self) -> String {
        "choose-language".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Modal(460.0)
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let mut chosen = None;
        egui::ScrollArea::vertical().max_height(super::max_content_height(ui)).show(ui, |ui| {
            egui::Grid::new("languages").num_columns(2).spacing([24.0, 2.0]).show(ui, |ui| {
                for (i, lang) in LANGUAGES.iter().enumerate() {
                    if ui.selectable_label(cx.settings.translation == lang.name, lang.label()).clicked() {
                        chosen = Some(lang.name);
                    }
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        });
        ui.add_space(6.0);
        ui.label(RichText::new(tr("The language can be changed later in Settings > Appearance.")).small());
        let close = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if chosen.is_none() && !close {
            return true;
        }
        // closing without a choice keeps English, as upstream
        cx.settings.translation = chosen.unwrap_or("English").to_string();
        i18n::set_language(&cx.settings.translation);
        cx.push(Action::SaveSettings);
        cx.push(Action::ApplyTheme);
        false
    }
}
