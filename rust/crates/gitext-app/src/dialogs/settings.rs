//! Port of `FormSettings` (settings pages) and a git config editor.

use egui::{RichText, Ui, Vec2};
use gitext_core::commands::{GitRefsSortBy, GitRefsSortOrder, LocalChangesAction};
use gitext_core::revision_reader::RevisionSortOrder;
use gitext_core::settings::{AppSettings, CommitTemplate, PullAction, Theme, UserScript};
use gitext_core::{Executable, GitArgs};

use super::{Action, Cx, Dialog, DialogKind};
use crate::i18n::{n_, tr, trf};

const PAGES: [&str; 10] = [
    n_("General"),
    n_("Appearance"),
    n_("Revision graph"),
    n_("Commit dialog"),
    n_("Diff viewer"),
    n_("Git"),
    n_("Pull & push"),
    n_("Commit templates"),
    n_("Scripts"),
    n_("Advanced"),
];

#[derive(Default)]
pub struct SettingsDialog {
    page: usize,
    edit: Option<AppSettings>,
    global_name: Option<String>,
    global_email: Option<String>,
    global_editor: Option<String>,
    /// Diff and merge tool editors (loaded when the Git page is first shown).
    tools: Option<[super::tools::ToolEdit; 2]>,
    /// `git version` of the configured git command (not run every frame).
    git_version: Option<(String, String)>,
    /// Result of the last Explorer menu registration.
    shell_ext_result: Option<Result<String, String>>,
    /// Reads what the Git page shows in the background as soon as the dialog opens: it takes
    /// a few git calls, each slow on Windows.
    git_page: Option<crate::tasks::Task<GitPageData>>,
}

/// What the Git page reads from git: the version and the global config.
struct GitPageData {
    git_command: String,
    version: String,
    name: String,
    email: String,
    editor: String,
    tools: [super::tools::ToolEdit; 2],
}

impl GitPageData {
    fn load(git_command: String) -> Self {
        use gitext_core::diff_tools::{read_config_list, ToolConfigStore, ToolType};
        let version = git_version(&git_command);
        let git = Executable::git(std::env::temp_dir());
        let global = read_config_list(&git, &GitArgs::new("config").arg("--global").arg("--list").arg("-z"));
        let global_get = |k: &str| global.get(k).map(|v| v.trim().to_string()).unwrap_or_default();
        let store = ToolConfigStore::snapshot(Executable::git(std::env::temp_dir()));
        let mut tools = [super::tools::ToolEdit::load(&store, ToolType::Diff), super::tools::ToolEdit::load(&store, ToolType::Merge)];
        // looking for the installed tools checks many paths: done here too, not on the page
        for t in &mut tools {
            t.detected();
        }
        GitPageData {
            git_command,
            version,
            name: global_get("user.name"),
            email: global_get("user.email"),
            editor: global_get("core.editor"),
            tools,
        }
    }
}

fn git_version(git_command: &str) -> String {
    Executable::new(if git_command.is_empty() { "git" } else { git_command }, std::env::temp_dir())
        .output(&GitArgs::new("version"))
        .unwrap_or_else(|e| e.to_string())
        .trim()
        .to_string()
}

impl SettingsDialog {
    /// Opens on a page (index in `PAGES`).
    pub fn at_page(page: usize) -> Self {
        SettingsDialog { page: page.min(PAGES.len() - 1), ..Default::default() }
    }
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
        tr("Settings").into()
    }

    /// Not the title: the window keeps its place when Apply changes the language.
    fn id(&self) -> String {
        "settings".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(860.0, 600.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let s = self.edit.get_or_insert_with(|| cx.settings.clone());
        if self.tools.is_none() && self.git_page.is_none() {
            let git_command = s.git_command.clone();
            self.git_page = Some(crate::tasks::Task::spawn(cx.ctx, move || GitPageData::load(git_command)));
        }
        if let Some(d) = self.git_page.as_mut().and_then(|t| t.try_take()) {
            self.git_page = None;
            self.git_version.get_or_insert((d.git_command, d.version));
            self.global_name.get_or_insert(d.name);
            self.global_email.get_or_insert(d.email);
            self.global_editor.get_or_insert(d.editor);
            self.tools.get_or_insert(d.tools);
        }
        let mut keep = true;
        egui::SidePanel::left("settings_pages").resizable(false).exact_width(170.0).show_inside(ui, |ui| {
            for (i, p) in PAGES.iter().enumerate() {
                if ui.selectable_label(self.page == i, tr(p)).clicked() {
                    self.page = i;
                }
            }
        });
        egui::TopBottomPanel::bottom("settings_buttons").show_inside(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr("Cancel")).clicked() {
                        keep = false;
                    }
                    let mut apply = false;
                    if ui.button(RichText::new(tr("OK")).strong()).clicked() {
                        apply = true;
                        keep = false;
                    }
                    if ui.button(tr("Apply")).clicked() {
                        apply = true;
                    }
                    if apply {
                        let reload = s.graph_config() != cx.settings.graph_config()
                            || s.show_git_notes != cx.settings.show_git_notes
                            || s.revision_sort_order != cx.settings.revision_sort_order
                            || s.max_revision_graph_commits != cx.settings.max_revision_graph_commits
                            // the texts of the working directory and index rows are made when loading
                            || s.translation != cx.settings.translation;
                        let refs_changed = (s.refs_sort_by, s.refs_sort_order) != (cx.settings.refs_sort_by, cx.settings.refs_sort_order);
                        *cx.settings = s.clone();
                        crate::i18n::set_language(&s.translation);
                        gitext_core::exec::set_git_command(&s.git_command);
                        gitext_core::exec::set_wsl_git_enabled(s.wsl_git_enabled);
                        if let Some(n) = &self.global_name {
                            global_set("user.name", n);
                        }
                        if let Some(e) = &self.global_email {
                            global_set("user.email", e);
                        }
                        if let Some(e) = &self.global_editor {
                            global_set("core.editor", e);
                        }
                        if let Some(tools) = &mut self.tools {
                            let store = gitext_core::diff_tools::ToolConfigStore::default();
                            for t in tools.iter_mut() {
                                if let Err(e) = t.save(&store) {
                                    cx.error(tr("Settings"), trf("Could not save the {0} tool: {1}", &[&t.tool_type.label(), &e]));
                                }
                            }
                        }
                        cx.push(Action::SaveSettings);
                        cx.push(Action::ApplyTheme);
                        if reload {
                            cx.push(Action::ReloadLog);
                        }
                        if refs_changed {
                            cx.push(Action::RefreshStatus);
                        }
                    }
                });
            });
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.heading(tr(PAGES[self.page]));
                ui.add_space(6.0);
                match self.page {
                    0 => {
                        ui.checkbox(&mut s.close_process_dialog_on_success, tr("Close the process dialog when the command succeeds"));
                        ui.checkbox(&mut s.follow_renames_in_file_history, tr("Follow renames in file history"));
                        ui.checkbox(&mut s.check_for_uncommitted_changes_in_checkout, tr("Ask what to do with local changes on checkout"));
                        ui.checkbox(&mut s.show_untracked_files, tr("Show untracked files in status and commit dialog"));
                        ui.horizontal(|ui| {
                            ui.label(tr("Recent repositories to remember"));
                            ui.add(egui::DragValue::new(&mut s.recent_repositories_history_size).range(1..=200));
                        });
                        ui.checkbox(&mut s.sort_recent_repos_alphabetically, tr("Sort recent repositories alphabetically"));
                        ui.horizontal(|ui| {
                            ui.label(tr("Default clone destination"));
                            ui.text_edit_singleline(&mut s.default_clone_destination);
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("Default action for local changes on checkout"));
                            egui::ComboBox::from_id_salt("lca").selected_text(format!("{:?}", s.default_local_changes_action)).show_ui(ui, |ui| {
                                for a in [LocalChangesAction::DontChange, LocalChangesAction::Merge, LocalChangesAction::Stash, LocalChangesAction::Reset] {
                                    ui.selectable_value(&mut s.default_local_changes_action, a, format!("{a:?}"));
                                }
                            });
                        });
                        if cfg!(windows) {
                            ui.add_space(8.0);
                            shell_ext_ui(ui, s, &mut self.shell_ext_result);
                        }
                    }
                    1 => {
                        ui.horizontal(|ui| {
                            ui.label(tr("Language"));
                            let current = crate::i18n::language(&s.translation);
                            egui::ComboBox::from_id_salt("language").selected_text(current.label()).height(400.0).show_ui(ui, |ui| {
                                for lang in crate::i18n::LANGUAGES {
                                    if ui.selectable_label(current.name == lang.name, lang.label()).clicked() {
                                        s.translation = lang.name.to_string();
                                    }
                                }
                            });
                            ui.hyperlink_to(tr("Help translate"), "https://github.com/gitextensions/gitextensions/wiki/Translations");
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("Theme"));
                            ui.radio_value(&mut s.theme, Theme::System, tr("Follow system"));
                            ui.radio_value(&mut s.theme, Theme::Light, tr("Light"));
                            ui.radio_value(&mut s.theme, Theme::Dark, tr("Dark"));
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("Zoom"));
                            ui.add(egui::Slider::new(&mut s.ui_scale, 0.5..=3.0).step_by(0.05));
                        });
                        ui.checkbox(&mut s.commit_info_position_right, tr("Show the commit details to the right of the graph (else below)"));
                        ui.horizontal(|ui| {
                            ui.label(tr("Font size"));
                            ui.add(egui::Slider::new(&mut s.font_size, 9.0..=24.0).step_by(0.5));
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("Code font size"));
                            ui.add(egui::Slider::new(&mut s.monospace_font_size, 9.0..=24.0).step_by(0.5));
                        });
                        ui.separator();
                        ui.checkbox(&mut s.show_author_column, tr("Show author column"));
                        ui.checkbox(&mut s.show_date_column, tr("Show date column"));
                        ui.checkbox(&mut s.show_id_column, tr("Show commit hash column"));
                        ui.checkbox(&mut s.show_relative_date, tr("Show relative dates"));
                        ui.checkbox(&mut s.show_author_date, tr("Show author date (instead of commit date)"));
                        ui.checkbox(&mut s.highlight_author_commits, tr("Highlight my commits"));
                        ui.checkbox(&mut s.left_panel_visible, tr("Show the left panel"));
                        ui.separator();
                        ui.label(egui::RichText::new(tr("Author images")).strong());
                        ui.checkbox(&mut s.show_author_avatar_column, tr("Show author image column"));
                        ui.checkbox(&mut s.show_author_avatar_in_commit_info, tr("Show author image in the commit details"));
                        ui.horizontal(|ui| {
                            use gitext_core::settings::AvatarProvider;
                            ui.label(tr("Get images from"));
                            ui.radio_value(&mut s.avatar_provider, AvatarProvider::None, tr("Author initials only"));
                            ui.radio_value(&mut s.avatar_provider, AvatarProvider::Gravatar, tr("GitHub / Gravatar (initials when none)"));
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("Keep downloaded images for"));
                            ui.add(egui::DragValue::new(&mut s.avatar_cache_days).range(1..=365));
                            ui.label(tr("days"));
                            if ui.button(tr("Clear image cache")).clicked() {
                                crate::avatars::clear_disk_cache();
                            }
                        });
                    }
                    2 => {
                        ui.checkbox(&mut s.merge_graph_lanes_having_common_parent, tr("Merge graph lanes having a common parent"));
                        ui.checkbox(&mut s.render_graph_with_diagonals, tr("Render graph with diagonals"));
                        ui.checkbox(&mut s.straighten_graph_diagonals, tr("Straighten graph diagonals"));
                        ui.horizontal(|ui| {
                            ui.label(tr("Straighten segments up to"));
                            ui.add(egui::DragValue::new(&mut s.straighten_graph_segments_limit).range(0..=1000));
                            ui.label(tr("segments per row"));
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("Lane width"));
                            ui.add(egui::Slider::new(&mut s.lane_width, 8.0..=32.0));
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("Row height"));
                            ui.add(egui::Slider::new(&mut s.row_height, 16.0..=40.0));
                        });
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(tr("Sort order"));
                            ui.radio_value(&mut s.revision_sort_order, RevisionSortOrder::GitDefault, tr("Git default"));
                            ui.radio_value(&mut s.revision_sort_order, RevisionSortOrder::AuthorDate, tr("Author date"));
                            ui.radio_value(&mut s.revision_sort_order, RevisionSortOrder::Topology, tr("Topology"));
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("Maximum number of commits (0 = all)"));
                            ui.add(egui::DragValue::new(&mut s.max_revision_graph_commits).speed(1000));
                        });
                        ui.checkbox(&mut s.show_git_notes, tr("Show git notes"));
                        ui.checkbox(&mut s.show_merge_commits, tr("Show merge commits"));
                        ui.horizontal(|ui| {
                            ui.label(tr("Sort branches in the left panel by"));
                            egui::ComboBox::from_id_salt("refsort").selected_text(format!("{:?}", s.refs_sort_by)).show_ui(ui, |ui| {
                                for k in GitRefsSortBy::ALL {
                                    ui.selectable_value(&mut s.refs_sort_by, k, format!("{k:?}"));
                                }
                            });
                            ui.add_enabled_ui(s.refs_sort_by != GitRefsSortBy::Default, |ui| {
                                ui.radio_value(&mut s.refs_sort_order, GitRefsSortOrder::Ascending, tr("Ascending"));
                                ui.radio_value(&mut s.refs_sort_order, GitRefsSortOrder::Descending, tr("Descending"));
                            });
                        });
                    }
                    3 => {
                        ui.checkbox(&mut s.remember_amend_commit_state, tr("Remember the amend state"));
                        ui.checkbox(&mut s.ensure_commit_message_second_line_empty, tr("Ensure the second line of the message is empty"));
                        ui.checkbox(&mut s.sign_off_by_default, tr("Sign-off commits by default"));
                        ui.checkbox(&mut s.push_after_commit, tr("Push after commit"));
                        ui.checkbox(&mut s.force_push_with_lease, tr("Push with --force-with-lease after amending a commit"));
                        ui.checkbox(&mut s.refresh_commit_dialog_on_form_focus, tr("Refresh the commit dialog when the window gets the focus"));
                        ui.horizontal(|ui| {
                            ui.label(tr("Max characters of the first line (0 = no limit)"));
                            ui.add(egui::DragValue::new(&mut s.commit_validation_max_cnt_chars_first_line));
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("Max characters per line (0 = no limit)"));
                            ui.add(egui::DragValue::new(&mut s.commit_validation_max_cnt_chars_per_line));
                        });
                        ui.checkbox(&mut s.commit_validation_second_line_must_be_empty, tr("Warn when the second line is not empty"));
                        ui.horizontal(|ui| {
                            ui.label(tr("Number of previous messages in history"));
                            ui.add(egui::DragValue::new(&mut s.commit_message_history_size).range(0..=200));
                        });
                    }
                    4 => {
                        ui.checkbox(&mut s.ignore_whitespace, tr("Ignore whitespace"));
                        ui.checkbox(&mut s.ignore_whitespace_changes, tr("Ignore whitespace changes"));
                        ui.checkbox(&mut s.ignore_whitespace_at_eol, tr("Ignore whitespace changes at end of line"));
                        ui.checkbox(&mut s.show_whitespace, tr("Show whitespace characters (· for spaces, → for tabs)"));
                        ui.checkbox(&mut s.show_entire_file, tr("Show entire file"));
                        ui.checkbox(&mut s.use_histogram_diff, tr("Use the histogram diff algorithm"));
                        ui.checkbox(&mut s.show_line_numbers, tr("Show line numbers"));
                        ui.horizontal(|ui| {
                            ui.label(tr("Context lines"));
                            ui.add(egui::DragValue::new(&mut s.context_lines).range(0..=1000));
                        });
                        ui.separator();
                        ui.label(RichText::new(tr("Blame")).strong());
                        ui.checkbox(&mut s.detect_copy_in_file_on_blame, tr("Detect moved and copied lines within a file (-M)"));
                        ui.checkbox(&mut s.detect_copy_in_all_on_blame, tr("Detect moved and copied lines across files (-C)"));
                        ui.checkbox(&mut s.ignore_whitespace_on_blame, tr("Ignore whitespace (-w)"));
                    }
                    5 => {
                        ui.horizontal(|ui| {
                            ui.label(tr("Git command"));
                            ui.text_edit_singleline(&mut s.git_command);
                        });
                        if self.git_page.is_none() && self.git_version.as_ref().is_none_or(|(cmd, _)| *cmd != s.git_command) {
                            self.git_version = Some((s.git_command.clone(), git_version(&s.git_command)));
                        }
                        ui.label(RichText::new(self.git_version.as_ref().map(|(_, v)| v.as_str()).unwrap_or_default()).small());
                        ui.checkbox(&mut s.wsl_git_enabled, tr("Use the git of the WSL distro for repositories in WSL (\\\\wsl$\\…, Windows only)"))
                            .on_hover_text(tr("Runs `wsl -d <distro> git` for these repositories: much faster than Windows git on WSL files, and no \"dubious ownership\" errors."));
                        ui.separator();
                        ui.label(RichText::new(tr("Global git configuration")).strong());
                        if self.git_page.is_some() {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(tr("Reading the git configuration…"));
                            });
                        } else {
                            let name = self.global_name.get_or_insert_with(|| global_get("user.name"));
                            ui.horizontal(|ui| {
                                ui.label(tr("User name"));
                                ui.text_edit_singleline(name);
                            });
                            let email = self.global_email.get_or_insert_with(|| global_get("user.email"));
                            ui.horizontal(|ui| {
                                ui.label(tr("User email"));
                                ui.text_edit_singleline(email);
                            });
                            let editor = self.global_editor.get_or_insert_with(|| global_get("core.editor"));
                            ui.horizontal(|ui| {
                                ui.label(tr("Editor (core.editor)"));
                                ui.text_edit_singleline(editor);
                            });
                            ui.separator();
                            ui.label(RichText::new(tr("Diff and merge tools")).strong());
                            let tools = self.tools.get_or_insert_with(|| {
                                let store = gitext_core::diff_tools::ToolConfigStore::default();
                                [
                                    super::tools::ToolEdit::load(&store, gitext_core::diff_tools::ToolType::Diff),
                                    super::tools::ToolEdit::load(&store, gitext_core::diff_tools::ToolType::Merge),
                                ]
                            });
                            for t in tools.iter_mut() {
                                t.ui(ui);
                                ui.add_space(6.0);
                            }
                            ui.checkbox(&mut s.show_available_diff_tools, tr("Show all available difftools in a dropdown"))
                                .on_hover_text(tr("Show all configured difftools in a dropdown.\nThe primary difftool can still be selected by clicking the main menu entry."));
                        }
                        ui.separator();
                        ui.label(RichText::new(tr("Applications")).strong());
                        ui.horizontal(|ui| {
                            ui.label(tr("File editor (empty = system default)"));
                            ui.text_edit_singleline(&mut s.editor);
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("Terminal (empty = auto detect)"));
                            ui.text_edit_singleline(&mut s.terminal);
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("File manager (empty = system default)"));
                            ui.text_edit_singleline(&mut s.file_manager);
                        });
                    }
                    6 => {
                        ui.label(tr("Default pull action"));
                        ui.radio_value(&mut s.pull_action, PullAction::Merge, tr("Merge"));
                        ui.radio_value(&mut s.pull_action, PullAction::Rebase, tr("Rebase"));
                        ui.radio_value(&mut s.pull_action, PullAction::Fetch, tr("Fetch"));
                        ui.checkbox(&mut s.auto_stash_on_pull, tr("Auto stash on pull"));
                        ui.checkbox(&mut s.prune_on_fetch, tr("Prune remote branches on fetch"));
                        ui.checkbox(&mut s.update_submodules_on_checkout, tr("Update submodules on checkout"));
                        ui.horizontal(|ui| {
                            ui.label(tr("Recurse submodules on push"));
                            ui.radio_value(&mut s.recurse_submodules_on_push, 0, tr("No"));
                            ui.radio_value(&mut s.recurse_submodules_on_push, 1, tr("Check"));
                            ui.radio_value(&mut s.recurse_submodules_on_push, 2, tr("On demand"));
                        });
                    }
                    7 => {
                        let mut remove = None;
                        for (i, t) in s.commit_templates.iter_mut().enumerate() {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(tr("Name"));
                                    ui.text_edit_singleline(&mut t.name);
                                    if ui.button(tr("Remove")).clicked() {
                                        remove = Some(i);
                                    }
                                });
                                ui.add(egui::TextEdit::multiline(&mut t.text).desired_rows(3).desired_width(f32::INFINITY));
                            });
                        }
                        if let Some(i) = remove {
                            s.commit_templates.remove(i);
                        }
                        if ui.button(format!("✚ {}", tr("Add template"))).clicked() {
                            s.commit_templates.push(CommitTemplate { name: tr("New template").into(), text: String::new() });
                        }
                    }
                    8 => {
                        ui.label(RichText::new(tr("Scripts are shown in the revision context menu. Arguments may use {sHash} and {WorkingDir}.")).small());
                        let mut remove = None;
                        for (i, sc) in s.user_scripts.iter_mut().enumerate() {
                            ui.group(|ui| {
                                egui::Grid::new(("script", i)).num_columns(2).show(ui, |ui| {
                                    ui.label(tr("Name"));
                                    ui.text_edit_singleline(&mut sc.name);
                                    ui.end_row();
                                    ui.label(tr("Command"));
                                    ui.text_edit_singleline(&mut sc.command);
                                    ui.end_row();
                                    ui.label(tr("Arguments"));
                                    ui.text_edit_singleline(&mut sc.arguments);
                                    ui.end_row();
                                });
                                ui.checkbox(&mut sc.ask_confirmation, tr("Ask for confirmation before running"));
                                ui.checkbox(&mut sc.run_in_background, tr("Run in the background (no process dialog)"));
                                if ui.button(tr("Remove")).clicked() {
                                    remove = Some(i);
                                }
                            });
                        }
                        if let Some(i) = remove {
                            s.user_scripts.remove(i);
                        }
                        if ui.button(format!("✚ {}", tr("Add script"))).clicked() {
                            s.user_scripts.push(UserScript { name: tr("New script").into(), command: "git".into(), arguments: String::new(), ask_confirmation: false, run_in_background: false });
                        }
                    }
                    _ => {
                        if ui.button(tr("Reset all settings to defaults")).clicked() {
                            *s = AppSettings::default();
                        }
                        if let Some(p) = AppSettings::default_path() {
                            ui.label(RichText::new(trf("Settings file: {0}", &[&p.display()])).small());
                        }
                    }
                }
            });
        });
        keep
    }
}

/// The Windows Explorer context menu (port of `ShellExtensionSettingsPage`).
fn shell_ext_ui(ui: &mut Ui, s: &mut AppSettings, result: &mut Option<Result<String, String>>) {
    use crate::shell_ext;
    ui.strong(tr("Windows Explorer context menu"));
    let registered = shell_ext::is_registered();
    ui.label(RichText::new(if registered { tr("The Git Extensions menu is shown when right-clicking files and folders.") } else { tr("The Git Extensions menu is not installed.") }).small());
    ui.label(tr("Items in the menu:"));
    egui::Grid::new("shell_ext_items").num_columns(3).show(ui, |ui| {
        for (n, item) in shell_ext::ITEMS.iter().enumerate() {
            let mut shown = !s.shell_menu_hidden_items.iter().any(|h| h == item.command);
            if ui.checkbox(&mut shown, item.text.trim_end_matches('.')).changed() {
                s.shell_menu_hidden_items.retain(|h| h != item.command);
                if !shown {
                    s.shell_menu_hidden_items.push(item.command.to_string());
                }
            }
            if n % 3 == 2 {
                ui.end_row();
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.button(if registered { tr("Update the menu") } else { tr("Install the menu") }).clicked() {
            *result = Some(shell_ext::register(&s.shell_menu_hidden_items).map(|_| tr("The menu is installed.").to_string()));
        }
        if registered && ui.button(tr("Remove the menu")).clicked() {
            *result = Some(shell_ext::unregister().map(|_| tr("The menu is removed.").to_string()));
        }
    });
    match result {
        Some(Ok(m)) => {
            ui.label(RichText::new(m.as_str()).small());
        }
        Some(Err(e)) => {
            ui.label(RichText::new(e.as_str()).small().color(ui.visuals().error_fg_color));
        }
        None => {}
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
        tr("Repository settings (git config)").into()
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
            ui.add(egui::TextEdit::singleline(&mut self.key).hint_text(tr("key, e.g. user.email")).desired_width(220.0));
            ui.add(egui::TextEdit::singleline(&mut self.value).hint_text(tr("value")).desired_width(300.0));
            if ui.button(tr("Set")).clicked() && !self.key.trim().is_empty() {
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
                    if ui.small_button(tr("Remove")).clicked() {
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
