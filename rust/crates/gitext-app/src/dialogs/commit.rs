//! Port of `FormCommit`: stage/unstage files and lines, write the message and commit.

use egui::{Key, Modifiers, RichText, Ui, Vec2};
use gitext_core::commands::{CommitOptions, UntrackedFilesMode};
use gitext_core::commit_message::{format_commit_message, CommitMessageManager};
use gitext_core::module::DiffOptions;
use gitext_core::patch::create_partial_patch;
use gitext_core::status::{GitItemStatus, StagedStatus};
use gitext_core::{GitModule, ObjectId};

use super::{Action, Confirm, Cx, Dialog, DialogKind};
use crate::tasks::{Loader, Task};
use crate::theme::Palette;
use crate::views::diff_viewer::{content_from_bytes, DiffViewer, ViewerCommand, ViewerContent};
use crate::views::file_list::FileList;
use crate::views::{plain_key, shortcut};

/// The context menu entries of the unstaged files: label, command, key.
const UNSTAGED_MENU: &[(&str, &str, Option<(Modifiers, Key)>)] = &[
    ("Stage", "stage", Some((Modifiers::NONE, Key::S))),
    ("Reset file changes…", "reset", Some((Modifiers::NONE, Key::R))),
    ("Delete file…", "delete", Some((Modifiers::NONE, Key::Delete))),
    ("Add to .gitignore…", "ignore", None),
    ("Assume unchanged", "assume", None),
    ("Skip worktree", "skip", None),
    ("Edit file", "open", Some((Modifiers::NONE, Key::F4))),
    ("Open", "open_with", Some((Modifiers::SHIFT, Key::F4))),
    ("Open with difftool", "difftool", Some((Modifiers::NONE, Key::F3))),
    ("File history", "history", Some((Modifiers::NONE, Key::H))),
    ("Blame", "blame", Some((Modifiers::NONE, Key::B))),
    ("Copy path", "copy", None),
];

/// The context menu entries of the staged files.
const STAGED_MENU: &[(&str, &str, Option<(Modifiers, Key)>)] = &[
    ("Unstage", "unstage", Some((Modifiers::NONE, Key::U))),
    ("Reset file changes…", "reset", Some((Modifiers::NONE, Key::R))),
    ("Edit file", "open", Some((Modifiers::NONE, Key::F4))),
    ("Open", "open_with", Some((Modifiers::SHIFT, Key::F4))),
    ("Open with difftool", "difftool", Some((Modifiers::NONE, Key::F3))),
    ("File history", "history", Some((Modifiers::NONE, Key::H))),
    ("Blame", "blame", Some((Modifiers::NONE, Key::B))),
    ("Copy path", "copy", None),
];

/// Shows the file context menu `entries`; returns the command clicked.
fn file_menu(ui: &mut Ui, entries: &[(&str, &'static str, Option<(Modifiers, Key)>)]) -> Option<&'static str> {
    let mut cmd = None;
    for &(label, key, shortcut) in entries {
        let text = shortcut.map(|(m, k)| ui.ctx().format_shortcut(&egui::KeyboardShortcut::new(m, k))).unwrap_or_default();
        if ui.add(egui::Button::new(label).shortcut_text(text)).clicked() {
            cmd = Some(key);
            ui.close_kind(egui::UiKind::Menu);
        }
    }
    cmd
}

/// The command of the key pressed in a file list with the keyboard focus.
fn file_key(ui: &Ui, entries: &[(&str, &'static str, Option<(Modifiers, Key)>)]) -> Option<&'static str> {
    entries.iter().find_map(|&(_, key, s)| s.filter(|&(m, k)| shortcut(ui.ctx(), m, k)).map(|_| key))
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
    template_text: Option<String>,
    /// The status of the browse window last seen, to reload when it changes (e.g. files were
    /// reset by another dialog or changed on disk).
    repo_status: Option<Vec<GitItemStatus>>,
}

fn message_id() -> egui::Id {
    egui::Id::new("commit_message_text")
}

impl CommitDialog {
    fn init(&mut self, cx: &mut Cx, m: &GitModule) {
        self.initialized = true;
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
            cx.error(if stage { "Stage" } else { "Unstage" }, e.to_string());
        }
        self.refresh();
        cx.push(Action::RefreshStatus);
    }

    fn start_commit(&mut self, cx: &mut Cx, m: &GitModule, push: bool) {
        let using_template = self.template_text.is_some();
        let message = format_commit_message(&self.message, using_template, cx.settings.ensure_commit_message_second_line_empty);
        if message.trim().is_empty() {
            self.error = Some("Please enter a commit message.".into());
            return;
        }
        if let Some(t) = &self.template_text {
            if self.message.trim() == t.trim() {
                self.error = Some("The commit message is the unchanged commit template.".into());
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
        "Commit".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(1200.0, 780.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let Some(m) = cx.module.cloned() else {
            ui.label("No repository");
            return !ui.button("Close").clicked();
        };
        if !self.initialized {
            self.init(cx, &m);
        }
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
                                cx.open(super::push::PushDialog::new(d, cx.settings, None));
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
        let items = self.status.request(cx.ctx, self.generation, move || mm.get_status(untracked, false).map_err(|e| e.to_string())).cloned();
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

        egui::SidePanel::left("commit_files").resizable(true).default_width(380.0).width_range(220.0..=800.0).show_inside(ui, |ui| {
            let total_h = ui.available_height();
            // Unstaged
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Unstaged ({})", unstaged.len())).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⬇⬇ Stage all").on_hover_text("Ctrl+S").clicked() {
                        if let Err(e) = m.stage_all() {
                            cx.error("Stage", e.to_string());
                        }
                        self.refresh();
                    }
                    if ui.add_enabled(!self.unstaged.selected.is_empty(), egui::Button::new("⬇ Stage")).on_hover_text("S").clicked() {
                        let files: Vec<String> = self.unstaged.selected_items(&unstaged).iter().map(|s| s.name.clone()).collect();
                        self.do_stage(&m, files, true, cx);
                    }
                    if ui.button("⟳").on_hover_text("Refresh (F5)").clicked() {
                        self.refresh();
                    }
                    if ui.selectable_label(self.show_untracked, "?").on_hover_text("Show untracked files").clicked() {
                        self.show_untracked = !self.show_untracked;
                        self.refresh();
                    }
                });
            });
            self.unstaged.toolbar(ui);
            let mut menu_cmd: Option<(&'static str, Vec<String>)> = None;
            ui.allocate_ui(Vec2::new(ui.available_width(), total_h * 0.5 - 70.0), |ui| {
                let r = self.unstaged.ui(ui, "commit_unstaged", &unstaged, |ui, sel| {
                    if let Some(key) = file_menu(ui, UNSTAGED_MENU) {
                        menu_cmd = Some((key, sel.iter().filter_map(|&i| unstaged.get(i)).map(|f| f.name.clone()).collect()));
                    }
                });
                if r.has_focus && !self.unstaged.selected.is_empty() {
                    if let Some(key) = file_key(ui, UNSTAGED_MENU) {
                        menu_cmd = Some((key, self.unstaged.selected_items(&unstaged).iter().map(|f| f.name.clone()).collect()));
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
            ui.separator();
            // Staged
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Staged ({})", staged.len())).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⬆⬆ Unstage all").clicked() {
                        if let Err(e) = m.unstage_all() {
                            cx.error("Unstage", e.to_string());
                        }
                        self.refresh();
                    }
                    if ui.add_enabled(!self.staged.selected.is_empty(), egui::Button::new("⬆ Unstage")).on_hover_text("U").clicked() {
                        let files: Vec<String> = self.staged.selected_items(&staged).iter().map(|s| s.name.clone()).collect();
                        self.do_stage(&m, files, false, cx);
                    }
                });
            });
            let mut menu_cmd: Option<(&'static str, Vec<String>)> = None;
            let r = self.staged.ui(ui, "commit_staged", &staged, |ui, sel| {
                if let Some(key) = file_menu(ui, STAGED_MENU) {
                    menu_cmd = Some((key, sel.iter().filter_map(|&i| staged.get(i)).map(|f| f.name.clone()).collect()));
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
        });

        // Commit message area
        egui::TopBottomPanel::bottom("commit_message").resizable(true).default_height(250.0).min_height(160.0).show_inside(ui, |ui| {
            ui.set_min_height(ui.available_height());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Commit message").strong());
                ui.menu_button("Templates…", |ui| {
                    if cx.settings.commit_templates.is_empty() {
                        ui.label(RichText::new("No templates (add them in Settings)").italics());
                    }
                    for t in cx.settings.commit_templates.clone() {
                        if ui.button(&t.name).clicked() {
                            self.message = t.text.clone();
                            ui.close_kind(egui::UiKind::Menu);
                        }
                    }
                });
                ui.menu_button("History…", |ui| {
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
                ui.menu_button("Options…", |ui| {
                    ui.checkbox(&mut self.sign_off, "Sign-off commit");
                    ui.checkbox(&mut self.no_verify, "No verify (skip hooks)");
                    ui.checkbox(&mut self.reset_author, "Reset author (with amend)");
                    ui.horizontal(|ui| {
                        ui.label("GPG sign:");
                        ui.radio_value(&mut self.gpg_sign, None, "config");
                        ui.radio_value(&mut self.gpg_sign, Some(true), "yes");
                        ui.radio_value(&mut self.gpg_sign, Some(false), "no");
                    });
                    ui.horizontal(|ui| {
                        ui.label("Author:");
                        ui.add(egui::TextEdit::singleline(&mut self.author).hint_text("Name <email>").desired_width(220.0));
                    });
                });
                let amend_before = self.amend;
                ui.checkbox(&mut self.amend, "Amend commit");
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
                warnings.push(format!("First line has {first_len} characters (max {max_first})"));
            }
            let max_line = cx.settings.commit_validation_max_cnt_chars_per_line;
            if max_line > 0 && self.message.lines().any(|l| l.chars().count() > max_line) {
                warnings.push(format!("Lines longer than {max_line} characters"));
            }
            if cx.settings.commit_validation_second_line_must_be_empty && self.message.lines().nth(1).is_some_and(|l| !l.trim().is_empty()) {
                warnings.push("Second line should be empty".into());
            }
            let buttons_h = 34.0;
            let text_h = (ui.available_height() - buttons_h - 10.0).max(60.0);
            egui::ScrollArea::vertical().max_height(text_h).id_salt("commit_msg_scroll").show(ui, |ui| {
                ui.add_sized(
                    Vec2::new(ui.available_width(), text_h),
                    egui::TextEdit::multiline(&mut self.message)
                        .id(message_id())
                        .font(egui::TextStyle::Monospace)
                        .hint_text("Commit message (first line: summary)")
                        .desired_rows(8),
                );
            });
            ui.horizontal(|ui| {
                let branch = cx.current_branch().unwrap_or_else(|| "(no branch)".into());
                let committable = !staged.is_empty() || self.amend || cx.data.is_some_and(|d| d.state.merging);
                if self.busy.is_some() {
                    ui.spinner();
                    ui.label("Committing…");
                } else {
                    let label = if self.amend { "Amend commit" } else { "Commit" };
                    if ui.add_enabled(committable, egui::Button::new(RichText::new(format!("✔ {label}")).strong())).on_hover_text("Ctrl+Enter in the message").clicked() {
                        self.start_commit(cx, &m, false);
                    }
                    if ui.add_enabled(committable, egui::Button::new(format!("{label} & push"))).clicked() {
                        self.start_commit(cx, &m, true);
                    }
                    if !committable && !unstaged.is_empty() && ui.button("Stage all and commit").clicked() {
                        if m.stage_all().is_ok() {
                            self.start_commit(cx, &m, false);
                        }
                        self.refresh();
                    }
                }
                ui.label(RichText::new(format!("on {branch}")).color(palette.muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Reset all changes…").clicked() {
                        cx.open(super::reset::ResetChangesDialog::all());
                    }
                    if ui.button("Stash").clicked() {
                        cx.run(super::GitRun::new("Stash", gitext_core::commands::stash_save(self.show_untracked, false, None, &[])));
                        self.refresh();
                    }
                    for w in &warnings {
                        ui.label(RichText::new(format!("⚠ {w}")).color(palette.warning));
                    }
                    ui.label(RichText::new(format!("{first_len}")).color(palette.muted)).on_hover_text("Length of the first line");
                });
            });
            if let Some(e) = &self.error {
                ui.colored_label(palette.error, e);
            }
        });

        // Diff of the selected file
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let (list, items, staged_side) = match self.side {
                Side::Unstaged => (&self.unstaged, &unstaged, false),
                Side::Staged => (&self.staged, &staged, true),
            };
            let file = list.selected.last().and_then(|&i| items.get(i)).cloned();
            let content = match &file {
                None => ViewerContent::Empty("Select a file to see the changes".into()),
                Some(f) => {
                    let opts = DiffOptions {
                        ignore_whitespace: cx.settings.ignore_whitespace,
                        context_lines: Some(cx.settings.context_lines),
                        ..Default::default()
                    };
                    let key = format!("{}|{}|{:?}|{}", staged_side, f.name, opts, self.generation);
                    let mm = m.clone();
                    let f2 = f.clone();
                    self.diff
                        .request(cx.ctx, key, move || {
                            let (first, second) = if staged_side { (None, ObjectId::INDEX) } else { (Some(ObjectId::INDEX), ObjectId::WORK_TREE) };
                            if f2.is_new && !f2.is_tracked {
                                return mm.get_file_bytes(ObjectId::WORK_TREE, &f2.name).map(|b| content_from_bytes(&b)).unwrap_or(ViewerContent::Empty("Cannot read file".into()));
                            }
                            match mm.get_file_diff(first, second, &f2.name, f2.old_name.as_deref(), &opts) {
                                Ok(d) if d.trim().is_empty() => ViewerContent::Empty("No differences".into()),
                                Ok(d) => ViewerContent::Diff(d),
                                Err(e) => ViewerContent::Empty(e.to_string()),
                            }
                        })
                        .cloned()
                        .unwrap_or(ViewerContent::Empty("Loading…".into()))
                }
            };
            if let Some(f) = &file {
                ui.label(RichText::new(&f.name).strong());
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
                                    cx.error("Stage lines", e.to_string());
                                }
                                self.refresh();
                            }
                        }
                        ViewerCommand::ResetSelectedLines => {
                            if let Some(p) = create_partial_patch(d, &sel, true) {
                                cx.open(Confirm::new("Reset lines", "Reset the selected lines? This cannot be undone.", "Reset", move |cx| {
                                    if let Some(m) = cx.module {
                                        if let Err(e) = m.apply_patch_text(&p, false, true) {
                                            cx.error("Reset lines", e.to_string());
                                        }
                                    }
                                    cx.push(Action::RefreshStatus);
                                }));
                                self.refresh();
                            }
                        }
                        ViewerCommand::CopyPatch | ViewerCommand::AddToCommitMessage => {}
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
                cx.error("Stage", e.to_string());
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
            "delete" => {
                let list = names.join("\n");
                cx.open(Confirm::new("Delete files", format!("Delete these files from disk?\n\n{list}"), "Delete", move |cx| {
                    if let Some(m) = cx.module {
                        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                        if let Err(e) = m.delete_untracked(&refs) {
                            cx.error("Delete", e.to_string());
                        }
                    }
                    cx.push(Action::RefreshStatus);
                }));
                self.refresh();
            }
            "ignore" => cx.open(super::gitignore::AddToGitIgnoreDialog::new(names)),
            "assume" => {
                let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                let _ = m.assume_unchanged(&refs, true);
                self.refresh();
            }
            "skip" => {
                let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                let _ = m.skip_worktree(&refs, true);
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
            "copy" => cx.push(Action::Copy(names.join("\n"))),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))), events, modifiers, ..Default::default() };
            let mut actions = Vec::new();
            let ctx = self.ctx.clone();
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut cx = Cx { ctx, module: Some(&self.module), data: None, settings: &mut self.settings, history: &mut self.history, actions: &mut actions, selected: &[] };
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
        let mut h = Harness { ctx: egui::Context::default(), module: GitModule::open(d).unwrap(), settings: AppSettings::default(), history: Default::default(), dialog: CommitDialog::default() };
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
