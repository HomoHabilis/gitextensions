//! Port of `FormArchive`.

use egui::Ui;
use gitext_core::{GitArgs, ObjectId};

use super::{ok_cancel, Cx, Dialog, GitRun};

pub struct ArchiveDialog {
    revision: String,
    format: String,
    path_filter: String,
    output: String,
}

impl ArchiveDialog {
    pub fn new(id: ObjectId) -> Self {
        ArchiveDialog { revision: if id.is_zero() { "HEAD".into() } else { id.to_string() }, format: "zip".into(), path_filter: String::new(), output: String::new() }
    }
}

impl Dialog for ArchiveDialog {
    fn title(&self) -> String {
        "Archive".into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        egui::Grid::new("archive").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Revision");
            ui.add(egui::TextEdit::singleline(&mut self.revision).desired_width(320.0));
            ui.end_row();
            ui.label("Format");
            ui.horizontal(|ui| {
                for f in ["zip", "tar", "tar.gz"] {
                    ui.radio_value(&mut self.format, f.to_string(), f);
                }
            });
            ui.end_row();
            ui.label("Only path");
            ui.add(egui::TextEdit::singleline(&mut self.path_filter).hint_text("optional").desired_width(320.0));
            ui.end_row();
            ui.label("Save as");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.output).desired_width(260.0));
                if ui.button("…").clicked() {
                    let name = format!("{}.{}", cx.module.map(|m| m.name()).unwrap_or_else(|| "archive".into()), self.format);
                    if let Some(p) = crate::util::save_file(None, &name) {
                        self.output = p;
                    }
                }
            });
            ui.end_row();
        });
        let (ok, cancel) = ok_cancel(ui, "Save", !self.output.trim().is_empty() && !self.revision.trim().is_empty());
        if ok {
            let args = GitArgs::new("archive")
                .arg(format!("--format={}", if self.format == "tar.gz" { "tar.gz" } else { &self.format }))
                .arg("-o")
                .arg(self.output.trim())
                .arg(self.revision.trim())
                .arg(self.path_filter.trim());
            cx.run(GitRun::new("Archive", args).no_refresh());
            return false;
        }
        !cancel
    }
}
