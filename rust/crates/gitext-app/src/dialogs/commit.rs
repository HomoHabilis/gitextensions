//! Port of `FormCommit`: stage/unstage files and lines, write the message and commit.

use egui::{Key, Modifiers, RichText, Ui, Vec2};
use gitext_core::commands::{CommitOptions, UntrackedFilesMode};
use gitext_core::commit_message::{format_commit_message, CommitMessageManager};
use gitext_core::patch::create_partial_patch;
use gitext_core::status::{GitItemStatus, StagedStatus};
use gitext_core::{GitModule, ObjectId};

use super::{Action, Confirm, Cx, Dialog, DialogKind};
use crate::i18n::{n_, tr, trf};
use crate::tasks::{Loader, Task};
use crate::theme::Palette;
use crate::views::diff_viewer::{content_from_bytes, DiffViewer, ViewerCommand, ViewerContent};
use crate::views::file_actions::{self, FileAction, Selection};
use crate::views::file_list::FileList;
use crate::views::{plain_key, shortcut};

/// A file menu item: label, command and shortcut.
type MenuEntry = (&'static str, &'static str, Option<(Modifiers, Key)>);

/// The context menu entries of the unstaged files: label, command, key.
/// (Delete, rename, open with, copy path and the like come from [`file_actions`].)
const UNSTAGED_MENU: &[MenuEntry] = &[
    (n_("Stage"), "stage", Some((Modifiers::NONE, Key::S))),
    (n_("Reset file changes…"), "reset", Some((Modifiers::NONE, Key::R))),
    (n_("Add to .gitignore…"), "ignore", None),
    (n_("Edit file"), "open", Some((Modifiers::NONE, Key::F4))),
    (n_("Open with difftool"), "difftool", Some((Modifiers::NONE, Key::F3))),
    (n_("File history"), "history", Some((Modifiers::NONE, Key::H))),
    (n_("Blame"), "blame", Some((Modifiers::NONE, Key::B))),
];

/// The context menu entries of the staged files.
const STAGED_MENU: &[MenuEntry] = &[
    (n_("Unstage"), "unstage", Some((Modifiers::NONE, Key::U))),
    (n_("Reset file changes…"), "reset", Some((Modifiers::NONE, Key::R))),
    (n_("Edit file"), "open", Some((Modifiers::NONE, Key::F4))),
    (n_("Open"), "open_with", Some((Modifiers::SHIFT, Key::F4))),
    (n_("Open with difftool"), "difftool", Some((Modifiers::NONE, Key::F3))),
    (n_("File history"), "history", Some((Modifiers::NONE, Key::H))),
    (n_("Blame"), "blame", Some((Modifiers::NONE, Key::B))),
];

/// Shows the file context menu `entries`; returns the command clicked.
fn file_menu(ui: &mut Ui, entries: &[MenuEntry]) -> Option<&'static str> {
    let mut cmd = None;
    for &(label, key, shortcut) in entries {
        let text = shortcut.map(|(m, k)| ui.ctx().format_shortcut(&egui::KeyboardShortcut::new(m, k))).unwrap_or_default();
        if ui.add(egui::Button::new(tr(label)).shortcut_text(text)).clicked() {
            cmd = Some(key);
            ui.close_kind(egui::UiKind::Menu);
        }
    }
    cmd
}

/// The command of the key pressed in a file list with the keyboard focus.
fn file_key(ui: &Ui, entries: &[MenuEntry]) -> Option<&'static str> {
    entries.iter().find_map(|&(_, key, s)| s.filter(|&(m, k)| shortcut(ui.ctx(), m, k)).map(|_| key))
}

/// "Assume unchanged" and "Skip worktree", checked when the selection has the flag (clicking
/// a checked one clears it, like upstream's check menu items).
fn index_flags_menu(ui: &mut Ui, items: &[&GitItemStatus]) -> Option<&'static str> {
    if !items.iter().any(|f| f.is_tracked && !f.is_new) {
        return None;
    }
    let assumed = items.iter().any(|f| f.is_assume_unchanged);
    let skipped = items.iter().any(|f| f.is_skip_worktree);
    let mut cmd = None;
    let r = ui.add(egui::Button::new(format!("{}{}", if assumed { "✔ " } else { "" }, tr("Assume unchanged"))));
    if r.on_hover_text(tr("Tell git not to check the status of this file (a big file that never changes).")).clicked() {
        cmd = Some(if assumed { "no_assume" } else { "assume" });
        ui.close_kind(egui::UiKind::Menu);
    }
    let r = ui.add(egui::Button::new(format!("{}{}", if skipped { "✔ " } else { "" }, tr("Skip worktree"))));
    if r.on_hover_text(tr("Hide local changes of a tracked file you don't want to commit (e.g. a config file).")).clicked() {
        cmd = Some(if skipped { "no_skip" } else { "skip" });
        ui.close_kind(egui::UiKind::Menu);
    }
    cmd
}

/// The line and column (1-based) of the text cursor in the commit message
/// (`commitCursorLine` / `commitCursorColumn`).
fn cursor_line_column(ctx: &egui::Context, text: &str) -> (usize, usize) {
    let index = egui::TextEdit::load_state(ctx, message_id()).and_then(|s| s.cursor.char_range()).map(|r| r.primary.index).unwrap_or(0);
    line_column(text, index)
}

fn line_column(text: &str, char_index: usize) -> (usize, usize) {
    let before: Vec<char> = text.chars().take(char_index).collect();
    let line = before.iter().filter(|&&c| c == '\n').count() + 1;
    let column = before.iter().rev().take_while(|&&c| c != '\n').count() + 1;
    (line, column)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Side {
    #[default]
    Unstaged,
    Staged,
}

#[derive(Default)]
pub struct CommitDialog {
    generation: u64,
    status: Loader<u64, Result<Vec<GitItemStatus>, String>>,
    unstaged: FileList,
    staged: FileList,
    side: Side,
    diff: Loader<String, ViewerContent>,
    viewer: DiffViewer,
    message: String,
    initialized: bool,
    manager: Option<CommitMessageManager>,
    amend: bool,
    sign_off: bool,
    no_verify: bool,
    reset_author: bool,
    author: String,
    gpg_sign: Option<bool>,
    recent_messages: Vec<String>,
    busy: Option<Task<Result<String, String>>>,
    push_after: bool,
    error: Option<String>,
    show_untracked: bool,
    /// Also list the ignored, assumed-unchanged and skip-worktree files (`tsmiShowIgnoredFiles` & co).
    show_ignored: bool,
    show_assumed: bool,
    show_skipped: bool,
    template_text: Option<String>,
    /// The status of the browse window last seen, to reload when it changes (e.g. files were
    /// reset by another dialog or changed on disk).
    repo_status: Option<Vec<GitItemStatus>>,
    /// Whether the application window had the focus last frame (refresh when it comes back).
    window_focused: bool,
    /// The status lists unresolved merge conflicts (no commit until they are solved).
    has_conflicts: bool,
}

/// Height of the commit error under the buttons (it scrolls when longer).
const ERROR_H: f32 = 60.0;

fn message_id() -> egui::Id {
    egui::Id::new("commit_message_text")
}

impl CommitDialog {
    fn init(&mut self, cx: &mut Cx, m: &GitModule) {
        self.initialized = true;
        self.window_focused = true;
        self.show_untracked = cx.settings.show_untracked_files;
        let mut mgr = CommitMessageManager::new(m.git_dir(), None);
        mgr.remember_amend_commit_state = cx.settings.remember_amend_commit_state;
        self.message = mgr.merge_or_commit_message();
        if self.message.trim().is_empty() {
            if let Some(merge) = m.merge_message() {
                self.message = merge;
            }
        }
        if self.message.trim().is_empty() {
            if let Some(path) = m.get_config("commit.template") {
                let p = if path.starts_with('~') { dirs::home_dir().unwrap_or_default().join(path.trim_start_matches("~/")) } else { m.work_dir().join(&path) };
                if let Ok(t) = std::fs::read_to_string(p) {
                    self.template_text = Some(t.clone());
                    self.message = t;
                }
            }
        }
        self.amend = mgr.amend_state();
        self.sign_off = cx.settings.sign_off_by_default;
        self.manager = Some(mgr);
        self.recent_messages = m.recent_commit_messages(cx.settings.commit_message_history_size);
    }

    fn refresh(&mut self) {
        self.generation += 1;
        self.diff.invalidate();
    }

    fn do_stage(&mut self, m: &GitModule, files: Vec<String>, stage: bool, cx: &mut Cx) {
        let refs: Vec<&str> = files.iter().map(String::as_str).collect();
        let r = if stage { m.stage_files(&refs) } else { m.unstage_files(&refs) };
        if let Err(e) = r {
            cx.error(if stage { tr("Stage") } else { tr("Unstage") }, e.to_string());
        }
        self.refresh();
        cx.push(Action::RefreshStatus);
    }

    fn start_commit(&mut self, cx: &mut Cx, m: &GitModule, push: bool) {
        if self.has_conflicts {
            self.error = Some(tr("There are unresolved merge conflicts, solve merge conflicts before committing.").into());
            return;
        }
        let using_template = self.template_text.is_some();
        let message = format_commit_message(&self.message, using_template, cx.settings.ensure_commit_message_second_line_empty);
        if message.trim().is_empty() {
            self.error = Some(tr("Please enter a commit message.").into());
            return;
        }
        if let Some(t) = &self.template_text {
            if self.message.trim() == t.trim() {
                self.error = Some(tr("The commit message is the unchanged commit template.").into());
                return;
            }
        }
        let options = CommitOptions {
            amend: self.amend,
            sign_off: self.sign_off,
            author: self.author.clone(),
            no_verify: self.no_verify,
            gpg_sign: self.gpg_sign,
            reset_author: self.reset_author,
            ..Default::default()
        };
        if let Some(mgr) = &self.manager {
            mgr.set_merge_or_commit_message(Some(&self.message));
        }
        let m = m.clone();
        self.push_after = push;
        self.error = None;
        self.busy = Some(Task::spawn(cx.ctx, move || m.commit(&message, options).map(|r| r.all_output()).map_err(|e| e.to_string())));
    }
}

impl Dialog for CommitDialog {
    fn title(&self) -> String {
        tr("Commit").into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(1200.0, 780.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let Some(m) = cx.module.cloned() else {
            ui.label(tr("No repository"));
            return !ui.button(tr("Close")).clicked();
        };
        if !self.initialized {
            self.init(cx, &m);
        }
        // files edited in another application show up when coming back (`RefreshDialogOnFormFocus`)
        let focused = cx.ctx.input(|i| i.focused);
        if focused && !self.window_focused && cx.settings.refresh_commit_dialog_on_form_focus {
            self.refresh();
        }
        self.window_focused = focused;
        let palette = Palette::for_ui(ui);
        let mut keep = true;

        // Commit result
        if let Some(t) = &mut self.busy {
            if let Some(r) = t.try_take() {
                self.busy = None;
                match r {
                    Ok(_) => {
                        if let Some(mgr) = &mut self.manager {
                            mgr.reset_commit_message();
                            mgr.set_amend_state(false);
                        }
                        cx.push(Action::Refresh);
                        if self.push_after || cx.settings.push_after_commit {
                            if let Some(d) = cx.data {
                                let mut push = super::push::PushDialog::new(d, cx.settings, None);
                                // an amended commit that was already pushed needs a forced push
                                if self.amend && cx.settings.force_push_with_lease {
                                    push = push.with_force(gitext_core::commands::ForcePushOptions::ForceWithLease);
                                }
                                cx.open(push);
                            }
                        }
                        return false;
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }

        let untracked = if self.show_untracked { UntrackedFilesMode::All } else { UntrackedFilesMode::No };
        let mm = m.clone();
        let (ignored, assumed, skipped) = (self.show_ignored, self.show_assumed, self.show_skipped);
        let items =
            self.status.request(cx.ctx, self.generation, move || mm.get_status_with(untracked, ignored, assumed, skipped).map_err(|e| e.to_string())).cloned();
        let items = match items {
            Some(Ok(i)) => i,
            Some(Err(e)) => {
                ui.colored_label(palette.error, e);
                Vec::new()
            }
            None => self.status.value.clone().and_then(|r| r.ok()).unwrap_or_default(),
        };
        let unstaged: Vec<GitItemStatus> = items.iter().filter(|s| s.staged == StagedStatus::WorkTree).cloned().collect();
        let staged: Vec<GitItemStatus> = items.iter().filter(|s| s.staged == StagedStatus::Index).cloned().collect();
        let conflicts = items.iter().any(|s| s.is_unmerged);
        self.has_conflicts = conflicts;

        if let Some(d) = cx.data {
            if self.repo_status.as_ref() != Some(&d.status) {
                if self.repo_status.is_some() {
                    self.refresh();
                }
                self.repo_status = Some(d.status.clone());
            }
        }
        self.unstaged.keep_position = true;
        self.staged.keep_position = true;
        self.shortcuts(ui, cx, &m, &unstaged);

        if conflicts {
            egui::TopBottomPanel::top("commit_conflicts").show_inside(ui, |ui| {
                egui::Frame::new().fill(palette.status_conflict.gamma_multiply(0.25)).inner_margin(6.0).corner_radius(4.0).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("⚠ {}", tr("There are unresolved merge conflicts"))).strong());
                        if ui.button(tr("Solve conflicts…")).clicked() {
                            cx.open(super::conflicts::ConflictsDialog::default());
                        }
                    });
                });
            });
        }
        egui::SidePanel::left("commit_files").resizable(true).default_width(380.0).width_range(220.0..=800.0).show_inside(ui, |ui| {
            let total_h = ui.available_height();
            // Unstaged
            ui.horizontal(|ui| {
                ui.label(RichText::new(trf("Unstaged ({0})", &[&unstaged.len()])).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(format!("⬇⬇ {}", tr("Stage all"))).on_hover_text("Ctrl+S").clicked() {
                        if let Err(e) = m.stage_all() {
                            cx.error(tr("Stage"), e.to_string());
                        }
                        self.refresh();
                    }
                    if ui.add_enabled(!self.unstaged.selected.is_empty(), egui::Button::new(format!("⬇ {}", tr("Stage")))).on_hover_text("S").clicked() {
                        let files: Vec<String> = self.unstaged.selected_items(&unstaged).iter().map(|s| s.name.clone()).collect();
                        self.do_stage(&m, files, true, cx);
                    }
                    if ui.button("⟳").on_hover_text(format!("{} (F5)", tr("Refresh"))).clicked() {
                        self.refresh();
                    }
                    let shown = [self.show_untracked, self.show_ignored, self.show_assumed, self.show_skipped];
                    ui.menu_button("👁", |ui| {
                        ui.checkbox(&mut self.show_untracked, tr("Show untracked files"));
                        ui.checkbox(&mut self.show_ignored, tr("Show ignored files"));
                        ui.checkbox(&mut self.show_assumed, tr("Show assumed-unchanged files"));
                        ui.checkbox(&mut self.show_skipped, tr("Show skip-worktree files"));
                    })
                    .response
                    .on_hover_text(tr("Files to show"));
                    if shown != [self.show_untracked, self.show_ignored, self.show_assumed, self.show_skipped] {
                        self.refresh();
                    }
                });
            });
            self.unstaged.toolbar(ui);
            let mut menu_cmd: Option<(&'static str, Vec<String>)> = None;
            let mut action: Option<FileAction> = None;
            ui.allocate_ui(Vec2::new(ui.available_width(), total_h * 0.5 - 70.0), |ui| {
                let r = self.unstaged.ui(ui, "commit_unstaged", &unstaged, |ui, sel| {
                    let names: Vec<String> = sel.iter().filter_map(|&i| unstaged.get(i)).map(|f| f.name.clone()).collect();
                    if let Some(key) = file_menu(ui, UNSTAGED_MENU) {
                        menu_cmd = Some((key, names.clone()));
                    }
                    let items: Vec<&GitItemStatus> = sel.iter().filter_map(|&i| unstaged.get(i)).collect();
                    if let Some(key) = index_flags_menu(ui, &items) {
                        menu_cmd = Some((key, names));
                    }
                    if let Some(a) = file_actions::menu(ui, &Selection { items, first: Some(ObjectId::INDEX), second: ObjectId::WORK_TREE }) {
                        action = Some(a);
                    }
                });
                if r.has_focus && !self.unstaged.selected.is_empty() {
                    if let Some(key) = file_key(ui, UNSTAGED_MENU) {
                        menu_cmd = Some((key, self.unstaged.selected_items(&unstaged).iter().map(|f| f.name.clone()).collect()));
                    } else if let Some(a) = file_actions::keys(
                        ui.ctx(),
                        &Selection { items: self.unstaged.selected_items(&unstaged), first: Some(ObjectId::INDEX), second: ObjectId::WORK_TREE },
                    ) {
                        action = Some(a);
                    }
                }
                // (also reported when the list changed and has no selection)
                if r.selection_changed && !self.unstaged.selected.is_empty() {
                    self.side = Side::Unstaged;
                    self.staged.clear();
                }
                if let Some(i) = r.double_clicked {
                    if let Some(f) = unstaged.get(i) {
                        self.do_stage(&m, vec![f.name.clone()], true, cx);
                    }
                }
            });
            if let Some((key, names)) = menu_cmd {
                self.file_command(key, names, &m, cx, false);
            }
            if let Some(a) = action {
                file_actions::run(a, &m, cx.actions);
                self.refresh();
            }
            ui.separator();
            // Staged
            ui.horizontal(|ui| {
                ui.label(RichText::new(trf("Staged ({0})", &[&staged.len()])).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(format!("⬆⬆ {}", tr("Unstage all"))).clicked() {
                        if let Err(e) = m.unstage_all() {
                            cx.error(tr("Unstage"), e.to_string());
                        }
                        self.refresh();
                    }
                    if ui.add_enabled(!self.staged.selected.is_empty(), egui::Button::new(format!("⬆ {}", tr("Unstage")))).on_hover_text("U").clicked() {
                        let files: Vec<String> = self.staged.selected_items(&staged).iter().map(|s| s.name.clone()).collect();
                        self.do_stage(&m, files, false, cx);
                    }
                });
            });
            let mut menu_cmd: Option<(&'static str, Vec<String>)> = None;
            let mut action: Option<FileAction> = None;
            let r = self.staged.ui(ui, "commit_staged", &staged, |ui, sel| {
                if let Some(key) = file_menu(ui, STAGED_MENU) {
                    menu_cmd = Some((key, sel.iter().filter_map(|&i| staged.get(i)).map(|f| f.name.clone()).collect()));
                }
                let items: Vec<&GitItemStatus> = sel.iter().filter_map(|&i| staged.get(i)).collect();
                if let Some(a) = file_actions::menu(ui, &Selection { items, first: None, second: ObjectId::INDEX }) {
                    action = Some(a);
                }
            });
            if r.has_focus && !self.staged.selected.is_empty() {
                if let Some(key) = file_key(ui, STAGED_MENU) {
                    menu_cmd = Some((key, self.staged.selected_items(&staged).iter().map(|f| f.name.clone()).collect()));
                }
            }
            if r.selection_changed && !self.staged.selected.is_empty() {
                self.side = Side::Staged;
                self.unstaged.clear();
            }
            if let Some(i) = r.double_clicked {
                if let Some(f) = staged.get(i) {
                    self.do_stage(&m, vec![f.name.clone()], false, cx);
                }
            }
            if let Some((key, names)) = menu_cmd {
                self.file_command(key, names, &m, cx, true);
            }
            if let Some(a) = action {
                file_actions::run(a, &m, cx.actions);
                self.refresh();
            }
        });

        // Commit message area
        // the diff above keeps at least a third of the dialog
        let max_message_h = (ui.available_height() * 2.0 / 3.0).max(160.0);
        egui::TopBottomPanel::bottom("commit_message").resizable(true).default_height(250.0).min_height(160.0).max_height(max_message_h).show_inside(
            ui,
            |ui| {
                ui.set_min_height(ui.available_height());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("Commit message")).strong());
                    ui.menu_button(tr("Templates…"), |ui| {
                        if cx.settings.commit_templates.is_empty() {
                            ui.label(RichText::new(tr("No templates (add them in Settings)")).italics());
                        }
                        for t in cx.settings.commit_templates.clone() {
                            if ui.button(&t.name).clicked() {
                                self.message = t.text.clone();
                                ui.close_kind(egui::UiKind::Menu);
                            }
                        }
                    });
                    ui.menu_button(tr("History…"), |ui| {
                        egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                            for msg in self.recent_messages.clone() {
                                let first = msg.lines().next().unwrap_or_default().to_string();
                                if ui.button(first).clicked() {
                                    self.message = msg.clone();
                                    ui.close_kind(egui::UiKind::Menu);
                                }
                            }
                        });
                    });
                    ui.menu_button(tr("Options…"), |ui| {
                        ui.checkbox(&mut self.sign_off, tr("Sign-off commit"));
                        ui.checkbox(&mut self.no_verify, tr("No verify (skip hooks)"));
                        ui.checkbox(&mut self.reset_author, tr("Reset author (with amend)"));
                        ui.horizontal(|ui| {
                            ui.label(tr("GPG sign:"));
                            ui.radio_value(&mut self.gpg_sign, None, tr("config"));
                            ui.radio_value(&mut self.gpg_sign, Some(true), tr("yes"));
                            ui.radio_value(&mut self.gpg_sign, Some(false), tr("no"));
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("Author:"));
                            ui.add(egui::TextEdit::singleline(&mut self.author).hint_text(tr("Name <email>")).desired_width(220.0));
                        });
                    });
                    let amend_before = self.amend;
                    ui.checkbox(&mut self.amend, tr("Amend commit"));
                    if self.amend && !amend_before {
                        if self.message.trim().is_empty() {
                            self.message = m.get_commit_message("HEAD");
                        }
                        if let Some(mgr) = &self.manager {
                            mgr.set_amend_state(true);
                        }
                    } else if !self.amend && amend_before {
                        if let Some(mgr) = &self.manager {
                            mgr.set_amend_state(false);
                        }
                    }
                });
                // validation (port of commit message validation settings)
                let first_len = self.message.lines().next().map(|l| l.chars().count()).unwrap_or(0);
                let mut warnings = Vec::new();
                let max_first = cx.settings.commit_validation_max_cnt_chars_first_line;
                if max_first > 0 && first_len > max_first {
                    warnings.push(trf("First line has {0} characters (max {1})", &[&first_len, &max_first]));
                }
                let max_line = cx.settings.commit_validation_max_cnt_chars_per_line;
                if max_line > 0 && self.message.lines().any(|l| l.chars().count() > max_line) {
                    warnings.push(trf("Lines longer than {0} characters", &[&max_line]));
                }
                if cx.settings.commit_validation_second_line_must_be_empty && self.message.lines().nth(1).is_some_and(|l| !l.trim().is_empty()) {
                    warnings.push(tr("Second line should be empty").into());
                }
                let buttons_h = 34.0;
                // room for the error below the buttons, else the panel grows by it every frame
                let error_h = if self.error.is_some() { ERROR_H + ui.spacing().item_spacing.y } else { 0.0 };
                let text_h = (ui.available_height() - buttons_h - error_h - 10.0).max(60.0);
                egui::ScrollArea::vertical().max_height(text_h).id_salt("commit_msg_scroll").show(ui, |ui| {
                    ui.add_sized(
                        Vec2::new(ui.available_width(), text_h),
                        egui::TextEdit::multiline(&mut self.message)
                            .id(message_id())
                            .font(egui::TextStyle::Monospace)
                            .hint_text(tr("Commit message (first line: summary)"))
                            .desired_rows(8),
                    );
                });
                ui.horizontal(|ui| {
                    let branch = cx.current_branch().unwrap_or_else(|| tr("(no branch)").into());
                    let committable = !staged.is_empty() || self.amend || cx.data.is_some_and(|d| d.state.merging);
                    if self.busy.is_some() {
                        ui.spinner();
                        ui.label(tr("Committing…"));
                    } else {
                        let label = if self.amend { tr("Amend commit") } else { tr("Commit") };
                        if ui
                            .add_enabled(committable, egui::Button::new(RichText::new(format!("✔ {label}")).strong()))
                            .on_hover_text(tr("Ctrl+Enter in the message"))
                            .clicked()
                        {
                            self.start_commit(cx, &m, false);
                        }
                        if ui.add_enabled(committable, egui::Button::new(trf("{0} & push", &[&label]))).clicked() {
                            self.start_commit(cx, &m, true);
                        }
                        if !committable && !unstaged.is_empty() && ui.button(tr("Stage all and commit")).clicked() {
                            if m.stage_all().is_ok() {
                                self.start_commit(cx, &m, false);
                            }
                            self.refresh();
                        }
                    }
                    ui.label(RichText::new(trf("on {0}", &[&branch])).color(palette.muted));
                    if ui.small_button(tr("Create branch…")).on_hover_text("Ctrl+B").clicked() {
                        if let Some(d) = cx.data {
                            cx.open(super::branch::CreateBranchDialog::new(d.head));
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.menu_button(format!("{} ⏷", tr("Reset / stash")), |ui| {
                            if ui.button(tr("Reset all changes…")).clicked() {
                                cx.open(super::reset::ResetChangesDialog::all());
                                ui.close_kind(egui::UiKind::Menu);
                            }
                            if ui.add_enabled(!unstaged.is_empty(), egui::Button::new(tr("Reset unstaged changes…"))).clicked() {
                                let tracked = unstaged.iter().any(|f| f.is_tracked && !f.is_new);
                                cx.open(super::reset::ResetChangesDialog::unstaged(tracked));
                                ui.close_kind(egui::UiKind::Menu);
                            }
                            ui.separator();
                            if ui.button(tr("Stash all changes")).clicked() {
                                cx.run(super::GitRun::new(tr("Stash"), gitext_core::commands::stash_save(self.show_untracked, false, None, &[])));
                                self.refresh();
                                ui.close_kind(egui::UiKind::Menu);
                            }
                            if ui.add_enabled(!staged.is_empty(), egui::Button::new(tr("Stash staged changes"))).clicked() {
                                cx.run(super::GitRun::new(tr("Stash staged changes"), gitext_core::GitArgs::new("stash").arg("push").arg("--staged")));
                                self.refresh();
                                ui.close_kind(egui::UiKind::Menu);
                            }
                        });
                        for w in &warnings {
                            ui.label(RichText::new(format!("⚠ {w}")).color(palette.warning));
                        }
                        ui.label(RichText::new(format!("{first_len}")).color(palette.muted)).on_hover_text(tr("Length of the first line"));
                        let (line, column) = cursor_line_column(ui.ctx(), &self.message);
                        ui.label(RichText::new(trf("Ln {0}, Col {1}", &[&line, &column])).color(palette.muted));
                    });
                });
                if let Some(e) = &self.error {
                    egui::ScrollArea::vertical().max_height(ERROR_H).id_salt("commit_error_scroll").show(ui, |ui| {
                        ui.colored_label(palette.error, e);
                    });
                }
            },
        );

        // Diff of the selected file
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let (list, items, staged_side) = match self.side {
                Side::Unstaged => (&self.unstaged, &unstaged, false),
                Side::Staged => (&self.staged, &staged, true),
            };
            let file = list.selected.last().and_then(|&i| items.get(i)).cloned();
            let content = match &file {
                None => ViewerContent::Empty(tr("Select a file to see the changes").into()),
                Some(f) => {
                    let opts = crate::views::revision_diff::diff_options(cx.settings);
                    let key = format!("{}|{}|{:?}|{}", staged_side, f.name, opts, self.generation);
                    let mm = m.clone();
                    let f2 = f.clone();
                    self.diff
                        .request(cx.ctx, key, move || {
                            let (first, second) = if staged_side { (None, ObjectId::INDEX) } else { (Some(ObjectId::INDEX), ObjectId::WORK_TREE) };
                            if f2.is_new && !f2.is_tracked {
                                return mm
                                    .get_file_bytes(ObjectId::WORK_TREE, &f2.name)
                                    .map(|b| content_from_bytes(&b))
                                    .unwrap_or(ViewerContent::Empty(tr("Cannot read file").into()));
                            }
                            match mm.get_file_diff(first, second, &f2.name, f2.old_name.as_deref(), &opts) {
                                Ok(d) if d.trim().is_empty() => ViewerContent::Empty(tr("No differences").into()),
                                Ok(d) => ViewerContent::Diff(d),
                                Err(e) => ViewerContent::Empty(e.to_string()),
                            }
                        })
                        .cloned()
                        .unwrap_or(ViewerContent::Empty(tr("Loading…").into()))
                }
            };
            if let Some(f) = &file {
                ui.label(RichText::new(&f.name).strong());
            }
            if self.viewer.toolbar(ui, &content, cx.settings) {
                cx.push(Action::SaveSettings);
            }
            let menu = if staged_side {
                vec![ViewerCommand::UnstageSelectedLines, ViewerCommand::AddToCommitMessage, ViewerCommand::CopyPatch]
            } else {
                vec![ViewerCommand::StageSelectedLines, ViewerCommand::ResetSelectedLines, ViewerCommand::AddToCommitMessage, ViewerCommand::CopyPatch]
            };
            if let Some(c) = self.viewer.ui(ui, &content, cx.settings.show_line_numbers, &menu) {
                if c == ViewerCommand::AddToCommitMessage {
                    self.add_to_message(&self.viewer.selected_text());
                } else if let ViewerContent::Diff(d) = &content {
                    let sel = self.viewer.selected_lines();
                    match c {
                        ViewerCommand::StageSelectedLines | ViewerCommand::UnstageSelectedLines => {
                            let reverse = c == ViewerCommand::UnstageSelectedLines;
                            if let Some(p) = create_partial_patch(d, &sel, reverse) {
                                if let Err(e) = m.apply_patch_text(&p, true, reverse) {
                                    cx.error(tr("Stage lines"), e.to_string());
                                }
                                self.refresh();
                            }
                        }
                        ViewerCommand::ResetSelectedLines => {
                            if let Some(p) = create_partial_patch(d, &sel, true) {
                                cx.open(Confirm::new(tr("Reset lines"), tr("Reset the selected lines? This cannot be undone."), tr("Reset"), move |cx| {
                                    if let Some(m) = cx.module {
                                        if let Err(e) = m.apply_patch_text(&p, false, true) {
                                            cx.error(tr("Reset lines"), e.to_string());
                                        }
                                    }
                                    cx.push(Action::RefreshStatus);
                                }));
                                self.refresh();
                            }
                        }
                        ViewerCommand::CopyPatch | ViewerCommand::CopyOldVersion | ViewerCommand::CopyNewVersion | ViewerCommand::AddToCommitMessage => {}
                    }
                }
            }
        });

        if ui.input(|i| i.key_pressed(egui::Key::Escape)) && self.busy.is_none() {
            if let Some(mgr) = &self.manager {
                mgr.set_merge_or_commit_message(Some(&self.message));
            }
            keep = false;
        }
        if !keep {
            cx.push(Action::RefreshStatus);
        }
        keep
    }
}

impl CommitDialog {
    /// The keys of the whole dialog (`FormCommit` hotkeys); runs before the widgets so that the
    /// message box does not take them.
    fn shortcuts(&mut self, ui: &Ui, cx: &mut Cx, m: &GitModule, unstaged: &[GitItemStatus]) {
        let ctx = ui.ctx().clone();
        let message_focused = ctx.memory(|mem| mem.has_focus(message_id()));
        let focus = |this: &mut Self, list: Option<Side>, viewer: bool| {
            this.unstaged.set_focus(list == Some(Side::Unstaged));
            this.staged.set_focus(list == Some(Side::Staged));
            this.viewer.set_focus(viewer);
            if let Some(id) = ctx.memory(|mem| mem.focused()) {
                ctx.memory_mut(|mem| mem.surrender_focus(id));
            }
        };
        if plain_key(ui.ctx(), Key::F5) {
            self.refresh();
        }
        if shortcut(ui.ctx(), Modifiers::COMMAND, Key::Enter) && self.busy.is_none() {
            if message_focused {
                self.start_commit(cx, m, false);
            } else {
                ctx.memory_mut(|mem| mem.request_focus(message_id()));
            }
        }
        if shortcut(ui.ctx(), Modifiers::COMMAND, Key::S) && !unstaged.is_empty() {
            if let Err(e) = m.stage_all() {
                cx.error(tr("Stage"), e.to_string());
            }
            self.refresh();
            cx.push(Action::RefreshStatus);
        }
        if shortcut(ui.ctx(), Modifiers::COMMAND, Key::Num1) {
            focus(self, Some(Side::Unstaged), false);
        }
        if shortcut(ui.ctx(), Modifiers::COMMAND, Key::Num2) {
            focus(self, None, true);
        }
        if shortcut(ui.ctx(), Modifiers::COMMAND, Key::Num3) {
            focus(self, Some(Side::Staged), false);
        }
        if shortcut(ui.ctx(), Modifiers::COMMAND, Key::Num4) {
            focus(self, None, false);
            ctx.memory_mut(|mem| mem.request_focus(message_id()));
        }
        if !self.viewer.has_focus(&ctx) && shortcut(ui.ctx(), Modifiers::COMMAND, Key::F) {
            focus(self, None, false);
            self.unstaged.focus_filter();
        }
        if shortcut(ui.ctx(), Modifiers::COMMAND, Key::B) {
            if let Some(d) = cx.data {
                cx.open(super::branch::CreateBranchDialog::new(d.head));
            }
        }
        // next / previous file of the current list; Alt+arrows move between changes in the diff
        let next = shortcut(ui.ctx(), Modifiers::COMMAND, Key::N);
        let previous = shortcut(ui.ctx(), Modifiers::COMMAND, Key::P);
        let (alt_next, alt_previous) = if self.viewer.has_focus(&ctx) {
            (false, false)
        } else {
            (
                shortcut(ui.ctx(), Modifiers::ALT, Key::ArrowDown) || shortcut(ui.ctx(), Modifiers::ALT, Key::ArrowRight),
                shortcut(ui.ctx(), Modifiers::ALT, Key::ArrowUp) || shortcut(ui.ctx(), Modifiers::ALT, Key::ArrowLeft),
            )
        };
        if next || previous || alt_next || alt_previous {
            if message_focused {
                self.side = Side::Staged;
            }
            let backwards = previous || alt_previous;
            let moved = match self.side {
                Side::Unstaged => self.unstaged.select_next(backwards),
                Side::Staged => self.staged.select_next(backwards),
            };
            if moved {
                match self.side {
                    Side::Unstaged => self.staged.clear(),
                    Side::Staged => self.unstaged.clear(),
                }
            }
        }
    }

    /// Appends `text` to the commit message (`AddSelectionToCommitMessage`).
    fn add_to_message(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if !self.message.is_empty() && !self.message.ends_with('\n') {
            self.message.push('\n');
        }
        self.message.push_str(text);
        self.message.push('\n');
    }

    fn file_command(&mut self, key: &str, names: Vec<String>, m: &GitModule, cx: &mut Cx, staged: bool) {
        let first = names.first().cloned().unwrap_or_default();
        match key {
            "stage" => self.do_stage(m, names, true, cx),
            "unstage" => self.do_stage(m, names, false, cx),
            "reset" => cx.open(super::reset::ResetChangesDialog::files(names)),
            "ignore" => cx.open(super::gitignore::AddToGitIgnoreDialog::new(names)),
            "assume" | "no_assume" | "skip" | "no_skip" => {
                let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                let r = match key {
                    "assume" | "no_assume" => m.assume_unchanged(&refs, key == "assume"),
                    _ => m.skip_worktree(&refs, key == "skip"),
                };
                if let Err(e) = r {
                    cx.error(tr("Update index"), e.to_string());
                }
                self.refresh();
            }
            "open" => crate::util::open_in_editor(&m.work_dir().join(&first), &cx.settings.editor),
            "open_with" => crate::util::open_with_system(&m.work_dir().join(&first).display().to_string()),
            "difftool" => {
                let args = gitext_core::GitArgs::new("difftool").arg("--find-renames").arg("--find-copies").arg_if(staged, "--cached").arg("--").arg(first);
                cx.push(Action::RunTool { tool_type: gitext_core::diff_tools::ToolType::Diff, args });
            }
            "history" => cx.open(super::file_history::FileHistoryDialog::new(first)),
            "blame" => cx.open(super::blame::BlameDialog::new(first, ObjectId::ZERO)),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_line_and_column() {
        assert_eq!(line_column("", 0), (1, 1));
        assert_eq!(line_column("summary\n\nbody", 3), (1, 4));
        assert_eq!(line_column("summary\n\nbody", 8), (2, 1));
        assert_eq!(line_column("summary\n\nbødy", 12), (3, 4));
    }
    use gitext_core::repo_history::RepositoryHistory;
    use gitext_core::settings::AppSettings;
    use std::process::Command;

    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let out = Command::new("git").current_dir(dir).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    struct Harness {
        ctx: egui::Context,
        module: GitModule,
        settings: AppSettings,
        history: RepositoryHistory,
        dialog: CommitDialog,
    }

    impl Harness {
        fn frame(&mut self, events: Vec<egui::Event>) {
            let modifiers = events.iter().find_map(|e| if let egui::Event::Key { modifiers, .. } = e { Some(*modifiers) } else { None }).unwrap_or_default();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))),
                events,
                modifiers,
                ..Default::default()
            };
            let mut actions = Vec::new();
            let ctx = self.ctx.clone();
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut cx = Cx {
                        ctx,
                        module: Some(&self.module),
                        data: None,
                        settings: &mut self.settings,
                        history: &mut self.history,
                        actions: &mut actions,
                        selected: &[],
                    };
                    self.dialog.ui(ui, &mut cx);
                });
            });
        }

        /// Draws frames until the file lists show `unstaged` and `staged` files.
        fn wait_for(&mut self, unstaged: usize, staged: usize) {
            for _ in 0..500 {
                self.frame(vec![]);
                let items = self.dialog.status.value.clone().and_then(|r| r.ok()).unwrap_or_default();
                let count = |s: StagedStatus| items.iter().filter(|i| i.staged == s).count();
                if self.dialog.status.value.is_some() && count(StagedStatus::WorkTree) == unstaged && count(StagedStatus::Index) == staged {
                    self.frame(vec![]);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            panic!("the status did not reach {unstaged} unstaged / {staged} staged files");
        }

        fn press(&mut self, key: egui::Key, modifiers: Modifiers) {
            self.frame(vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
        }
    }

    /// S stages the selected unstaged file and selects the next one, U unstages, Ctrl+S stages all.
    #[test]
    fn stage_and_unstage_keys() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        git(d, &["init", "-q"]);
        git(d, &["config", "user.email", "a@b"]);
        git(d, &["config", "user.name", "a"]);
        for f in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(d.join(f), "1\n").unwrap();
        }
        git(d, &["add", "."]);
        git(d, &["commit", "-q", "-m", "init"]);
        for f in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(d.join(f), "2\n").unwrap();
        }
        let mut h = Harness {
            ctx: egui::Context::default(),
            module: GitModule::open(d).unwrap(),
            settings: AppSettings::default(),
            history: Default::default(),
            dialog: CommitDialog::default(),
        };
        h.wait_for(3, 0);

        h.dialog.unstaged.selected = vec![0];
        h.dialog.unstaged.set_focus(true);
        h.press(egui::Key::S, Modifiers::NONE);
        assert_eq!(git(d, &["diff", "--cached", "--name-only"]), "a.txt\n");
        h.wait_for(2, 1);
        let names = |h: &Harness, l: &FileList| {
            let items = h.dialog.status.value.clone().and_then(|r| r.ok()).unwrap_or_default();
            let side = if std::ptr::eq(l, &h.dialog.unstaged) { StagedStatus::WorkTree } else { StagedStatus::Index };
            let items: Vec<GitItemStatus> = items.into_iter().filter(|i| i.staged == side).collect();
            l.selected_items(&items).iter().map(|i| i.name.clone()).collect::<Vec<_>>()
        };
        assert_eq!(names(&h, &h.dialog.unstaged), ["b.txt"], "the next file is selected");

        // a letter typed into the commit message does not stage, even though the list was clicked last
        h.ctx.memory_mut(|m| m.request_focus(message_id()));
        h.frame(vec![]);
        h.frame(vec![egui::Event::Text("s".into())]);
        h.press(egui::Key::S, Modifiers::NONE);
        assert_eq!(git(d, &["diff", "--cached", "--name-only"]), "a.txt\n");
        h.ctx.memory_mut(|m| m.surrender_focus(message_id()));
        h.dialog.unstaged.set_focus(false);

        h.dialog.staged.selected = vec![0];
        h.dialog.staged.set_focus(true);
        h.press(egui::Key::U, Modifiers::NONE);
        assert_eq!(git(d, &["diff", "--cached", "--name-only"]), "");
        h.wait_for(3, 0);

        h.press(egui::Key::S, Modifiers::COMMAND);
        assert_eq!(git(d, &["diff", "--cached", "--name-only"]), "a.txt\nb.txt\nc.txt\n");
    }
}
