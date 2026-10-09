//! Port of `RevisionGridControl` / `RevisionDataGridView`: the commit list with the graph,
//! ref labels, author, date and hash columns.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::Instant;

use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use egui_extras::{Column, TableBuilder};
use gitext_core::revision::{index_revision, work_tree_revision};
use gitext_core::revision_reader::{read_log, read_autostash, LogOptions};
use gitext_core::settings::{AppSettings, BranchFilterMode, GraphDrawStyle};
use gitext_core::{GitModule, GitRef, GitRevision, ObjectId};
use gitext_graph::hover::{HoverHighlight, VisibleRowRange};
use gitext_graph::render::{draw_row, Brush, Metrics, Primitive, RevisionGraphDrawStyle};
use gitext_graph::{RevisionGraph, RevisionGraphConfig, MAX_LANES};

use crate::repo::RepoData;
use crate::theme::Palette;
use crate::util::{format_date, short_date};

/// What the text filter of the toolbar searches (`RevisionFilter` / `FilterInfo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextFilterKind {
    #[default]
    Message,
    Author,
    Committer,
    Hash,
    DiffContains,
    Path,
}

impl TextFilterKind {
    pub const ALL: [TextFilterKind; 6] = [
        TextFilterKind::Message,
        TextFilterKind::Author,
        TextFilterKind::Committer,
        TextFilterKind::Hash,
        TextFilterKind::DiffContains,
        TextFilterKind::Path,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TextFilterKind::Message => "Commit message",
            TextFilterKind::Author => "Author",
            TextFilterKind::Committer => "Committer",
            TextFilterKind::Hash => "Hash",
            TextFilterKind::DiffContains => "Diff contains (slow)",
            TextFilterKind::Path => "Path filter",
        }
    }
}

/// Filters applied to the log (`FilterInfo`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevisionFilter {
    pub text: String,
    pub kind: TextFilterKind,
    pub since: String,
    pub until: String,
    pub path: String,
    pub follow: bool,
}

impl RevisionFilter {
    pub fn is_active(&self) -> bool {
        !self.text.trim().is_empty() || !self.since.trim().is_empty() || !self.until.trim().is_empty() || !self.path.trim().is_empty()
    }
}

enum LogMsg {
    Batch(Vec<GitRevision>),
    Done,
    Error(String),
}

/// What the user did in the grid this frame.
#[derive(Debug, Clone, Default)]
pub struct GridEvents {
    pub selection_changed: bool,
    pub double_clicked: Option<ObjectId>,
    pub context_action: Option<GridCommand>,
}

/// Context menu commands of the revision grid (`RevisionGridMenuCommands`).
#[derive(Debug, Clone)]
pub enum GridCommand {
    Checkout(ObjectId),
    CheckoutBranch(String),
    CreateBranch(ObjectId),
    CreateTag(ObjectId),
    ResetCurrentBranchHere(ObjectId),
    RebaseOnto(ObjectId),
    MergeInto(ObjectId),
    CherryPick(ObjectId),
    Revert(ObjectId),
    DeleteBranch(String),
    DeleteTag(String),
    RenameBranch(String),
    PushBranch(String),
    Archive(ObjectId),
    FormatPatch(ObjectId),
    CompareSelected,
    CompareWithWorkTree(ObjectId),
    ShowInFileTree(ObjectId),
    CopyToClipboard(String),
    NavigateToParent,
    NavigateToChild,
    GoToCommit,
    BisectGood,
    BisectBad,
    BisectSkip,
    OpenCommitDialog,
    ApplyStash(String),
    PopStash(String),
    DropStash(String),
    RunScript(usize, ObjectId),
    FixupCommit(ObjectId, &'static str),
    InteractiveRebase(ObjectId),
}

pub struct RevisionGrid {
    /// Whether the selection last moved with the keyboard (then the next rows are likely next).
    pub moved_by_keyboard: bool,
    pub graph: RevisionGraph,
    rx: Option<Receiver<LogMsg>>,
    cancel: Arc<AtomicBool>,
    pub loading: bool,
    pub error: Option<String>,
    pub loaded_count: usize,
    load_started: Option<Instant>,
    pub load_duration_ms: Option<u128>,
    /// Selected commits in selection order (the first one is the base for diffs).
    pub selected: Vec<ObjectId>,
    anchor_row: Option<usize>,
    pub scroll_to_row: Option<usize>,
    /// Select this commit once it is loaded.
    pub pending_select: Option<ObjectId>,
    artificial_inserted: bool,
    head: ObjectId,
    work_tree_changes: usize,
    index_changes: usize,
    pub filter: RevisionFilter,
    quick_search: String,
    quick_search_time: Option<Instant>,
    hover: HoverHighlight,
    hovered_label: Option<(usize, String)>,
    visible: VisibleRowRange,
    lane_columns: i32,
    pub has_focus: bool,
    user_email: String,
}

impl Default for RevisionGrid {
    fn default() -> Self {
        RevisionGrid {
            moved_by_keyboard: false,
            graph: RevisionGraph::new(RevisionGraphConfig::default()),
            rx: None,
            cancel: Arc::new(AtomicBool::new(false)),
            loading: false,
            error: None,
            loaded_count: 0,
            load_started: None,
            load_duration_ms: None,
            selected: Vec::new(),
            anchor_row: None,
            scroll_to_row: None,
            pending_select: None,
            artificial_inserted: false,
            head: ObjectId::ZERO,
            work_tree_changes: 0,
            index_changes: 0,
            filter: RevisionFilter::default(),
            quick_search: String::new(),
            quick_search_time: None,
            hover: HoverHighlight::default(),
            hovered_label: None,
            visible: VisibleRowRange { from_index: 0, count: 30 },
            lane_columns: 1,
            has_focus: true,
            user_email: String::new(),
        }
    }
}

pub fn graph_config(s: &AppSettings) -> RevisionGraphConfig {
    let g = s.graph_config();
    RevisionGraphConfig {
        merge_graph_lanes_having_common_parent: g.merge_graph_lanes_having_common_parent,
        render_graph_with_diagonals: g.render_graph_with_diagonals,
        straighten_graph_diagonals: g.straighten_graph_diagonals,
        straighten_graph_segments_limit: g.straighten_graph_segments_limit,
    }
}

/// Builds the `git log` arguments from the settings and filter (port of `RevisionFilter`/`BuildFilter`).
pub fn log_options(settings: &AppSettings, filter: &RevisionFilter, current_branch: Option<&str>) -> LogOptions {
    let mut rev = Vec::new();
    match settings.branch_filter_mode {
        BranchFilterMode::All => {
            if !settings.show_stashes {
                rev.push("--exclude=refs/stash".to_string());
            }
            if !settings.show_remote_branches {
                rev.push("--exclude=refs/remotes/*".to_string());
            }
            if !settings.show_tags {
                rev.push("--exclude=refs/tags/*".to_string());
            }
            rev.push("--all".to_string());
            rev.push("--boundary".to_string());
        }
        BranchFilterMode::Current => rev.push(current_branch.map(str::to_string).unwrap_or_else(|| "HEAD".into())),
        BranchFilterMode::Specific => {
            let branches: Vec<String> = settings.branch_filter.split_whitespace().map(str::to_string).collect();
            if branches.is_empty() {
                rev.push("HEAD".into());
            }
            for b in branches {
                if b.contains(['*', '?', '[']) {
                    rev.push(format!("--branches={b}"));
                } else {
                    rev.push(b);
                }
            }
        }
    }
    if settings.show_reflog_references {
        rev.push("--reflog".into());
    }
    if settings.show_first_parent {
        rev.push("--first-parent".into());
    }
    if !settings.show_merge_commits {
        rev.push("--no-merges".into());
    }
    let text = filter.text.trim();
    let mut path_filter = Vec::new();
    if !text.is_empty() {
        match filter.kind {
            TextFilterKind::Message => {
                rev.push(format!("--grep={text}"));
                rev.push("--regexp-ignore-case".into());
            }
            TextFilterKind::Author => {
                rev.push(format!("--author={text}"));
                rev.push("--regexp-ignore-case".into());
            }
            TextFilterKind::Committer => {
                rev.push(format!("--committer={text}"));
                rev.push("--regexp-ignore-case".into());
            }
            TextFilterKind::DiffContains => rev.push(format!("-G{text}")),
            TextFilterKind::Hash => {}
            TextFilterKind::Path => path_filter.push(text.to_string()),
        }
    }
    if !filter.path.trim().is_empty() {
        path_filter.push(filter.path.trim().to_string());
        if filter.follow {
            rev.push("--follow".into());
        }
    }
    if !filter.since.trim().is_empty() {
        rev.push(format!("--since={}", filter.since.trim()));
    }
    if !filter.until.trim().is_empty() {
        rev.push(format!("--until={}", filter.until.trim()));
    }
    LogOptions {
        revision_filter: rev,
        path_filter,
        has_notes: settings.show_git_notes,
        sort_order: settings.revision_sort_order,
        oldest_body: gitext_core::revision_reader::unix_time_days_ago(gitext_core::revision_reader::OFFSET_DAYS_FOR_OLDEST_BODY),
        max_count: (settings.max_revision_graph_commits > 0).then_some(settings.max_revision_graph_commits),
    }
}

impl RevisionGrid {
    /// (Re)loads the log in the background, keeping the selection.
    pub fn reload(&mut self, ctx: &egui::Context, module: &GitModule, data: &RepoData, settings: &AppSettings) {
        self.cancel.store(true, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Arc::clone(&cancel);
        if self.pending_select.is_none() {
            self.pending_select = self.selected.first().copied().or(Some(if settings.show_artificial_commits && !data.status.is_empty() {
                ObjectId::WORK_TREE
            } else {
                data.head
            }));
        }
        self.graph = RevisionGraph::new(graph_config(settings));
        self.graph.only_first_parent = settings.show_first_parent;
        self.graph.head_id = data.head;
        self.head = data.head;
        self.artificial_inserted = !settings.show_artificial_commits || data.is_bare || self.filter.is_active();
        self.work_tree_changes = data.work_tree_changes();
        self.index_changes = data.index_changes();
        self.loading = true;
        self.error = None;
        self.loaded_count = 0;
        self.load_started = Some(Instant::now());
        self.hover = HoverHighlight::default();
        self.user_email = data.user_email.clone();

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let options = log_options(settings, &self.filter, data.current_branch.as_deref());
        let work_dir = module.work_dir().to_path_buf();
        let git_dir = module.git_dir().to_path_buf();
        let refs = data.refs_by_commit.clone();
        let show_stashes = settings.show_stashes;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let attach = |mut revs: Vec<GitRevision>| {
                for r in &mut revs {
                    if let Some(refs) = refs.get(&r.object_id) {
                        r.refs = refs.clone();
                    }
                }
                revs
            };
            if show_stashes {
                if let Some(autostash) = read_autostash(&git_dir, "Autostash") {
                    let _ = tx.send(LogMsg::Batch(vec![autostash]));
                }
            }
            let result = read_log(&work_dir, &options, cancel, |batch| {
                let _ = tx.send(LogMsg::Batch(attach(batch)));
                ctx.request_repaint();
            });
            let _ = tx.send(match result {
                Ok(_) => LogMsg::Done,
                Err(e) => LogMsg::Error(e.to_string()),
            });
            ctx.request_repaint();
        });
    }

    /// Updates the artificial commit change counts without reloading.
    pub fn update_status_counts(&mut self, data: &RepoData) {
        self.work_tree_changes = data.work_tree_changes();
        self.index_changes = data.index_changes();
        // the artificial rows show the counts in their subject
        for (id, subject) in [
            (ObjectId::WORK_TREE, format!("Working directory ({})", plural(self.work_tree_changes, "change"))),
            (ObjectId::INDEX, format!("Commit index ({})", plural(self.index_changes, "change"))),
        ] {
            if let Some(n) = self.graph.try_get_node(&id) {
                if let Some(rev) = &mut self.graph.store.nodes[n].revision {
                    rev.subject = subject;
                }
            }
        }
    }

    /// `(id, parents)` of the commits around the selected one (the next two rows below it and
    /// the row above), the likely next selections.
    pub fn neighbor_revisions(&mut self) -> Vec<(ObjectId, Vec<ObjectId>)> {
        let Some(row) = self.selected_revision().and_then(|id| self.graph.try_get_row_index(&id)) else { return Vec::new() };
        let mut out = Vec::new();
        for r in [Some(row + 1), Some(row + 2), row.checked_sub(1)].into_iter().flatten() {
            let Some(n) = self.graph.get_node_for_row(r) else { continue };
            if let Some(rev) = self.graph.store.nodes[n].revision.as_ref().filter(|rev| !rev.is_artificial()) {
                out.push((rev.object_id, rev.parents().to_vec()));
            }
        }
        out
    }

    /// Drains loaded revisions into the graph.
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        let Some(rx) = &self.rx else { return false };
        let mut changed = false;
        let mut added_this_frame = 0;
        while let Ok(msg) = rx.try_recv() {
            changed = true;
            match msg {
                LogMsg::Batch(revs) => {
                    added_this_frame += revs.len();
                    for r in revs {
                        self.graph.add(r);
                    }
                    self.loaded_count = self.graph.count();
                }
                LogMsg::Done => {
                    self.loading = false;
                    self.graph.loading_completed();
                    self.load_duration_ms = self.load_started.map(|s| s.elapsed().as_millis());
                    self.insert_artificial();
                    self.rx = None;
                    break;
                }
                LogMsg::Error(e) => {
                    self.loading = false;
                    self.error = Some(e);
                    self.graph.loading_completed();
                    self.rx = None;
                    break;
                }
            }
            if added_this_frame > 50_000 {
                ctx.request_repaint();
                break;
            }
        }
        if changed {
            self.insert_artificial();
            if let Some(id) = self.pending_select {
                if let Some(row) = self.graph.try_get_row_index(&id) {
                    self.selected = vec![id];
                    self.anchor_row = Some(row);
                    self.scroll_to_row = Some(row);
                    self.pending_select = None;
                } else if !self.loading {
                    self.pending_select = None;
                    if self.selected.is_empty() {
                        if let Some(n) = self.graph.get_node_for_row(0) {
                            self.selected = vec![self.graph.store.nodes[n].object_id];
                        }
                    }
                }
            }
        }
        changed
    }

    fn insert_artificial(&mut self) {
        if self.artificial_inserted {
            return;
        }
        let head = self.head;
        if head.is_zero() || self.graph.contains(&head) || !self.loading {
            let wt = work_tree_revision(head, &format!("Working directory ({})", plural(self.work_tree_changes, "change")));
            let ix = index_revision(head, &format!("Commit index ({})", plural(self.index_changes, "change")));
            let parents = if head.is_zero() || !self.graph.contains(&head) { vec![] } else { vec![head] };
            self.graph.insert(wt, ix, &parents);
            self.artificial_inserted = true;
        }
    }

    pub fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn selected_revision(&self) -> Option<ObjectId> {
        self.selected.last().copied()
    }

    pub fn revision(&self, id: &ObjectId) -> Option<&GitRevision> {
        let n = self.graph.try_get_node(id)?;
        self.graph.store.nodes[n].revision.as_ref()
    }

    pub fn select(&mut self, id: ObjectId) {
        self.moved_by_keyboard = false;
        if let Some(row) = self.graph.try_get_row_index(&id) {
            self.selected = vec![id];
            self.anchor_row = Some(row);
            self.scroll_to_row = Some(row);
        } else {
            self.pending_select = Some(id);
        }
    }

    fn select_row(&mut self, row: usize, ctrl: bool, shift: bool) {
        let Some(n) = self.graph.get_node_for_row(row) else { return };
        let id = self.graph.store.nodes[n].object_id;
        if shift {
            if let Some(anchor) = self.anchor_row {
                let (a, b) = if anchor <= row { (anchor, row) } else { (row, anchor) };
                let mut sel = Vec::new();
                for r in a..=b {
                    if let Some(n) = self.graph.get_node_for_row(r) {
                        sel.push(self.graph.store.nodes[n].object_id);
                    }
                }
                if anchor > row {
                    sel.reverse();
                }
                // keep the anchor as the first (base) selection
                self.selected = sel;
                return;
            }
        }
        if ctrl {
            if let Some(i) = self.selected.iter().position(|s| *s == id) {
                self.selected.remove(i);
            } else {
                self.selected.push(id);
            }
        } else {
            self.selected = vec![id];
        }
        self.anchor_row = Some(row);
    }

    fn row_of_selected(&mut self) -> Option<usize> {
        let id = self.selected.last().copied()?;
        self.graph.try_get_row_index(&id)
    }

    /// Navigates to the first parent of the selected revision (`NavigateToParent`).
    pub fn navigate_parent(&mut self) {
        if let Some(id) = self.selected_revision() {
            if let Some(p) = self.revision(&id).and_then(|r| r.parents().first().copied()) {
                self.select(p);
            }
        }
    }

    pub fn navigate_child(&mut self) {
        if let Some(id) = self.selected_revision() {
            if let Some(n) = self.graph.try_get_node(&id) {
                if let Some(&c) = self.graph.store.nodes[n].children.first() {
                    let cid = self.graph.store.nodes[c].object_id;
                    self.select(cid);
                }
            }
        }
    }

    fn handle_keys(&mut self, ui: &Ui, count: usize) -> bool {
        if !self.has_focus || count == 0 || ui.ctx().wants_keyboard_input() {
            return false;
        }
        let (up, down, pgup, pgdn, home, end, shift) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::PageUp),
                i.key_pressed(egui::Key::PageDown),
                i.key_pressed(egui::Key::Home),
                i.key_pressed(egui::Key::End),
                i.modifiers.shift,
            )
        });
        let current = self.row_of_selected().unwrap_or(0) as i64;
        let page = self.visible.count.max(1) as i64;
        let target = if up {
            current - 1
        } else if down {
            current + 1
        } else if pgup {
            current - page
        } else if pgdn {
            current + page
        } else if home {
            0
        } else if end {
            count as i64 - 1
        } else {
            return self.quick_search(ui);
        };
        let target = target.clamp(0, count as i64 - 1) as usize;
        if shift {
            if self.anchor_row.is_none() {
                self.anchor_row = Some(current as usize);
            }
            self.select_row(target, false, true);
        } else {
            self.select_row(target, false, false);
        }
        self.scroll_to_row = Some(target);
        true
    }

    /// Port of `QuickSearchProvider`: typing jumps to the next matching commit.
    fn quick_search(&mut self, ui: &Ui) -> bool {
        let typed: String = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Text(t) if !i.modifiers.ctrl && !i.modifiers.command => Some(t.clone()),
                    _ => None,
                })
                .collect()
        });
        if typed.is_empty() {
            return false;
        }
        if self.quick_search_time.is_some_and(|t| t.elapsed().as_millis() > 1500) {
            self.quick_search.clear();
        }
        self.quick_search.push_str(&typed);
        self.quick_search_time = Some(Instant::now());
        let needle = self.quick_search.to_lowercase();
        let start = self.row_of_selected().unwrap_or(0);
        let count = self.graph.count();
        for offset in 0..count {
            let row = (start + offset) % count;
            if let Some(n) = self.graph.get_node_for_row(row) {
                let node = &self.graph.store.nodes[n];
                if let Some(r) = &node.revision {
                    let hit = r.subject.to_lowercase().contains(&needle)
                        || r.author.to_lowercase().contains(&needle)
                        || r.guid().starts_with(&needle)
                        || r.refs.iter().any(|g| g.name.to_lowercase().contains(&needle));
                    if hit {
                        self.select_row(row, false, false);
                        self.scroll_to_row = Some(row);
                        return true;
                    }
                }
            }
        }
        true
    }

    /// Shows the grid. `data` provides refs for labels and context menus.
    pub fn ui(&mut self, ui: &mut Ui, settings: &AppSettings, data: &RepoData) -> GridEvents {
        let mut events = GridEvents::default();
        self.poll(ui.ctx());
        let palette = Palette::for_ui(ui);
        let count = self.graph.count();

        if count == 0 {
            ui.centered_and_justified(|ui| {
                if self.loading {
                    ui.spinner();
                } else if let Some(e) = &self.error {
                    ui.colored_label(palette.error, e);
                } else if self.filter.is_active() {
                    ui.label("No commits match the filter.");
                } else {
                    ui.label("This repository does not contain any commits.");
                }
            });
            return events;
        }

        let row_height = settings.row_height.max(16.0);
        let metrics = Metrics { lane_width: settings.lane_width, lane_line_width: 2.0, node_dimension: (row_height * 0.45).round().clamp(8.0, 12.0), row_height };

        // Ensure rows for the visible range (+ margin) have graph lanes.
        let last_needed = self.visible.from_index + self.visible.count + 10;
        self.graph.cache_to(last_needed, last_needed);
        let valid = self.graph.valid_row_count();
        let mut max_lanes = 1;
        for r in self.visible.from_index..(self.visible.from_index + self.visible.count).min(valid) {
            if let Some(row) = self.graph.row(r as i64) {
                max_lanes = max_lanes.max(row.get_lane_count(&self.graph.store));
            }
        }
        // Avoid jitter: grow immediately, shrink slowly.
        self.lane_columns = if max_lanes > self.lane_columns { max_lanes } else { (self.lane_columns + max_lanes) / 2 }.clamp(1, MAX_LANES);
        let graph_width = (self.lane_columns as f32 * metrics.lane_width + 8.0).max(24.0);

        let keyboard_moved = self.handle_keys(ui, count);
        if keyboard_moved {
            events.selection_changed = true;
            self.moved_by_keyboard = true;
        }

        let draw_style = match settings.graph_draw_style {
            GraphDrawStyle::Normal => RevisionGraphDrawStyle::Normal,
            GraphDrawStyle::DrawNonRelativesGray => RevisionGraphDrawStyle::DrawNonRelativesGray,
            GraphDrawStyle::HighlightSelected => RevisionGraphDrawStyle::HighlightSelected,
        };
        if draw_style == RevisionGraphDrawStyle::HighlightSelected {
            if let Some(id) = self.selected_revision() {
                self.graph.highlight_branch(&id);
            }
        }

        let selected: HashSet<ObjectId> = self.selected.iter().copied().collect();
        let scroll_to = self.scroll_to_row.take();
        let mut first_visible = usize::MAX;
        let mut last_visible = 0;
        let mut new_hover: Option<(usize, GitRef)> = None;
        let mut clicked: Option<(usize, bool, bool)> = None;
        let mut double_clicked = None;
        let mut context: Option<GridCommand> = None;
        let mut focus_clicked = false;
        let author_email = self.user_email.clone();

        // No vertical gap between rows: the graph lines of adjacent rows must touch.
        let item_spacing = ui.spacing().item_spacing;
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut table = TableBuilder::new(ui)
            .striped(false)
            .resizable(true)
            .sense(Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(graph_width))
            .column(Column::remainder().at_least(200.0).clip(true));
        if settings.show_author_column {
            table = table.column(Column::initial(150.0).at_least(60.0).clip(true));
        }
        if settings.show_date_column {
            table = table.column(Column::initial(120.0).at_least(60.0).clip(true));
        }
        if settings.show_id_column {
            table = table.column(Column::initial(76.0).at_least(40.0).clip(true));
        }
        if let Some(row) = scroll_to {
            table = table.scroll_to_row(row, Some(egui::Align::Center));
        }

        let hover_ids = self.hover.highlighted_ids().cloned();
        let graph = &mut self.graph;
        table
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.label(egui::RichText::new("Graph").small().color(palette.muted));
                });
                header.col(|ui| {
                    ui.label(egui::RichText::new("Message").small().color(palette.muted));
                });
                if settings.show_author_column {
                    header.col(|ui| {
                        ui.label(egui::RichText::new("Author").small().color(palette.muted));
                    });
                }
                if settings.show_date_column {
                    header.col(|ui| {
                        ui.label(egui::RichText::new("Date").small().color(palette.muted));
                    });
                }
                if settings.show_id_column {
                    header.col(|ui| {
                        ui.label(egui::RichText::new("Commit").small().color(palette.muted));
                    });
                }
            })
            .body(|body| {
                body.rows(row_height, count, |mut row| {
                    let index = row.index();
                    first_visible = first_visible.min(index);
                    last_visible = last_visible.max(index);
                    let Some(node_idx) = graph.get_node_for_row(index) else { return };
                    let node = &graph.store.nodes[node_idx];
                    let id = node.object_id;
                    let is_relative = node.is_relative;
                    let Some(rev) = node.revision.clone() else { return };
                    row.set_selected(selected.contains(&id));

                    // Graph column
                    row.col(|ui| {
                        let rect = ui.max_rect();
                        let rect = Rect::from_min_size(Pos2::new(rect.left(), rect.top()), Vec2::new(graph_width, row_height));
                        if index < valid {
                            let prims = draw_row(graph, index as i64, &metrics, draw_style, self_head(&rev, graph), hover_ids.as_ref());
                            paint_graph(ui, rect, &prims, &palette, is_relative || draw_style == RevisionGraphDrawStyle::Normal);
                        }
                    });

                    let highlight = settings.highlight_author_commits && !author_email.is_empty() && rev.author_email == author_email && !selected.contains(&id);
                    let paint_highlight = |ui: &mut Ui| {
                        if highlight {
                            ui.painter().rect_filled(ui.max_rect().expand2(Vec2::new(4.0, 0.0)), 0.0, palette.author_highlight_bg);
                        }
                    };
                    // Message column with ref labels
                    row.col(|ui| {
                        paint_highlight(ui);
                        let dim = !is_relative && draw_style != RevisionGraphDrawStyle::Normal;
                        for r in visible_refs(&rev.refs, settings) {
                            let resp = ref_label(ui, r, &palette, data.current_branch.as_deref());
                            if resp.hovered() {
                                new_hover = Some((index, r.clone()));
                            }
                        }
                        let mut text = egui::RichText::new(&rev.subject);
                        if rev.is_artificial() {
                            text = text.italics().color(palette.artificial_fg);
                        } else if rev.is_autostash {
                            text = text.italics();
                        } else if dim {
                            text = text.color(palette.muted);
                        }
                        let multiline = if rev.has_multi_line_message { " …" } else { "" };
                        ui.add(egui::Label::new(text).truncate().selectable(false));
                        if !multiline.is_empty() {
                            ui.label(egui::RichText::new(multiline).color(palette.muted));
                        }
                        if let Some(notes) = rev.notes.as_deref().filter(|n| !n.is_empty()) {
                            ui.label(egui::RichText::new(format!("[{}]", notes.lines().next().unwrap_or_default())).small().color(palette.muted));
                        }
                    });
                    if settings.show_author_column {
                        row.col(|ui| {
                            paint_highlight(ui);
                            if !rev.is_artificial() {
                                ui.add(egui::Label::new(egui::RichText::new(&rev.author).color(if is_relative { ui.visuals().text_color() } else { palette.muted })).truncate().selectable(false));
                            }
                        });
                    }
                    if settings.show_date_column {
                        row.col(|ui| {
                            paint_highlight(ui);
                            if !rev.is_artificial() {
                                let t = if settings.show_author_date { rev.author_unix_time } else { rev.commit_unix_time };
                                let label = if settings.show_relative_date { short_date(t) } else { format_date(t) };
                                ui.add(egui::Label::new(egui::RichText::new(label).color(palette.muted)).truncate().selectable(false)).on_hover_text(format_date(t));
                            }
                        });
                    }
                    if settings.show_id_column {
                        row.col(|ui| {
                            paint_highlight(ui);
                            if !rev.is_artificial() {
                                ui.add(egui::Label::new(egui::RichText::new(id.to_short_string()).monospace().color(palette.muted)).selectable(false));
                            }
                        });
                    }

                    let resp = row.response();
                    // select on press, as the original grid does: the diff starts loading while
                    // the button is still down
                    if resp.contains_pointer() && resp.ctx.input(|i| i.pointer.primary_pressed()) {
                        let m = resp.ctx.input(|i| i.modifiers);
                        clicked = Some((index, m.command || m.ctrl, m.shift));
                        focus_clicked = true;
                    }
                    if resp.double_clicked() {
                        double_clicked = Some(id);
                    }
                    if resp.secondary_clicked() && !selected.contains(&id) {
                        clicked = Some((index, false, false));
                    }
                    resp.context_menu(|ui| {
                        if let Some(c) = revision_context_menu(ui, &rev, data, settings) {
                            context = Some(c);
                            ui.close_kind(egui::UiKind::Menu);
                        }
                    });
                });
            });
        ui.spacing_mut().item_spacing = item_spacing;

        if first_visible != usize::MAX {
            self.visible = VisibleRowRange { from_index: first_visible, count: last_visible - first_visible + 1 };
            // Rows were shown without graph lanes (the cache was built for the previous scroll
            // position, e.g. after a jump): draw again with the cache built for this range.
            if last_visible >= valid && valid < count {
                ui.ctx().request_repaint();
            }
        }
        if let Some((row, ctrl, shift)) = clicked {
            self.select_row(row, ctrl, shift);
            events.selection_changed = true;
            self.moved_by_keyboard = false;
        }
        if focus_clicked {
            self.has_focus = true;
        } else if ui.input(|i| i.pointer.any_click()) && !ui.ui_contains_pointer() {
            self.has_focus = false;
        }
        events.double_clicked = double_clicked;
        events.context_action = context;

        // Hover highlighting of the ancestry of a hovered ref label
        let hover_key = new_hover.as_ref().map(|(row, r)| (*row, r.complete_name.clone()));
        if hover_key != self.hovered_label {
            self.hovered_label = hover_key;
            let (row, r) = match &new_hover {
                Some((row, r)) => (*row as i64, Some(r)),
                None => (-1, None),
            };
            self.hover.set(&mut self.graph, r, row, self.visible);
        }
        if self.loading {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        events
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 0 {
        format!("no {word}s")
    } else if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// The head id used for the node outline.
fn self_head(_rev: &GitRevision, graph: &RevisionGraph) -> ObjectId {
    graph.head_id
}

/// Refs shown as labels, filtered by settings.
pub fn visible_refs<'a>(refs: &'a [GitRef], settings: &AppSettings) -> Vec<&'a GitRef> {
    let mut v: Vec<&GitRef> = refs
        .iter()
        .filter(|r| !r.is_dereference)
        .filter(|r| settings.show_remote_branches || !r.is_remote())
        .filter(|r| settings.show_tags || !r.is_tag())
        .filter(|r| !gitext_core::git_ref::ref_name::is_remote_head(&r.complete_name))
        .collect();
    // current branch first, then local branches, remote, tags
    v.sort_by_key(|r| (!r.is_selected, !r.is_head(), !r.is_remote(), !r.is_tag(), r.name.clone()));
    v
}

/// Draws a ref label (port of `RevisionGridRefRenderer`).
pub fn ref_label(ui: &mut Ui, r: &GitRef, palette: &Palette, current_branch: Option<&str>) -> egui::Response {
    let is_current = r.is_head() && current_branch == Some(r.name.as_str());
    let (fill, prefix) = if r.is_stash() {
        (palette.label_stash, "☰ ")
    } else if r.is_tag() {
        (palette.label_tag, "🏷 ")
    } else if r.is_remote() {
        (palette.label_remote, "")
    } else if r.is_head() {
        (if is_current { palette.label_current_branch } else { palette.label_branch }, if is_current { "★ " } else { "" })
    } else {
        (palette.label_other, "")
    };
    let text = format!("{prefix}{}", if r.is_stash() { "stash".to_string() } else { r.name.clone() });
    let font = FontId::proportional(ui.text_style_height(&egui::TextStyle::Small).max(11.0));
    let galley = ui.painter().layout_no_wrap(text, font, palette.label_text);
    let size = galley.size() + Vec2::new(10.0, 3.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let rect = rect.shrink2(Vec2::new(0.0, 0.5));
    ui.painter().rect_filled(rect, 4.0, fill);
    if is_current {
        ui.painter().rect_stroke(rect, 4.0, Stroke::new(1.0_f32, palette.lanes[2]), egui::StrokeKind::Middle);
    }
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, palette.label_text);
    resp.on_hover_text(r.complete_name.as_str())
}

fn brush_color(b: Brush, palette: &Palette) -> Color32 {
    match b {
        Brush::Lane(i) => palette.lane(i),
        Brush::NonRelative => palette.non_relative,
    }
}

/// Paints graph primitives in `rect` (port of the GDI+ drawing in `GraphRenderer`).
pub fn paint_graph(ui: &Ui, rect: Rect, prims: &[Primitive], palette: &Palette, _relative: bool) {
    let painter = ui.painter_at(rect);
    let o = rect.min.to_vec2() + Vec2::new(2.0, 0.0);
    let p = |pt: gitext_graph::Point| Pos2::new(pt.x, pt.y) + o;
    for prim in prims {
        match prim {
            Primitive::Line { from, to, brush, width, .. } => {
                painter.line_segment([p(*from), p(*to)], Stroke::new(*width, brush_color(*brush, palette)));
            }
            Primitive::Bezier { points, brush, width } => {
                let shape = egui::epaint::CubicBezierShape::from_points_stroke(
                    [p(points[0]), p(points[1]), p(points[2]), p(points[3])],
                    false,
                    Color32::TRANSPARENT,
                    Stroke::new(*width, brush_color(*brush, palette)),
                );
                painter.add(shape);
            }
            Primitive::Node { center, size, square, outline, brush } => {
                let c = p(*center);
                let color = brush_color(*brush, palette);
                if *square {
                    let r = Rect::from_center_size(c, Vec2::splat(*size));
                    painter.rect_filled(r, 1.5, color);
                    if *outline {
                        painter.rect_stroke(r.expand(1.5), 2.0, Stroke::new(2.0_f32, palette.node_outline), egui::StrokeKind::Middle);
                    }
                } else {
                    painter.circle_filled(c, size / 2.0, color);
                    if *outline {
                        painter.circle_stroke(c, size / 2.0 + 1.5, Stroke::new(2.0_f32, palette.node_outline));
                    }
                }
            }
        }
    }
}

/// The context menu of a revision row (port of `RevisionGridControl` context menu).
pub fn revision_context_menu(ui: &mut Ui, rev: &GitRevision, data: &RepoData, settings: &AppSettings) -> Option<GridCommand> {
    let id = rev.object_id;
    let mut cmd = None;
    if rev.is_artificial() {
        if ui.button("Commit…").clicked() {
            cmd = Some(GridCommand::OpenCommitDialog);
        }
        if ui.button("Compare with selected").clicked() {
            cmd = Some(GridCommand::CompareSelected);
        }
        return cmd;
    }
    let branches: Vec<&GitRef> = rev.refs.iter().filter(|r| r.is_head()).collect();
    let remote_branches: Vec<&GitRef> = rev.refs.iter().filter(|r| r.is_remote() && !gitext_core::git_ref::ref_name::is_remote_head(&r.complete_name)).collect();
    let tags: Vec<&GitRef> = rev.refs.iter().filter(|r| r.is_tag() && !r.is_dereference).collect();

    if let Some(sel) = &rev.reflog_selector {
        let stash = sel.trim_start_matches("refs/").to_string();
        ui.label(egui::RichText::new(&stash).strong());
        if ui.button("Apply stash").clicked() {
            cmd = Some(GridCommand::ApplyStash(stash.clone()));
        }
        if ui.button("Pop stash").clicked() {
            cmd = Some(GridCommand::PopStash(stash.clone()));
        }
        if ui.button("Drop stash…").clicked() {
            cmd = Some(GridCommand::DropStash(stash));
        }
        ui.separator();
    }

    if !branches.is_empty() || !remote_branches.is_empty() {
        ui.menu_button("Checkout branch", |ui| {
            for b in branches.iter().chain(remote_branches.iter()) {
                if ui.button(&b.name).clicked() {
                    cmd = Some(GridCommand::CheckoutBranch(b.name.clone()));
                    ui.close_kind(egui::UiKind::Menu);
                }
            }
        });
    }
    if ui.button("Checkout revision…").clicked() {
        cmd = Some(GridCommand::Checkout(id));
    }
    ui.separator();
    if ui.button("Create new branch here…").clicked() {
        cmd = Some(GridCommand::CreateBranch(id));
    }
    if ui.button("Create new tag here…").clicked() {
        cmd = Some(GridCommand::CreateTag(id));
    }
    if !branches.is_empty() {
        ui.menu_button("Rename branch", |ui| {
            for b in &branches {
                if ui.button(&b.name).clicked() {
                    cmd = Some(GridCommand::RenameBranch(b.name.clone()));
                    ui.close_kind(egui::UiKind::Menu);
                }
            }
        });
        ui.menu_button("Push branch", |ui| {
            for b in &branches {
                if ui.button(&b.name).clicked() {
                    cmd = Some(GridCommand::PushBranch(b.name.clone()));
                    ui.close_kind(egui::UiKind::Menu);
                }
            }
        });
    }
    if !branches.is_empty() || !remote_branches.is_empty() {
        ui.menu_button("Delete branch", |ui| {
            for b in branches.iter().chain(remote_branches.iter()) {
                if ui.button(&b.name).clicked() {
                    cmd = Some(GridCommand::DeleteBranch(b.complete_name.clone()));
                    ui.close_kind(egui::UiKind::Menu);
                }
            }
        });
    }
    if !tags.is_empty() {
        ui.menu_button("Delete tag", |ui| {
            for t in &tags {
                if ui.button(&t.name).clicked() {
                    cmd = Some(GridCommand::DeleteTag(t.name.clone()));
                    ui.close_kind(egui::UiKind::Menu);
                }
            }
        });
    }
    ui.separator();
    let current = data.current_branch.clone().unwrap_or_else(|| "HEAD".into());
    if ui.button(format!("Reset current branch ({current}) to here…")).clicked() {
        cmd = Some(GridCommand::ResetCurrentBranchHere(id));
    }
    if ui.button(format!("Merge into current branch ({current})…")).clicked() {
        cmd = Some(GridCommand::MergeInto(id));
    }
    if ui.button(format!("Rebase current branch ({current}) on this…")).clicked() {
        cmd = Some(GridCommand::RebaseOnto(id));
    }
    if ui.button("Interactive rebase from here…").clicked() {
        cmd = Some(GridCommand::InteractiveRebase(id));
    }
    ui.menu_button("Advanced", |ui| {
        if ui.button("Create a fixup commit").clicked() {
            cmd = Some(GridCommand::FixupCommit(id, "fixup"));
            ui.close_kind(egui::UiKind::Menu);
        }
        if ui.button("Create a squash commit").clicked() {
            cmd = Some(GridCommand::FixupCommit(id, "squash"));
            ui.close_kind(egui::UiKind::Menu);
        }
        if ui.button("Create an amend commit").clicked() {
            cmd = Some(GridCommand::FixupCommit(id, "amend"));
            ui.close_kind(egui::UiKind::Menu);
        }
    });
    ui.separator();
    if ui.button("Cherry pick commit…").clicked() {
        cmd = Some(GridCommand::CherryPick(id));
    }
    if ui.button("Revert commit…").clicked() {
        cmd = Some(GridCommand::Revert(id));
    }
    ui.separator();
    ui.menu_button("Copy to clipboard", |ui| {
        let mut copy = |label: String, value: String| {
            if ui.button(label).clicked() {
                cmd = Some(GridCommand::CopyToClipboard(value));
                ui.close_kind(egui::UiKind::Menu);
            }
        };
        copy(format!("Commit hash  {}", id.to_short_string()), id.to_string());
        copy("Message".into(), rev.body().unwrap_or(&rev.subject).to_string());
        copy("Subject".into(), rev.subject.clone());
        copy(format!("Author  {}", rev.author), format!("{} <{}>", rev.author, rev.author_email));
        copy("Date".into(), format_date(rev.author_unix_time));
        for r in rev.refs.iter() {
            copy(format!("Ref  {}", r.name), r.name.clone());
        }
    });
    ui.menu_button("Navigate", |ui| {
        if ui.button("Go to parent").clicked() {
            cmd = Some(GridCommand::NavigateToParent);
            ui.close_kind(egui::UiKind::Menu);
        }
        if ui.button("Go to child").clicked() {
            cmd = Some(GridCommand::NavigateToChild);
            ui.close_kind(egui::UiKind::Menu);
        }
        if ui.button("Go to commit…").clicked() {
            cmd = Some(GridCommand::GoToCommit);
            ui.close_kind(egui::UiKind::Menu);
        }
    });
    if ui.button("Compare selected commits").clicked() {
        cmd = Some(GridCommand::CompareSelected);
    }
    if ui.button("Compare with working directory").clicked() {
        cmd = Some(GridCommand::CompareWithWorkTree(id));
    }
    if ui.button("Show in file tree").clicked() {
        cmd = Some(GridCommand::ShowInFileTree(id));
    }
    ui.separator();
    if ui.button("Archive revision…").clicked() {
        cmd = Some(GridCommand::Archive(id));
    }
    if ui.button("Format patch…").clicked() {
        cmd = Some(GridCommand::FormatPatch(id));
    }
    if data.state.bisecting {
        ui.menu_button("Bisect", |ui| {
            if ui.button("Mark as good").clicked() {
                cmd = Some(GridCommand::BisectGood);
                ui.close_kind(egui::UiKind::Menu);
            }
            if ui.button("Mark as bad").clicked() {
                cmd = Some(GridCommand::BisectBad);
                ui.close_kind(egui::UiKind::Menu);
            }
            if ui.button("Skip").clicked() {
                cmd = Some(GridCommand::BisectSkip);
                ui.close_kind(egui::UiKind::Menu);
            }
        });
    }
    if !settings.user_scripts.is_empty() {
        ui.menu_button("Run script", |ui| {
            for (i, s) in settings.user_scripts.iter().enumerate() {
                if ui.button(&s.name).clicked() {
                    cmd = Some(GridCommand::RunScript(i, id));
                    ui.close_kind(egui::UiKind::Menu);
                }
            }
        });
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_options_follow_settings() {
        let mut s = AppSettings::default();
        let o = log_options(&s, &RevisionFilter::default(), Some("main"));
        assert!(o.revision_filter.contains(&"--all".to_string()));
        s.show_tags = false;
        s.show_first_parent = true;
        let f = RevisionFilter { text: "fix".into(), kind: TextFilterKind::Author, ..Default::default() };
        let o = log_options(&s, &f, Some("main"));
        let pos_exclude = o.revision_filter.iter().position(|a| a == "--exclude=refs/tags/*").unwrap();
        let pos_all = o.revision_filter.iter().position(|a| a == "--all").unwrap();
        assert!(pos_exclude < pos_all, "exclude must precede --all");
        assert!(o.revision_filter.contains(&"--first-parent".to_string()));
        assert!(o.revision_filter.contains(&"--author=fix".to_string()));
        s.branch_filter_mode = BranchFilterMode::Current;
        let o = log_options(&s, &RevisionFilter { path: "src".into(), follow: true, ..Default::default() }, Some("main"));
        assert_eq!(o.revision_filter[0], "main");
        assert_eq!(o.path_filter, ["src"]);
        assert!(o.revision_filter.contains(&"--follow".to_string()));
    }

    #[test]
    fn visible_refs_sorting_and_filtering() {
        let id = ObjectId::random();
        let mut cur = GitRef::from_complete_name(id, "refs/heads/main");
        cur.is_selected = true;
        let refs = vec![
            GitRef::from_complete_name(id, "refs/tags/v1"),
            GitRef::from_complete_name(id, "refs/remotes/origin/main"),
            GitRef::from_complete_name(id, "refs/remotes/origin/HEAD"),
            cur,
            GitRef::from_complete_name(id, "refs/heads/feature"),
        ];
        let mut s = AppSettings::default();
        let v: Vec<&str> = visible_refs(&refs, &s).iter().map(|r| r.name.as_str()).collect();
        assert_eq!(v, ["main", "feature", "origin/main", "v1"]);
        s.show_remote_branches = false;
        s.show_tags = false;
        let v: Vec<&str> = visible_refs(&refs, &s).iter().map(|r| r.name.as_str()).collect();
        assert_eq!(v, ["main", "feature"]);
    }
}
