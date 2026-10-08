//! Port of `RevisionDiffControl`: changed files between revisions and their diff.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use egui::{RichText, Ui};
use gitext_core::module::DiffOptions;
use gitext_core::settings::AppSettings;
use gitext_core::status::GitItemStatus;
use gitext_core::{GitModule, ObjectId};

use super::diff_viewer::{content_from_bytes, DiffViewer, ViewerCommand, ViewerContent};
use super::file_list::FileList;
use crate::tasks::{Loader, Task};
use crate::theme::Palette;

/// File commands from the diff tab context menu.
#[derive(Debug, Clone)]
pub enum DiffCommand {
    Blame { file: String, rev: ObjectId },
    FileHistory(String),
    ShowInFileTree { rev: ObjectId, file: String },
    FilterPath(String),
    OpenWorkFile(String),
    OpenRevisionFile { rev: ObjectId, file: String },
    SaveAs { rev: ObjectId, file: String },
    ResetFileTo { rev: ObjectId, files: Vec<String> },
    Stage(Vec<String>),
    Unstage(Vec<String>),
    ResetWorkFiles(Vec<String>),
    CopyPaths(Vec<String>),
    OpenContainingFolder(String),
    ExternalDiff { first: Option<ObjectId>, second: ObjectId, file: String },
    AddToGitIgnore(Vec<String>),
    StagePatch { patch: String, reverse: bool },
    ResetPatch(String),
}

type FilesKey = (Option<ObjectId>, ObjectId, bool);
type Files = Arc<Vec<GitItemStatus>>;
type Content = Arc<ViewerContent>;

/// The most recent entries, oldest first.
struct Recent<K, V> {
    entries: VecDeque<(K, V)>,
    capacity: usize,
}

impl<K: PartialEq, V: Clone> Recent<K, V> {
    fn new(capacity: usize) -> Self {
        Recent { entries: VecDeque::new(), capacity }
    }

    fn get(&self, key: &K) -> Option<V> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    fn contains(&self, key: &K) -> bool {
        self.entries.iter().any(|(k, _)| k == key)
    }

    fn insert(&mut self, key: K, value: V) {
        self.entries.retain(|(k, _)| *k != key);
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back((key, value));
    }
}

/// File lists and diffs of commits. Commits do not change, so the entries never go stale; the
/// work tree and the index are never cached. Shared with the background loads and prefetches.
struct DiffCache {
    files: Recent<FilesKey, Files>,
    diffs: Recent<String, Content>,
}

impl Default for DiffCache {
    fn default() -> Self {
        DiffCache { files: Recent::new(64), diffs: Recent::new(128) }
    }
}

type SharedCache = Arc<Mutex<DiffCache>>;

fn cacheable(first: Option<ObjectId>, second: ObjectId) -> bool {
    !second.is_artificial() && first.is_none_or(|f| !f.is_artificial())
}

pub fn diff_options(settings: &AppSettings) -> DiffOptions {
    DiffOptions {
        ignore_whitespace: settings.ignore_whitespace,
        ignore_whitespace_changes: settings.ignore_whitespace_changes,
        context_lines: Some(if settings.show_entire_file { 99999 } else { settings.context_lines }),
        histogram: settings.use_histogram_diff,
        word_diff: false,
    }
}

fn diff_key(first: Option<ObjectId>, second: ObjectId, f: &GitItemStatus, opts: &DiffOptions, combined: bool) -> String {
    format!("{first:?}|{second}|{}|{:?}|{opts:?}|{combined}", f.name, f.old_name)
}

/// The changed files between `first` (`None`: the parent) and `second`.
fn load_files(m: &GitModule, first: Option<ObjectId>, second: ObjectId, combined: bool) -> Result<Vec<GitItemStatus>, String> {
    if combined {
        // only files changed against all parents
        let out = m
            .run_checked(&gitext_core::GitArgs::new("diff-tree").args(["--cc", "--name-status", "-z", "--no-commit-id"]).arg(second.to_string()))
            .map(|r| r.stdout_str())
            .map_err(|e| e.to_string())?;
        let parts: Vec<&str> = out.split('\0').filter(|s| !s.is_empty()).collect();
        Ok(parts
            .chunks(2)
            .filter_map(|c| (c.len() == 2).then(|| GitItemStatus::from_status_character(gitext_core::status::StagedStatus::None, c[1], c[0].chars().next().unwrap_or('M'))))
            .collect())
    } else {
        m.get_diff_files(first, second).map_err(|e| e.to_string())
    }
}

/// The diff of one file.
fn load_diff(m: &GitModule, first: Option<ObjectId>, second: ObjectId, combined: bool, f: &GitItemStatus, opts: &DiffOptions) -> ViewerContent {
    if f.is_submodule {
        let d = m.get_file_diff(first, second, &f.name, f.old_name.as_deref(), opts).unwrap_or_default();
        return ViewerContent::Diff(d);
    }
    let result = if combined { m.get_combined_diff(second, &f.name) } else { m.get_file_diff(first, second, &f.name, f.old_name.as_deref(), opts) };
    diff_content(m, second, f, result.map_err(|e| e.to_string()))
}

/// What to show for the diff of `f`.
fn diff_content(m: &GitModule, second: ObjectId, f: &GitItemStatus, result: Result<String, String>) -> ViewerContent {
    if f.is_submodule {
        return ViewerContent::Diff(result.unwrap_or_default());
    }
    match result {
        Ok(d) if d.contains("Binary files") && !d.contains("\n@@") => ViewerContent::Binary(format!("Binary file {} changed", f.name)),
        Ok(d) if d.trim().is_empty() => {
            if f.is_new && second == ObjectId::WORK_TREE {
                m.get_file_bytes(second, &f.name).map(|b| content_from_bytes(&b)).unwrap_or(ViewerContent::Empty("Empty file".into()))
            } else {
                ViewerContent::Empty("No differences (possibly only whitespace or file mode changes)".into())
            }
        }
        Ok(d) => ViewerContent::Diff(d),
        Err(e) => ViewerContent::Empty(e),
    }
}


/// Loads the files of a commit and the diff of its first file (the one shown when the commit is
/// selected) into the cache. Returns the files.
fn load_into_cache(cache: &SharedCache, m: &GitModule, first: Option<ObjectId>, second: ObjectId, combined: bool, opts: &DiffOptions) -> Result<Files, String> {
    // the files and the first diff from one git process: starting a process is slow on Windows.
    // git is stopped once the first diff is read, rather than producing the diffs of all files.
    if let (false, Some(parent)) = (combined, first) {
        if let Ok((files, patches)) = m.get_diff_files_with_patches(parent, second, opts, 0) {
            let contents: Vec<(String, Content)> = files
                .iter()
                .zip(patches)
                .map(|(f, p)| (diff_key(first, second, f, opts, combined), Arc::new(diff_content(m, second, f, Ok(p)))))
                .collect();
            let files = Arc::new(files);
            let mut c = cache.lock().unwrap();
            for (key, content) in contents {
                c.diffs.insert(key, content);
            }
            c.files.insert((first, second, combined), Arc::clone(&files));
            return Ok(files);
        }
    }
    let files = Arc::new(load_files(m, first, second, combined)?);
    if let Some(f) = files.first() {
        let key = diff_key(first, second, f, opts, combined);
        if !cache.lock().unwrap().diffs.contains(&key) {
            let content = Arc::new(load_diff(m, first, second, combined, f, opts));
            cache.lock().unwrap().diffs.insert(key, content);
        }
    }
    cache.lock().unwrap().files.insert((first, second, combined), Arc::clone(&files));
    Ok(files)
}

/// The selected file (by index into the file list it belongs to) and the diff settings.
struct ShownKey {
    files: Files,
    index: usize,
    first: Option<ObjectId>,
    second: ObjectId,
    combined: bool,
    opts: DiffOptions,
}

impl ShownKey {
    fn matches(&self, files: &Files, index: usize, first: Option<ObjectId>, second: ObjectId, combined: bool, opts: &DiffOptions) -> bool {
        Arc::ptr_eq(&self.files, files) && self.index == index && self.first == first && self.second == second && self.combined == combined && self.opts == *opts
    }
}

#[derive(Default)]
pub struct RevisionDiffView {
    files: Loader<FilesKey, Result<Files, String>>,
    pub list: FileList,
    diff: Loader<String, Content>,
    /// The diff shown and what it was loaded for, so that redrawing it needs no key or lookup.
    shown: Option<(ShownKey, Content)>,
    pub viewer: DiffViewer,
    pub parent_index: usize,
    last_key: Option<(Option<ObjectId>, ObjectId)>,
    /// Whether the file list of `last_key` was shown (logged in the command log).
    files_shown: bool,
    cache: SharedCache,
    prefetch: Option<Task<()>>,
}

impl RevisionDiffView {
    pub fn invalidate(&mut self) {
        self.files.invalidate();
        self.diff.invalidate();
        self.shown = None;
    }

    /// Loads the files and the first diff of `commits` (`(id, parents)`, the rows next to the
    /// selected one) in the background, so that moving the selection there shows them at once.
    /// Starts after the selected commit is loaded, one batch at a time.
    pub fn prefetch(&mut self, ctx: &egui::Context, module: &GitModule, commits: &[(ObjectId, Vec<ObjectId>)], settings: &AppSettings) {
        if let Some(t) = &mut self.prefetch {
            if t.try_take().is_none() {
                return;
            }
            self.prefetch = None;
        }
        // the selected commit first
        if self.last_key.is_some_and(|(first, second)| cacheable(first, second) && !self.cache.lock().unwrap().files.contains(&(first, second, false))) {
            return;
        }
        let missing: Vec<(Option<ObjectId>, ObjectId)> = {
            let cache = self.cache.lock().unwrap();
            commits
                .iter()
                .filter(|(id, parents)| parents.len() == 1 && cacheable(Some(parents[0]), *id))
                .map(|(id, parents)| (Some(parents[0]), *id))
                .filter(|(first, second)| !cache.files.contains(&(*first, *second, false)))
                .collect()
        };
        if missing.is_empty() {
            return;
        }
        let (m, cache, opts) = (module.clone(), Arc::clone(&self.cache), diff_options(settings));
        self.prefetch = Some(Task::spawn(ctx, move || {
            for (first, second) in missing {
                let _ = load_into_cache(&cache, &m, first, second, false, &opts);
            }
        }));
    }

    /// Shows the diff between `first` (older, `None` = parent) and `second`.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        module: &GitModule,
        first: Option<ObjectId>,
        second: Option<ObjectId>,
        parents: &[ObjectId],
        settings: &AppSettings,
        id: &str,
    ) -> Option<DiffCommand> {
        let palette = Palette::for_ui(ui);
        let Some(second) = second else {
            ui.label(RichText::new("No commit selected").italics().color(palette.muted));
            return None;
        };
        let mut cmd = None;

        // Merge commits: choose the parent to compare with (or the combined diff)
        let mut combined = false;
        let mut first = first;
        if first.is_none() && parents.len() > 1 {
            ui.horizontal(|ui| {
                ui.label("Compare with:");
                egui::ComboBox::from_id_salt((id, "parent")).selected_text(if self.parent_index < parents.len() {
                    format!("Parent {} ({})", self.parent_index + 1, parents[self.parent_index].to_short_string())
                } else {
                    "Combined diff".to_string()
                }).show_ui(ui, |ui| {
                    for (i, p) in parents.iter().enumerate() {
                        ui.selectable_value(&mut self.parent_index, i, format!("Parent {} ({})", i + 1, p.to_short_string()));
                    }
                    ui.selectable_value(&mut self.parent_index, parents.len(), "Combined diff");
                });
            });
            if self.parent_index < parents.len() {
                first = Some(parents[self.parent_index]);
            } else {
                combined = true;
                first = Some(parents[0]);
            }
        } else {
            self.parent_index = 0;
            // the parent is known: saves a git process (rev-parse) per load
            if first.is_none() && parents.len() == 1 {
                first = Some(parents[0]);
            }
        }

        let key = (first, second);
        if self.last_key != Some(key) {
            self.last_key = Some(key);
            self.files_shown = false;
            self.list.clear();
        }

        let files_key = (first, second, combined);
        let use_cache = cacheable(first, second);
        let opts = diff_options(settings);
        let cached = if use_cache { self.cache.lock().unwrap().files.get(&files_key) } else { None };
        let files = match cached {
            Some(files) => Some(Ok(files)),
            None => {
                let m = module.clone();
                let cache = Arc::clone(&self.cache);
                let opts = opts.clone();
                self.files
                    .request(ui.ctx(), files_key, move || {
                        if use_cache {
                            // also loads the diff of the first file, shown right after
                            load_into_cache(&cache, &m, first, second, combined, &opts)
                        } else {
                            load_files(&m, first, second, combined).map(Arc::new)
                        }
                    })
                    .cloned()
            }
        };
        let Some(files) = files else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading changes…");
            });
            return None;
        };
        if !self.files_shown {
            self.files_shown = true;
            gitext_core::exec::log_event(format!("diff tab: files of {} shown", second.to_short_string()));
        }
        let files = match files {
            Ok(f) => f,
            Err(e) => {
                ui.colored_label(palette.error, e);
                return None;
            }
        };
        if self.list.selected.is_empty() && !files.is_empty() {
            self.list.select_first(&files);
        }

        let is_work_tree = second == ObjectId::WORK_TREE;
        let is_index = second == ObjectId::INDEX;
        egui::SidePanel::left(format!("{id}_files")).resizable(true).default_width(300.0).width_range(150.0..=900.0).show_inside(ui, |ui| {
            ui.label(RichText::new(format!("{} changed file{}", files.len(), if files.len() == 1 { "" } else { "s" })).small().color(palette.muted));
            self.list.toolbar(ui);
            let resp = self.list.ui(ui, &format!("{id}_list"), &files, |ui, sel| {
                let names: Vec<String> = sel.iter().filter_map(|&i| files.get(i)).map(|f| f.name.clone()).collect();
                let Some(first_name) = names.first().cloned() else { return };
                if is_work_tree {
                    if ui.button("Stage").clicked() {
                        cmd = Some(DiffCommand::Stage(names.clone()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if ui.button("Reset file changes…").clicked() {
                        cmd = Some(DiffCommand::ResetWorkFiles(names.clone()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if ui.button("Add to .gitignore…").clicked() {
                        cmd = Some(DiffCommand::AddToGitIgnore(names.clone()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                } else if is_index && ui.button("Unstage").clicked() {
                    cmd = Some(DiffCommand::Unstage(names.clone()));
                    ui.close_kind(egui::UiKind::Menu);
                }
                if !second.is_artificial() {
                    if ui.button("Reset file(s) to this revision…").clicked() {
                        cmd = Some(DiffCommand::ResetFileTo { rev: second, files: names.clone() });
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if let Some(f) = first.filter(|f| !f.is_artificial()) {
                        if ui.button("Reset file(s) to parent revision…").clicked() {
                            cmd = Some(DiffCommand::ResetFileTo { rev: f, files: names.clone() });
                            ui.close_kind(egui::UiKind::Menu);
                        }
                    }
                }
                ui.separator();
                if ui.button("Open working directory file").clicked() {
                    cmd = Some(DiffCommand::OpenWorkFile(first_name.clone()));
                    ui.close_kind(egui::UiKind::Menu);
                }
                if !second.is_artificial() && ui.button("Open this revision (temp file)").clicked() {
                    cmd = Some(DiffCommand::OpenRevisionFile { rev: second, file: first_name.clone() });
                    ui.close_kind(egui::UiKind::Menu);
                }
                if !second.is_artificial() && ui.button("Save as…").clicked() {
                    cmd = Some(DiffCommand::SaveAs { rev: second, file: first_name.clone() });
                    ui.close_kind(egui::UiKind::Menu);
                }
                if ui.button("Open with external difftool").clicked() {
                    cmd = Some(DiffCommand::ExternalDiff { first, second, file: first_name.clone() });
                    ui.close_kind(egui::UiKind::Menu);
                }
                ui.separator();
                if ui.button("Blame").clicked() {
                    cmd = Some(DiffCommand::Blame { file: first_name.clone(), rev: second });
                    ui.close_kind(egui::UiKind::Menu);
                }
                if ui.button("File history").clicked() {
                    cmd = Some(DiffCommand::FileHistory(first_name.clone()));
                    ui.close_kind(egui::UiKind::Menu);
                }
                if !second.is_artificial() && ui.button("Show in file tree").clicked() {
                    cmd = Some(DiffCommand::ShowInFileTree { rev: second, file: first_name.clone() });
                    ui.close_kind(egui::UiKind::Menu);
                }
                if ui.button("Filter commits by this path").clicked() {
                    cmd = Some(DiffCommand::FilterPath(first_name.clone()));
                    ui.close_kind(egui::UiKind::Menu);
                }
                ui.separator();
                if ui.button("Copy path(s)").clicked() {
                    cmd = Some(DiffCommand::CopyPaths(names.clone()));
                    ui.close_kind(egui::UiKind::Menu);
                }
                if ui.button("Open containing folder").clicked() {
                    cmd = Some(DiffCommand::OpenContainingFolder(first_name.clone()));
                    ui.close_kind(egui::UiKind::Menu);
                }
            });
            if resp.double_clicked.is_some() {
                if let Some(f) = files.get(resp.double_clicked.unwrap()) {
                    cmd = Some(if is_work_tree { DiffCommand::OpenWorkFile(f.name.clone()) } else { DiffCommand::Blame { file: f.name.clone(), rev: second } });
                }
            }
        });

        // Diff of the selected file
        let selected = self.list.selected.last().copied().filter(|&i| i < files.len());
        let selected_file = selected.map(|i| &files[i]);
        let content: Content = match selected {
            None => Arc::new(ViewerContent::Empty(if files.is_empty() { "No changes".into() } else { "Select a file".into() })),
            Some(index) => match &self.shown {
                Some((k, content)) if k.matches(&files, index, first, second, combined, &opts) => Arc::clone(content),
                _ => {
                    let f = &files[index];
                    let dk = diff_key(first, second, f, &opts, combined);
                    let cached = if use_cache { self.cache.lock().unwrap().diffs.get(&dk) } else { None };
                    let content = match cached {
                        Some(content) => Some(content),
                        None => {
                            let m = module.clone();
                            let f2 = f.clone();
                            let cache = Arc::clone(&self.cache);
                            let key = dk.clone();
                            let opts = opts.clone();
                            self.diff
                                .request(ui.ctx(), dk, move || {
                                    let content = Arc::new(load_diff(&m, first, second, combined, &f2, &opts));
                                    if use_cache {
                                        cache.lock().unwrap().diffs.insert(key, Arc::clone(&content));
                                    }
                                    content
                                })
                                .cloned()
                        }
                    };
                    match content {
                        Some(content) => {
                            let key = ShownKey { files: Arc::clone(&files), index, first, second, combined, opts: opts.clone() };
                            gitext_core::exec::log_event(format!("diff tab: diff of {} shown", f.name));
                            self.shown = Some((key, Arc::clone(&content)));
                            content
                        }
                        None => Arc::new(ViewerContent::Empty("Loading…".into())),
                    }
                }
            },
        };
        ui.horizontal(|ui| {
            if let Some(f) = selected_file {
                ui.label(RichText::new(&f.name).strong());
                if let Some(p) = &f.rename_copy_percentage {
                    ui.label(RichText::new(format!("({}% similar to {})", p, f.old_name.as_deref().unwrap_or_default())).small().color(palette.muted));
                }
            }
        });
        let menu: Vec<ViewerCommand> = if is_work_tree {
            vec![ViewerCommand::StageSelectedLines, ViewerCommand::ResetSelectedLines, ViewerCommand::CopyPatch]
        } else if is_index {
            vec![ViewerCommand::UnstageSelectedLines, ViewerCommand::CopyPatch]
        } else {
            vec![ViewerCommand::CopyPatch]
        };
        if let Some(c) = self.viewer.ui(ui, &content, settings.show_line_numbers, &menu) {
            if let ViewerContent::Diff(d) = &*content {
                let sel = self.viewer.selected_lines();
                match c {
                    ViewerCommand::StageSelectedLines => {
                        if let Some(p) = gitext_core::patch::create_partial_patch(d, &sel, false) {
                            cmd = Some(DiffCommand::StagePatch { patch: p, reverse: false });
                        }
                    }
                    ViewerCommand::UnstageSelectedLines => {
                        if let Some(p) = gitext_core::patch::create_partial_patch(d, &sel, true) {
                            cmd = Some(DiffCommand::StagePatch { patch: p, reverse: true });
                        }
                    }
                    ViewerCommand::ResetSelectedLines => {
                        if let Some(p) = gitext_core::patch::create_partial_patch(d, &sel, true) {
                            cmd = Some(DiffCommand::ResetPatch(p));
                        }
                    }
                    ViewerCommand::CopyPatch => {}
                }
            }
        }
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::Instant;

    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let out = Command::new("git").current_dir(dir).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Time and git processes until a commit's files and first diff are loaded, against loading
    /// the files then the diff as before. Run in a repository with history:
    /// `GITEXT_BENCH_REPO=<path> cargo test --release -p gitext-app load_latency -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn load_latency() {
        let Some(repo) = std::env::var_os("GITEXT_BENCH_REPO") else { return };
        let module = GitModule::open(repo).unwrap();
        let opts = diff_options(&AppSettings::default());
        let out = module.git().output(&gitext_core::GitArgs::new("rev-list").args(["--no-merges", "--max-count=8", "--skip=2000", "--parents", "HEAD"])).unwrap();
        let count = || gitext_core::exec::command_log_entries().len();
        for line in out.lines() {
            let ids: Vec<ObjectId> = line.split(' ').filter_map(ObjectId::try_parse).collect();
            let (second, first) = (ids[0], Some(ids[1]));
            let (n, t) = (count(), Instant::now());
            let files = load_files(&module, first, second, false).unwrap();
            let _ = load_diff(&module, first, second, false, &files[0], &opts);
            let (before, before_n) = (t.elapsed(), count() - n);
            let cache = SharedCache::default();
            let (n, t) = (count(), Instant::now());
            let files = load_into_cache(&cache, &module, first, second, false, &opts).unwrap();
            let (after, after_n) = (t.elapsed(), count() - n);
            let cached = cache.lock().unwrap().diffs.entries.len();
            eprintln!(
                "{}: {} files | before {:.1} ms, {} git | after {:.1} ms, {} git, {} diffs cached",
                second.to_short_string(),
                files.len(),
                before.as_secs_f64() * 1000.0,
                before_n,
                after.as_secs_f64() * 1000.0,
                after_n,
                cached
            );
        }
    }

    /// Per-frame cost of showing an already loaded diff. Run with
    /// `cargo test --release -p gitext-app frame_cost -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn frame_cost() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        git(d, &["init", "-q"]);
        git(d, &["config", "user.email", "a@b"]);
        git(d, &["config", "user.name", "a"]);
        std::fs::write(d.join("a.txt"), "x\n").unwrap();
        git(d, &["add", "."]);
        git(d, &["commit", "-qm", "base"]);
        // one large file shown first, and many small ones in the list
        // in subdirectories, so that the tree mode has folders
        std::fs::create_dir_all(d.join("src/a")).unwrap();
        let big: String = (0..20_000).map(|i| format!("line {i} with some text to make it a typical source code line length\n")).collect();
        std::fs::write(d.join("000_big.txt"), big).unwrap();
        let files: usize = std::env::var("BENCH_FILES").ok().and_then(|v| v.parse().ok()).unwrap_or(2_000);
        for i in 0..files {
            std::fs::write(d.join(format!("src/{}f{i:04}.txt", if i % 2 == 0 { "a/" } else { "" })), format!("{i}\n")).unwrap();
        }
        git(d, &["add", "."]);
        git(d, &["commit", "-qm", "big"]);
        let parent: ObjectId = git(d, &["rev-parse", "HEAD~1"]).parse().unwrap();
        let head: ObjectId = git(d, &["rev-parse", "HEAD"]).parse().unwrap();

        let module = GitModule::open(d).unwrap();
        let settings = AppSettings::default();
        let ctx = egui::Context::default();
        let mut view = RevisionDiffView::default();
        view.list.tree_mode = std::env::var("BENCH_TREE").is_ok_and(|v| !v.is_empty());
        let input = || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
        let frame = |view: &mut RevisionDiffView| {
            let _ = ctx.run(input(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    view.ui(ui, &module, None, Some(head), &[parent], &settings, "bench");
                });
            });
        };
        let start = Instant::now();
        while view.viewer.line_count() < 20_000 {
            frame(&mut view);
            assert!(start.elapsed().as_secs() < 30, "diff not loaded");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        for _ in 0..20 {
            frame(&mut view);
        }
        let n = 300;
        let t = Instant::now();
        for _ in 0..n {
            frame(&mut view);
        }
        eprintln!("frame_cost: {:.3} ms per frame ({} lines, {} files)", t.elapsed().as_secs_f64() * 1000.0 / n as f64, view.viewer.line_count(), files + 1);
    }
}
