//! Diff and merge tool settings (port of the tool part of `GitConfigSettingsPage`) and the
//! dialog shown when no tool is installed.

use std::path::PathBuf;

use egui::{RichText, Ui};
use gitext_core::diff_tools::{self, ToolConfigStore, ToolConfiguration, ToolType};
use gitext_core::exec::GitResult;

use super::{Cx, Dialog};
use crate::theme::Palette;

/// Editor for the diff or the merge tool of the global git config.
pub struct ToolEdit {
    pub tool_type: ToolType,
    name: String,
    path: String,
    command: String,
    loaded: (String, String, String),
    /// Installed tools (detected once).
    detected: Option<Vec<(&'static str, PathBuf)>>,
    /// Whether the executable of the path / command exists, for the text it was checked for.
    exists: Option<(String, bool)>,
    message: Option<String>,
}

impl ToolEdit {
    pub fn load(store: &ToolConfigStore, tool_type: ToolType) -> Self {
        let name = store.configured_tool(tool_type);
        let path = store.tool_setting(&name, tool_type, "path");
        let command = store.tool_setting(&name, tool_type, "cmd");
        ToolEdit {
            tool_type,
            loaded: (name.clone(), path.clone(), command.clone()),
            name,
            path,
            command,
            detected: None,
            exists: None,
            message: None,
        }
    }

    pub fn is_changed(&self) -> bool {
        (self.name.trim(), self.path.trim(), self.command.trim()) != (self.loaded.0.trim(), self.loaded.1.trim(), self.loaded.2.trim())
    }

    /// Writes the global git config (`ConfigureDiffMergeTool` / `UnsetCurrentTool`).
    pub fn save(&mut self, store: &ToolConfigStore) -> GitResult<()> {
        if !self.is_changed() {
            return Ok(());
        }
        if self.name.trim().is_empty() {
            store.unset(self.tool_type)?;
        } else {
            store.configure(&self.name, self.tool_type, &self.path, &self.command)?;
        }
        self.loaded = (self.name.clone(), self.path.clone(), self.command.clone());
        Ok(())
    }

    fn detected(&mut self) -> &[(&'static str, PathBuf)] {
        let t = self.tool_type;
        self.detected.get_or_insert_with(|| diff_tools::detect(t).into_iter().map(|(tool, p)| (tool.name, p)).collect())
    }

    /// Fills the path and the command of `name` (as when a tool is chosen in Git Extensions).
    fn select(&mut self, name: &str, path: Option<String>) {
        self.name = name.to_string();
        let found = |tool: &diff_tools::DiffMergeTool| tool.find();
        if let Some(c) = diff_tools::load_tool_config(name, path.as_deref(), None, &found) {
            if diff_tools::get(name).is_some() {
                self.path = c.path.clone();
                self.command = c.full_command(self.tool_type).to_string();
            } else if let Some(p) = path {
                self.path = p;
            }
        }
        self.exists = None;
    }

    /// Rebuilds the command from the tool and the path.
    fn suggest_command(&mut self) {
        if let Some(tool) = diff_tools::get(&self.name) {
            let path = if self.path.trim().is_empty() { tool.exe_file_name().to_string() } else { self.path.trim().to_string() };
            let c = ToolConfiguration::new(tool.exe_file_name(), &path, tool.command(ToolType::Diff), tool.command(ToolType::Merge));
            self.command = c.full_command(self.tool_type).to_string();
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let palette = Palette::for_ui(ui);
        let t = self.tool_type;
        let label = match t {
            ToolType::Diff => "Diff tool",
            ToolType::Merge => "Merge tool",
        };
        let detected: Vec<(&'static str, PathBuf)> = self.detected().to_vec();
        egui::Grid::new(("tool_grid", label)).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label(label);
            ui.horizontal(|ui| {
                let shown = diff_tools::get(&self.name).map(|tool| tool.display_name.to_string()).unwrap_or_else(|| {
                    if self.name.is_empty() {
                        "(none)".to_string()
                    } else {
                        self.name.clone()
                    }
                });
                let mut choice: Option<(String, Option<String>)> = None;
                egui::ComboBox::from_id_salt(("tool_combo", label)).selected_text(shown).width(230.0).show_ui(ui, |ui| {
                    if ui.selectable_label(self.name.is_empty(), "(none)").clicked() {
                        choice = Some((String::new(), None));
                    }
                    for tool in diff_tools::tools_for(t) {
                        let found = detected.iter().find(|(n, _)| *n == tool.name);
                        let text = if found.is_some() { format!("✔ {}", tool.display_name) } else { tool.display_name.to_string() };
                        let r = ui.selectable_label(self.name.eq_ignore_ascii_case(tool.name), text);
                        let r = match found {
                            Some((_, p)) => r.on_hover_text(p.display().to_string()),
                            None => r.on_hover_text("Not found on this computer"),
                        };
                        if r.clicked() {
                            choice = Some((tool.name.to_string(), found.map(|(_, p)| p.display().to_string())));
                        }
                    }
                });
                if let Some((name, path)) = choice {
                    if name.is_empty() {
                        self.name.clear();
                        self.path.clear();
                        self.command.clear();
                        self.exists = None;
                    } else {
                        self.select(&name, path);
                    }
                }
                ui.label(RichText::new("name").small().color(palette.muted));
                ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(110.0)).on_hover_text("The git tool name (--tool=<name>); any name for a custom tool");
            });
            ui.end_row();

            ui.label("Path");
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(380.0).hint_text("executable of the tool"));
                if r.changed() {
                    self.exists = None;
                }
                if ui.button("Browse…").clicked() {
                    let start = PathBuf::from(self.path.trim());
                    let start = start.parent().filter(|p| p.is_dir()).map(|p| p.display().to_string());
                    let filter: Option<(&str, &[&str])> = if cfg!(windows) { Some(("Programs", &["exe", "cmd", "bat"])) } else { None };
                    if let Some(p) = crate::util::pick_file(start.as_deref(), filter) {
                        self.path = p;
                        self.suggest_command();
                        self.exists = None;
                    }
                }
                if ui.button("Detect").on_hover_text("Look for the tool in the usual install locations").clicked() {
                    self.detected = None;
                    let detected = self.detected().to_vec();
                    let current = detected.iter().find(|(n, _)| n.eq_ignore_ascii_case(self.name.trim())).cloned();
                    match current.or_else(|| detected.first().cloned()) {
                        Some((name, p)) => {
                            self.select(name, Some(p.display().to_string()));
                            self.message = None;
                        }
                        None => self.message = Some(format!("No {} tool was found. {}", t.label(), diff_tools::install_hint())),
                    }
                }
            });
            ui.end_row();

            ui.label("Command");
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.command).desired_width(380.0).font(egui::TextStyle::Monospace));
                if r.changed() {
                    self.exists = None;
                }
                if ui.add_enabled(diff_tools::get(&self.name).is_some(), egui::Button::new("Suggest")).on_hover_text("The command line for this tool and path").clicked() {
                    self.suggest_command();
                }
            });
            ui.end_row();
        });

        // status
        let exe = if !self.command.trim().is_empty() { diff_tools::command_executable(&self.command) } else { self.path.trim().to_string() };
        if self.exists.as_ref().is_none_or(|(e, _)| *e != exe) {
            let ok = if exe.is_empty() { diff_tools::get(&self.name).is_some_and(|tool| tool.find().is_some()) } else { diff_tools::executable_exists(&exe) };
            self.exists = Some((exe.clone(), ok));
        }
        let ok = self.exists.as_ref().is_some_and(|(_, ok)| *ok);
        if let Some(m) = &self.message {
            ui.label(RichText::new(m).color(palette.warning));
        } else if self.name.trim().is_empty() {
            if detected.is_empty() {
                ui.label(RichText::new(format!("No {} tool is configured or installed. {}", t.label(), diff_tools::install_hint())).color(palette.warning));
            } else {
                let names: Vec<&str> = detected.iter().filter_map(|(n, _)| diff_tools::get(n).map(|tool| tool.display_name)).collect();
                ui.label(
                    RichText::new(format!("No {} tool is configured; {} will be used. Installed: {}.", t.label(), names[0], names.join(", "))).color(palette.muted),
                );
            }
        } else if ok {
            ui.label(RichText::new(format!("✔ {}", if exe.is_empty() { "found" } else { exe.as_str() })).color(palette.success));
        } else {
            let fallback = detected.iter().find(|(n, _)| !n.eq_ignore_ascii_case(self.name.trim())).and_then(|(n, _)| diff_tools::get(n));
            let mut text = format!("⚠ The executable '{exe}' was not found.");
            if let Some(f) = fallback {
                text.push_str(&format!(" {} will be used instead.", f.display_name));
            } else {
                text.push(' ');
                text.push_str(diff_tools::install_hint());
            }
            ui.label(RichText::new(text).color(palette.warning));
        }
    }
}

/// Shown when a diff or merge tool is needed and none is configured or installed.
pub struct NoToolDialog {
    pub tool_type: ToolType,
}

impl Dialog for NoToolDialog {
    fn title(&self) -> String {
        format!("No {} tool", self.tool_type.label())
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.set_max_width(520.0);
        ui.label(format!("No external {} tool is configured or installed.", self.tool_type.label()));
        ui.add_space(4.0);
        ui.label(diff_tools::install_hint());
        if !cfg!(windows) && !cfg!(target_os = "macos") {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Install Meld:");
                ui.code("sudo apt install meld");
                if ui.small_button("Copy").clicked() {
                    cx.push(super::Action::Copy("sudo apt install meld".into()));
                }
            });
        }
        ui.add_space(8.0);
        let mut keep = true;
        ui.horizontal(|ui| {
            if ui.button("Open settings").clicked() {
                cx.open(super::settings::SettingsDialog::at_page(5));
                keep = false;
            }
            if ui.button("Close").clicked() {
                keep = false;
            }
        });
        keep
    }
}
