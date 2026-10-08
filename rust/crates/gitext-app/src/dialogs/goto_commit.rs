//! Port of `FormGoToCommit`.

use egui::Ui;

use super::{ok_cancel, Action, Cx, Dialog};

#[derive(Default)]
pub struct GoToCommitDialog {
    text: String,
    error: Option<String>,
}

impl Dialog for GoToCommitDialog {
    fn title(&self) -> String {
        "Go to commit".into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.label("Commit hash, branch, tag or any revision expression:");
        ui.add(egui::TextEdit::singleline(&mut self.text).desired_width(380.0).font(egui::TextStyle::Monospace)).request_focus();
        if let Some(d) = cx.data {
            let tags: Vec<String> = d.tags().map(|t| t.name.clone()).filter(|t| !self.text.is_empty() && t.contains(&self.text)).take(8).collect();
            for t in tags {
                if ui.link(&t).clicked() {
                    self.text = t;
                }
            }
        }
        if let Some(e) = &self.error {
            ui.colored_label(egui::Color32::RED, e);
        }
        let (ok, cancel) = ok_cancel(ui, "Go", !self.text.trim().is_empty());
        if ok || (ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.text.trim().is_empty()) {
            let id = cx.module.map(|m| m.rev_parse(self.text.trim())).unwrap_or_default();
            if id.is_zero() {
                self.error = Some("Revision not found".into());
                return true;
            }
            cx.push(Action::SelectRevision(id));
            return false;
        }
        !cancel
    }
}
