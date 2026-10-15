//! File actions shared by the file lists of the Diff tab, the commit dialog and the stash dialog
//! (port of parts of `FileStatusList.ContextMenu`, `CopyPathsToolStripMenuItem` and
//! `RememberFileContextMenuController`): copy path variants, open with, rename / move, delete,
//! stop tracking, cherry-pick file changes and diff with a remembered file.

use std::sync::Mutex;

use egui::{Key, Modifiers, Ui};
use gitext_core::args::{to_mount_path, to_native_path, to_posix_path};
use gitext_core::status::{GitItemStatus, StagedStatus};
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
    if commitish.is_some() || second.is_some() {
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
        FileAction::DiffWithRemembered(second) => {
            if let Some((_, first)) = remembered() {
                let args = GitArgs::new("difftool").arg("--find-renames").arg("--find-copies").arg(first).arg(second);
                actions.push(Action::RunTool { tool_type: gitext_core::diff_tools::ToolType::Diff, args });
            }
        }
    }
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
