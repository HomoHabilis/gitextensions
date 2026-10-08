//! Port of `FormGitCommandLog`.

use egui::{RichText, Ui, Vec2};
use gitext_core::exec::{clear_command_log, command_log_entries};

use super::{Action, Cx, Dialog, DialogKind};

pub struct CommandLogDialog;

impl Dialog for CommandLogDialog {
    fn title(&self) -> String {
        "Git command log".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(960.0, 520.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let entries = command_log_entries();
        ui.horizontal(|ui| {
            ui.label(format!("{} commands", entries.len()));
            if ui.button("Clear").clicked() {
                clear_command_log();
            }
            if ui.button("Copy all").clicked() {
                cx.push(Action::Copy(entries.iter().map(|e| e.column_line()).collect::<Vec<_>>().join("\n")));
            }
        });
        ui.separator();
        egui::ScrollArea::both().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            for e in &entries {
                ui.label(RichText::new(e.column_line()).monospace()).on_hover_text(&e.working_dir);
            }
        });
        cx.ctx.request_repaint_after(std::time::Duration::from_millis(500));
        !ui.input(|i| i.key_pressed(egui::Key::Escape))
    }
}
