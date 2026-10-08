//! Port of `FormOpenDirectory` ("Open local repository"): a path with the recent repositories,
//! a parent folder button and the folder dialog. Typing the path also works where no file
//! dialog can be shown (WSL, minimal desktops).

use std::path::{Path, PathBuf};

use egui::{RichText, Ui, Vec2};
use gitext_core::GitModule;

use super::{Action, Cx, Dialog, DialogKind};
use crate::theme::Palette;

pub struct OpenRepoDialog {
    path: String,
    recent: Vec<String>,
    focus: bool,
}

impl OpenRepoDialog {
    pub fn new(start: Option<String>, recent: Vec<String>) -> Self {
        let path = start.or_else(|| recent.first().cloned()).unwrap_or_default();
        OpenRepoDialog { path, recent, focus: true }
    }
}

/// Port of `FormOpenDirectory.OpenGitRepository` validation: the folder, or the repository the
/// folder belongs to.
fn find_repository(path: &str) -> Option<PathBuf> {
    let p = Path::new(path.trim());
    if p.as_os_str().is_empty() || !p.is_dir() {
        return None;
    }
    p.ancestors().find(|a| GitModule::is_valid_git_working_dir(a)).map(Path::to_path_buf)
}

impl Dialog for OpenRepoDialog {
    fn title(&self) -> String {
        "Open local repository".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(640.0, 150.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let palette = Palette::for_ui(ui);
        let mut open = false;
        let mut keep = true;
        ui.horizontal(|ui| {
            ui.label("Directory");
            let w = ui.available_width() - 150.0;
            let r = ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(w - 26.0).hint_text(if cfg!(windows) {
                r"C:\src\repo or \\wsl$\Ubuntu\home\me\repo"
            } else {
                "/home/me/src/repo"
            }));
            if std::mem::take(&mut self.focus) {
                r.request_focus();
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                open = true;
            }
            // recent repositories (the combo box of the original)
            egui::ComboBox::from_id_salt("open_recent").selected_text("").width(18.0).show_ui(ui, |ui| {
                ui.set_min_width(420.0);
                for r in &self.recent {
                    if ui.selectable_label(*r == self.path, r).clicked() {
                        self.path = r.clone();
                    }
                }
            });
            if ui.button("⬆").on_hover_text("Go to parent directory").clicked() {
                if let Some(parent) = Path::new(self.path.trim()).parent() {
                    self.path = parent.display().to_string();
                }
            }
            if ui.button("Browse…").clicked() {
                let start = Some(self.path.trim()).filter(|p| Path::new(p).is_dir()).map(str::to_string);
                if let Some(p) = crate::util::pick_folder(start.as_deref()) {
                    self.path = p;
                }
            }
        });

        let repo = find_repository(&self.path);
        ui.add_space(4.0);
        if crate::util::file_dialog_unavailable() {
            ui.label(
                RichText::new("No folder dialog is available here: type or paste the path (installing zenity or kdialog adds the dialog).")
                    .small()
                    .color(palette.warning),
            );
        }
        if self.path.trim().is_empty() {
            ui.label(RichText::new("Choose the folder of a git repository.").small().color(palette.muted));
        } else if !Path::new(self.path.trim()).is_dir() {
            ui.label(RichText::new("This folder does not exist.").small().color(palette.warning));
        } else if repo.is_none() {
            ui.label(RichText::new("This folder is not in a git repository.").small().color(palette.warning));
        } else if let Some(r) = repo.as_deref().filter(|r| *r != Path::new(self.path.trim())) {
            ui.label(RichText::new(format!("Opens the repository {}", r.display())).small().color(palette.muted));
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
                // a folder git accepts but the file system check does not (e.g. a WSL path
                // with odd permissions) can still be tried
                let can_open = Path::new(self.path.trim()).is_dir();
                if ui.add_enabled(can_open, egui::Button::new(RichText::new("Open").strong())).clicked() {
                    open = true;
                }
            });
        });
        if open && Path::new(self.path.trim()).is_dir() {
            let path = repo.unwrap_or_else(|| PathBuf::from(self.path.trim()));
            cx.push(Action::OpenRepo(path));
            keep = false;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            keep = false;
        }
        keep
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_repository_of_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("src/deep")).unwrap();
        assert_eq!(find_repository(&repo.display().to_string()), Some(repo.clone()));
        assert_eq!(find_repository(&repo.join("src/deep").display().to_string()), Some(repo.clone()));
        assert_eq!(find_repository(&dir.path().display().to_string()), None);
        assert_eq!(find_repository(""), None);
        assert_eq!(find_repository("/no/such/folder"), None);
    }
}
