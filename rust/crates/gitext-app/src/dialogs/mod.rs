//! Dialogs of the application (ports of the `Form*` classes of `GitUI.CommandsDialogs`).

use std::path::PathBuf;

use egui::{Ui, Vec2};
use gitext_core::settings::AppSettings;
use gitext_core::repo_history::RepositoryHistory;
use gitext_core::{GitArgs, GitModule, ObjectId};

use crate::repo::RepoData;

pub mod about;
pub mod archive;
pub mod bisect;
pub mod blame;
pub mod branch;
pub mod checkout;
pub mod cherry_pick;
pub mod cleanup;
pub mod clone;
pub mod command_log;
pub mod commit;
pub mod compare;
pub mod conflicts;
pub mod editor;
pub mod file_history;
pub mod gitignore;
pub mod goto_commit;
pub mod init;
pub mod merge;
pub mod open_repo;
pub mod message;
pub mod patch;
pub mod process;
pub mod pull;
pub mod push;
pub mod rebase;
pub mod reflog;
pub mod remotes;
pub mod reset;
pub mod settings;
pub mod stash;
pub mod submodules;
pub mod tag;
pub mod tools;
pub mod verify;
pub mod worktrees;

/// A request from a view or dialog, processed by the application after the frame.
pub enum Action {
    OpenRepo(PathBuf),
    CloseRepo,
    /// Reload refs, status and the revision log.
    Refresh,
    /// Reload refs and status only (keeps the log).
    RefreshStatus,
    OpenDialog(Box<dyn Dialog>),
    /// Runs git commands in a process dialog; `then` actions run on success.
    RunGit(GitRun),
    /// Starts the diff or merge tool (`git difftool` / `git mergetool` + `args` without the
    /// command), in the background; failures are reported.
    RunTool { tool_type: gitext_core::diff_tools::ToolType, args: GitArgs },
    Message { title: String, text: String, error: bool },
    SelectRevision(ObjectId),
    SaveSettings,
    ApplyTheme,
    /// Open the commit dialog.
    OpenCommit,
    /// Switch the browse window to the given tab (0 commit, 1 diff, 2 file tree).
    ShowTab(usize),
    /// Show the file tree at a revision and select a path.
    ShowInFileTree(ObjectId, Option<String>),
    /// Set the revision grid path filter.
    FilterPath(String),
    /// Reload the log with new settings (branch filter etc).
    ReloadLog,
    /// Copy text to the clipboard.
    Copy(String),
    Exit,
}

/// Commands to run in a [`process::ProcessDialog`].
pub struct GitRun {
    pub title: String,
    pub commands: Vec<GitArgs>,
    pub working_dir: Option<PathBuf>,
    pub refresh: bool,
    pub then: Vec<Action>,
    /// Offer to solve merge conflicts if the command fails with conflicts.
    pub check_conflicts: bool,
    /// Keep the dialog open even on success.
    pub keep_open: bool,
}

impl GitRun {
    pub fn new(title: impl Into<String>, args: GitArgs) -> Self {
        GitRun { title: title.into(), commands: vec![args], working_dir: None, refresh: true, then: Vec::new(), check_conflicts: false, keep_open: false }
    }

    pub fn many(title: impl Into<String>, commands: Vec<GitArgs>) -> Self {
        GitRun { title: title.into(), commands, working_dir: None, refresh: true, then: Vec::new(), check_conflicts: false, keep_open: false }
    }

    pub fn conflicts(mut self) -> Self {
        self.check_conflicts = true;
        self
    }

    pub fn then(mut self, a: Action) -> Self {
        self.then.push(a);
        self
    }

    pub fn in_dir(mut self, dir: PathBuf) -> Self {
        self.working_dir = Some(dir);
        self
    }

    pub fn no_refresh(mut self) -> Self {
        self.refresh = false;
        self
    }

    pub fn keep_open(mut self) -> Self {
        self.keep_open = true;
        self
    }
}

/// Context passed to dialogs.
pub struct Cx<'a> {
    pub ctx: &'a egui::Context,
    pub module: Option<&'a GitModule>,
    pub data: Option<&'a RepoData>,
    pub settings: &'a mut AppSettings,
    #[allow(dead_code)] // kept for dialogs that manage the recent repository list
    pub history: &'a mut RepositoryHistory,
    pub actions: &'a mut Vec<Action>,
    /// Selected revisions of the browse window.
    pub selected: &'a [ObjectId],
}

impl<'a> Cx<'a> {
    pub fn push(&mut self, a: Action) {
        self.actions.push(a);
    }

    pub fn run(&mut self, run: GitRun) {
        self.actions.push(Action::RunGit(run));
    }

    pub fn error(&mut self, title: &str, text: impl Into<String>) {
        self.actions.push(Action::Message { title: title.into(), text: text.into(), error: true });
    }

    pub fn open(&mut self, d: impl Dialog + 'static) {
        self.actions.push(Action::OpenDialog(Box::new(d)));
    }

    pub fn current_branch(&self) -> Option<String> {
        self.data.and_then(|d| d.current_branch.clone())
    }
}

/// How a dialog is presented.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DialogKind {
    /// A small modal-like window in the center.
    Modal(f32),
    /// A large resizable window (separate native window when supported).
    Window(Vec2),
}

pub trait Dialog {
    fn title(&self) -> String;
    fn kind(&self) -> DialogKind {
        DialogKind::Modal(420.0)
    }
    /// Draws the dialog; return `false` to close it.
    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool;
    /// Unique id (dialogs with the same id are not opened twice).
    fn id(&self) -> String {
        self.title()
    }
}

/// Standard OK/Cancel button row. Returns (ok, cancel).
pub fn ok_cancel(ui: &mut Ui, ok_text: &str, ok_enabled: bool) -> (bool, bool) {
    let mut ok = false;
    let mut cancel = false;
    ui.add_space(6.0);
    ui.separator();
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            cancel = ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
            ok = ui.add_enabled(ok_enabled, egui::Button::new(egui::RichText::new(ok_text).strong())).clicked();
        });
    });
    (ok, cancel)
}

/// A labelled row in a form grid.
pub fn form_row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.label(label);
    add(ui);
    ui.end_row();
}

/// A combo box over a list of strings with free text.
pub fn branch_combo(ui: &mut Ui, id: &str, value: &mut String, items: &[String], width: f32) {
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(value).desired_width(width - 28.0));
        egui::ComboBox::from_id_salt(id).selected_text("").width(20.0).height(320.0).show_ui(ui, |ui| {
            for item in items {
                if ui.selectable_label(value == item, item).clicked() {
                    *value = item.clone();
                }
            }
        });
    });
}

/// Simple yes/no confirmation which runs actions on "yes".
pub struct Confirm {
    pub title: String,
    pub text: String,
    pub yes: String,
    pub on_yes: Option<Box<dyn FnOnce(&mut Cx)>>,
}

impl Confirm {
    pub fn new(title: impl Into<String>, text: impl Into<String>, yes: impl Into<String>, on_yes: impl FnOnce(&mut Cx) + 'static) -> Self {
        Confirm { title: title.into(), text: text.into(), yes: yes.into(), on_yes: Some(Box::new(on_yes)) }
    }
}

impl Dialog for Confirm {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn id(&self) -> String {
        format!("{}:{}", self.title, self.text)
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.label(&self.text);
        let (ok, cancel) = ok_cancel(ui, &self.yes, true);
        if ok {
            if let Some(f) = self.on_yes.take() {
                f(cx);
            }
            return false;
        }
        !cancel
    }
}
