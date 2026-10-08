//! Generic text file editor (`FormEditor`, used for .gitattributes, .mailmap, .git/config) and
//! the sparse working copy dialog (`FormSparseWorkingCopy`).

use std::path::PathBuf;

use egui::{RichText, Ui, Vec2};
use gitext_core::GitArgs;

use super::{ok_cancel, Action, Cx, Dialog, DialogKind, GitRun};

pub struct FileEditorDialog {
    title: String,
    relative: Option<String>,
    path: Option<PathBuf>,
    text: Option<String>,
}

impl FileEditorDialog {
    /// Edits a file relative to the working directory.
    pub fn work_file(relative: &str, title: &str) -> Self {
        FileEditorDialog { title: title.into(), relative: Some(relative.into()), path: None, text: None }
    }

    pub fn path(path: PathBuf, title: &str) -> Self {
        FileEditorDialog { title: title.into(), relative: None, path: Some(path), text: None }
    }
}

impl Dialog for FileEditorDialog {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(720.0, 560.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let path = match (&self.path, &self.relative, cx.module) {
            (Some(p), _, _) => p.clone(),
            (None, Some(r), Some(m)) => m.work_dir().join(r),
            _ => return false,
        };
        let text = self.text.get_or_insert_with(|| std::fs::read_to_string(&path).unwrap_or_default());
        ui.label(RichText::new(path.display().to_string()).small());
        let h = ui.available_height() - 50.0;
        egui::ScrollArea::vertical().max_height(h).show(ui, |ui| {
            ui.add_sized(Vec2::new(ui.available_width(), h), egui::TextEdit::multiline(text).font(egui::TextStyle::Monospace).code_editor());
        });
        let (ok, cancel) = ok_cancel(ui, "Save", true);
        if ok {
            if let Err(e) = std::fs::write(&path, text.as_bytes()) {
                cx.error("Save", e.to_string());
                return true;
            }
            cx.push(Action::RefreshStatus);
            return false;
        }
        !cancel
    }
}

#[derive(Default)]
pub struct SparseCheckoutDialog {
    enabled: Option<bool>,
    patterns: String,
}

impl Dialog for SparseCheckoutDialog {
    fn title(&self) -> String {
        "Sparse working copy".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(620.0, 480.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let Some(m) = cx.module else { return false };
        if self.enabled.is_none() {
            let p = m.sparse_checkout_patterns();
            self.enabled = Some(p.is_some());
            self.patterns = p.unwrap_or_else(|| "/*\n".into());
        }
        let enabled = self.enabled.as_mut().unwrap();
        ui.checkbox(enabled, "Enable sparse working copy");
        ui.label("Patterns (one per line, like .gitignore):");
        ui.add_enabled(*enabled, egui::TextEdit::multiline(&mut self.patterns).font(egui::TextStyle::Monospace).desired_rows(14).desired_width(f32::INFINITY));
        let (ok, cancel) = ok_cancel(ui, "Save and refresh", true);
        if ok {
            if *enabled {
                let _ = std::fs::create_dir_all(m.git_dir().join("info"));
                let _ = std::fs::write(m.git_dir().join("info").join("sparse-checkout"), &self.patterns);
                cx.run(GitRun::many(
                    "Sparse checkout",
                    vec![GitArgs::new("config").arg("core.sparseCheckout").arg("true"), GitArgs::new("read-tree").arg("-mu").arg("HEAD")],
                ));
            } else {
                cx.run(GitRun::new("Sparse checkout", GitArgs::new("sparse-checkout").arg("disable")));
            }
            return false;
        }
        !cancel
    }
}
