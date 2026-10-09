//! Port of `FormBrowse`: the main window with toolbar, repository objects tree, revision
//! grid and the commit / diff / file tree tabs.

use std::time::{Duration, Instant};

use egui::{RichText, Ui};
use gitext_core::commands::{self, LocalChangesAction, UntrackedFilesMode};
use gitext_core::settings::{AppSettings, BranchFilterMode, PullAction};
use gitext_core::status::GitItemStatus;
use gitext_core::{GitArgs, GitModule, ObjectId};

use crate::dialogs::{self, Action, GitRun};
use crate::repo::RepoData;
use crate::tasks::Task;
use crate::theme::Palette;
use crate::views::commit_info::{CommitInfo, CommitInfoLink};
use crate::views::file_tree_view::FileTreeView;
use crate::views::left_panel::{LeftCommand, LeftPanel};
use crate::views::revision_diff::{DiffCommand, RevisionDiffView};
use crate::views::revision_grid::{GridCommand, RevisionGrid, TextFilterKind};

pub struct BrowseView {
    pub module: GitModule,
    pub data: RepoData,
    data_task: Option<Task<RepoData>>,
    reload_log_after_data: bool,
    first_load: bool,
    pub grid: RevisionGrid,
    left: LeftPanel,
    pub tab: usize,
    commit_info: CommitInfo,
    diff: RevisionDiffView,
    pub file_tree: FileTreeView,
    /// When the last status refresh completed (or a full refresh was started).
    last_status_refresh: Instant,
    /// Background status check (port of `GitStatusMonitor`); does not show the busy spinner.
    status_task: Option<Task<(ObjectId, Vec<GitItemStatus>)>>,
    /// How long the last status check took, to space checks out in slow repositories.
    status_duration: Duration,
    filter_input: String,
    filter_kind: TextFilterKind,
}

impl BrowseView {
    pub fn new(ctx: &egui::Context, module: GitModule, settings: &AppSettings) -> Self {
        let mut v = BrowseView {
            module,
            data: RepoData::default(),
            data_task: None,
            reload_log_after_data: true,
            first_load: true,
            grid: RevisionGrid::default(),
            left: LeftPanel::default(),
            tab: settings.last_browse_tab.min(2),
            commit_info: CommitInfo::default(),
            diff: RevisionDiffView::default(),
            file_tree: FileTreeView::default(),
            last_status_refresh: Instant::now(),
            status_task: None,
            status_duration: Duration::ZERO,
            filter_input: String::new(),
            filter_kind: TextFilterKind::Message,
        };
        v.refresh(ctx, settings, true);
        v
    }

    /// Reloads repository data (and the log when `reload_log`).
    pub fn refresh(&mut self, ctx: &egui::Context, settings: &AppSettings, reload_log: bool) {
        let m = self.module.clone();
        let untracked = settings.show_untracked_files;
        self.data_task = Some(Task::spawn(ctx, move || RepoData::load(&m, untracked)));
        self.reload_log_after_data |= reload_log;
        self.last_status_refresh = Instant::now();
        self.commit_info.invalidate();
        self.diff.invalidate();
        self.file_tree.invalidate();
    }

    pub fn reload_log(&mut self, ctx: &egui::Context, settings: &AppSettings) {
        self.grid.reload(ctx, &self.module, &self.data, settings);
        self.diff.invalidate();
    }

    pub fn is_loading(&self) -> bool {
        self.data_task.is_some() || self.grid.loading
    }

    fn poll(&mut self, ctx: &egui::Context, settings: &AppSettings) {
        if let Some(t) = &mut self.data_task {
            if let Some(d) = t.try_take() {
                let status_changed = d.status != self.data.status;
                self.data = d;
                self.data_task = None;
                if self.reload_log_after_data {
                    self.reload_log_after_data = false;
                    if self.first_load {
                        self.first_load = false;
                    }
                    self.grid.reload(ctx, &self.module, &self.data, settings);
                } else if status_changed {
                    self.grid.update_status_counts(&self.data);
                    if self.grid.selected.iter().any(|s| s.is_artificial()) {
                        self.diff.invalidate();
                    }
                }
            }
        }
        // Periodic status refresh (port of `GitStatusMonitor`): only `git status` and HEAD, in
        // the background. The interval grows with the duration of the check so slow
        // repositories (or platforms with slow process creation) are not kept busy.
        if let Some(t) = &mut self.status_task {
            if let Some((head, status)) = t.try_take() {
                self.status_task = None;
                self.status_duration = self.last_status_refresh.elapsed();
                self.last_status_refresh = Instant::now();
                if self.data_task.is_none() {
                    if head != self.data.head {
                        // committed, checked out or reset outside of the app
                        self.refresh(ctx, settings, true);
                    } else if status != self.data.status {
                        self.data.status = status;
                        self.grid.update_status_counts(&self.data);
                        if self.grid.selected.iter().any(|s| s.is_artificial()) {
                            self.diff.invalidate();
                        }
                    }
                }
            }
        }
        let interval = Duration::from_secs(5).max(self.status_duration * 4);
        let focused = ctx.input(|i| i.focused);
        if focused && self.data_task.is_none() && self.status_task.is_none() && !self.data.is_bare && self.last_status_refresh.elapsed() > interval {
            let m = self.module.clone();
            let mode = if settings.show_untracked_files { UntrackedFilesMode::All } else { UntrackedFilesMode::No };
            self.last_status_refresh = Instant::now();
            self.status_task = Some(Task::spawn(ctx, move || (m.head_id(), m.get_status(mode, true).unwrap_or_default())));
        }
        ctx.request_repaint_after(Duration::from_secs(5));
    }

    fn selected_parents(&self) -> Vec<ObjectId> {
        self.grid.selected_revision().and_then(|id| self.grid.revision(&id)).map(|r| r.parents().to_vec()).unwrap_or_default()
    }

    /// Toolbar (`ToolStripMain`) and filter bar (`FilterToolBar`).
    pub fn toolbar(&mut self, ui: &mut Ui, settings: &mut AppSettings, actions: &mut Vec<Action>) {
        let palette = Palette::for_ui(ui);
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("📂 {}", self.module.name())).strong()).on_hover_text(self.module.work_dir().display().to_string());
            let branch = self.data.current_branch.clone().unwrap_or_else(|| "(no branch)".into());
            let br = ui.button(format!("🔀 {branch}")).on_hover_text("Checkout branch");
            if br.clicked() {
                actions.push(Action::OpenDialog(Box::new(dialogs::checkout::CheckoutBranchDialog::new(&self.data, None, false))));
            }
            ui.separator();
            if ui.button("⟳").on_hover_text("Refresh (F5)").clicked() {
                actions.push(Action::Refresh);
            }
            let changes = self.data.work_tree_changes() + self.data.index_changes();
            let commit_text = if changes > 0 { format!("✔ Commit ({changes})") } else { "✔ Commit".to_string() };
            let mut commit = egui::Button::new(commit_text);
            if changes > 0 {
                commit = commit.fill(palette.status_added.gamma_multiply(0.25));
            }
            if ui.add(commit).on_hover_text("Commit changes (Ctrl+Space)").clicked() {
                actions.push(Action::OpenCommit);
            }
            let (ahead, behind) = self.data.current_ref().and_then(|r| self.data.ahead_behind.get(&r.name)).map(|ab| (ab.ahead, ab.behind)).unwrap_or((0, 0));
            let pull_label = if behind > 0 { format!("⬇ Pull ({behind})") } else { "⬇ Pull".into() };
            ui.menu_button(pull_label, |ui| {
                let mut item = |ui: &mut Ui, label: &str, a: PullAction| {
                    if ui.button(label).clicked() {
                        actions.push(Action::OpenDialog(Box::new(dialogs::pull::PullDialog::new(&self.data, Some(a)))));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                };
                item(ui, "Pull - merge", PullAction::Merge);
                item(ui, "Pull - rebase", PullAction::Rebase);
                item(ui, "Fetch", PullAction::Fetch);
                item(ui, "Fetch all", PullAction::FetchAll);
                item(ui, "Fetch and prune all", PullAction::FetchPruneAll);
                ui.separator();
                if ui.button("Open pull dialog…").clicked() {
                    actions.push(Action::OpenDialog(Box::new(dialogs::pull::PullDialog::new(&self.data, None))));
                    ui.close_kind(egui::UiKind::Menu);
                }
            });
            let push_label = if ahead > 0 { format!("⬆ Push ({ahead})") } else { "⬆ Push".into() };
            if ui.button(push_label).on_hover_text("Push").clicked() {
                actions.push(Action::OpenDialog(Box::new(dialogs::push::PushDialog::new(&self.data, settings, None))));
            }
            ui.menu_button("☰ Stash", |ui| {
                if ui.button("Stash changes").clicked() {
                    actions.push(Action::RunGit(GitRun::new("Stash", commands::stash_save(settings.show_untracked_files, false, None, &[]))));
                    ui.close_kind(egui::UiKind::Menu);
                }
                if ui.button("Stash staged changes").clicked() {
                    actions.push(Action::RunGit(GitRun::new("Stash staged", GitArgs::new("stash").arg("push").arg("--staged"))));
                    ui.close_kind(egui::UiKind::Menu);
                }
                if ui.add_enabled(!self.data.stashes.is_empty(), egui::Button::new("Stash pop")).clicked() {
                    actions.push(Action::RunGit(GitRun::new("Stash pop", GitArgs::new("stash").arg("pop")).conflicts()));
                    ui.close_kind(egui::UiKind::Menu);
                }
                ui.separator();
                if ui.button("Manage stashes…").clicked() {
                    actions.push(Action::OpenDialog(Box::new(dialogs::stash::StashDialog::default())));
                    ui.close_kind(egui::UiKind::Menu);
                }
                if ui.button("Create a stash with message…").clicked() {
                    actions.push(Action::OpenDialog(Box::new(dialogs::stash::CreateStashDialog::default())));
                    ui.close_kind(egui::UiKind::Menu);
                }
            });
            ui.separator();
            if ui.button("🗁").on_hover_text("Open working directory in file manager").clicked() {
                crate::util::open_with_system(&self.module.work_dir().display().to_string());
            }
            if ui.button(">_").on_hover_text("Open terminal").clicked() {
                crate::util::open_terminal(self.module.work_dir(), &settings.terminal);
            }
            if ui.button("⚙").on_hover_text("Settings").clicked() {
                actions.push(Action::OpenDialog(Box::new(dialogs::settings::SettingsDialog::default())));
            }
            if self.is_loading() {
                ui.spinner();
            }
        });

        // Filter toolbar
        ui.horizontal(|ui| {
            ui.label("Branches:");
            let before = (settings.branch_filter_mode, settings.branch_filter.clone());
            egui::ComboBox::from_id_salt("branch_filter_mode")
                .selected_text(match settings.branch_filter_mode {
                    BranchFilterMode::All => "All branches",
                    BranchFilterMode::Current => "Current branch",
                    BranchFilterMode::Specific => "Filtered",
                })
                .width(120.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut settings.branch_filter_mode, BranchFilterMode::All, "All branches");
                    ui.selectable_value(&mut settings.branch_filter_mode, BranchFilterMode::Current, "Current branch");
                    ui.selectable_value(&mut settings.branch_filter_mode, BranchFilterMode::Specific, "Filtered");
                });
            if settings.branch_filter_mode == BranchFilterMode::Specific {
                let r = ui.add(egui::TextEdit::singleline(&mut settings.branch_filter).hint_text("branch patterns").desired_width(150.0));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    actions.push(Action::ReloadLog);
                }
            }
            if (settings.branch_filter_mode, settings.branch_filter.clone()) != before && settings.branch_filter_mode != BranchFilterMode::Specific {
                actions.push(Action::ReloadLog);
                actions.push(Action::SaveSettings);
            }
            ui.separator();
            egui::ComboBox::from_id_salt("filter_kind").selected_text(self.filter_kind.label()).width(130.0).show_ui(ui, |ui| {
                for k in TextFilterKind::ALL {
                    ui.selectable_value(&mut self.filter_kind, k, k.label());
                }
            });
            let r = ui.add(egui::TextEdit::singleline(&mut self.filter_input).hint_text("Filter revisions (Enter)").desired_width(220.0));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.apply_text_filter(actions);
            }
            if ui.button("🔍").on_hover_text("Apply filter").clicked() {
                self.apply_text_filter(actions);
            }
            if self.grid.filter.is_active() && ui.button(RichText::new("✖ Clear filter").color(palette.warning)).clicked() {
                self.grid.filter = Default::default();
                self.filter_input.clear();
                actions.push(Action::ReloadLog);
            }
            ui.separator();
            let mut toggle = |ui: &mut Ui, v: &mut bool, label: &str, hint: &str| {
                if ui.selectable_label(*v, label).on_hover_text(hint).clicked() {
                    *v = !*v;
                    actions.push(Action::ReloadLog);
                    actions.push(Action::SaveSettings);
                }
            };
            toggle(ui, &mut settings.show_first_parent, "1st parent", "Show first parents only");
            toggle(ui, &mut settings.show_remote_branches, "☁ Remotes", "Show remote branches");
            toggle(ui, &mut settings.show_tags, "🏷 Tags", "Show tags");
            toggle(ui, &mut settings.show_stashes, "☰ Stashes", "Show stashes");
            toggle(ui, &mut settings.show_reflog_references, "↺ Reflog", "Show reflog references");
            toggle(ui, &mut settings.show_artificial_commits, "📝 Changes", "Show working directory and index as commits");
        });
    }

    fn apply_text_filter(&mut self, actions: &mut Vec<Action>) {
        if self.filter_kind == TextFilterKind::Hash {
            let id = self.module.rev_parse(self.filter_input.trim());
            if id.is_zero() {
                actions.push(Action::Message { title: "Filter".into(), text: format!("Revision '{}' not found", self.filter_input), error: true });
            } else {
                self.grid.select(id);
            }
            return;
        }
        self.grid.filter.text = self.filter_input.clone();
        self.grid.filter.kind = self.filter_kind;
        actions.push(Action::ReloadLog);
    }

    pub fn set_path_filter(&mut self, path: String) {
        self.grid.filter.path = path;
        self.grid.filter.follow = true;
    }

    /// Status bar with the repository state (merge/rebase in progress etc.).
    pub fn status_bar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let palette = Palette::for_ui(ui);
        ui.horizontal(|ui| {
            if let Some(desc) = self.data.state.description() {
                ui.label(RichText::new(format!("⚠ {desc}")).color(palette.warning).strong());
                if self.data.has_conflicts() {
                    if ui.button("Solve conflicts…").clicked() {
                        actions.push(Action::OpenDialog(Box::new(dialogs::conflicts::ConflictsDialog::default())));
                    }
                }
                let st = self.data.state;
                if st.rebasing || st.applying_patch {
                    if ui.button("Continue").clicked() {
                        actions.push(Action::RunGit(GitRun::new("Continue", if st.applying_patch { commands::resolved_mailbox() } else { commands::continue_rebase() }).conflicts()));
                    }
                    if ui.button("Skip").clicked() {
                        actions.push(Action::RunGit(GitRun::new("Skip", if st.applying_patch { commands::skip_mailbox() } else { commands::skip_rebase() }).conflicts()));
                    }
                    if ui.button("Abort").clicked() {
                        actions.push(Action::RunGit(GitRun::new("Abort", if st.applying_patch { commands::abort_mailbox() } else { commands::abort_rebase() })));
                    }
                } else if st.merging {
                    if ui.button("Commit merge…").clicked() {
                        actions.push(Action::OpenCommit);
                    }
                    if ui.button("Abort merge").clicked() {
                        actions.push(Action::RunGit(GitRun::new("Abort merge", commands::abort_merge())));
                    }
                } else if st.cherry_picking {
                    if ui.button("Continue").clicked() {
                        actions.push(Action::RunGit(GitRun::new("Continue cherry-pick", GitArgs::new("cherry-pick").arg("--continue")).conflicts()));
                    }
                    if ui.button("Abort").clicked() {
                        actions.push(Action::RunGit(GitRun::new("Abort cherry-pick", GitArgs::new("cherry-pick").arg("--abort"))));
                    }
                } else if st.reverting {
                    if ui.button("Continue").clicked() {
                        actions.push(Action::RunGit(GitRun::new("Continue revert", GitArgs::new("revert").arg("--continue")).conflicts()));
                    }
                    if ui.button("Abort").clicked() {
                        actions.push(Action::RunGit(GitRun::new("Abort revert", GitArgs::new("revert").arg("--abort"))));
                    }
                } else if st.bisecting && ui.button("Bisect…").clicked() {
                    actions.push(Action::OpenDialog(Box::new(dialogs::bisect::BisectDialog)));
                }
                ui.separator();
            }
            let branch = self.data.current_branch.clone().unwrap_or_else(|| "(no branch)".into());
            ui.label(RichText::new(format!("🔀 {branch}")).color(palette.muted));
            if let Some(r) = self.data.current_ref() {
                if !r.merge_with.is_empty() {
                    ui.label(RichText::new(format!("tracking {}/{}", r.tracking_remote, r.merge_with)).color(palette.muted));
                }
            }
            ui.separator();
            let count = self.grid.graph.count();
            let mut s = format!("{count} commits");
            if self.grid.loading {
                s.push_str(" (loading…)");
            } else if let Some(ms) = self.grid.load_duration_ms {
                s.push_str(&format!(" in {ms} ms"));
            }
            ui.label(RichText::new(s).color(palette.muted));
            if self.grid.filter.is_active() {
                ui.label(RichText::new("(filtered)").color(palette.warning));
            }
            if let Some(e) = &self.data.error {
                ui.label(RichText::new(e).color(palette.error));
            }
        });
    }

    pub fn ui(&mut self, ctx: &egui::Context, ui: &mut Ui, settings: &mut AppSettings, actions: &mut Vec<Action>) {
        crate::prof::scope("poll", || self.poll(ctx, settings));
        let selected = self.grid.selected_revision();
        if settings.left_panel_visible {
            egui::SidePanel::left("left_panel").resizable(true).default_width(240.0).width_range(150.0..=500.0).show_inside(ui, |ui| {
                if let Some(c) = crate::prof::scope("left", || self.left.ui(ui, &self.data, selected)) {
                    self.handle_left(c, settings, actions);
                }
            });
        }

        egui::TopBottomPanel::bottom("browse_tabs").resizable(true).default_height(380.0).min_height(120.0).show_inside(ui, |ui| {
            ui.set_min_height(ui.available_height());
            ui.horizontal(|ui| {
                for (i, label) in ["📄 Commit", "± Diff", "🌲 File tree"].iter().enumerate() {
                    if ui.selectable_label(self.tab == i, *label).clicked() {
                        self.tab = i;
                        settings.last_browse_tab = i;
                    }
                }
            });
            ui.separator();
            match self.tab {
                0 => {
                    let rev = selected.and_then(|id| self.grid.revision(&id)).cloned();
                    let children = selected
                        .and_then(|id| self.grid.graph.try_get_node(&id))
                        .map(|n| self.grid.graph.store.nodes[n].children.iter().map(|&c| self.grid.graph.store.nodes[c].object_id).collect())
                        .unwrap_or_default();
                    if let Some(CommitInfoLink::Select(id)) = crate::prof::scope("commit_info", || self.commit_info.ui(ui, &self.module, rev.as_ref(), children)) {
                        self.grid.select(id);
                    }
                }
                1 => {
                    let (first, second) = self.diff_revisions();
                    let parents = if first.is_none() { self.selected_parents() } else { vec![] };
                    if let Some(c) = self.diff.ui(ui, &self.module, first, second, &parents, settings, "browse_diff") {
                        self.handle_diff(c, settings, actions);
                    }
                    // preloading the next commits costs git processes (slow on Windows): only
                    // when moving through the commits with the keyboard, not on clicks
                    if self.grid.selected.len() == 1 && self.grid.moved_by_keyboard {
                        let neighbors = self.grid.neighbor_revisions();
                        self.diff.prefetch(ctx, &self.module, &neighbors, settings);
                    }
                }
                _ => {
                    // the work tree / index are shown at HEAD
                    let tree_rev = selected.map(|id| if id.is_artificial() { self.data.head } else { id });
                    if let Some(c) = self.file_tree.ui(ui, &self.module, tree_rev, settings.show_line_numbers) {
                        self.handle_diff(c, settings, actions);
                    }
                }
            }
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            let events = crate::prof::scope("grid", || self.grid.ui(ui, settings, &self.data));
            if events.selection_changed {
                if let Some(id) = self.grid.selected_revision() {
                    gitext_core::exec::log_event(format!("selected {}", id.to_short_string()));
                    crate::prof::trace_frames();
                }
                self.diff.list.clear();
                // the tabs are drawn before the grid: draw again now to load the new selection
                ctx.request_repaint();
            }
            if let Some(id) = events.double_clicked {
                if id.is_artificial() {
                    actions.push(Action::OpenCommit);
                } else {
                    self.tab = 1;
                }
            }
            if let Some(c) = events.context_action {
                self.handle_grid(c, settings, actions);
            }
        });
    }

    /// (first, second) revisions for the diff tab from the grid selection.
    pub fn diff_revisions(&self) -> (Option<ObjectId>, Option<ObjectId>) {
        match self.grid.selected.as_slice() {
            [] => (None, None),
            [one] => {
                if *one == ObjectId::WORK_TREE {
                    (Some(ObjectId::INDEX), Some(*one))
                } else if *one == ObjectId::INDEX {
                    (Some(self.data.head), Some(*one))
                } else {
                    (None, Some(*one))
                }
            }
            [first, .., last] => (Some(*first), Some(*last)),
        }
    }

    pub fn handle_left(&mut self, c: LeftCommand, settings: &mut AppSettings, actions: &mut Vec<Action>) {
        use dialogs::*;
        let open = |actions: &mut Vec<Action>, d: Box<dyn Dialog>| actions.push(Action::OpenDialog(d));
        match c {
            LeftCommand::Select(id) => self.grid.select(id),
            LeftCommand::Checkout(name) => {
                if self.data.status.iter().any(|s| s.is_tracked) && settings.check_for_uncommitted_changes_in_checkout {
                    open(actions, Box::new(checkout::CheckoutBranchDialog::new(&self.data, Some(name), false)));
                } else {
                    actions.push(Action::RunGit(GitRun::new(format!("Checkout {name}"), commands::checkout(&name, LocalChangesAction::DontChange))));
                }
            }
            LeftCommand::CheckoutRemote(name) => open(actions, Box::new(checkout::CheckoutBranchDialog::new(&self.data, Some(name), true))),
            LeftCommand::Merge(name) => open(actions, Box::new(merge::MergeDialog::new(&self.data, &name))),
            LeftCommand::Rebase(name) => open(actions, Box::new(rebase::RebaseDialog::new(&self.data, &name))),
            LeftCommand::CreateBranch(id) => open(actions, Box::new(branch::CreateBranchDialog::new(id))),
            LeftCommand::Rename(name) => open(actions, Box::new(branch::RenameBranchDialog::new(&name))),
            LeftCommand::Delete(full) => open(actions, Box::new(branch::DeleteBranchDialog::new(&self.data, Some(full)))),
            LeftCommand::DeleteRemoteBranch(name) => open(actions, Box::new(branch::DeleteRemoteBranchDialog::new(&self.data, &name))),
            LeftCommand::FilterBranch(full) => {
                settings.branch_filter_mode = BranchFilterMode::Specific;
                settings.branch_filter = full;
                actions.push(Action::ReloadLog);
            }
            LeftCommand::Push(name) => open(actions, Box::new(push::PushDialog::new(&self.data, settings, Some(name)))),
            LeftCommand::Pull => open(actions, Box::new(pull::PullDialog::new(&self.data, None))),
            LeftCommand::FetchRemote(remote) => {
                actions.push(Action::RunGit(GitRun::new(format!("Fetch {remote}"), self.module.fetch_args(&remote, "", "", None, settings.prune_on_fetch, false))));
            }
            LeftCommand::PruneRemote(remote) => actions.push(Action::RunGit(GitRun::new(format!("Prune {remote}"), GitArgs::new("remote").arg("prune").arg(remote)))),
            LeftCommand::ManageRemotes => open(actions, Box::new(remotes::RemotesDialog::default())),
            LeftCommand::DeleteTag(name) => open(actions, Box::new(tag::DeleteTagDialog::new(&self.data, Some(name)))),
            LeftCommand::PushTag(name) => open(actions, Box::new(push::PushDialog::tag(&self.data, settings, &name))),
            LeftCommand::ApplyStash(s) => actions.push(Action::RunGit(GitRun::new(format!("Apply {s}"), GitArgs::new("stash").arg("apply").arg(s)).conflicts())),
            LeftCommand::PopStash(s) => actions.push(Action::RunGit(GitRun::new(format!("Pop {s}"), GitArgs::new("stash").arg("pop").arg(s)).conflicts())),
            LeftCommand::DropStash(s) => {
                let s2 = s.clone();
                open(actions, Box::new(Confirm::new("Drop stash", format!("Drop {s}? This cannot be undone."), "Drop", move |cx| cx.run(GitRun::new("Drop stash", GitArgs::new("stash").arg("drop").arg(s2))))));
            }
            LeftCommand::OpenSubmodule(path) => actions.push(Action::OpenRepo(self.module.work_dir().join(path))),
            LeftCommand::UpdateSubmodule(path) => actions.push(Action::RunGit(GitRun::new(format!("Update {path}"), commands::submodule_update(&[&path], false)))),
            LeftCommand::SyncSubmodule(path) => actions.push(Action::RunGit(GitRun::new(format!("Sync {path}"), commands::submodule_sync(Some(&path))))),
            LeftCommand::ManageSubmodules => open(actions, Box::new(submodules::SubmodulesDialog::default())),
            LeftCommand::OpenWorktree(path) => actions.push(Action::OpenRepo(path.into())),
            LeftCommand::RemoveWorktree(path) => {
                let p = path.clone();
                open(actions, Box::new(Confirm::new("Remove worktree", format!("Remove worktree '{path}'?"), "Remove", move |cx| cx.run(GitRun::new("Remove worktree", GitArgs::new("worktree").arg("remove").arg(p))))));
            }
            LeftCommand::ManageWorktrees => open(actions, Box::new(worktrees::WorktreesDialog::default())),
            LeftCommand::SetUpstream(name) => open(actions, Box::new(branch::SetUpstreamDialog::new(&self.data, &name))),
            LeftCommand::CreateStash => open(actions, Box::new(stash::CreateStashDialog::default())),
        }
    }

    pub fn handle_grid(&mut self, c: GridCommand, settings: &mut AppSettings, actions: &mut Vec<Action>) {
        use dialogs::*;
        let open = |actions: &mut Vec<Action>, d: Box<dyn Dialog>| actions.push(Action::OpenDialog(d));
        match c {
            GridCommand::Checkout(id) => open(actions, Box::new(checkout::CheckoutRevisionDialog::new(id))),
            GridCommand::CheckoutBranch(name) => {
                let remote = self.data.remote_branches().any(|r| r.name == name);
                open(actions, Box::new(checkout::CheckoutBranchDialog::new(&self.data, Some(name), remote)));
            }
            GridCommand::CreateBranch(id) => open(actions, Box::new(branch::CreateBranchDialog::new(id))),
            GridCommand::CreateTag(id) => open(actions, Box::new(tag::CreateTagDialog::new(id))),
            GridCommand::ResetCurrentBranchHere(id) => open(actions, Box::new(reset::ResetBranchDialog::new(&self.data, id))),
            GridCommand::RebaseOnto(id) => open(actions, Box::new(rebase::RebaseDialog::new(&self.data, &id.to_string()))),
            GridCommand::InteractiveRebase(id) => {
                let mut d = rebase::RebaseDialog::new(&self.data, &format!("{id}~1"));
                d.interactive = true;
                open(actions, Box::new(d));
            }
            GridCommand::MergeInto(id) => {
                let name = self.grid.revision(&id).and_then(|r| r.refs.iter().find(|r| r.is_head() || r.is_remote()).map(|r| r.name.clone())).unwrap_or_else(|| id.to_string());
                open(actions, Box::new(merge::MergeDialog::new(&self.data, &name)));
            }
            GridCommand::CherryPick(id) => {
                let rev = self.grid.revision(&id).cloned();
                open(actions, Box::new(cherry_pick::CherryPickDialog::new(rev, false)));
            }
            GridCommand::Revert(id) => {
                let rev = self.grid.revision(&id).cloned();
                open(actions, Box::new(cherry_pick::CherryPickDialog::new(rev, true)));
            }
            GridCommand::DeleteBranch(full) => open(actions, Box::new(branch::DeleteBranchDialog::new(&self.data, Some(full)))),
            GridCommand::DeleteTag(name) => open(actions, Box::new(tag::DeleteTagDialog::new(&self.data, Some(name)))),
            GridCommand::RenameBranch(name) => open(actions, Box::new(branch::RenameBranchDialog::new(&name))),
            GridCommand::PushBranch(name) => open(actions, Box::new(push::PushDialog::new(&self.data, settings, Some(name)))),
            GridCommand::Archive(id) => open(actions, Box::new(archive::ArchiveDialog::new(id))),
            GridCommand::FormatPatch(id) => open(actions, Box::new(patch::FormatPatchDialog::new(&self.grid.selected, id))),
            GridCommand::CompareSelected => {
                self.tab = 1;
            }
            GridCommand::CompareWithWorkTree(id) => {
                self.grid.selected = vec![id, ObjectId::WORK_TREE];
                self.tab = 1;
            }
            GridCommand::ShowInFileTree(id) => {
                self.grid.select(id);
                self.tab = 2;
            }
            GridCommand::CopyToClipboard(s) => actions.push(Action::Copy(s)),
            GridCommand::NavigateToParent => self.grid.navigate_parent(),
            GridCommand::NavigateToChild => self.grid.navigate_child(),
            GridCommand::GoToCommit => open(actions, Box::new(goto_commit::GoToCommitDialog::default())),
            GridCommand::BisectGood => actions.push(Action::RunGit(GitRun::new("Bisect good", commands::continue_bisect(commands::GitBisectOption::Good, &self.grid.selected)).keep_open())),
            GridCommand::BisectBad => actions.push(Action::RunGit(GitRun::new("Bisect bad", commands::continue_bisect(commands::GitBisectOption::Bad, &self.grid.selected)).keep_open())),
            GridCommand::BisectSkip => actions.push(Action::RunGit(GitRun::new("Bisect skip", commands::continue_bisect(commands::GitBisectOption::Skip, &self.grid.selected)).keep_open())),
            GridCommand::OpenCommitDialog => actions.push(Action::OpenCommit),
            GridCommand::ApplyStash(s) => self.handle_left(LeftCommand::ApplyStash(s), settings, actions),
            GridCommand::PopStash(s) => self.handle_left(LeftCommand::PopStash(s), settings, actions),
            GridCommand::DropStash(s) => self.handle_left(LeftCommand::DropStash(s), settings, actions),
            GridCommand::RunScript(i, id) => {
                if let Some(script) = settings.user_scripts.get(i).cloned() {
                    let args = script.arguments.replace("{sHash}", &id.to_string()).replace("{WorkingDir}", &self.module.work_dir().display().to_string());
                    if script.command == "git" || script.command.is_empty() {
                        actions.push(Action::RunGit(GitRun::new(script.name.clone(), GitArgs::empty().args(args.split_whitespace()))));
                    } else {
                        let _ = std::process::Command::new(&script.command).args(args.split_whitespace()).current_dir(self.module.work_dir()).spawn();
                    }
                }
            }
            GridCommand::FixupCommit(id, kind) => {
                actions.push(Action::RunGit(GitRun::new(format!("Create {kind} commit"), GitArgs::new("commit").arg(format!("--fixup={}{id}", if kind == "fixup" { String::new() } else { format!("{kind}:") })).arg("--no-edit"))));
            }
        }
    }

    pub fn handle_diff(&mut self, c: DiffCommand, settings: &mut AppSettings, actions: &mut Vec<Action>) {
        use dialogs::*;
        let m = &self.module;
        match c {
            DiffCommand::Blame { file, rev } => actions.push(Action::OpenDialog(Box::new(blame::BlameDialog::new(file, rev)))),
            DiffCommand::FileHistory(file) => actions.push(Action::OpenDialog(Box::new(file_history::FileHistoryDialog::new(file)))),
            DiffCommand::ShowInFileTree { rev, file } => actions.push(Action::ShowInFileTree(rev, Some(file))),
            DiffCommand::FilterPath(p) => actions.push(Action::FilterPath(p)),
            DiffCommand::OpenWorkFile(f) => crate::util::open_in_editor(&m.work_dir().join(f), &settings.editor),
            DiffCommand::OpenRevisionFile { rev, file } => {
                if let Ok(bytes) = m.get_file_bytes(rev, &file) {
                    let name = std::path::Path::new(&file).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let path = std::env::temp_dir().join(format!("{}_{name}", rev.to_short_string()));
                    if std::fs::write(&path, bytes).is_ok() {
                        crate::util::open_in_editor(&path, &settings.editor);
                    }
                }
            }
            DiffCommand::SaveAs { rev, file } => {
                let name = std::path::Path::new(&file).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if let Some(dest) = crate::util::save_file(None, &name) {
                    match m.get_file_bytes(rev, &file) {
                        Ok(b) => {
                            if let Err(e) = std::fs::write(dest, b) {
                                actions.push(Action::Message { title: "Save as".into(), text: e.to_string(), error: true });
                            }
                        }
                        Err(e) => actions.push(Action::Message { title: "Save as".into(), text: e.to_string(), error: true }),
                    }
                }
            }
            DiffCommand::ResetFileTo { rev, files } => {
                let list = files.join("\n");
                actions.push(Action::OpenDialog(Box::new(Confirm::new("Reset files", format!("Reset these files to {}?\n\n{list}", rev.to_short_string()), "Reset", move |cx| {
                    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
                    cx.run(GitRun::new("Reset files", GitArgs::new("checkout").arg(rev.to_string()).arg("--").args(refs)));
                }))));
            }
            DiffCommand::Stage(files) => {
                let refs: Vec<&str> = files.iter().map(String::as_str).collect();
                if let Err(e) = m.stage_files(&refs) {
                    actions.push(Action::Message { title: "Stage".into(), text: e.to_string(), error: true });
                }
                actions.push(Action::RefreshStatus);
            }
            DiffCommand::Unstage(files) => {
                let refs: Vec<&str> = files.iter().map(String::as_str).collect();
                if let Err(e) = m.unstage_files(&refs) {
                    actions.push(Action::Message { title: "Unstage".into(), text: e.to_string(), error: true });
                }
                actions.push(Action::RefreshStatus);
            }
            DiffCommand::ResetWorkFiles(files) => actions.push(Action::OpenDialog(Box::new(reset::ResetChangesDialog::files(files)))),
            DiffCommand::CopyPaths(p) => actions.push(Action::Copy(p.join("\n"))),
            DiffCommand::OpenContainingFolder(f) => {
                let p = m.work_dir().join(f);
                crate::util::open_with_system(&p.parent().unwrap_or(m.work_dir()).display().to_string());
            }
            DiffCommand::ExternalDiff { first, second, file } => {
                let mut args = GitArgs::new("difftool").arg("--find-renames").arg("--find-copies");
                if second == ObjectId::INDEX {
                    args.add("--cached");
                } else if second != ObjectId::WORK_TREE {
                    args.add(first.map(|f| f.to_string()).unwrap_or_else(|| format!("{second}^")));
                    args.add(second.to_string());
                }
                args.add("--");
                args.add(file);
                actions.push(Action::RunTool { tool_type: gitext_core::diff_tools::ToolType::Diff, args });
            }
            DiffCommand::AddToGitIgnore(files) => actions.push(Action::OpenDialog(Box::new(gitignore::AddToGitIgnoreDialog::new(files)))),
            DiffCommand::StagePatch { patch, reverse } => {
                if let Err(e) = m.apply_patch_text(&patch, true, reverse) {
                    actions.push(Action::Message { title: "Stage lines".into(), text: e.to_string(), error: true });
                }
                actions.push(Action::RefreshStatus);
            }
            DiffCommand::ResetPatch(patch) => {
                actions.push(Action::OpenDialog(Box::new(Confirm::new("Reset lines", "Reset the selected lines in the working directory? This cannot be undone.", "Reset", move |cx| {
                    if let Some(m) = cx.module {
                        if let Err(e) = m.apply_patch_text(&patch, false, true) {
                            cx.error("Reset lines", e.to_string());
                        }
                    }
                    cx.push(Action::RefreshStatus);
                }))));
            }
        }
    }
}
