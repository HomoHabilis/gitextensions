//! Port of `FormCommit`: stage/unstage files and lines, write the message and commit.

use egui::{RichText, Ui, Vec2};
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

        if ui.input(|i| i.key_pressed(egui::Key::F5)) {
            self.refresh();
        }

        egui::SidePanel::left("commit_files").resizable(true).default_width(380.0).width_range(220.0..=800.0).show_inside(ui, |ui| {
            let total_h = ui.available_height();
            // Unstaged
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Unstaged ({})", unstaged.len())).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⬇⬇ Stage all").clicked() {
                        if let Err(e) = m.stage_all() {
                            cx.error("Stage", e.to_string());
                        }
                        self.refresh();
                    }
                    if ui.add_enabled(!self.unstaged.selected.is_empty(), egui::Button::new("⬇ Stage")).clicked() {
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
                    let names: Vec<String> = sel.iter().filter_map(|&i| unstaged.get(i)).map(|f| f.name.clone()).collect();
                    for (label, key) in [
                        ("Stage", "stage"),
                        ("Reset file changes…", "reset"),
                        ("Delete file…", "delete"),
                        ("Add to .gitignore…", "ignore"),
                        ("Assume unchanged", "assume"),
                        ("Skip worktree", "skip"),
                        ("Open", "open"),
                        ("Open with difftool", "difftool"),
                        ("File history", "history"),
                        ("Blame", "blame"),
                        ("Copy path", "copy"),
                    ] {
                        if ui.button(label).clicked() {
                            menu_cmd = Some((key, names.clone()));
                            ui.close_menu();
                        }
                    }
                });
                if r.selection_changed {
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
                    if ui.add_enabled(!self.staged.selected.is_empty(), egui::Button::new("⬆ Unstage")).clicked() {
                        let files: Vec<String> = self.staged.selected_items(&staged).iter().map(|s| s.name.clone()).collect();
                        self.do_stage(&m, files, false, cx);
                    }
                });
            });
            let mut menu_cmd: Option<(&'static str, Vec<String>)> = None;
            let r = self.staged.ui(ui, "commit_staged", &staged, |ui, sel| {
                let names: Vec<String> = sel.iter().filter_map(|&i| staged.get(i)).map(|f| f.name.clone()).collect();
                for (label, key) in [("Unstage", "unstage"), ("Open", "open"), ("File history", "history"), ("Blame", "blame"), ("Copy path", "copy")] {
                    if ui.button(label).clicked() {
                        menu_cmd = Some((key, names.clone()));
                        ui.close_menu();
                    }
                }
            });
            if r.selection_changed {
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
                            ui.close_menu();
                        }
                    }
                });
                ui.menu_button("History…", |ui| {
                    egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                        for msg in self.recent_messages.clone() {
                            let first = msg.lines().next().unwrap_or_default().to_string();
                            if ui.button(first).clicked() {
                                self.message = msg.clone();
                                ui.close_menu();
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
                    egui::TextEdit::multiline(&mut self.message).font(egui::TextStyle::Monospace).hint_text("Commit message (first line: summary)").desired_rows(8),
                );
            });
            if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)) && self.busy.is_none() {
                self.start_commit(cx, &m, false);
            }
            ui.horizontal(|ui| {
                let branch = cx.current_branch().unwrap_or_else(|| "(no branch)".into());
                let committable = !staged.is_empty() || self.amend || cx.data.is_some_and(|d| d.state.merging);
                if self.busy.is_some() {
                    ui.spinner();
                    ui.label("Committing…");
                } else {
                    let label = if self.amend { "Amend commit" } else { "Commit" };
                    if ui.add_enabled(committable, egui::Button::new(RichText::new(format!("✔ {label}")).strong())).on_hover_text("Ctrl+Enter").clicked() {
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
                vec![ViewerCommand::UnstageSelectedLines, ViewerCommand::CopyPatch]
            } else {
                vec![ViewerCommand::StageSelectedLines, ViewerCommand::ResetSelectedLines, ViewerCommand::CopyPatch]
            };
            if let Some(c) = self.viewer.ui(ui, &content, cx.settings.show_line_numbers, &menu) {
                if let ViewerContent::Diff(d) = &content {
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
                        ViewerCommand::CopyPatch => {}
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
