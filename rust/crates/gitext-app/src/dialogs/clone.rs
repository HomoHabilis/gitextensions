//! Port of `FormClone`.

use egui::Ui;
use gitext_core::settings::AppSettings;
use gitext_core::{commands, Executable, GitArgs};

use super::{ok_cancel, Action, Cx, Dialog, GitRun};
use crate::tasks::Task;

pub struct CloneDialog {
    url: String,
    destination: String,
    subdirectory: String,
    auto_subdirectory: bool,
    branch: String,
    branches: Vec<String>,
    load: Option<Task<Result<Vec<String>, String>>>,
    bare: bool,
    submodules: bool,
    shallow: bool,
    depth: u32,
    single_branch: bool,
    open_after: bool,
    error: Option<String>,
}

impl CloneDialog {
    pub fn new(url: Option<String>, settings: &AppSettings) -> Self {
        let url = url.unwrap_or_default();
        CloneDialog {
            subdirectory: gitext_core::url_util::clone_directory_name(&url),
            url,
            destination: settings.default_clone_destination.clone(),
            auto_subdirectory: true,
            branch: String::new(),
            branches: Vec::new(),
            load: None,
            bare: false,
            submodules: true,
            shallow: false,
            depth: 1,
            single_branch: false,
            open_after: true,
            error: None,
        }
    }
}

impl Dialog for CloneDialog {
    fn title(&self) -> String {
        "Clone".into()
    }

    fn kind(&self) -> super::DialogKind {
        super::DialogKind::Modal(560.0)
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        if let Some(t) = &mut self.load {
            if let Some(r) = t.try_take() {
                self.load = None;
                match r {
                    Ok(b) => self.branches = b,
                    Err(e) => self.error = Some(e),
                }
            }
        }
        egui::Grid::new("clone").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Repository to clone");
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.url).desired_width(360.0).hint_text("https://… or a local path"));
                if r.changed() && self.auto_subdirectory {
                    self.subdirectory = gitext_core::url_util::clone_directory_name(&self.url);
                }
                if ui.button("…").clicked() {
                    if let Some(p) = crate::util::pick_folder(None) {
                        self.url = p;
                        self.subdirectory = gitext_core::url_util::clone_directory_name(&self.url);
                    }
                }
            });
            ui.end_row();
            ui.label("Destination");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.destination).desired_width(360.0));
                if ui.button("…").clicked() {
                    if let Some(p) = crate::util::pick_folder(Some(&self.destination)) {
                        self.destination = p;
                    }
                }
            });
            ui.end_row();
            ui.label("Subdirectory to create");
            if ui.add(egui::TextEdit::singleline(&mut self.subdirectory).desired_width(360.0)).changed() {
                self.auto_subdirectory = false;
            }
            ui.end_row();
            ui.label("Branch");
            ui.horizontal(|ui| {
                super::branch_combo(ui, "clone_branch", &mut self.branch, &self.branches, 300.0);
                if self.load.is_some() {
                    ui.spinner();
                } else if ui.button("Load").on_hover_text("Load the remote branches").clicked() && !self.url.trim().is_empty() {
                    let url = self.url.trim().to_string();
                    self.load = Some(Task::spawn(cx.ctx, move || {
                        let exe = Executable::git(std::env::temp_dir());
                        exe.output(&GitArgs::new("ls-remote").arg("--heads").arg(url))
                            .map(|o| o.lines().filter_map(|l| l.split('\t').nth(1)).map(|r| r.trim_start_matches("refs/heads/").to_string()).collect())
                            .map_err(|e| e.to_string())
                    }));
                }
            });
            ui.end_row();
        });
        let target = std::path::Path::new(self.destination.trim()).join(self.subdirectory.trim());
        ui.label(egui::RichText::new(format!("The repository will be cloned to {}", target.display())).small());
        ui.checkbox(&mut self.bare, "Bare repository (no working directory)");
        ui.checkbox(&mut self.submodules, "Initialize all submodules");
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.shallow, "Shallow clone, depth:");
            ui.add_enabled(self.shallow, egui::DragValue::new(&mut self.depth).range(1..=100000));
        });
        ui.checkbox(&mut self.single_branch, "Clone only the selected branch");
        ui.checkbox(&mut self.open_after, "Open the repository after cloning");
        if let Some(e) = &self.error {
            ui.colored_label(egui::Color32::RED, e);
        }
        let valid = !self.url.trim().is_empty() && !self.destination.trim().is_empty() && !self.subdirectory.trim().is_empty();
        let (ok, cancel) = ok_cancel(ui, "Clone", valid);
        if ok {
            if target.exists() && std::fs::read_dir(&target).map(|mut d| d.next().is_some()).unwrap_or(false) {
                self.error = Some(format!("'{}' exists and is not empty.", target.display()));
                return true;
            }
            let _ = std::fs::create_dir_all(self.destination.trim());
            let branch = if self.branch.trim().is_empty() { Some("") } else { Some(self.branch.trim()) };
            let args = commands::clone(
                self.url.trim(),
                &target.display().to_string(),
                self.bare,
                self.submodules,
                branch,
                self.shallow.then_some(self.depth),
                self.single_branch.then_some(true),
            );
            let mut run = GitRun::new(format!("Clone {}", self.url.trim()), args).in_dir(self.destination.trim().into()).no_refresh();
            if self.open_after {
                run = run.then(Action::OpenRepo(target.clone()));
            }
            cx.settings.default_clone_destination = self.destination.trim().to_string();
            cx.push(Action::SaveSettings);
            cx.run(run);
            return false;
        }
        !cancel
    }
}
