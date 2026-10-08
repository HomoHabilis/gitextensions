//! Port of `DashboardControl` (start page): recent and favourite repositories, and the
//! open / clone / create commands.

use std::path::PathBuf;

use egui::{RichText, Ui, Vec2};
use gitext_core::repo_history::{RepositoryAnchor, RepositoryHistory};

use crate::theme::Palette;

pub enum DashboardCommand {
    Open(PathBuf),
    OpenDialog,
    Clone,
    Init,
    RemoveRecent(String),
    Pin(String, bool),
    SetCategory(String, Option<String>),
    ShowInFolder(String),
    Settings,
}

#[derive(Default)]
pub struct Dashboard {
    filter: String,
    new_category: String,
}

impl Dashboard {
    pub fn ui(&mut self, ui: &mut Ui, history: &RepositoryHistory, sort_alphabetically: bool) -> Option<DashboardCommand> {
        let palette = Palette::for_ui(ui);
        let mut cmd = None;
        egui::SidePanel::left("dashboard_actions").resizable(false).exact_width(270.0).show_inside(ui, |ui| {
            ui.add_space(14.0);
            ui.label(RichText::new("Git Extensions").size(26.0).strong());
            ui.label(RichText::new(format!("version {}", env!("CARGO_PKG_VERSION"))).color(palette.muted));
            ui.add_space(24.0);
            let big = |ui: &mut Ui, icon: &str, text: &str, hint: &str| {
                ui.add_sized(Vec2::new(240.0, 44.0), egui::Button::new(RichText::new(format!("{icon}  {text}")).size(16.0))).on_hover_text(hint).clicked()
            };
            if big(ui, "🗁", "Open repository", "Open an existing repository") {
                cmd = Some(DashboardCommand::OpenDialog);
            }
            ui.add_space(6.0);
            if big(ui, "⬇", "Clone repository", "Clone a remote repository") {
                cmd = Some(DashboardCommand::Clone);
            }
            ui.add_space(6.0);
            if big(ui, "✚", "Create new repository", "Initialise a new repository") {
                cmd = Some(DashboardCommand::Init);
            }
            ui.add_space(24.0);
            if ui.link("⚙ Settings").clicked() {
                cmd = Some(DashboardCommand::Settings);
            }
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.heading("Recent repositories");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("🔍 Filter").desired_width(220.0));
                });
            });
            ui.separator();
            let filter = self.filter.to_lowercase();
            let (top, rest) = history.split_recent(sort_alphabetically);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let mut show = |ui: &mut Ui, repos: Vec<&gitext_core::repo_history::Repository>, cmd: &mut Option<DashboardCommand>| {
                    for r in repos.into_iter().filter(|r| filter.is_empty() || r.path.to_lowercase().contains(&filter)) {
                        let exists = std::path::Path::new(&r.path).is_dir();
                        let resp = ui
                            .horizontal(|ui| {
                                let name = RichText::new(r.name()).size(16.0).strong();
                                let name = if exists { name } else { name.color(palette.muted).strikethrough() };
                                let b = ui.add(egui::Button::new(name).frame(false));
                                ui.label(RichText::new(&r.path).color(palette.muted));
                                if r.anchor == RepositoryAnchor::AnchoredInTop {
                                    ui.label("📌");
                                }
                                b
                            })
                            .inner;
                        if resp.clicked() {
                            *cmd = Some(DashboardCommand::Open(PathBuf::from(&r.path)));
                        }
                        let path = r.path.clone();
                        resp.context_menu(|ui| {
                            if ui.button("Open").clicked() {
                                *cmd = Some(DashboardCommand::Open(PathBuf::from(&path)));
                                ui.close_kind(egui::UiKind::Menu);
                            }
                            let pinned = r.anchor == RepositoryAnchor::AnchoredInTop;
                            if ui.button(if pinned { "Unpin" } else { "Pin to top" }).clicked() {
                                *cmd = Some(DashboardCommand::Pin(path.clone(), !pinned));
                                ui.close_kind(egui::UiKind::Menu);
                            }
                            ui.menu_button("Move to category", |ui| {
                                for c in history.categories() {
                                    if ui.button(&c).clicked() {
                                        *cmd = Some(DashboardCommand::SetCategory(path.clone(), Some(c.clone())));
                                        ui.close_kind(egui::UiKind::Menu);
                                    }
                                }
                                ui.horizontal(|ui| {
                                    ui.add(egui::TextEdit::singleline(&mut self.new_category).hint_text("New category").desired_width(120.0));
                                    if ui.button("Add").clicked() && !self.new_category.trim().is_empty() {
                                        *cmd = Some(DashboardCommand::SetCategory(path.clone(), Some(self.new_category.trim().to_string())));
                                        self.new_category.clear();
                                        ui.close_kind(egui::UiKind::Menu);
                                    }
                                });
                            });
                            if ui.button("Show in folder").clicked() {
                                *cmd = Some(DashboardCommand::ShowInFolder(path.clone()));
                                ui.close_kind(egui::UiKind::Menu);
                            }
                            if ui.button("Remove from list").clicked() {
                                *cmd = Some(DashboardCommand::RemoveRecent(path.clone()));
                                ui.close_kind(egui::UiKind::Menu);
                            }
                        });
                        ui.add_space(4.0);
                    }
                };
                show(ui, top, &mut cmd);
                show(ui, rest, &mut cmd);
                if history.recent.is_empty() {
                    ui.add_space(20.0);
                    ui.label(RichText::new("No recent repositories yet. Open, clone or create a repository to get started.").italics().color(palette.muted));
                }
                for category in history.categories() {
                    ui.add_space(12.0);
                    ui.heading(&category);
                    ui.separator();
                    for r in history.favourites.iter().filter(|r| r.category.as_deref() == Some(category.as_str())) {
                        let resp = ui.horizontal(|ui| {
                            let b = ui.add(egui::Button::new(RichText::new(r.name()).size(16.0).strong()).frame(false));
                            ui.label(RichText::new(&r.path).color(palette.muted));
                            b
                        });
                        if resp.inner.clicked() {
                            cmd = Some(DashboardCommand::Open(PathBuf::from(&r.path)));
                        }
                        let path = r.path.clone();
                        resp.inner.context_menu(|ui| {
                            if ui.button("Remove from category").clicked() {
                                cmd = Some(DashboardCommand::SetCategory(path.clone(), None));
                                ui.close_kind(egui::UiKind::Menu);
                            }
                        });
                    }
                }
            });
        });
        cmd
    }
}
