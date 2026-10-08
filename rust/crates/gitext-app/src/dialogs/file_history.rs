//! Port of `FormFileHistory`: the commits of a file with diff, content and blame.

use egui::{RichText, Ui, Vec2};
use gitext_core::GitArgs;
use gitext_core::ObjectId;

use super::{Cx, Dialog, DialogKind};
use crate::tasks::Loader;
use crate::theme::Palette;
use crate::views::diff_viewer::{content_from_bytes, DiffViewer, ViewerContent};
use crate::views::revision_grid::RevisionGrid;

pub struct FileHistoryDialog {
    file: String,
    grid: RevisionGrid,
    started: bool,
    tab: usize,
    diff: Loader<(ObjectId, usize), ViewerContent>,
    viewer: DiffViewer,
    follow: bool,
}

impl FileHistoryDialog {
    pub fn new(file: String) -> Self {
        let mut grid = RevisionGrid::default();
        grid.filter.path = file.clone();
        grid.filter.follow = true;
        FileHistoryDialog { file, grid, started: false, tab: 0, diff: Loader::default(), viewer: DiffViewer::default(), follow: true }
    }
}

impl Dialog for FileHistoryDialog {
    fn title(&self) -> String {
        format!("File history - {}", self.file)
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(1100.0, 760.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let palette = Palette::for_ui(ui);
        let Some(m) = cx.module.cloned() else { return false };
        let data = cx.data.cloned().unwrap_or_default();
        if !self.started {
            self.started = true;
            let mut settings = cx.settings.clone();
            settings.show_artificial_commits = false;
            settings.branch_filter_mode = gitext_core::settings::BranchFilterMode::Current;
            self.grid.filter.follow = self.follow;
            self.grid.reload(cx.ctx, &m, &data, &settings);
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(&self.file).strong());
            if ui.checkbox(&mut self.follow, "Follow renames").changed() {
                self.started = false;
            }
            if self.grid.loading {
                ui.spinner();
            }
        });
        let selected = self.grid.selected_revision();
        egui::TopBottomPanel::bottom("fh_bottom").resizable(true).default_height(ui.available_height() * 0.55).show_inside(ui, |ui| {
            ui.set_min_height(ui.available_height());
            ui.horizontal(|ui| {
                for (i, l) in ["± Diff", "🗋 View", "Blame"].iter().enumerate() {
                    if ui.selectable_label(self.tab == i, *l).clicked() {
                        if i == 2 {
                            if let Some(id) = selected {
                                cx.open(super::blame::BlameDialog::new(self.file.clone(), id));
                            }
                        } else {
                            self.tab = i;
                        }
                    }
                }
            });
            ui.separator();
            let Some(id) = selected else {
                ui.label(RichText::new("Select a commit").color(palette.muted));
                return;
            };
            let file = self.file.clone();
            let follow = self.follow;
            let tab = self.tab;
            let mm = m.clone();
            let content = self
                .diff
                .request(cx.ctx, (id, tab), move || {
                    // name of the file in this commit (it may have been renamed)
                    let name = mm
                        .run(&GitArgs::new("log").arg("-1").arg_if(follow, "--follow").arg("--name-only").arg("--format=").arg(id.to_string()).arg("--").arg(&file))
                        .map(|r| r.stdout_str().lines().next().unwrap_or_default().trim().to_string())
                        .unwrap_or_default();
                    let name = if name.is_empty() { file.clone() } else { name };
                    if tab == 0 {
                        let out = mm
                            .run(&GitArgs::with_config(&gitext_core::commands::DIFF_CONFIGS, "log").args(["-1", "-p", "-M", "--format="]).arg_if(follow, "--follow").arg(id.to_string()).arg("--").arg(&file))
                            .map(|r| r.stdout_str())
                            .unwrap_or_default();
                        if out.trim().is_empty() { ViewerContent::Empty("No changes to this file".into()) } else { ViewerContent::Diff(out) }
                    } else {
                        match mm.get_file_bytes(id, &name) {
                            Ok(b) => content_from_bytes(&b),
                            Err(_) => ViewerContent::Empty(format!("{name} does not exist in this commit")),
                        }
                    }
                })
                .cloned()
                .unwrap_or(ViewerContent::Empty("Loading…".into()));
            self.viewer.ui(ui, &content, cx.settings.show_line_numbers, &[]);
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let mut settings = cx.settings.clone();
            settings.show_artificial_commits = false;
            let events = self.grid.ui(ui, &settings, &data);
            if let Some(id) = events.double_clicked {
                cx.push(super::Action::SelectRevision(id));
            }
        });
        !ui.input(|i| i.key_pressed(egui::Key::Escape))
    }
}
