//! Port of `RevisionDiffControl`: changed files between revisions and their diff.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use egui::{RichText, Ui};
use gitext_core::module::DiffOptions;
use gitext_core::settings::{AppSettings, DiffAppearance};
use gitext_core::status::GitItemStatus;
use gitext_core::{GitArgs, GitModule, ObjectId};

use super::diff_viewer::{content_from_bytes, DiffViewer, ViewerCommand, ViewerContent};
use super::file_actions;
use super::file_list::FileList;
use crate::i18n::{tr, trf};
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
    AddToGitIgnore(Vec<String>),
    StagePatch { patch: String, reverse: bool },
    ResetPatch(String),
    /// The viewer toolbar changed the diff options in the settings.
    SaveSettings,
    /// A file action of the file list context menu.
    File(file_actions::FileAction),
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

/// A context menu entry showing its key.
fn menu_item(ui: &mut Ui, label: &str, key: &str) -> bool {
    ui.add(egui::Button::new(label).shortcut_text(key)).clicked()
}

fn cacheable(first: Option<ObjectId>, second: ObjectId) -> bool {
    !second.is_artificial() && first.is_none_or(|f| !f.is_artificial())
}

pub fn diff_options(settings: &AppSettings) -> DiffOptions {
    DiffOptions {
        ignore_whitespace: settings.ignore_whitespace,
        ignore_whitespace_changes: settings.ignore_whitespace_changes,
        ignore_whitespace_at_eol: settings.ignore_whitespace_at_eol,
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

/// The revision arguments of a diff between `first` (`None`: the parent, the empty tree for
/// a first commit) and `second`, the work tree and the index included.
fn diff_revs(first: Option<ObjectId>, second: ObjectId) -> Vec<String> {
    if second == ObjectId::WORK_TREE {
        // against the index: no revision
        first.filter(|f| !f.is_artificial()).map(|f| vec![f.to_string()]).unwrap_or_default()
    } else if second == ObjectId::INDEX {
        std::iter::once("--cached".to_string()).chain(first.filter(|f| !f.is_artificial()).map(|f| f.to_string())).collect()
    } else {
        vec![first.map_or_else(|| gitext_core::module::EMPTY_TREE_ID.to_string(), |f| f.to_string()), second.to_string()]
    }
}

/// The file arguments of a diff (the old name of a renamed file too).
fn diff_paths(f: &GitItemStatus) -> Vec<String> {
    std::iter::once(f.name.clone()).chain(f.old_name.clone().filter(|o| *o != f.name)).collect()
}

/// git's coloured output as viewer content, or `empty` when there is none.
fn colored(result: gitext_core::GitResult<gitext_core::exec::ExecResult>, empty: &str) -> ViewerContent {
    match result {
        Ok(r) if !r.stdout.is_empty() => ViewerContent::Colored(r.stdout_str()),
        Ok(r) if !r.success() => ViewerContent::Empty(r.all_output().trim().to_string()),
        Ok(_) => ViewerContent::Empty(empty.to_string()),
        Err(e) => ViewerContent::Empty(e.to_string()),
    }
}

/// The word diff of a file (`--word-diff=color`); `None` when git shows nothing (an untracked
/// file), so that the patch is shown instead.
fn load_word_diff(m: &GitModule, first: Option<ObjectId>, second: ObjectId, f: &GitItemStatus, opts: &DiffOptions) -> Option<ViewerContent> {
    // changed words as a background, like upstream's default (reverse colouring)
    let configs = [gitext_core::commands::DIFF_CONFIGS.as_slice(), &[("color.diff.old", "red reverse"), ("color.diff.new", "green reverse")]].concat();
    let args = GitArgs::with_config(&configs, "diff")
        .args(["--no-ext-diff", "--find-renames", "--find-copies", "--color=always", "--word-diff=color"])
        // upstream's minimal words (diff.wordregex): punctuation is a word of its own
        .arg("--word-diff-regex=[A-Za-z0-9_]+|[^[:space:]]")
        .args(opts.extra_args())
        .args(diff_revs(first, second))
        .arg("--")
        .args(diff_paths(f));
    match m.run(&args) {
        Ok(r) if r.stdout.is_empty() && r.success() => None,
        r => Some(colored(r, tr("No differences"))),
    }
}

/// The difftastic view of a file: `git difftool --tool=difftastic`, set up by the user as for
/// upstream (`difftool.difftastic.cmd`).
fn load_difftastic(m: &GitModule, first: Option<ObjectId>, second: ObjectId, f: &GitItemStatus, settings: &DifftasticSettings) -> ViewerContent {
    let configured = m.run(&GitArgs::new("config").args(["--get", "difftool.difftastic.cmd"])).is_ok_and(|r| !r.stdout_str().trim().is_empty());
    if !configured {
        return ViewerContent::Empty(tr("Difftastic is not set up as a git difftool. Install difftastic and set difftool.difftastic.cmd (difft \"$LOCAL\" \"$REMOTE\") in your git config.").into());
    }
    let mut exe = m.git();
    exe.env = [
        ("DFT_COLOR", "always".to_string()),
        ("DFT_BACKGROUND", if settings.dark { "dark" } else { "light" }.to_string()),
        ("DFT_SYNTAX_HIGHLIGHT", if settings.syntax { "on" } else { "off" }.to_string()),
        ("DFT_CONTEXT", settings.context.to_string()),
        ("DFT_STRIP_CR", if settings.ignore_whitespace { "on" } else { "off" }.to_string()),
        ("DFT_WIDTH", settings.width.to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let args = GitArgs::new("difftool").args(["--find-renames", "--find-copies", "-y", "--tool=difftastic"]).args(diff_revs(first, second)).arg("--").args(diff_paths(f));
    colored(exe.run(&args), tr("No differences"))
}

/// What the difftastic view depends on, besides the revisions and the file.
#[derive(Debug, Clone, PartialEq)]
struct DifftasticSettings {
    dark: bool,
    syntax: bool,
    context: u32,
    ignore_whitespace: bool,
    /// The viewer width in characters (`DFT_WIDTH`, both sides).
    width: usize,
}

/// `git range-diff first...second`: how the commits of two branches differ.
fn load_range_diff(m: &GitModule, first: ObjectId, second: ObjectId, opts: &DiffOptions) -> ViewerContent {
    let rev = |id: ObjectId| if id.is_artificial() { "HEAD".to_string() } else { id.to_string() };
    let args = GitArgs::with_config(&gitext_core::commands::DIFF_CONFIGS, "range-diff")
        .args(["--find-renames", "--find-copies", "--color=always"])
        .args(opts.extra_args())
        .arg(format!("{}...{}", rev(first), rev(second)));
    colored(m.run(&args), tr("The commits are the same"))
}

/// The options of "find in commit files using git grep".
#[derive(Debug, Clone, PartialEq, Eq)]
struct GrepQuery {
    text: String,
    ignore_case: bool,
    whole_word: bool,
}

impl GrepQuery {
    fn args(&self, mut a: GitArgs) -> GitArgs {
        if self.ignore_case {
            a.add("--ignore-case");
        }
        if self.whole_word {
            a.add("--word-regexp");
        }
        a.add("-e");
        a.add(self.text.clone());
        a
    }
}

/// The revision argument of git grep: none for the work tree.
fn grep_rev(second: ObjectId) -> Option<String> {
    if second == ObjectId::WORK_TREE {
        None
    } else if second == ObjectId::INDEX {
        Some("--cached".into())
    } else {
        Some(second.to_string())
    }
}

/// The files of `second` containing the text (`GetGrepFiles`).
fn grep_files(m: &GitModule, second: ObjectId, q: &GrepQuery) -> Result<Vec<String>, String> {
    let rev = grep_rev(second);
    let args = q.args(GitArgs::new("grep").args(["--files-with-matches", "-z"])).args(rev.clone()).arg("--");
    let r = m.run(&args).map_err(|e| e.to_string())?;
    // exit code 1: no match
    if !r.success() && !r.stderr.is_empty() {
        return Err(r.stderr_str().trim().to_string());
    }
    let commit = rev.is_some_and(|r| r != "--cached");
    Ok(r.stdout_str()
        .split('\0')
        .filter(|s| !s.is_empty())
        // a commit's files come as `<commit>:<path>`
        .map(|s| if commit { s.split_once(':').map_or(s, |(_, p)| p) } else { s }.to_string())
        .collect())
}

/// The matching lines of a file with their line numbers (`GetGrepFile`).
fn grep_file(m: &GitModule, second: ObjectId, file: &str, q: &GrepQuery, context: u32) -> ViewerContent {
    let configs = [
        ("color.grep.linenumber", ""),
        ("color.grep.separator", ""),
        ("color.grep.function", "dim"),
        ("color.grep.matchSelected", "yellow reverse"),
        ("color.grep.matchContext", "yellow reverse"),
    ];
    let args = q
        .args(GitArgs::with_config(&configs, "grep").args(["--line-number", "--show-function", "--color=always", "-h"]).arg(format!("--context={context}")))
        .args(grep_rev(second))
        .arg("--")
        .arg(file);
    colored(m.run(&args), tr("No matches"))
}

/// What to show for the diff of `f`.
fn diff_content(m: &GitModule, second: ObjectId, f: &GitItemStatus, result: Result<String, String>) -> ViewerContent {
    if f.is_submodule {
        return ViewerContent::Diff(result.unwrap_or_default());
    }
    match result {
        Ok(d) if d.contains("Binary files") && !d.contains("\n@@") => ViewerContent::Binary(trf("Binary file {0} changed", &[&f.name])),
        Ok(d) if d.trim().is_empty() => {
            if f.is_new && second == ObjectId::WORK_TREE {
                m.get_file_bytes(second, &f.name).map(|b| content_from_bytes(&b)).unwrap_or(ViewerContent::Empty(tr("Empty file").into()))
            } else {
                ViewerContent::Empty(tr("No differences (possibly only whitespace or file mode changes)").into())
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
    /// "Find in commit files using git grep".
    grep: GrepState,
    /// The range-diff of the two selected commits is shown instead of a file.
    range_diff: bool,
}

#[derive(Default)]
struct GrepState {
    open: bool,
    focus: bool,
    text: String,
    files: Loader<(ObjectId, GrepQuery), Result<Vec<String>, String>>,
    /// The file whose matches are shown instead of a diff.
    selected: Option<String>,
}

impl RevisionDiffView {
    pub fn invalidate(&mut self) {
        self.files.invalidate();
        self.diff.invalidate();
        self.grep.files.invalidate();
        self.shown = None;
    }

    /// Opens "find in commit files using git grep" with the text selected in the viewer
    /// (Ctrl+Shift+F, `FindInCommitFilesUsingGitGrep`).
    pub fn open_grep(&mut self) {
        self.grep.open = true;
        self.grep.focus = true;
        let selected = self.viewer.selected_text();
        if !selected.is_empty() && !selected.contains('\n') {
            self.grep.text = selected.trim().to_string();
        }
    }

    /// The search box and the files it found; returns true when the options changed.
    fn grep_ui(&mut self, ui: &mut Ui, module: &GitModule, second: ObjectId, settings: &mut AppSettings, palette: &Palette) -> bool {
        let mut changed = false;
        // right to left: the box takes the room the buttons leave, never widening the panel
        let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("✖").on_hover_text(tr("Close (Esc)")).clicked() {
                self.grep.open = false;
            }
            if ui.selectable_label(settings.grep_match_whole_word, "W").on_hover_text(tr("Match whole word")).clicked() {
                settings.grep_match_whole_word = !settings.grep_match_whole_word;
                changed = true;
            }
            if ui.selectable_label(!settings.grep_ignore_case, "Aa").on_hover_text(tr("Match case")).clicked() {
                settings.grep_ignore_case = !settings.grep_ignore_case;
                changed = true;
            }
            let r = ui.add(egui::TextEdit::singleline(&mut self.grep.text).hint_text(tr("Find in commit files (git grep)")).desired_width(ui.available_width()));
            if std::mem::take(&mut self.grep.focus) {
                r.request_focus();
            }
            if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.grep.open = false;
            }
        });
        if !self.grep.open {
            self.grep.selected = None;
            return changed;
        }
        let query = GrepQuery { text: self.grep.text.clone(), ignore_case: settings.grep_ignore_case, whole_word: settings.grep_match_whole_word };
        if query.text.is_empty() {
            self.grep.selected = None;
            return changed;
        }
        let m = module.clone();
        let q = query.clone();
        // one search at a time, the latest text next: each search is a git process
        let found = self.grep.files.request_latest(ui.ctx(), (second, query), move || grep_files(&m, second, &q)).cloned();
        match found {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(tr("Searching…")).small().color(palette.muted));
                });
            }
            Some(Err(e)) => {
                ui.colored_label(palette.error, e);
            }
            Some(Ok(files)) => {
                ui.label(RichText::new(if files.len() == 1 { trf("{0} file found", &[&files.len()]) } else { trf("{0} files found", &[&files.len()]) }).small().color(palette.muted));
                egui::ScrollArea::vertical().id_salt("grep_files").max_height(ui.available_height() * 0.4).auto_shrink([false, true]).show(ui, |ui| {
                    for f in &files {
                        let selected = self.grep.selected.as_deref() == Some(f.as_str());
                        if ui.selectable_label(selected, f).clicked() {
                            self.grep.selected = Some(f.clone());
                            self.range_diff = false;
                        }
                    }
                });
                if self.grep.selected.as_ref().is_some_and(|s| !files.contains(s)) {
                    self.grep.selected = None;
                }
            }
        }
        ui.separator();
        changed
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
            // at the same time: the batch takes as long as one git process instead of the sum
            std::thread::scope(|s| {
                for (first, second) in missing {
                    let (cache, m, opts) = (&cache, &m, &opts);
                    s.spawn(move || load_into_cache(cache, m, first, second, false, opts));
                }
            });
        }));
    }

    /// What replaces the patch of the selected file: the range-diff of two commits, the matches
    /// of a git grep, or a word diff / difftastic view. Loaded like the diffs, by key.
    #[allow(clippy::too_many_arguments)]
    fn special_content(
        &mut self,
        ui: &Ui,
        module: &GitModule,
        range_from: Option<ObjectId>,
        second: ObjectId,
        file: Option<&GitItemStatus>,
        first: Option<ObjectId>,
        combined: bool,
        opts: &DiffOptions,
        settings: &AppSettings,
    ) -> Option<Content> {
        let m = module.clone();
        let o = opts.clone();
        let (key, load): (String, Box<dyn FnOnce() -> ViewerContent + Send>) = if let Some(from) = range_from {
            (format!("range|{from}|{second}|{opts:?}"), Box::new(move || load_range_diff(&m, from, second, &o)))
        } else if let Some(f) = self.grep.selected.clone().filter(|_| self.grep.open) {
            let q = GrepQuery { text: self.grep.text.clone(), ignore_case: settings.grep_ignore_case, whole_word: settings.grep_match_whole_word };
            let context = if settings.show_entire_file { 100_000 } else { settings.context_lines };
            (format!("grep|{second}|{f}|{q:?}|{context}"), Box::new(move || grep_file(&m, second, &f, &q, context)))
        } else {
            let f = file.filter(|f| !combined && !f.is_submodule)?.clone();
            match settings.diff_appearance {
                DiffAppearance::Patch => return None,
                DiffAppearance::WordDiff => {
                    let key = format!("word|{}", diff_key(first, second, &f, opts, combined));
                    (key, Box::new(move || load_word_diff(&m, first, second, &f, &o).unwrap_or_else(|| load_diff(&m, first, second, false, &f, &o))))
                }
                DiffAppearance::Difftastic => {
                    // (the font size first: `fonts` holds the context lock)
                    let font = egui::FontId::monospace(ui.text_style_height(&egui::TextStyle::Monospace));
                    let char_w = ui.fonts(|fonts| fonts.glyph_width(&font, 'M'));
                    // both sides, as upstream: 88 to 200 characters, even
                    let width = ((ui.available_width() / char_w) as usize).saturating_sub(12).clamp(88, 200) / 2 * 2;
                    let dt = DifftasticSettings {
                        dark: ui.visuals().dark_mode,
                        syntax: settings.syntax_highlighting,
                        context: if settings.show_entire_file { 9000 } else { settings.context_lines },
                        ignore_whitespace: settings.ignore_whitespace || settings.ignore_whitespace_changes || settings.ignore_whitespace_at_eol,
                        width,
                    };
                    let key = format!("difftastic|{}|{dt:?}", diff_key(first, second, &f, opts, combined));
                    (key, Box::new(move || load_difftastic(&m, first, second, &f, &dt)))
                }
            }
        };
        let content = self.diff.request(ui.ctx(), key, move || Arc::new(load())).cloned();
        Some(content.unwrap_or_else(|| Arc::new(ViewerContent::Empty(tr("Loading…").into()))))
    }

    /// Shows the diff between `first` (older, `None` = parent) and `second`.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        module: &GitModule,
        first: Option<ObjectId>,
        second: Option<ObjectId>,
        parents: &[ObjectId],
        settings: &mut AppSettings,
        id: &str,
    ) -> Option<DiffCommand> {
        let palette = Palette::for_ui(ui);
        let Some(second) = second else {
            ui.label(RichText::new(tr("No commit selected")).italics().color(palette.muted));
            return None;
        };
        let mut cmd = None;
        // two commits are selected: their range-diff can be shown
        let selected_first = first.filter(|f| !f.is_artificial() && !second.is_artificial());

        // Merge commits: choose the parent to compare with (or the combined diff)
        let mut combined = false;
        let mut first = first;
        if first.is_none() && parents.len() > 1 {
            ui.horizontal(|ui| {
                ui.label(tr("Compare with:"));
                egui::ComboBox::from_id_salt((id, "parent")).selected_text(if self.parent_index < parents.len() {
                    trf("Parent {0} ({1})", &[&(self.parent_index + 1), &parents[self.parent_index].to_short_string()])
                } else {
                    tr("Combined diff").to_string()
                }).show_ui(ui, |ui| {
                    for (i, p) in parents.iter().enumerate() {
                        ui.selectable_value(&mut self.parent_index, i, trf("Parent {0} ({1})", &[&(i + 1), &p.to_short_string()]));
                    }
                    ui.selectable_value(&mut self.parent_index, parents.len(), tr("Combined diff"));
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
                ui.label(tr("Loading changes…"));
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
        let mut list_focus = false;
        egui::SidePanel::left(format!("{id}_files")).resizable(true).default_width(300.0).width_range(150.0..=900.0).show_inside(ui, |ui| {
            ui.label(RichText::new(if files.len() == 1 { trf("{0} changed file", &[&files.len()]) } else { trf("{0} changed files", &[&files.len()]) }).small().color(palette.muted));
            self.list.toolbar(ui);
            ui.horizontal(|ui| {
                if ui.selectable_label(self.grep.open, RichText::new(format!("🔍 {}", tr("Find in files"))).small()).on_hover_text(format!("{} (Ctrl+Shift+F)", tr("Find in commit files using git grep"))).clicked() {
                    if self.grep.open {
                        self.grep.open = false;
                        self.grep.selected = None;
                    } else {
                        self.open_grep();
                    }
                }
                if let Some(f) = selected_first {
                    let hover = format!("{}: git range-diff {}...{}", tr("Compare the commits of both branches"), f.to_short_string(), second.to_short_string());
                    if ui.selectable_label(self.range_diff, RichText::new(tr("Range diff")).small()).on_hover_text(hover).clicked() {
                        self.range_diff = !self.range_diff;
                        self.grep.selected = None;
                    }
                }
            });
            if self.grep.open && self.grep_ui(ui, module, second, settings, &palette) {
                cmd = Some(DiffCommand::SaveSettings);
            }
            let tools = if settings.show_available_diff_tools { file_actions::available_tools(module, ui.ctx()) } else { Vec::new() };
            let resp = self.list.ui(ui, &format!("{id}_list"), &files, |ui, sel| {
                let names: Vec<String> = sel.iter().filter_map(|&i| files.get(i)).map(|f| f.name.clone()).collect();
                let Some(first_name) = names.first().cloned() else { return };
                if is_work_tree {
                    if menu_item(ui, tr("Stage"), "S") {
                        cmd = Some(DiffCommand::Stage(names.clone()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if menu_item(ui, tr("Reset file changes…"), "R") {
                        cmd = Some(DiffCommand::ResetWorkFiles(names.clone()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if ui.button(tr("Add to .gitignore…")).clicked() {
                        cmd = Some(DiffCommand::AddToGitIgnore(names.clone()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                } else if is_index && menu_item(ui, tr("Unstage"), "U") {
                    cmd = Some(DiffCommand::Unstage(names.clone()));
                    ui.close_kind(egui::UiKind::Menu);
                }
                if !second.is_artificial() {
                    if menu_item(ui, tr("Reset file(s) to this revision…"), "R") {
                        cmd = Some(DiffCommand::ResetFileTo { rev: second, files: names.clone() });
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if let Some(f) = first.filter(|f| !f.is_artificial()) {
                        if ui.button(tr("Reset file(s) to parent revision…")).clicked() {
                            cmd = Some(DiffCommand::ResetFileTo { rev: f, files: names.clone() });
                            ui.close_kind(egui::UiKind::Menu);
                        }
                    }
                }
                ui.separator();
                let selection = file_actions::Selection { items: sel.iter().filter_map(|&i| files.get(i)).collect(), first, second };
                if let Some(a) = file_actions::difftool_menu(ui, &selection, parents, &tools) {
                    cmd = Some(DiffCommand::File(a));
                }
                if menu_item(ui, tr("Open working directory file"), "F4") {
                    cmd = Some(DiffCommand::OpenWorkFile(first_name.clone()));
                    ui.close_kind(egui::UiKind::Menu);
                }
                if !second.is_artificial() && menu_item(ui, tr("Open this revision (temp file)"), "Ctrl+F3") {
                    cmd = Some(DiffCommand::OpenRevisionFile { rev: second, file: first_name.clone() });
                    ui.close_kind(egui::UiKind::Menu);
                }
                if !second.is_artificial() && ui.button(tr("Save as…")).clicked() {
                    cmd = Some(DiffCommand::SaveAs { rev: second, file: first_name.clone() });
                    ui.close_kind(egui::UiKind::Menu);
                }
                ui.separator();
                if menu_item(ui, tr("Blame"), "B") {
                    cmd = Some(DiffCommand::Blame { file: first_name.clone(), rev: second });
                    ui.close_kind(egui::UiKind::Menu);
                }
                if menu_item(ui, tr("File history"), "H") {
                    cmd = Some(DiffCommand::FileHistory(first_name.clone()));
                    ui.close_kind(egui::UiKind::Menu);
                }
                if !second.is_artificial() && menu_item(ui, tr("Show in file tree"), "T") {
                    cmd = Some(DiffCommand::ShowInFileTree { rev: second, file: first_name.clone() });
                    ui.close_kind(egui::UiKind::Menu);
                }
                if menu_item(ui, tr("Filter commits by this path"), "F") {
                    cmd = Some(DiffCommand::FilterPath(first_name.clone()));
                    ui.close_kind(egui::UiKind::Menu);
                }
                ui.separator();
                if ui.button(tr("Open containing folder")).clicked() {
                    cmd = Some(DiffCommand::OpenContainingFolder(first_name.clone()));
                    ui.close_kind(egui::UiKind::Menu);
                }
                let selection = file_actions::Selection { items: sel.iter().filter_map(|&i| files.get(i)).collect(), first, second };
                if let Some(a) = file_actions::menu_without_remember(ui, &selection) {
                    cmd = Some(DiffCommand::File(a));
                }
            });
            if matches!(cmd, Some(DiffCommand::File(file_actions::FileAction::HideToolList))) {
                settings.show_available_diff_tools = false;
                cmd = Some(DiffCommand::SaveSettings);
            }
            // the keys of the file list (`RevisionDiffControl` hotkeys)
            let names: Vec<String> = self.list.selected_items(&files).iter().map(|f| f.name.clone()).collect();
            if resp.has_focus && cmd.is_none() {
                if let Some(first_name) = names.first().cloned() {
                    use crate::views::{plain_key, shortcut};
                    use egui::{Key, Modifiers};
                    let ctx = ui.ctx().clone();
                    if is_work_tree && plain_key(&ctx, Key::S) {
                        cmd = Some(DiffCommand::Stage(names.clone()));
                    } else if is_index && plain_key(&ctx, Key::U) {
                        cmd = Some(DiffCommand::Unstage(names.clone()));
                    } else if plain_key(&ctx, Key::R) {
                        if second.is_artificial() {
                            cmd = Some(DiffCommand::ResetWorkFiles(names.clone()));
                        } else {
                            cmd = Some(DiffCommand::ResetFileTo { rev: second, files: names.clone() });
                        }
                    } else if plain_key(&ctx, Key::H) {
                        cmd = Some(DiffCommand::FileHistory(first_name));
                    } else if plain_key(&ctx, Key::B) {
                        cmd = Some(DiffCommand::Blame { file: first_name, rev: second });
                    } else if plain_key(&ctx, Key::F3) && first != Some(ObjectId::COMBINED_DIFF) {
                        let files = self.list.selected_items(&files).iter().filter(|f| !f.is_submodule).map(|f| file_actions::DiffFile { name: f.name.clone(), old_name: f.old_name.clone(), is_tracked: f.is_tracked }).collect();
                        cmd = Some(DiffCommand::File(file_actions::FileAction::Difftool { kind: gitext_core::diff_tools::FileDiffKind::FirstToSecond, first, second, files, tool: None }));
                    } else if plain_key(&ctx, Key::F4) {
                        cmd = Some(DiffCommand::OpenWorkFile(first_name));
                    } else if !second.is_artificial() && shortcut(&ctx, Modifiers::COMMAND, Key::F3) {
                        cmd = Some(DiffCommand::OpenRevisionFile { rev: second, file: first_name });
                    } else if !second.is_artificial() && plain_key(&ctx, Key::T) {
                        cmd = Some(DiffCommand::ShowInFileTree { rev: second, file: first_name });
                    } else if plain_key(&ctx, Key::F) {
                        cmd = Some(DiffCommand::FilterPath(first_name));
                    } else if let Some(a) = file_actions::keys(&ctx, &file_actions::Selection { items: self.list.selected_items(&files), first, second }) {
                        cmd = Some(DiffCommand::File(a));
                    }
                }
                if parents.len() > 1 {
                    if crate::views::shortcut(ui.ctx(), egui::Modifiers::COMMAND, egui::Key::ArrowLeft) {
                        self.parent_index = 0;
                        ui.ctx().request_repaint();
                    } else if crate::views::shortcut(ui.ctx(), egui::Modifiers::COMMAND, egui::Key::ArrowRight) {
                        self.parent_index = parents.len() - 1;
                        ui.ctx().request_repaint();
                    }
                }
            }
            if resp.selection_changed {
                self.grep.selected = None;
                self.range_diff = false;
            }
            list_focus = resp.has_focus;
            if resp.double_clicked.is_some() {
                if let Some(f) = files.get(resp.double_clicked.unwrap()) {
                    cmd = Some(if is_work_tree { DiffCommand::OpenWorkFile(f.name.clone()) } else { DiffCommand::Blame { file: f.name.clone(), rev: second } });
                }
            }
        });

        // Diff of the selected file
        let selected = self.list.selected.last().copied().filter(|&i| i < files.len());
        let selected_file = selected.map(|i| &files[i]);
        let special = self.special_content(ui, module, selected_first.filter(|_| self.range_diff), second, selected_file, first, combined, &opts, settings);
        let content: Content = if let Some(content) = special {
            content
        } else { match selected {
            None => Arc::new(ViewerContent::Empty(if files.is_empty() { tr("No changes").into() } else { tr("Select a file").into() })),
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
                        None => Arc::new(ViewerContent::Empty(tr("Loading…").into())),
                    }
                }
            },
        } };
        ui.horizontal(|ui| {
            if let (true, Some(f)) = (self.range_diff, selected_first) {
                ui.label(RichText::new(format!("{} {}...{}", tr("Range diff"), f.to_short_string(), second.to_short_string())).strong());
            } else if let Some(f) = &self.grep.selected {
                ui.label(RichText::new(f).strong());
                ui.label(RichText::new(trf("matches of \"{0}\"", &[&self.grep.text])).small().color(palette.muted));
            } else if let Some(f) = selected_file {
                ui.label(RichText::new(&f.name).strong());
                if let Some(p) = &f.rename_copy_percentage {
                    ui.label(RichText::new(trf("({0}% similar to {1})", &[p, &f.old_name.as_deref().unwrap_or_default()])).small().color(palette.muted));
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
        let shown_file = if self.range_diff && selected_first.is_some() { None } else { self.grep.selected.as_deref().or(selected_file.map(|f| f.name.as_str())) };
        self.viewer.set_file_name(shown_file);
        self.viewer.appearance_menu = true;
        if self.viewer.toolbar(ui, &content, settings) {
            cmd = Some(DiffCommand::SaveSettings);
        }
        if (list_focus || self.viewer.has_focus(ui.ctx())) && crate::views::shortcut(ui.ctx(), egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::F) {
            self.open_grep();
        }
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
                    ViewerCommand::CopyPatch | ViewerCommand::CopyOldVersion | ViewerCommand::CopyNewVersion | ViewerCommand::AddToCommitMessage => {}
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

    /// A repository with two commits changing `a.rs`; returns the two commit ids.
    fn two_commits(d: &std::path::Path) -> (ObjectId, ObjectId) {
        git(d, &["init", "-q"]);
        git(d, &["config", "user.email", "a@b"]);
        git(d, &["config", "user.name", "a"]);
        std::fs::write(d.join("a.rs"), "fn main() {\n    let total = 1;\n}\n").unwrap();
        git(d, &["add", "."]);
        git(d, &["commit", "-qm", "one"]);
        std::fs::write(d.join("a.rs"), "fn main() {\n    let count = 2;\n}\n").unwrap();
        git(d, &["commit", "-qam", "two"]);
        (git(d, &["rev-parse", "HEAD~1"]).parse().unwrap(), git(d, &["rev-parse", "HEAD"]).parse().unwrap())
    }

    /// Runs frames of the diff tab until the viewer shows content matching `done`.
    fn show_until(view: &mut RevisionDiffView, module: &GitModule, first: Option<ObjectId>, second: ObjectId, parents: &[ObjectId], settings: &mut AppSettings, done: impl Fn(&ViewerContent) -> bool) -> ViewerContent {
        let ctx = egui::Context::default();
        let input = || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
        let start = Instant::now();
        loop {
            let _ = ctx.run(input(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    view.ui(ui, module, first, Some(second), parents, settings, "test");
                });
            });
            if let Some(c) = view.viewer.content().filter(|c| done(c)) {
                return c.clone();
            }
            assert!(start.elapsed().as_secs() < 20, "not shown: {:?}", view.viewer.content());
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn word_diff_range_diff_and_grep_views() {
        let dir = tempfile::tempdir().unwrap();
        let (one, two) = two_commits(dir.path());
        let module = GitModule::open(dir.path()).unwrap();
        let mut settings = AppSettings { diff_appearance: DiffAppearance::WordDiff, ..AppSettings::default() };
        let mut view = RevisionDiffView::default();
        let colored = |c: &ViewerContent| matches!(c, ViewerContent::Colored(_));
        let ViewerContent::Colored(word) = show_until(&mut view, &module, None, two, &[one], &mut settings, colored) else { unreachable!() };
        let plain = gitext_core::patch::strip_ansi(&word);
        assert!(plain.contains("totalcount") && plain.contains("12"), "{plain}");

        // difftastic needs a difftool set up (none in a test repository; the view used to
        // deadlock measuring the font)
        settings.diff_appearance = DiffAppearance::Difftastic;
        let not_set_up = |c: &ViewerContent| matches!(c, ViewerContent::Empty(m) if m.contains("difftool.difftastic.cmd"));
        if git(dir.path(), &["config", "--default", "", "--get", "difftool.difftastic.cmd"]).is_empty() {
            show_until(&mut view, &module, None, two, &[one], &mut settings, not_set_up);
        }
        settings.diff_appearance = DiffAppearance::WordDiff;

        // the range-diff of two selected commits
        view.range_diff = true;
        let ViewerContent::Colored(range) = show_until(&mut view, &module, Some(one), two, &[], &mut settings, colored) else { unreachable!() };
        assert!(gitext_core::patch::strip_ansi(&range).contains("two"), "{range}");

        // git grep: the files containing the text, then its matches in a file
        view.range_diff = false;
        view.grep.open = true;
        view.grep.text = "count".into();
        settings.diff_appearance = DiffAppearance::Patch;
        let start = Instant::now();
        while view.grep.selected.is_none() {
            let ctx = egui::Context::default();
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    view.ui(ui, &module, None, Some(two), &[one], &mut settings, "test");
                });
            });
            if let Some(Ok(files)) = view.grep.files.value.clone() {
                assert_eq!(files, ["a.rs"]);
                view.grep.selected = Some(files[0].clone());
            }
            assert!(start.elapsed().as_secs() < 20, "grep did not finish");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let ViewerContent::Colored(matches) = show_until(&mut view, &module, None, two, &[one], &mut settings, colored) else { unreachable!() };
        assert!(gitext_core::patch::strip_ansi(&matches).contains("2:    let count = 2;"), "{matches}");
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
        let mut settings = AppSettings::default();
        let ctx = egui::Context::default();
        let mut view = RevisionDiffView::default();
        view.list.tree_mode = std::env::var("BENCH_TREE").is_ok_and(|v| !v.is_empty());
        let input = || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
        let mut frame = |view: &mut RevisionDiffView| {
            let _ = ctx.run(input(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    view.ui(ui, &module, None, Some(head), &[parent], &mut settings, "bench");
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
