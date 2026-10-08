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
    files: Recent<FilesKey, Vec<GitItemStatus>>,
    diffs: Recent<String, ViewerContent>,
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
        Err(e) => ViewerContent::Empty(e.to_string()),
    }
}

/// Loads the files of a commit and the diff of its first file (the one shown when the commit is
/// selected) into the cache. Returns the files.
fn load_into_cache(cache: &SharedCache, m: &GitModule, first: Option<ObjectId>, second: ObjectId, combined: bool, opts: &DiffOptions) -> Result<Vec<GitItemStatus>, String> {
    let files = load_files(m, first, second, combined)?;
    if let Some(f) = files.first() {
        let key = diff_key(first, second, f, opts, combined);
        if !cache.lock().unwrap().diffs.contains(&key) {
            let content = load_diff(m, first, second, combined, f, opts);
            cache.lock().unwrap().diffs.insert(key, content);
        }
    }
    cache.lock().unwrap().files.insert((first, second, combined), files.clone());
    Ok(files)
}

#[derive(Default)]
pub struct RevisionDiffView {
    files: Loader<FilesKey, Result<Vec<GitItemStatus>, String>>,
    pub list: FileList,
    diff: Loader<String, ViewerContent>,
    pub viewer: DiffViewer,
    pub parent_index: usize,
    last_key: Option<(Option<ObjectId>, ObjectId)>,
    cache: SharedCache,
    prefetch: Option<Task<()>>,
}

impl RevisionDiffView {
    pub fn invalidate(&mut self) {
        self.files.invalidate();
        self.diff.invalidate();
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
                            load_files(&m, first, second, combined)
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
        let selected_file = self.list.selected.last().and_then(|&i| files.get(i)).cloned();
        let content = match &selected_file {
            None => ViewerContent::Empty(if files.is_empty() { "No changes".into() } else { "Select a file".into() }),
            Some(f) => {
                let dk = diff_key(first, second, f, &opts, combined);
                let cached = if use_cache { self.cache.lock().unwrap().diffs.get(&dk) } else { None };
                let content = match cached {
                    Some(content) => Some(content),
                    None => {
                        let m = module.clone();
                        let f2 = f.clone();
                        let cache = Arc::clone(&self.cache);
                        let key = dk.clone();
                        self.diff
                            .request(ui.ctx(), dk, move || {
                                let content = load_diff(&m, first, second, combined, &f2, &opts);
                                if use_cache {
                                    cache.lock().unwrap().diffs.insert(key, content.clone());
                                }
                                content
                            })
                            .cloned()
                    }
                };
                content.unwrap_or(ViewerContent::Empty("Loading…".into()))
            }
        };
        ui.horizontal(|ui| {
            if let Some(f) = &selected_file {
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
            if let ViewerContent::Diff(d) = &content {
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
