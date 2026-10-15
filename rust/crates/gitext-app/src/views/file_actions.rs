//! File actions shared by the file lists of the Diff tab, the commit dialog and the stash dialog
//! (port of parts of `FileStatusList.ContextMenu`, `CopyPathsToolStripMenuItem` and
//! `RememberFileContextMenuController`): copy path variants, open with, rename / move, delete,
//! stop tracking, cherry-pick file changes and diff with a remembered file.

use std::sync::Mutex;

use egui::{Key, Modifiers, Ui};
use gitext_core::args::{to_mount_path, to_native_path, to_posix_path};
use gitext_core::status::{GitItemStatus, StagedStatus};
use gitext_core::diff_tools::FileDiffKind;
use gitext_core::{GitArgs, GitModule, ObjectId};

use crate::dialogs::{ok_cancel, Action, Confirm, Cx, Dialog};
use crate::views::{plain_key, shortcut};

/// How "Copy path" writes the paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    FullNative,
    RelativeNative,
    RelativePosix,
    FullWsl,
    FullCygwin,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileAction {
    CopyPaths { files: Vec<String>, kind: PathKind },
    /// Opens the work tree file with the application chosen by the user (Windows "Open with"
    /// dialog; the default application elsewhere).
    OpenWith(String),
    RenameMove(String),
    Delete(Vec<String>),
    StopTracking(Vec<String>),
    CherryPick { first: Option<ObjectId>, second: ObjectId, files: Vec<String> },
    /// Remembers a file version (label, git commitish) to diff another one with.
    Remember { label: String, commitish: String },
    /// Opens the difftool between the remembered file and this commitish.
    DiffWithRemembered(String),
    /// Opens the difftool for each file (`OpenFilesWithDiffTool`), with the configured tool or
    /// the one picked in the dropdown.
    Difftool { kind: FileDiffKind, first: Option<ObjectId>, second: ObjectId, files: Vec<DiffFile>, tool: Option<String> },
    /// Opens the difftool between two file versions given as git commitishes.
    DiffCommitishes { first: String, second: String, tool: Option<String> },
    /// "Disable this dropdown": the difftools are no longer listed (the host saves the setting).
    HideToolList,
}

/// A file of a difftool action.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffFile {
    pub name: String,
    pub old_name: Option<String>,
    pub is_tracked: bool,
}

/// The selection a file menu acts on.
pub struct Selection<'a> {
    pub items: Vec<&'a GitItemStatus>,
    /// The older revision (`None`: the parent of `second`).
    pub first: Option<ObjectId>,
    pub second: ObjectId,
}

impl Selection<'_> {
    fn names(&self) -> Vec<String> {
        self.items.iter().map(|i| i.name.clone()).collect()
    }

    fn is_work_tree(&self) -> bool {
        self.second == ObjectId::WORK_TREE
    }

    fn single(&self) -> Option<&GitItemStatus> {
        match self.items.as_slice() {
            [one] if !one.is_submodule => Some(one),
            _ => None,
        }
    }

    /// The git commitish of the selected file in the second revision (`RememberFileContextMenuController.GetGitCommit`):
    /// `<id>:<path>` for a commit, `:<path>` for the index; none for the work tree, which git
    /// cannot take as the first side of a diff, nor for a deleted file.
    fn commitish(&self) -> Option<String> {
        let item = self.single()?;
        if item.is_deleted || self.is_work_tree() {
            return None;
        }
        let name = to_posix_path(&item.name);
        Some(if self.second == ObjectId::INDEX { format!(":{name}") } else { format!("{}:{name}", self.second) })
    }

    /// The commitish of the selected file to diff with the remembered one (a work tree file
    /// is its path).
    fn second_commitish(&self) -> Option<String> {
        let item = self.single()?;
        if item.is_deleted {
            return None;
        }
        if self.is_work_tree() {
            return Some(to_posix_path(&item.name));
        }
        self.commitish()
    }
}

/// The remembered file (label, commitish), shared by all file lists like upstream's singleton.
static REMEMBERED: Mutex<Option<(String, String)>> = Mutex::new(None);

fn remembered() -> Option<(String, String)> {
    REMEMBERED.lock().ok().and_then(|r| r.clone())
}

fn item(ui: &mut Ui, text: &str, shortcut_text: &str) -> bool {
    let clicked = ui.add(egui::Button::new(text).shortcut_text(shortcut_text)).clicked();
    if clicked {
        ui.close_kind(egui::UiKind::Menu);
    }
    clicked
}

/// The "Copy path" submenu.
pub fn copy_path_menu(ui: &mut Ui, files: &[String]) -> Option<FileAction> {
    let mut action = None;
    ui.menu_button("Copy path", |ui| {
        let kinds = [
            ("Full path", PathKind::FullNative),
            ("Relative path", PathKind::RelativeNative),
            ("Relative path (POSIX)", PathKind::RelativePosix),
            ("Full path (WSL)", PathKind::FullWsl),
            ("Full path (Cygwin)", PathKind::FullCygwin),
        ];
        for (label, kind) in kinds {
            if item(ui, label, "") {
                action = Some(FileAction::CopyPaths { files: files.to_vec(), kind });
            }
        }
    });
    action
}

/// The file actions of the context menu of a file list.
pub fn menu(ui: &mut Ui, sel: &Selection) -> Option<FileAction> {
    menu_impl(ui, sel, true)
}

/// [`menu`] without "Remember file for diff", for lists that have the [`difftool_menu`].
pub fn menu_without_remember(ui: &mut Ui, sel: &Selection) -> Option<FileAction> {
    menu_impl(ui, sel, false)
}

fn menu_impl(ui: &mut Ui, sel: &Selection, remember: bool) -> Option<FileAction> {
    let mut action = None;
    if sel.items.is_empty() {
        return None;
    }
    let names = sel.names();
    let work_tree = sel.is_work_tree();
    let files: Vec<&GitItemStatus> = sel.items.iter().copied().filter(|i| !i.is_submodule).collect();
    ui.separator();
    // as upstream, for any revision: it opens the working directory file
    if let Some(f) = sel.single().filter(|f| !f.is_deleted || !work_tree) {
        if item(ui, if cfg!(windows) { "Open working directory file with…" } else { "Open with default application" }, "Shift+F4") {
            action = Some(FileAction::OpenWith(f.name.clone()));
        }
    }
    if work_tree {
        if let Some(f) = sel.single().filter(|f| !f.is_deleted) {
            if item(ui, "Rename / move…", "F2") {
                action = Some(FileAction::RenameMove(f.name.clone()));
            }
        }
        if !files.is_empty() && item(ui, if files.len() == 1 { "Delete file…" } else { "Delete files…" }, "Del") {
            action = Some(FileAction::Delete(files.iter().map(|f| f.name.clone()).collect()));
        }
    }
    if (work_tree || sel.second == ObjectId::INDEX) && files.iter().any(|f| f.is_tracked && !f.is_new) && item(ui, "Stop tracking", "") {
        action = Some(FileAction::StopTracking(files.iter().filter(|f| f.is_tracked && !f.is_new).map(|f| f.name.clone()).collect()));
    }
    if !sel.second.is_artificial() && !files.is_empty() && item(ui, "Cherry pick changes", "") {
        action = Some(FileAction::CherryPick { first: sel.first, second: sel.second, files: files.iter().map(|f| f.name.clone()).collect() });
    }
    if let Some(a) = copy_path_menu(ui, &names) {
        action = Some(a);
    }
    let commitish = sel.commitish();
    let second = sel.second_commitish();
    if remember && (commitish.is_some() || second.is_some()) {
        ui.menu_button("Remember file for diff", |ui| {
            let remembered = remembered();
            if let Some(c) = &commitish {
                let label = format!("{} @ {}", sel.items[0].name, if sel.second == ObjectId::INDEX { "index".to_string() } else { sel.second.to_short_string() });
                if item(ui, "Remember this file", "") {
                    action = Some(FileAction::Remember { label, commitish: c.clone() });
                }
            }
            match (&remembered, &second) {
                (Some((label, _)), Some(s)) => {
                    if item(ui, &format!("Diff with remembered {label}"), "") {
                        action = Some(FileAction::DiffWithRemembered(s.clone()));
                    }
                }
                _ => {
                    ui.add_enabled(false, egui::Button::new("Diff with remembered file"));
                }
            }
        });
    }
    action
}

/// The keys of the file actions in a file list with the keyboard focus.
pub fn keys(ctx: &egui::Context, sel: &Selection) -> Option<FileAction> {
    if sel.items.is_empty() {
        return None;
    }
    if let Some(f) = sel.single().filter(|f| !f.is_deleted || !sel.is_work_tree()) {
        if shortcut(ctx, Modifiers::SHIFT, Key::F4) {
            return Some(FileAction::OpenWith(f.name.clone()));
        }
    }
    if !sel.is_work_tree() {
        return None;
    }
    let single = sel.single().filter(|f| !f.is_deleted).map(|f| f.name.clone());
    if plain_key(ctx, Key::Delete) {
        let files: Vec<String> = sel.items.iter().filter(|i| !i.is_submodule).map(|i| i.name.clone()).collect();
        return (!files.is_empty()).then_some(FileAction::Delete(files));
    }
    if let Some(name) = single {
        if plain_key(ctx, Key::F2) {
            return Some(FileAction::RenameMove(name));
        }
    }
    None
}

/// The text "Copy path" puts on the clipboard (`CopyPathsToolStripMenuItem.GetFilePaths`).
pub fn path_text(work_dir: &std::path::Path, files: &[String], kind: PathKind) -> String {
    let full = |f: &str| work_dir.join(f).display().to_string();
    files
        .iter()
        .map(|f| match kind {
            PathKind::FullNative => to_native_path(&full(f)),
            PathKind::RelativeNative => to_native_path(f),
            PathKind::RelativePosix => to_posix_path(f),
            PathKind::FullWsl => to_mount_path(&full(f), "/mnt/"),
            PathKind::FullCygwin => to_mount_path(&full(f), "/cygdrive/"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Opens `path` with the application the user chooses (the Windows "Open with" dialog), or
/// the default application where the system has no such dialog.
fn open_with_chooser(path: &std::path::Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("rundll32.exe");
        // rundll32 takes the rest of its command line as the path, unquoted
        cmd.raw_arg(format!("shell32.dll,OpenAs_RunDLL {}", path.display()));
        crate::util::spawn_detached(&mut cmd);
    }
    #[cfg(not(windows))]
    crate::util::open_with_system(&path.display().to_string());
}

/// Runs a file action; refreshes the status after changes.
pub fn run(action: FileAction, m: &GitModule, actions: &mut Vec<Action>) {
    let error = |actions: &mut Vec<Action>, title: &str, e: String| actions.push(Action::Message { title: title.into(), text: e, error: true });
    match action {
        FileAction::CopyPaths { files, kind } => actions.push(Action::Copy(path_text(m.work_dir(), &files, kind))),
        FileAction::OpenWith(f) => {
            let path = m.work_dir().join(&f);
            if path.is_file() {
                open_with_chooser(&path);
            } else {
                error(actions, "Open with", format!("{f} does not exist in the working directory."));
            }
        }
        FileAction::RenameMove(f) => actions.push(Action::OpenDialog(Box::new(RenameMoveDialog::new(f)))),
        FileAction::Delete(files) => {
            let list = files.clone();
            let text = format!("Are you sure you want to delete the {} selected file(s) from disk?", files.len());
            actions.push(Action::OpenDialog(Box::new(Confirm::new("Delete", text, "Delete", move |cx| {
                if let Some(m) = cx.module {
                    if let Err(e) = delete_files(m, cx, &files) {
                        cx.error("Delete file failed", e);
                    }
                }
                cx.push(Action::RefreshStatus);
            }).with_list(list))));
        }
        FileAction::StopTracking(files) => {
            let refs: Vec<&str> = files.iter().map(String::as_str).collect();
            if let Err(e) = m.stop_tracking(&refs) {
                error(actions, "Stop tracking", e.to_string());
            }
            actions.push(Action::RefreshStatus);
        }
        FileAction::CherryPick { first, second, files } => {
            let refs: Vec<&str> = files.iter().map(String::as_str).collect();
            if let Err(e) = m.cherry_pick_file_changes(first, second, &refs) {
                error(actions, "Cherry pick changes", e.to_string());
            }
            actions.push(Action::RefreshStatus);
        }
        FileAction::Remember { label, commitish } => {
            if let Ok(mut r) = REMEMBERED.lock() {
                *r = Some((label, commitish));
            }
        }
        FileAction::Difftool { kind, first, second, files, tool } => {
            for f in files {
                let args = gitext_core::diff_tools::file_difftool_args(kind, first, second, &f.name, f.old_name.as_deref(), f.is_tracked);
                actions.push(run_difftool(args, tool.clone()));
            }
        }
        FileAction::DiffCommitishes { first, second, tool } => {
            let args = GitArgs::new("difftool").arg("--find-renames").arg("--find-copies").arg(first).arg(second);
            actions.push(run_difftool(args, tool));
        }
        FileAction::HideToolList => {}
        FileAction::DiffWithRemembered(second) => {
            if let Some((_, first)) = remembered() {
                let args = GitArgs::new("difftool").arg("--find-renames").arg("--find-copies").arg(first).arg(second);
                actions.push(Action::RunTool { tool_type: gitext_core::diff_tools::ToolType::Diff, args });
            }
        }
    }
}

/// Starts the configured difftool, or the named one.
fn run_difftool(args: GitArgs, tool: Option<String>) -> Action {
    match tool {
        Some(tool) => Action::RunDiffToolNamed { tool, args },
        None => Action::RunTool { tool_type: gitext_core::diff_tools::ToolType::Diff, args },
    }
}

/// The difftools usable now (`git difftool --tool-help`), shared by all lists and loaded once
/// in the background like upstream's `CustomDiffMergeToolCache`. Empty while loading.
static TOOLS: Mutex<Option<Option<Vec<String>>>> = Mutex::new(None);

pub fn available_tools(m: &GitModule, ctx: &egui::Context) -> Vec<String> {
    let Ok(mut tools) = TOOLS.lock() else { return Vec::new() };
    match &*tools {
        Some(Some(list)) => return list.clone(),
        Some(None) => return Vec::new(),
        None => {}
    }
    *tools = Some(None);
    let (exe, ctx) = (m.git(), ctx.clone());
    std::thread::spawn(move || {
        let get = |args: GitArgs| exe.run(&args).map(|r| r.stdout_str()).unwrap_or_default();
        let help = get(GitArgs::new("difftool").arg("--tool-help"));
        let mut default = get(GitArgs::new("config").args(["--get", gitext_core::diff_tools::DIFF_TOOL_KEY]));
        if default.trim().is_empty() {
            default = get(GitArgs::new("config").args(["--get", gitext_core::diff_tools::DIFF_TOOL_NO_GUI_KEY]));
        }
        if let Ok(mut t) = TOOLS.lock() {
            *t = Some(Some(gitext_core::diff_tools::parse_tool_help(&help, default.trim())));
        }
        ctx.request_repaint();
    });
    Vec::new()
}

/// The `<rev>:<path>` of a file version (`RememberFileContextMenuController.GetGitCommit`):
/// `rev` `None` is the parent of `second`; the work tree is the file path, the index `:<path>`.
fn version_commitish(rev: Option<ObjectId>, second: ObjectId, name: &str) -> String {
    let name = to_posix_path(name);
    match rev {
        Some(ObjectId::WORK_TREE) => name,
        Some(ObjectId::INDEX) => format!(":{name}"),
        Some(id) => format!("{id}:{name}"),
        None => format!("{second}^:{name}"),
    }
}

/// A difftool item: a button, or a dropdown of the available tools (the first one is the
/// configured tool) when there are several (`CustomDiffMergeToolProvider`).
fn tool_item(ui: &mut Ui, enabled: bool, label: &str, key: &str, tools: &[String], out: &mut Option<FileAction>, make: impl Fn(Option<String>) -> FileAction) {
    if tools.len() <= 1 || !enabled {
        if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(key)).clicked() {
            *out = Some(make(None));
            ui.close_kind(egui::UiKind::Menu);
        }
        return;
    }
    ui.menu_button(label, |ui| {
        for (i, t) in tools.iter().enumerate() {
            let text = if i == 0 { egui::RichText::new(t).strong() } else { egui::RichText::new(t) };
            if ui.add(egui::Button::new(text).shortcut_text(if i == 0 { key } else { "" })).clicked() {
                *out = Some(make(Some(t.clone())));
                ui.close_kind(egui::UiKind::Menu);
            }
        }
        ui.separator();
        if ui.button("Disable this dropdown").clicked() {
            *out = Some(FileAction::HideToolList);
            ui.close_kind(egui::UiKind::Menu);
        }
    });
}

fn describe(id: Option<ObjectId>, parents: &[ObjectId]) -> String {
    match id {
        Some(ObjectId::WORK_TREE) => "Working directory".into(),
        Some(ObjectId::INDEX) => "Commit index".into(),
        Some(id) => id.to_short_string(),
        None => match parents {
            [p] => p.to_short_string(),
            [] => "(none)".into(),
            _ => "(multiple)".into(),
        },
    }
}

/// Which "… -> Working directory" items the difftool menu shows and enables.
#[derive(Debug, PartialEq, Eq)]
struct DifftoolRules {
    hide_to_local: bool,
    first_to_local: bool,
    second_to_local: bool,
}

/// `FileStatusListContextMenuController`.
fn difftool_rules(sel: &Selection, parents: &[ObjectId]) -> DifftoolRules {
    let combined = sel.first == Some(ObjectId::COMBINED_DIFF);
    let firsts: Vec<ObjectId> = sel.first.map_or_else(|| parents.to_vec(), |f| vec![f]);
    let all_new = sel.items.iter().all(|i| i.is_new);
    let all_deleted = sel.items.iter().all(|i| i.is_deleted);
    // A is only known not to exist when it is the parent and B is new
    let first_is_parent = sel.first.is_none_or(|f| parents.contains(&f));
    DifftoolRules {
        hide_to_local: (sel.second == ObjectId::WORK_TREE && firsts.iter().all(|f| *f == ObjectId::INDEX))
            || (sel.second == ObjectId::INDEX && firsts.iter().all(|f| *f == ObjectId::WORK_TREE)),
        first_to_local: !combined && (!first_is_parent || !all_new) && !firsts.contains(&ObjectId::WORK_TREE),
        second_to_local: !combined && !all_deleted && sel.second != ObjectId::WORK_TREE,
    }
}

/// The "Open with difftool" submenu of the Diff tab (`tsmiOpenWithDifftool`): first to
/// second, either to the working directory file, two selected files, and the remembered file.
/// `parents` are the parents of `second` (the first side when `sel.first` is `None`).
pub fn difftool_menu(ui: &mut Ui, sel: &Selection, parents: &[ObjectId], tools: &[String]) -> Option<FileAction> {
    let mut action = None;
    if sel.items.is_empty() {
        return None;
    }
    let combined = sel.first == Some(ObjectId::COMBINED_DIFF);
    ui.menu_button("Open with difftool", |ui| {
        let caption = |ui: &mut Ui, text: String| {
            ui.add_enabled(false, egui::Label::new(egui::RichText::new(text).italics()));
        };
        caption(ui, format!("Second: B {}", describe(Some(sel.second), parents)));
        caption(ui, format!("First: A {}", describe(sel.first, parents)));
        ui.separator();
        let files: Vec<DiffFile> = sel
            .items
            .iter()
            .filter(|i| !i.is_submodule)
            .map(|i| DiffFile { name: i.name.clone(), old_name: i.old_name.clone(), is_tracked: i.is_tracked })
            .collect();
        let DifftoolRules { hide_to_local, first_to_local, second_to_local } = difftool_rules(sel, parents);
        let make = |kind: FileDiffKind| {
            let files = files.clone();
            move |tool| FileAction::Difftool { kind, first: sel.first, second: sel.second, files: files.clone(), tool }
        };
        tool_item(ui, !combined && !files.is_empty(), "First -> Second", "F3", tools, &mut action, make(FileDiffKind::FirstToSecond));
        if !hide_to_local {
            tool_item(ui, second_to_local && !files.is_empty(), "Second -> Working directory", "", tools, &mut action, make(FileDiffKind::SecondToLocal));
            tool_item(ui, first_to_local && !files.is_empty(), "First -> Working directory", "", tools, &mut action, make(FileDiffKind::FirstToLocal));
        }
        let single = sel.single();
        if single.is_some() || sel.items.len() == 2 {
            ui.separator();
        }
        // two selected files: the first in the list against the second
        if let [a, b] = sel.items.as_slice() {
            let enabled = !combined && !a.is_submodule && !b.is_submodule && !a.is_deleted && !b.is_deleted && sel.second != ObjectId::WORK_TREE;
            let (fa, fb) = (version_commitish(Some(sel.second), sel.second, &a.name), version_commitish(Some(sel.second), sel.second, &b.name));
            tool_item(ui, enabled, "Diff the selected files", "", tools, &mut action, move |tool| FileAction::DiffCommitishes { first: fa.clone(), second: fb.clone(), tool });
        }
        if let Some(item) = single {
            if let Some((label, remembered)) = remembered() {
                let enabled = !item.is_deleted;
                let second = version_commitish(Some(sel.second), sel.second, &item.name);
                tool_item(ui, enabled, &format!("Diff with remembered {label}"), "", tools, &mut action, move |tool| FileAction::DiffCommitishes { first: remembered.clone(), second: second.clone(), tool });
            }
            // the remembered file is the first side of a diff: never the work tree
            let second_ok = !item.is_deleted && sel.second != ObjectId::WORK_TREE;
            if ui.add_enabled(second_ok, egui::Button::new("Remember Second for diff")).clicked() {
                let label = format!("{} @ {}", item.name, describe(Some(sel.second), parents));
                action = Some(FileAction::Remember { label, commitish: version_commitish(Some(sel.second), sel.second, &item.name) });
                ui.close_kind(egui::UiKind::Menu);
            }
            let first_name = item.old_name.clone().filter(|o| !o.is_empty()).unwrap_or_else(|| item.name.clone());
            let first_ok = !item.is_new && !combined && sel.first != Some(ObjectId::WORK_TREE) && (sel.first.is_some() || parents.len() == 1);
            if ui.add_enabled(first_ok, egui::Button::new("Remember First for diff")).clicked() {
                let first = sel.first.or(parents.first().copied());
                let label = format!("{first_name} @ {}", describe(first, parents));
                action = Some(FileAction::Remember { label, commitish: version_commitish(first, sel.second, &first_name) });
                ui.close_kind(egui::UiKind::Menu);
            }
        }
    });
    action
}

/// Deletes work tree files (staged ones are unstaged first, like `DeleteFromFilesystem`).
fn delete_files(m: &GitModule, cx: &mut Cx, files: &[String]) -> Result<(), String> {
    let staged: Vec<&str> = cx
        .data
        .map(|d| d.status.iter().filter(|s| s.staged == StagedStatus::Index && files.contains(&s.name)).map(|s| s.name.as_str()).collect())
        .unwrap_or_default();
    if !staged.is_empty() {
        m.unstage_files(&staged).map_err(|e| e.to_string())?;
    }
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    m.delete_untracked(&refs).map_err(|e| e.to_string())
}

/// Asks for the new name of a file (`Move_Click`): repeats until the move succeeds or is
/// cancelled.
pub struct RenameMoveDialog {
    old: String,
    new: String,
    error: Option<String>,
    focus: bool,
}

impl RenameMoveDialog {
    pub fn new(old: String) -> Self {
        RenameMoveDialog { new: old.clone(), old, error: None, focus: true }
    }
}

impl Dialog for RenameMoveDialog {
    fn title(&self) -> String {
        "Rename / move".into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.label(format!("Rename or move '{}' to:", self.old));
        let r = ui.add(egui::TextEdit::singleline(&mut self.new).desired_width(380.0));
        if std::mem::take(&mut self.focus) {
            r.request_focus();
        }
        if let Some(e) = &self.error {
            ui.colored_label(crate::theme::Palette::for_ui(ui).error, e);
        }
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let valid = !self.new.trim().is_empty() && self.new.trim() != self.old;
        let (ok, cancel) = ok_cancel(ui, "Rename", valid);
        if (ok || enter) && valid {
            let Some(m) = cx.module else { return false };
            match m.move_file(&self.old, &self.new) {
                Ok(()) => {
                    cx.push(Action::RefreshStatus);
                    return false;
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        }
        !cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(name: &str, x: char) -> GitItemStatus {
        GitItemStatus::from_status_character(StagedStatus::WorkTree, name, x)
    }

    #[test]
    fn copy_path_variants() {
        let dir = std::path::Path::new("/repo");
        let files = vec!["src/a.rs".to_string(), "b.txt".to_string()];
        assert_eq!(path_text(dir, &files, PathKind::RelativePosix), "src/a.rs\nb.txt");
        if !cfg!(windows) {
            assert_eq!(path_text(dir, &files, PathKind::FullNative), "/repo/src/a.rs\n/repo/b.txt");
            assert_eq!(path_text(dir, &files, PathKind::FullWsl), "/repo/src/a.rs\n/repo/b.txt");
        }
    }

    #[test]
    fn commitish_of_the_remembered_file() {
        let id = ObjectId::try_parse("1234567890123456789012345678901234567890").unwrap();
        let f = status("dir/a.txt", 'M');
        let sel = |second| Selection { items: vec![&f], first: None, second };
        assert_eq!(sel(id).commitish().as_deref(), Some("1234567890123456789012345678901234567890:dir/a.txt"));
        assert_eq!(sel(ObjectId::INDEX).commitish().as_deref(), Some(":dir/a.txt"));
        assert_eq!(sel(ObjectId::WORK_TREE).commitish(), None, "git cannot diff a path with a blob in this order");
        assert_eq!(sel(ObjectId::WORK_TREE).second_commitish().as_deref(), Some("dir/a.txt"));
        let d = status("gone.txt", 'D');
        assert_eq!(Selection { items: vec![&d], first: None, second: id }.commitish(), None);
    }

    /// The Delete and F2 keys act on work tree files only.
    #[test]
    fn keys_only_in_the_work_tree() {
        let f = status("a.txt", 'M');
        let ctx = egui::Context::default();
        let press = |k: Key| egui::RawInput { events: vec![egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }], ..Default::default() };
        let run = |second, k| {
            let mut out = None;
            let _ = ctx.run(press(k), |ctx| out = keys(ctx, &Selection { items: vec![&f], first: None, second }));
            out
        };
        assert_eq!(run(ObjectId::WORK_TREE, Key::Delete), Some(FileAction::Delete(vec!["a.txt".into()])));
        assert_eq!(run(ObjectId::WORK_TREE, Key::F2), Some(FileAction::RenameMove("a.txt".into())));
        assert_eq!(run(ObjectId::INDEX, Key::Delete), None);
    }

    #[test]
    fn difftool_items_follow_upstream_rules() {
        let commit = ObjectId::parse("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1").unwrap();
        let parent = ObjectId::parse("b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2").unwrap();
        let (modified, new, deleted) = (status("m.txt", 'M'), status("n.txt", 'A'), status("d.txt", 'D'));
        let rules = |items: Vec<&GitItemStatus>, first, second| difftool_rules(&Selection { items, first, second }, &[parent]);
        let all = |hide, first, second| DifftoolRules { hide_to_local: hide, first_to_local: first, second_to_local: second };
        assert_eq!(rules(vec![&modified], None, commit), all(false, true, true));
        // a new file has no first version, a deleted one no second
        assert_eq!(rules(vec![&new], None, commit), all(false, false, true));
        assert_eq!(rules(vec![&deleted], None, commit), all(false, true, false));
        // a commit picked as first can have the file even when it is new against the parent
        assert_eq!(rules(vec![&new], Some(commit), commit), all(false, true, true));
        // the working directory is already a side
        assert_eq!(rules(vec![&modified], Some(ObjectId::INDEX), ObjectId::WORK_TREE), all(true, true, false));
        assert_eq!(rules(vec![&modified], Some(commit), ObjectId::WORK_TREE), all(false, true, false));
        assert_eq!(rules(vec![&modified], Some(ObjectId::WORK_TREE), ObjectId::INDEX), all(true, false, true));
        assert_eq!(rules(vec![&modified], Some(ObjectId::COMBINED_DIFF), commit), all(false, false, false));
    }

    #[test]
    fn open_with_from_any_revision() {
        let f = status("a.txt", 'M');
        let ctx = egui::Context::default();
        let shift_f4 = egui::RawInput { events: vec![egui::Event::Key { key: Key::F4, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::SHIFT }], modifiers: Modifiers::SHIFT, ..Default::default() };
        let commit = ObjectId::parse("0123456789012345678901234567890123456789").unwrap();
        for second in [ObjectId::WORK_TREE, ObjectId::INDEX, commit] {
            let mut out = None;
            let _ = ctx.run(shift_f4.clone(), |ctx| out = keys(ctx, &Selection { items: vec![&f], first: None, second }));
            assert_eq!(out, Some(FileAction::OpenWith("a.txt".into())), "{second:?}");
        }
    }
}
