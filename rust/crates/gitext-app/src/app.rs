//! The application: menu bar, dashboard / browse window, dialogs and action processing.

use std::path::{Path, PathBuf};

use egui::{Key, KeyboardShortcut, Modifiers, RichText};
use gitext_core::app_title::{generate_title, repository_description, APPLICATION_NAME};
use gitext_core::repo_history::{RepositoryAnchor, RepositoryHistory};
use gitext_core::settings::{AppSettings, GraphDrawStyle, Theme};
use gitext_core::{GitArgs, GitModule, ObjectId};

use crate::browse::BrowseView;
use crate::dialogs::{self, Action, Cx, Dialog, DialogKind};
use crate::views::dashboard::{Dashboard, DashboardCommand};

/// What to show at startup (from the command line).
#[derive(Debug, Clone, Default)]
pub enum StartCommand {
    #[default]
    Browse,
    Commit,
    Clone(Option<String>),
    Init(Option<String>),
    Blame(String, Option<String>),
    FileHistory(String),
    Settings,
    About,
    Pull,
    Push,
    Stash,
    Merge(Option<String>),
    Rebase(Option<String>),
    Checkout,
    Branch,
    Tag,
    Remotes,
    Reflog,
    Archive,
    ApplyPatch(Option<String>),
    Cleanup,
    MergeConflicts,
    Search,
    GitIgnore,
    /// Open a file with the diff tool (run without a window, see `main`).
    DiffTool(String),
    /// Reset the changes of paths (`[]`: all changes).
    ResetChanges(Vec<String>),
    AddFiles(String),
    ViewDiff,
    CheckoutRevision,
    /// Add (`true`) or remove the Explorer context menu (run without a window, see `main`).
    ShellExt(bool),
}

/// A diff / merge tool running in the background.
type ToolRun = crate::tasks::Task<gitext_core::exec::GitResult<gitext_core::exec::ExecResult>>;

pub struct GitExtApp {
    pub settings: AppSettings,
    settings_path: Option<PathBuf>,
    pub history: RepositoryHistory,
    history_path: Option<PathBuf>,
    pub browse: Option<BrowseView>,
    dashboard: Dashboard,
    dialogs: Vec<Box<dyn Dialog>>,
    actions: Vec<Action>,
    theme_applied: bool,
    start: Option<StartCommand>,
    exit_after_dialog: bool,
    last_title: String,
    /// Repository description for the title, cached per working directory.
    title_description: Option<(std::path::PathBuf, String)>,
    /// Diff / merge tools started in the background (title, result when the tool exits).
    tool_runs: Vec<(String, ToolRun)>,
    /// Hides the window until its first frames are painted (Windows); `None` once shown.
    startup_window: Option<crate::startup_window::StartupWindow>,
}

impl GitExtApp {
    pub fn new(cc: &eframe::CreationContext<'_>, repo: Option<PathBuf>, start: StartCommand, startup_window: crate::startup_window::StartupWindow) -> Self {
        let settings_path = AppSettings::default_path();
        let settings = settings_path.as_deref().map(AppSettings::load).unwrap_or_default();
        gitext_core::exec::set_git_command(&settings.git_command);
        gitext_core::exec::set_wsl_git_enabled(settings.wsl_git_enabled);
        let history_path = RepositoryHistory::default_path();
        let history = history_path.as_deref().map(|p| RepositoryHistory::load(p, settings.recent_repositories_history_size)).unwrap_or_default();
        let mut app = GitExtApp {
            settings,
            settings_path,
            history,
            history_path,
            browse: None,
            dashboard: Dashboard::default(),
            dialogs: Vec::new(),
            actions: Vec::new(),
            theme_applied: false,
            exit_after_dialog: !matches!(start, StartCommand::Browse),
            start: Some(start),
            last_title: String::new(),
            title_description: None,
            tool_runs: Vec::new(),
            startup_window: Some(startup_window),
        };
        if let Some(path) = repo {
            app.open_repo(&cc.egui_ctx, &path, false);
        }
        app
    }

    fn save_settings(&self) {
        if let Some(p) = &self.settings_path {
            let _ = self.settings.save(p);
        }
    }

    fn save_history(&self) {
        if let Some(p) = &self.history_path {
            let _ = self.history.save(p);
        }
    }

    fn module(&self) -> Option<&GitModule> {
        self.browse.as_ref().map(|b| &b.module)
    }

    fn open_repo(&mut self, ctx: &egui::Context, path: &Path, report_errors: bool) {
        match GitModule::open(path) {
            Ok(module) => {
                if let Some(b) = &mut self.browse {
                    b.grid.cancel();
                }
                self.history.max_recent = self.settings.recent_repositories_history_size;
                if self.history.add_as_most_recent(&module.work_dir().display().to_string()) {
                    self.save_history();
                }
                self.browse = Some(BrowseView::new(ctx, module, &self.settings));
            }
            Err(e) => {
                if !report_errors {
                    return;
                }
                let text = e.to_string();
                if let Some(safe_dir) = dubious_ownership_safe_directory(&text) {
                    // port of UIReporter.ReportDubiousOwnership: offer to trust the repository
                    let path = path.to_path_buf();
                    self.open_dialog(Box::new(dialogs::Confirm::new(
                        "Open repository",
                        format!(
                            "{text}\n\nGit refuses to work in a repository owned by another user. Trust this repository?\n\
                             This runs: git config --global --add safe.directory {safe_dir}"
                        ),
                        "Trust and open",
                        move |cx| {
                            let r = gitext_core::Executable::git(std::env::temp_dir())
                                .run_checked(&gitext_core::GitArgs::new("config").arg("--global").arg("--add").arg("safe.directory").arg(&safe_dir));
                            match r {
                                Ok(_) => cx.push(Action::OpenRepo(path)),
                                Err(e) => cx.error("Open repository", e.to_string()),
                            }
                        },
                    )));
                    return;
                }
                let mut text = text;
                if cfg!(windows) && gitext_core::wsl::is_wsl_path(&path.to_string_lossy()) {
                    text.push_str(if self.settings.wsl_git_enabled {
                        "\n\nThis repository is run with the git of the WSL distro: make sure git is installed there \
                         (sudo apt install git), or turn off \"Use the git of the WSL distro\" in Settings > Git."
                    } else {
                        "\n\nTurn on \"Use the git of the WSL distro\" in Settings > Git to run WSL repositories with the git of the distro."
                    });
                }
                self.actions.push(Action::Message { title: "Open repository".into(), text, error: true });
            }
        }
    }

    /// Starts the diff / merge tool (port of `OpenWithDifftool` with `RunDetached`): the
    /// configured tool, or a detected one, or the "no tool" dialog.
    fn run_tool(&mut self, ctx: &egui::Context, tool_type: gitext_core::diff_tools::ToolType, args: GitArgs) {
        use gitext_core::diff_tools::{self, ToolConfigStore, ToolLaunch};
        let Some(m) = self.module() else { return };
        let exe = m.git();
        // the git of a WSL distro uses the tools configured in the distro
        let launch = if exe.wsl_distro.is_empty() { diff_tools::resolve_launch(&ToolConfigStore::new(exe.clone()), tool_type) } else { Some(ToolLaunch::Configured) };
        let Some(launch) = launch else {
            self.open_dialog(Box::new(dialogs::tools::NoToolDialog { tool_type }));
            return;
        };
        let args = diff_tools::launch_args(&launch, tool_type, &args);
        let title = format!("{} tool", if tool_type == diff_tools::ToolType::Diff { "Diff" } else { "Merge" });
        self.tool_runs.push((title, crate::tasks::Task::spawn(ctx, move || exe.run(&args))));
    }

    /// Reports diff / merge tools that failed to start.
    fn poll_tool_runs(&mut self) {
        let mut finished = Vec::new();
        self.tool_runs.retain_mut(|(title, task)| match task.try_take() {
            Some(r) => {
                finished.push((title.clone(), r));
                false
            }
            None => true,
        });
        for (title, r) in finished {
            let error = match r {
                Ok(r) if r.success() => continue,
                Ok(r) => format!("The tool exited with code {}.\n\n{}", r.exit_code, r.all_output().trim()),
                Err(e) => e.to_string(),
            };
            self.actions.push(Action::Message { title, text: error, error: true });
        }
    }

    fn open_dialog(&mut self, d: Box<dyn Dialog>) {
        let id = d.id();
        if self.dialogs.iter().any(|x| x.id() == id) {
            return;
        }
        self.dialogs.push(d);
    }

    fn run_start_command(&mut self) {
        let Some(start) = self.start.take() else { return };
        use dialogs::*;
        let data = self.browse.as_ref().map(|b| b.data.clone()).unwrap_or_default();
        let d: Option<Box<dyn Dialog>> = match start {
            StartCommand::Browse => None,
            StartCommand::Commit => Some(Box::new(commit::CommitDialog::default())),
            StartCommand::Clone(url) => Some(Box::new(clone::CloneDialog::new(url, &self.settings))),
            StartCommand::Init(path) => Some(Box::new(init::InitDialog::new(path))),
            StartCommand::Blame(file, line) => Some(Box::new(blame::BlameDialog::with_line(file, ObjectId::ZERO, line.and_then(|l| l.parse().ok())))),
            StartCommand::FileHistory(file) => Some(Box::new(file_history::FileHistoryDialog::new(file))),
            StartCommand::Settings => Some(Box::new(settings::SettingsDialog::default())),
            StartCommand::About => Some(Box::new(about::AboutDialog)),
            StartCommand::Pull => Some(Box::new(pull::PullDialog::new(&data, None))),
            StartCommand::Push => Some(Box::new(push::PushDialog::new(&data, &self.settings, None))),
            StartCommand::Stash => Some(Box::new(stash::StashDialog::default())),
            StartCommand::Merge(b) => Some(Box::new(merge::MergeDialog::new(&data, b.as_deref().unwrap_or("")))),
            StartCommand::Rebase(b) => Some(Box::new(rebase::RebaseDialog::new(&data, b.as_deref().unwrap_or("")))),
            StartCommand::Checkout => Some(Box::new(checkout::CheckoutBranchDialog::new(&data, None, false))),
            StartCommand::Branch => Some(Box::new(branch::CreateBranchDialog::new(data.head))),
            StartCommand::Tag => Some(Box::new(tag::CreateTagDialog::new(data.head))),
            StartCommand::Remotes => Some(Box::new(remotes::RemotesDialog::default())),
            StartCommand::Reflog => Some(Box::new(reflog::ReflogDialog::default())),
            StartCommand::Archive => Some(Box::new(archive::ArchiveDialog::new(data.head))),
            StartCommand::ApplyPatch(p) => Some(Box::new(patch::ApplyPatchDialog::new(p))),
            StartCommand::Cleanup => Some(Box::new(cleanup::CleanupDialog::default())),
            StartCommand::MergeConflicts => Some(Box::new(conflicts::ConflictsDialog::default())),
            StartCommand::Search => {
                // the file search of the original is the "Find file" box of the file tree tab
                self.actions.push(Action::ShowTab(2));
                None
            }
            StartCommand::GitIgnore => Some(Box::new(gitignore::GitIgnoreDialog::new(false))),
            StartCommand::ResetChanges(paths) if paths.is_empty() => Some(Box::new(reset::ResetChangesDialog::all())),
            StartCommand::ResetChanges(paths) => Some(Box::new(reset::ResetChangesDialog::files(paths))),
            StartCommand::AddFiles(pattern) => Some(Box::new(gitignore::AddFilesDialog::new(pattern))),
            StartCommand::ViewDiff => Some(Box::new(compare::CompareDialog::new(&data))),
            StartCommand::CheckoutRevision => Some(Box::new(checkout::CheckoutRevisionDialog::new(ObjectId::ZERO))),
            // run by `main` without a window
            StartCommand::DiffTool(_) | StartCommand::ShellExt(_) => None,
        };
        match d {
            Some(d) => self.open_dialog(d),
            None => self.exit_after_dialog = false,
        }
    }

    fn process_actions(&mut self, ctx: &egui::Context) {
        let mut guard = 0;
        if !self.actions.is_empty() {
            // dialogs opened here are drawn in the next frame
            ctx.request_repaint();
        }
        while !self.actions.is_empty() && guard < 20 {
            guard += 1;
            let actions = std::mem::take(&mut self.actions);
            for a in actions {
                match a {
                    Action::OpenRepo(p) => self.open_repo(ctx, &p, true),
                    Action::CloseRepo => {
                        if let Some(b) = &mut self.browse {
                            b.grid.cancel();
                        }
                        self.browse = None;
                    }
                    Action::Refresh => {
                        if let Some(b) = &mut self.browse {
                            b.refresh(ctx, &self.settings, true);
                        }
                    }
                    Action::RefreshStatus => {
                        if let Some(b) = &mut self.browse {
                            b.refresh(ctx, &self.settings, false);
                        }
                    }
                    Action::ReloadLog => {
                        if let Some(b) = &mut self.browse {
                            b.reload_log(ctx, &self.settings);
                        }
                    }
                    Action::OpenDialog(d) => self.open_dialog(d),
                    Action::RunGit(run) => self.open_dialog(Box::new(dialogs::process::ProcessDialog::new(run))),
                    Action::RunTool { tool_type, args } => self.run_tool(ctx, tool_type, args),
                    Action::Message { title, text, error } => self.open_dialog(Box::new(dialogs::message::MessageDialog { title, text, error })),
                    Action::SelectRevision(id) => {
                        if let Some(b) = &mut self.browse {
                            b.grid.select(id);
                        }
                    }
                    Action::SaveSettings => self.save_settings(),
                    Action::ApplyTheme => self.theme_applied = false,
                    Action::OpenCommit => self.open_dialog(Box::new(dialogs::commit::CommitDialog::default())),
                    Action::ShowTab(t) => {
                        if let Some(b) = &mut self.browse {
                            b.tab = t;
                        }
                    }
                    Action::ShowInFileTree(id, path) => {
                        if let Some(b) = &mut self.browse {
                            b.grid.select(id);
                            b.tab = 2;
                            b.file_tree.reveal = path.clone();
                            b.file_tree.selected = path;
                        }
                    }
                    Action::FilterPath(p) => {
                        if let Some(b) = &mut self.browse {
                            b.set_path_filter(p);
                            b.reload_log(ctx, &self.settings);
                        }
                    }
                    Action::Copy(s) => crate::util::copy_to_clipboard(ctx, s),
                    Action::Exit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                }
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let pressed = |k: KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&k));
        if pressed(KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus)) || pressed(KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals)) {
            self.settings.ui_scale = (self.settings.ui_scale + 0.1).min(3.0);
            self.theme_applied = false;
        }
        if pressed(KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus)) {
            self.settings.ui_scale = (self.settings.ui_scale - 0.1).max(0.5);
            self.theme_applied = false;
        }
        // the keys of the main window do not apply while a dialog (always kept on top) is open,
        // as in Git Extensions where each dialog is a separate window
        if !self.dialogs.is_empty() {
            return;
        }
        if pressed(KeyboardShortcut::new(Modifiers::COMMAND, Key::O)) {
            self.open_folder_dialog();
        }
        let Some(b) = &mut self.browse else { return };
        if crate::views::plain_key(ctx, Key::F5) {
            self.actions.push(Action::Refresh);
        }
        if pressed(KeyboardShortcut::new(Modifiers::COMMAND, Key::Space)) {
            self.actions.push(Action::OpenCommit);
        }
        if crate::views::shortcut(ctx, Modifiers::ALT, Key::ArrowLeft) {
            b.grid.navigate_parent();
        }
        if crate::views::shortcut(ctx, Modifiers::ALT, Key::ArrowRight) {
            b.grid.navigate_child();
        }
        b.shortcuts(ctx, &mut self.settings, &mut self.actions);
    }

    /// "Open local repository" (`FormOpenDirectory`).
    fn open_folder_dialog(&mut self) {
        let start = self.module().map(|m| m.work_dir().display().to_string());
        let recent = self.history.recent.iter().map(|r| r.path.clone()).collect();
        self.open_dialog(Box::new(dialogs::open_repo::OpenRepoDialog::new(start, recent)));
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        use dialogs::*;
        let has_repo = self.browse.is_some();
        let data = self.browse.as_ref().map(|b| b.data.clone()).unwrap_or_default();
        let head = data.head;
        let mut open: Vec<Box<dyn Dialog>> = Vec::new();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Start", |ui| {
                if ui.button("Open…  (Ctrl+O)").clicked() {
                    self.open_folder_dialog();
                    ui.close_kind(egui::UiKind::Menu);
                }
                if ui.button("Clone repository…").clicked() {
                    open.push(Box::new(clone::CloneDialog::new(None, &self.settings)));
                    ui.close_kind(egui::UiKind::Menu);
                }
                if ui.button("Create new repository…").clicked() {
                    open.push(Box::new(init::InitDialog::new(None)));
                    ui.close_kind(egui::UiKind::Menu);
                }
                ui.menu_button("Recent repositories", |ui| {
                    for r in self.history.recent.clone() {
                        if ui.button(&r.path).clicked() {
                            self.actions.push(Action::OpenRepo(PathBuf::from(&r.path)));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                    }
                    ui.separator();
                    if ui.button("Clear recent repositories").clicked() {
                        self.history.recent.retain(|r| r.anchor != RepositoryAnchor::None);
                        self.save_history();
                        ui.close_kind(egui::UiKind::Menu);
                    }
                });
                ui.separator();
                if ui.add_enabled(has_repo, egui::Button::new("Close (go to Dashboard)  (Ctrl+W)")).clicked() {
                    self.actions.push(Action::CloseRepo);
                    ui.close_kind(egui::UiKind::Menu);
                }
                if ui.button("Exit").clicked() {
                    self.actions.push(Action::Exit);
                    ui.close_kind(egui::UiKind::Menu);
                }
            });
            if ui.button("Dashboard").clicked() {
                self.actions.push(Action::CloseRepo);
            }
            ui.add_enabled_ui(has_repo, |ui| {
                ui.menu_button("Repository", |ui| {
                    if ui.button("Refresh  (F5)").clicked() {
                        self.actions.push(Action::Refresh);
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if ui.button("File explorer").clicked() {
                        if let Some(m) = self.module() {
                            crate::util::open_with_system(&m.work_dir().display().to_string());
                        }
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if ui.button("Terminal  (Ctrl+G)").clicked() {
                        if let Some(m) = self.module() {
                            crate::util::open_terminal(m.work_dir(), &self.settings.terminal);
                        }
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    ui.separator();
                    if ui.button("Manage remotes…").clicked() {
                        open.push(Box::new(remotes::RemotesDialog::default()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if ui.button("Manage submodules…").clicked() {
                        open.push(Box::new(submodules::SubmodulesDialog::default()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if ui.button("Manage worktrees…  (Ctrl+Alt+W)").clicked() {
                        open.push(Box::new(worktrees::WorktreesDialog::default()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    ui.separator();
                    if ui.button("Edit .gitignore…").clicked() {
                        open.push(Box::new(gitignore::GitIgnoreDialog::new(false)));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if ui.button("Edit .git/info/exclude…").clicked() {
                        open.push(Box::new(gitignore::GitIgnoreDialog::new(true)));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if ui.button("Edit .gitattributes…").clicked() {
                        open.push(Box::new(editor::FileEditorDialog::work_file(".gitattributes", "Edit .gitattributes")));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if ui.button("Edit .mailmap…").clicked() {
                        open.push(Box::new(editor::FileEditorDialog::work_file(".mailmap", "Edit .mailmap")));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    ui.menu_button("Git maintenance", |ui| {
                        if ui.button("Compress git database").clicked() {
                            self.actions.push(Action::RunGit(GitRun::new("Compress git database", GitArgs::new("gc").arg("--prune")).keep_open()));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button("Recover lost objects…").clicked() {
                            open.push(Box::new(verify::VerifyDialog::default()));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button("Delete index.lock").clicked() {
                            if let Some(m) = self.module() {
                                let p = m.git_dir().join("index.lock");
                                let text = if p.exists() {
                                    match std::fs::remove_file(&p) {
                                        Ok(()) => "index.lock deleted.".to_string(),
                                        Err(e) => e.to_string(),
                                    }
                                } else {
                                    "index.lock not found.".to_string()
                                };
                                self.actions.push(Action::Message { title: "Delete index.lock".into(), text, error: false });
                            }
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button("Edit .git/config…").clicked() {
                            if let Some(m) = self.module() {
                                open.push(Box::new(editor::FileEditorDialog::path(m.git_dir().join("config"), "Edit .git/config")));
                            }
                            ui.close_kind(egui::UiKind::Menu);
                        }
                    });
                    if ui.button("Sparse working copy…").clicked() {
                        open.push(Box::new(editor::SparseCheckoutDialog::default()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if ui.button("Repository settings (git config)…").clicked() {
                        open.push(Box::new(settings::GitConfigDialog::default()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                });
                ui.menu_button("Commands", |ui| {
                    let mut item = |ui: &mut egui::Ui, label: &str, d: &mut dyn FnMut() -> Box<dyn Dialog>| {
                        if ui.button(label).clicked() {
                            open.push(d());
                            ui.close_kind(egui::UiKind::Menu);
                        }
                    };
                    item(ui, "Commit…  (Ctrl+Space)", &mut || Box::new(commit::CommitDialog::default()));
                    item(ui, "Pull/Fetch…  (Ctrl+Down)", &mut || Box::new(pull::PullDialog::new(&data, None)));
                    item(ui, "Push…  (Ctrl+Up)", &mut || Box::new(push::PushDialog::new(&data, &self.settings, None)));
                    item(ui, "Manage stashes…", &mut || Box::new(stash::StashDialog::default()));
                    item(ui, "Reset changes…", &mut || Box::new(reset::ResetChangesDialog::all()));
                    item(ui, "Clean working directory…", &mut || Box::new(cleanup::CleanupDialog::default()));
                    ui.separator();
                    item(ui, "Create branch…  (Ctrl+B)", &mut || Box::new(branch::CreateBranchDialog::new(head)));
                    item(ui, "Checkout branch…  (Ctrl+.)", &mut || Box::new(checkout::CheckoutBranchDialog::new(&data, None, false)));
                    item(ui, "Merge branches…  (Ctrl+M)", &mut || Box::new(merge::MergeDialog::new(&data, "")));
                    item(ui, "Rebase…  (Ctrl+Shift+E)", &mut || Box::new(rebase::RebaseDialog::new(&data, "")));
                    item(ui, "Delete branch…", &mut || Box::new(branch::DeleteBranchDialog::new(&data, None)));
                    ui.separator();
                    item(ui, "Create tag…  (Ctrl+T)", &mut || Box::new(tag::CreateTagDialog::new(head)));
                    item(ui, "Delete tag…", &mut || Box::new(tag::DeleteTagDialog::new(&data, None)));
                    ui.separator();
                    item(ui, "Checkout revision…", &mut || Box::new(checkout::CheckoutRevisionDialog::new(head)));
                    item(ui, "Cherry pick…", &mut || Box::new(cherry_pick::CherryPickDialog::new(None, false)));
                    item(ui, "Archive revision…", &mut || Box::new(archive::ArchiveDialog::new(head)));
                    item(ui, "Format patch…", &mut || Box::new(patch::FormatPatchDialog::new(&[], head)));
                    item(ui, "Apply patch…", &mut || Box::new(patch::ApplyPatchDialog::new(None)));
                    item(ui, "View patch file…", &mut || Box::new(patch::ViewPatchDialog::default()));
                    ui.separator();
                    item(ui, "Compare branches (diff)…", &mut || Box::new(compare::CompareDialog::new(&data)));
                    item(ui, "Bisect…", &mut || Box::new(bisect::BisectDialog));
                    item(ui, "Solve merge conflicts…", &mut || Box::new(conflicts::ConflictsDialog::default()));
                    item(ui, "View reflog…", &mut || Box::new(reflog::ReflogDialog::default()));
                    item(ui, "Go to commit…  (Ctrl+Shift+G)", &mut || Box::new(goto_commit::GoToCommitDialog::default()));
                });
            });
            ui.menu_button("View", |ui| {
                let mut changed = false;
                changed |= ui.checkbox(&mut self.settings.left_panel_visible, "Show left panel  (Ctrl+Alt+C)").changed();
                ui.separator();
                changed |= ui.checkbox(&mut self.settings.show_author_column, "Author column").changed();
                changed |= ui.checkbox(&mut self.settings.show_date_column, "Date column").changed();
                changed |= ui.checkbox(&mut self.settings.show_id_column, "Commit hash column").changed();
                changed |= ui.checkbox(&mut self.settings.show_relative_date, "Relative dates").changed();
                changed |= ui.checkbox(&mut self.settings.show_author_date, "Show author date (else commit date)").changed();
                changed |= ui.checkbox(&mut self.settings.highlight_author_commits, "Highlight my commits").changed();
                ui.separator();
                ui.menu_button("Graph", |ui| {
                    let mut reload = false;
                    reload |= ui.radio_value(&mut self.settings.graph_draw_style, GraphDrawStyle::Normal, "Color all branches").changed();
                    reload |= ui.radio_value(&mut self.settings.graph_draw_style, GraphDrawStyle::DrawNonRelativesGray, "Draw non relatives gray").changed();
                    reload |= ui.radio_value(&mut self.settings.graph_draw_style, GraphDrawStyle::HighlightSelected, "Highlight selected branch").changed();
                    ui.separator();
                    reload |= ui.checkbox(&mut self.settings.merge_graph_lanes_having_common_parent, "Merge lanes having a common parent").changed();
                    reload |= ui.checkbox(&mut self.settings.render_graph_with_diagonals, "Render with diagonals").changed();
                    reload |= ui.checkbox(&mut self.settings.straighten_graph_diagonals, "Straighten diagonals").changed();
                    if reload {
                        self.actions.push(Action::ReloadLog);
                        self.save_settings();
                    }
                });
                ui.menu_button("Theme", |ui| {
                    for (t, l) in [(Theme::System, "Follow system"), (Theme::Light, "Light"), (Theme::Dark, "Dark")] {
                        if ui.radio_value(&mut self.settings.theme, t, l).changed() {
                            self.theme_applied = false;
                            self.save_settings();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Zoom");
                    if ui.button("−").clicked() {
                        self.settings.ui_scale = (self.settings.ui_scale - 0.1).max(0.5);
                        self.theme_applied = false;
                    }
                    ui.label(format!("{:.0}%", self.settings.ui_scale * 100.0));
                    if ui.button("+").clicked() {
                        self.settings.ui_scale = (self.settings.ui_scale + 0.1).min(3.0);
                        self.theme_applied = false;
                    }
                });
                if changed {
                    self.save_settings();
                }
            });
            ui.menu_button("Tools", |ui| {
                if ui.button("Git command log…").clicked() {
                    open.push(Box::new(command_log::CommandLogDialog));
                    ui.close_kind(egui::UiKind::Menu);
                }
                if ui.button("Settings…  (Ctrl+,)").clicked() {
                    open.push(Box::new(settings::SettingsDialog::default()));
                    ui.close_kind(egui::UiKind::Menu);
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("User manual").clicked() {
                    crate::util::open_with_system("https://git-extensions-documentation.readthedocs.io/");
                    ui.close_kind(egui::UiKind::Menu);
                }
                if ui.button("Command line help…").clicked() {
                    open.push(Box::new(about::CommandLineHelpDialog));
                    ui.close_kind(egui::UiKind::Menu);
                }
                if ui.button("About Git Extensions…").clicked() {
                    open.push(Box::new(about::AboutDialog));
                    ui.close_kind(egui::UiKind::Menu);
                }
            });
        });
        for d in open {
            self.open_dialog(d);
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match &self.browse {
            None => APPLICATION_NAME.to_string(),
            Some(b) => {
                let work_dir = b.module.work_dir();
                if self.title_description.as_ref().is_none_or(|(dir, _)| dir != work_dir) {
                    let desc = repository_description(work_dir, |p: &Path| GitModule::is_valid_git_working_dir(p));
                    self.title_description = Some((work_dir.to_path_buf(), desc));
                }
                let desc = self.title_description.as_ref().map(|(_, d)| d.clone()).unwrap_or_default();
                let path = (!b.grid.filter.path.is_empty()).then(|| b.grid.filter.path.clone());
                generate_title(Some(&desc), b.data.current_branch.as_deref(), "(no branch)", path.as_deref())
            }
        };
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }

    fn draw_dialogs(&mut self, ctx: &egui::Context) {
        let mut dialogs = std::mem::take(&mut self.dialogs);
        let mut keep = Vec::with_capacity(dialogs.len());
        let screen = ctx.screen_rect();
        let topmost = dialogs.len().saturating_sub(1);
        for (i, mut d) in dialogs.drain(..).enumerate() {
            let mut open = true;
            let mut still = true;
            let title = d.title();
            let window = egui::Window::new(RichText::new(&title).strong()).id(egui::Id::new(("dialog", d.id()))).collapsible(false).open(&mut open);
            let window = match d.kind() {
                DialogKind::Modal(w) => window.resizable(false).default_width(w).pivot(egui::Align2::CENTER_CENTER).default_pos(screen.center()),
                DialogKind::Window(size) => {
                    let size = egui::vec2(size.x.min(screen.width() - 40.0), size.y.min(screen.height() - 60.0));
                    window.resizable(true).default_size(size).pivot(egui::Align2::CENTER_CENTER).default_pos(screen.center())
                }
            };
            let (module, data, selected) = match &self.browse {
                Some(b) => (Some(&b.module), Some(&b.data), b.grid.selected.as_slice()),
                None => (None, None, &[][..]),
            };
            let mut cx = Cx { ctx, module, data, settings: &mut self.settings, history: &mut self.history, actions: &mut self.actions, selected };
            window.show(ctx, |ui| {
                if i == topmost {
                    ui.ctx().move_to_top(ui.layer_id());
                }
                still = d.ui(ui, &mut cx);
            });
            if open && still {
                keep.push(d);
            }
        }
        // dialogs opened while drawing were pushed into self.dialogs via actions
        keep.append(&mut self.dialogs);
        self.dialogs = keep;
    }
}

impl eframe::App for GitExtApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        crate::prof::start_frame(frame.info().cpu_usage);
        let _frame_timer = crate::prof::FrameTimer::start();
        if self.startup_window.as_mut().is_some_and(|w| w.on_frame(ctx)) {
            self.startup_window = None;
        }
        if !self.theme_applied {
            crate::theme::apply(ctx, self.settings.theme, self.settings.ui_scale, self.settings.font_size, self.settings.monospace_font_size);
            self.theme_applied = true;
        }
        if self.start.is_some() && (self.browse.as_ref().is_none_or(|b| !b.data.refs.is_empty() || !b.is_loading())) {
            self.run_start_command();
        }
        self.poll_tool_runs();
        crate::prof::scope("shortcuts", || self.shortcuts(ctx));
        crate::prof::scope("title", || self.update_title(ctx));
        // the keyboard belongs to the topmost dialog: the main window does not see the keys
        let held_keys: Vec<egui::Event> = if self.dialogs.is_empty() {
            Vec::new()
        } else {
            ctx.input_mut(|i| {
                let (keys, rest) = std::mem::take(&mut i.events)
                    .into_iter()
                    .partition(|e| matches!(e, egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_)));
                i.events = rest;
                keys
            })
        };

        egui::TopBottomPanel::top("menu").show(ctx, |ui| self.menu_bar(ui));
        if let Some(browse) = &mut self.browse {
            egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
                ui.add_space(2.0);
                crate::prof::scope("toolbar", || browse.toolbar(ui, &mut self.settings, &mut self.actions));
                ui.add_space(2.0);
            });
            egui::TopBottomPanel::bottom("statusbar").show(ctx, |ui| crate::prof::scope("statusbar", || browse.status_bar(ui, &mut self.actions)));
            egui::CentralPanel::default().show(ctx, |ui| {
                browse.ui(ctx, ui, &mut self.settings, &mut self.actions);
            });
        } else {
            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some(cmd) = self.dashboard.ui(ui, &self.history, self.settings.sort_recent_repos_alphabetically) {
                    match cmd {
                        DashboardCommand::Open(p) => self.actions.push(Action::OpenRepo(p)),
                        DashboardCommand::OpenDialog => self.open_folder_dialog(),
                        DashboardCommand::Clone => self.open_dialog(Box::new(dialogs::clone::CloneDialog::new(None, &self.settings))),
                        DashboardCommand::Init => self.open_dialog(Box::new(dialogs::init::InitDialog::new(None))),
                        DashboardCommand::RemoveRecent(p) => {
                            self.history.remove_recent(&p);
                            self.save_history();
                        }
                        DashboardCommand::Pin(p, pin) => {
                            self.history.set_anchor(&p, if pin { RepositoryAnchor::AnchoredInTop } else { RepositoryAnchor::None });
                            self.save_history();
                        }
                        DashboardCommand::SetCategory(p, c) => {
                            self.history.assign_category(&p, c.as_deref());
                            self.save_history();
                        }
                        DashboardCommand::ShowInFolder(p) => crate::util::open_with_system(&p),
                        DashboardCommand::Settings => self.open_dialog(Box::new(dialogs::settings::SettingsDialog::default())),
                    }
                }
            });
        }

        ctx.input_mut(|i| i.events.extend(held_keys));
        self.draw_dialogs(ctx);
        self.process_actions(ctx);

        if self.exit_after_dialog && self.start.is_none() && self.dialogs.is_empty() {
            // started for a single command (e.g. `gitext commit`): exit when done
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(b) = &mut self.browse {
            b.grid.cancel();
        }
        self.save_settings();
        self.save_history();
    }
}


/// The `safe.directory` value git suggests in a "detected dubious ownership" error, if any
/// (`BugReportInvoker.DubiousOwnershipSecurityConfigString`).
fn dubious_ownership_safe_directory(error: &str) -> Option<String> {
    if !error.contains("dubious ownership") {
        return None;
    }
    let line = error.lines().find(|l| l.contains("--add safe.directory"))?;
    let value = line.split("safe.directory").nth(1)?.trim();
    let value = value.trim_matches(|c| c == '\'' || c == '"');
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_directory_from_dubious_ownership_error() {
        let err = "git rev-parse failed with exit code 128:\n\
            fatal: detected dubious ownership in repository at '//wsl.localhost/Ubuntu/home/jack/repo'\n\
            '//wsl.localhost/Ubuntu/home/jack/repo' is owned by:\n\
            \t'S-1-5-21-1'\n\
            but the current user is:\n\
            \t'S-1-5-21-2'\n\
            To add an exception for this directory, call:\n\
            \n\
            \tgit config --global --add safe.directory '%(prefix)///wsl.localhost/Ubuntu/home/jack/repo'";
        assert_eq!(dubious_ownership_safe_directory(err).as_deref(), Some("%(prefix)///wsl.localhost/Ubuntu/home/jack/repo"));
        assert_eq!(dubious_ownership_safe_directory("fatal: not a git repository"), None);
    }
}
