//! Port of `FormSettings` (settings pages) and a git config editor.

use egui::{RichText, Ui, Vec2};
use gitext_core::commands::{GitRefsSortBy, LocalChangesAction};
use gitext_core::revision_reader::RevisionSortOrder;
use gitext_core::settings::{AppSettings, CommitTemplate, PullAction, Theme, UserScript};
use gitext_core::{Executable, GitArgs};

use super::{Action, Cx, Dialog, DialogKind};

const PAGES: [&str; 10] = [
    "General",
    "Appearance",
    "Revision graph",
    "Commit dialog",
    "Diff viewer",
    "Git",
    "Pull & push",
    "Commit templates",
    "Scripts",
    "Advanced",
];

#[derive(Default)]
pub struct SettingsDialog {
    page: usize,
    edit: Option<AppSettings>,
    global_name: Option<String>,
    global_email: Option<String>,
    global_editor: Option<String>,
    merge_tool: Option<String>,
    diff_tool: Option<String>,
}

fn global_get(key: &str) -> String {
    Executable::git(std::env::temp_dir())
        .run(&GitArgs::new("config").arg("--global").arg("--get").arg(key))
        .map(|r| r.stdout_str().trim().to_string())
        .unwrap_or_default()
}

fn global_set(key: &str, value: &str) {
    let exe = Executable::git(std::env::temp_dir());
    if value.trim().is_empty() {
        let _ = exe.run(&GitArgs::new("config").arg("--global").arg("--unset").arg(key));
    } else {
        let _ = exe.run(&GitArgs::new("config").arg("--global").arg(key).arg(value.trim()));
    }
}

impl Dialog for SettingsDialog {
    fn title(&self) -> String {
        "Settings".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(860.0, 600.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let s = self.edit.get_or_insert_with(|| cx.settings.clone());
        let mut keep = true;
        egui::SidePanel::left("settings_pages").resizable(false).exact_width(170.0).show_inside(ui, |ui| {
            for (i, p) in PAGES.iter().enumerate() {
                if ui.selectable_label(self.page == i, *p).clicked() {
                    self.page = i;
                }
            }
        });
        egui::TopBottomPanel::bottom("settings_buttons").show_inside(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                    let mut apply = false;
                    if ui.button(RichText::new("OK").strong()).clicked() {
                        apply = true;
                        keep = false;
                    }
                    if ui.button("Apply").clicked() {
                        apply = true;
                    }
                    if apply {
                        let reload = s.graph_config() != cx.settings.graph_config()
                            || s.show_git_notes != cx.settings.show_git_notes
                            || s.revision_sort_order != cx.settings.revision_sort_order
                            || s.max_revision_graph_commits != cx.settings.max_revision_graph_commits;
                        *cx.settings = s.clone();
                        gitext_core::exec::set_git_command(&s.git_command);
                        if let Some(n) = &self.global_name {
                            global_set("user.name", n);
                        }
                        if let Some(e) = &self.global_email {
                            global_set("user.email", e);
                        }
                        if let Some(e) = &self.global_editor {
                            global_set("core.editor", e);
                        }
                        if let Some(t) = &self.merge_tool {
                            global_set("merge.tool", t);
                        }
                        if let Some(t) = &self.diff_tool {
                            global_set("diff.tool", t);
                        }
                        cx.push(Action::SaveSettings);
                        cx.push(Action::ApplyTheme);
                        if reload {
                            cx.push(Action::ReloadLog);
                        }
                    }
                });
            });
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.heading(PAGES[self.page]);
                ui.add_space(6.0);
                match self.page {
                    0 => {
                        ui.checkbox(&mut s.close_process_dialog_on_success, "Close the process dialog when the command succeeds");
                        ui.checkbox(&mut s.follow_renames_in_file_history, "Follow renames in file history");
                        ui.checkbox(&mut s.check_for_uncommitted_changes_in_checkout, "Ask what to do with local changes on checkout");
                        ui.checkbox(&mut s.show_untracked_files, "Show untracked files in status and commit dialog");
                        ui.horizontal(|ui| {
                            ui.label("Recent repositories to remember");
                            ui.add(egui::DragValue::new(&mut s.recent_repositories_history_size).range(1..=200));
                        });
                        ui.checkbox(&mut s.sort_recent_repos_alphabetically, "Sort recent repositories alphabetically");
                        ui.horizontal(|ui| {
                            ui.label("Default clone destination");
                            ui.text_edit_singleline(&mut s.default_clone_destination);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Default action for local changes on checkout");
                            egui::ComboBox::from_id_salt("lca").selected_text(format!("{:?}", s.default_local_changes_action)).show_ui(ui, |ui| {
                                for a in [LocalChangesAction::DontChange, LocalChangesAction::Merge, LocalChangesAction::Stash, LocalChangesAction::Reset] {
                                    ui.selectable_value(&mut s.default_local_changes_action, a, format!("{a:?}"));
                                }
                            });
                        });
                    }
                    1 => {
                        ui.horizontal(|ui| {
                            ui.label("Theme");
                            ui.radio_value(&mut s.theme, Theme::System, "Follow system");
                            ui.radio_value(&mut s.theme, Theme::Light, "Light");
                            ui.radio_value(&mut s.theme, Theme::Dark, "Dark");
                        });
                        ui.horizontal(|ui| {
                            ui.label("Zoom");
                            ui.add(egui::Slider::new(&mut s.ui_scale, 0.5..=3.0).step_by(0.05));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Font size");
                            ui.add(egui::Slider::new(&mut s.font_size, 9.0..=24.0).step_by(0.5));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Code font size");
                            ui.add(egui::Slider::new(&mut s.monospace_font_size, 9.0..=24.0).step_by(0.5));
                        });
                        ui.separator();
                        ui.checkbox(&mut s.show_author_column, "Show author column");
                        ui.checkbox(&mut s.show_date_column, "Show date column");
                        ui.checkbox(&mut s.show_id_column, "Show commit hash column");
                        ui.checkbox(&mut s.show_relative_date, "Show relative dates");
                        ui.checkbox(&mut s.show_author_date, "Show author date (instead of commit date)");
                        ui.checkbox(&mut s.highlight_author_commits, "Highlight my commits");
                        ui.checkbox(&mut s.left_panel_visible, "Show the left panel");
                    }
                    2 => {
                        ui.checkbox(&mut s.merge_graph_lanes_having_common_parent, "Merge graph lanes having a common parent");
                        ui.checkbox(&mut s.render_graph_with_diagonals, "Render graph with diagonals");
                        ui.checkbox(&mut s.straighten_graph_diagonals, "Straighten graph diagonals");
                        ui.horizontal(|ui| {
                            ui.label("Straighten segments up to");
                            ui.add(egui::DragValue::new(&mut s.straighten_graph_segments_limit).range(0..=1000));
                            ui.label("segments per row");
                        });
                        ui.horizontal(|ui| {
                            ui.label("Lane width");
                            ui.add(egui::Slider::new(&mut s.lane_width, 8.0..=32.0));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Row height");
                            ui.add(egui::Slider::new(&mut s.row_height, 16.0..=40.0));
                        });
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("Sort order");
                            ui.radio_value(&mut s.revision_sort_order, RevisionSortOrder::GitDefault, "Git default");
                            ui.radio_value(&mut s.revision_sort_order, RevisionSortOrder::AuthorDate, "Author date");
                            ui.radio_value(&mut s.revision_sort_order, RevisionSortOrder::Topology, "Topology");
                        });
                        ui.horizontal(|ui| {
                            ui.label("Maximum number of commits (0 = all)");
                            ui.add(egui::DragValue::new(&mut s.max_revision_graph_commits).speed(1000));
                        });
                        ui.checkbox(&mut s.show_git_notes, "Show git notes");
                        ui.checkbox(&mut s.show_merge_commits, "Show merge commits");
                        ui.horizontal(|ui| {
                            ui.label("Sort branches in the left panel by");
                            egui::ComboBox::from_id_salt("refsort").selected_text(format!("{:?}", s.refs_sort_by)).show_ui(ui, |ui| {
                                for k in GitRefsSortBy::ALL {
                                    ui.selectable_value(&mut s.refs_sort_by, k, format!("{k:?}"));
                                }
                            });
                        });
                    }
                    3 => {
                        ui.checkbox(&mut s.remember_amend_commit_state, "Remember the amend state");
                        ui.checkbox(&mut s.ensure_commit_message_second_line_empty, "Ensure the second line of the message is empty");
                        ui.checkbox(&mut s.sign_off_by_default, "Sign-off commits by default");
                        ui.checkbox(&mut s.push_after_commit, "Push after commit");
                        ui.horizontal(|ui| {
                            ui.label("Max characters of the first line (0 = no limit)");
                            ui.add(egui::DragValue::new(&mut s.commit_validation_max_cnt_chars_first_line));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Max characters per line (0 = no limit)");
                            ui.add(egui::DragValue::new(&mut s.commit_validation_max_cnt_chars_per_line));
                        });
                        ui.checkbox(&mut s.commit_validation_second_line_must_be_empty, "Warn when the second line is not empty");
                        ui.horizontal(|ui| {
                            ui.label("Number of previous messages in history");
                            ui.add(egui::DragValue::new(&mut s.commit_message_history_size).range(0..=200));
                        });
                    }
                    4 => {
                        ui.checkbox(&mut s.ignore_whitespace, "Ignore whitespace");
                        ui.checkbox(&mut s.ignore_whitespace_changes, "Ignore whitespace changes");
                        ui.checkbox(&mut s.show_entire_file, "Show entire file");
                        ui.checkbox(&mut s.use_histogram_diff, "Use the histogram diff algorithm");
                        ui.checkbox(&mut s.show_line_numbers, "Show line numbers");
                        ui.horizontal(|ui| {
                            ui.label("Context lines");
                            ui.add(egui::DragValue::new(&mut s.context_lines).range(0..=1000));
                        });
                        ui.separator();
                        ui.label(RichText::new("Blame").strong());
                        ui.checkbox(&mut s.detect_copy_in_file_on_blame, "Detect moved and copied lines within a file (-M)");
                        ui.checkbox(&mut s.detect_copy_in_all_on_blame, "Detect moved and copied lines across files (-C)");
                        ui.checkbox(&mut s.ignore_whitespace_on_blame, "Ignore whitespace (-w)");
                    }
                    5 => {
                        ui.horizontal(|ui| {
                            ui.label("Git command");
                            ui.text_edit_singleline(&mut s.git_command);
                        });
                        let version = Executable::new(if s.git_command.is_empty() { "git" } else { &s.git_command }, std::env::temp_dir())
                            .output(&GitArgs::new("version"))
                            .unwrap_or_else(|e| e.to_string());
                        ui.label(RichText::new(version.trim()).small());
                        ui.separator();
                        ui.label(RichText::new("Global git configuration").strong());
                        let name = self.global_name.get_or_insert_with(|| global_get("user.name"));
                        ui.horizontal(|ui| {
                            ui.label("User name");
                            ui.text_edit_singleline(name);
                        });
                        let email = self.global_email.get_or_insert_with(|| global_get("user.email"));
                        ui.horizontal(|ui| {
                            ui.label("User email");
                            ui.text_edit_singleline(email);
                        });
                        let editor = self.global_editor.get_or_insert_with(|| global_get("core.editor"));
                        ui.horizontal(|ui| {
                            ui.label("Editor (core.editor)");
                            ui.text_edit_singleline(editor);
                        });
                        let mt = self.merge_tool.get_or_insert_with(|| global_get("merge.tool"));
                        ui.horizontal(|ui| {
                            ui.label("Merge tool (merge.tool)");
                            ui.text_edit_singleline(mt);
                        });
                        let dt = self.diff_tool.get_or_insert_with(|| global_get("diff.tool"));
                        ui.horizontal(|ui| {
                            ui.label("Diff tool (diff.tool)");
                            ui.text_edit_singleline(dt);
                        });
                        ui.separator();
                        ui.label(RichText::new("Applications").strong());
                        ui.horizontal(|ui| {
                            ui.label("File editor (empty = system default)");
                            ui.text_edit_singleline(&mut s.editor);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Terminal (empty = auto detect)");
                            ui.text_edit_singleline(&mut s.terminal);
                        });
                    }
                    6 => {
                        ui.label("Default pull action");
                        ui.radio_value(&mut s.pull_action, PullAction::Merge, "Merge");
                        ui.radio_value(&mut s.pull_action, PullAction::Rebase, "Rebase");
                        ui.radio_value(&mut s.pull_action, PullAction::Fetch, "Fetch");
                        ui.checkbox(&mut s.auto_stash_on_pull, "Auto stash on pull");
                        ui.checkbox(&mut s.prune_on_fetch, "Prune remote branches on fetch");
                        ui.checkbox(&mut s.update_submodules_on_checkout, "Update submodules on checkout");
                        ui.horizontal(|ui| {
                            ui.label("Recurse submodules on push");
                            ui.radio_value(&mut s.recurse_submodules_on_push, 0, "No");
                            ui.radio_value(&mut s.recurse_submodules_on_push, 1, "Check");
                            ui.radio_value(&mut s.recurse_submodules_on_push, 2, "On demand");
                        });
                    }
                    7 => {
                        let mut remove = None;
                        for (i, t) in s.commit_templates.iter_mut().enumerate() {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label("Name");
                                    ui.text_edit_singleline(&mut t.name);
                                    if ui.button("Remove").clicked() {
                                        remove = Some(i);
                                    }
                                });
                                ui.add(egui::TextEdit::multiline(&mut t.text).desired_rows(3).desired_width(f32::INFINITY));
                            });
                        }
                        if let Some(i) = remove {
                            s.commit_templates.remove(i);
                        }
                        if ui.button("✚ Add template").clicked() {
                            s.commit_templates.push(CommitTemplate { name: "New template".into(), text: String::new() });
                        }
                    }
                    8 => {
                        ui.label(RichText::new("Scripts are shown in the revision context menu. Arguments may use {sHash} and {WorkingDir}.").small());
                        let mut remove = None;
                        for (i, sc) in s.user_scripts.iter_mut().enumerate() {
                            ui.group(|ui| {
                                egui::Grid::new(("script", i)).num_columns(2).show(ui, |ui| {
                                    ui.label("Name");
                                    ui.text_edit_singleline(&mut sc.name);
                                    ui.end_row();
                                    ui.label("Command");
                                    ui.text_edit_singleline(&mut sc.command);
                                    ui.end_row();
                                    ui.label("Arguments");
                                    ui.text_edit_singleline(&mut sc.arguments);
                                    ui.end_row();
                                });
                                if ui.button("Remove").clicked() {
                                    remove = Some(i);
                                }
                            });
                        }
                        if let Some(i) = remove {
                            s.user_scripts.remove(i);
                        }
                        if ui.button("✚ Add script").clicked() {
                            s.user_scripts.push(UserScript { name: "New script".into(), command: "git".into(), arguments: String::new(), ask_confirmation: false, run_in_background: false });
                        }
                    }
                    _ => {
                        ui.checkbox(&mut s.show_git_command_line, "Show the git command line in process dialogs");
                        ui.checkbox(&mut s.refresh_commit_dialog_on_form_focus, "Refresh the commit dialog on focus");
                        if ui.button("Reset all settings to defaults").clicked() {
                            *s = AppSettings::default();
                        }
                        if let Some(p) = AppSettings::default_path() {
                            ui.label(RichText::new(format!("Settings file: {}", p.display())).small());
                        }
                    }
                }
            });
        });
        keep
    }
}

/// Editor for the repository's local git configuration.
#[derive(Default)]
pub struct GitConfigDialog {
    entries: Option<Vec<(String, String)>>,
    key: String,
    value: String,
}

impl Dialog for GitConfigDialog {
    fn title(&self) -> String {
        "Repository settings (git config)".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(760.0, 520.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let Some(m) = cx.module.cloned() else { return false };
        let entries = self.entries.get_or_insert_with(|| {
            m.run(&GitArgs::new("config").arg("--local").arg("--list").arg("-z"))
                .map(|r| {
                    r.stdout_str()
                        .split('\0')
                        .filter(|s| !s.is_empty())
                        .map(|e| e.split_once('\n').map(|(k, v)| (k.to_string(), v.to_string())).unwrap_or((e.to_string(), String::new())))
                        .collect()
                })
                .unwrap_or_default()
        });
        let mut reload = false;
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.key).hint_text("key, e.g. user.email").desired_width(220.0));
            ui.add(egui::TextEdit::singleline(&mut self.value).hint_text("value").desired_width(300.0));
            if ui.button("Set").clicked() && !self.key.trim().is_empty() {
                if let Err(e) = m.set_config(self.key.trim(), &self.value, false) {
                    cx.error("git config", e.to_string());
                }
                reload = true;
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("gitconfig").striped(true).num_columns(3).show(ui, |ui| {
                for (k, v) in entries.iter() {
                    if ui.link(k).clicked() {
                        self.key = k.clone();
                        self.value = v.clone();
                    }
                    ui.label(v);
                    if ui.small_button("Remove").clicked() {
                        let _ = m.unset_config(k, false);
                        reload = true;
                    }
                    ui.end_row();
                }
            });
        });
        if reload {
            self.entries = None;
            cx.push(Action::RefreshStatus);
        }
        !ui.input(|i| i.key_pressed(egui::Key::Escape))
    }
}
