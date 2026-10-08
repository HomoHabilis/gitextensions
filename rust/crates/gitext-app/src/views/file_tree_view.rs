//! Port of `RevisionFileTreeControl`: the files of a revision as a tree with a file viewer.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{CollapsingHeader, RichText, Ui};
use gitext_core::tree::{GitItem, GitObjectType};
use gitext_core::{GitModule, ObjectId};

use super::diff_viewer::{content_from_bytes, DiffViewer, ViewerContent};
use super::revision_diff::DiffCommand;
use crate::tasks::Loader;
use crate::theme::Palette;

/// Sorted directory listings by path ("" = root); item names are relative to their directory.
type Tree = HashMap<String, Listing>;

type Listing = Arc<Vec<GitItem>>;

/// Directories first, then by name.
fn sort_items(items: &mut [GitItem]) {
    items.sort_by_cached_key(|i| (i.object_type != GitObjectType::Tree, i.name.to_lowercase()));
}

fn build_tree(items: Vec<GitItem>) -> Tree {
    let mut dirs: HashMap<String, Vec<GitItem>> = HashMap::new();
    dirs.entry(String::new()).or_default();
    for mut item in items {
        let (dir, name) = match item.name.rsplit_once('/') {
            Some((d, n)) => (d.to_string(), n.to_string()),
            None => (String::new(), item.name.clone()),
        };
        if item.object_type == GitObjectType::Tree {
            dirs.entry(item.name.clone()).or_default();
        }
        item.name = name;
        dirs.entry(dir).or_default().push(item);
    }
    dirs.into_iter()
        .map(|(k, mut v)| {
            sort_items(&mut v);
            (k, Arc::new(v))
        })
        .collect()
}

#[derive(Default)]
pub struct FileTreeView {
    rev: Option<ObjectId>,
    /// Every directory listing of the revision, read with one git call.
    tree: Loader<ObjectId, Tree>,
    /// Directory listings by path ("" = root), read one by one until `tree` is loaded.
    dirs: HashMap<String, Loader<(ObjectId, String), Listing>>,
    pub selected: Option<String>,
    content: Loader<(ObjectId, String), ViewerContent>,
    viewer: DiffViewer,
    find: String,
    all_files: Loader<ObjectId, Vec<String>>,
    /// Expand to this path on the next frame.
    pub reveal: Option<String>,
}

impl FileTreeView {
    pub fn invalidate(&mut self) {
        self.tree.invalidate();
        self.dirs.clear();
        self.content.invalidate();
        self.all_files.invalidate();
    }

    pub fn ui(&mut self, ui: &mut Ui, module: &GitModule, rev: Option<ObjectId>, show_line_numbers: bool) -> Option<DiffCommand> {
        let palette = Palette::for_ui(ui);
        let Some(rev) = rev else {
            ui.label(RichText::new("No commit selected").italics().color(palette.muted));
            return None;
        };
        // The caller maps the work tree / index to HEAD (no git call per frame here)
        let tree_rev = rev;
        if tree_rev.is_zero() {
            ui.label(RichText::new("No files").italics().color(palette.muted));
            return None;
        }
        if self.rev != Some(tree_rev) {
            self.rev = Some(tree_rev);
            self.dirs.clear();
        }
        let m = module.clone();
        self.tree.request_latest(ui.ctx(), tree_rev, move || build_tree(m.ls_tree_all(tree_rev).unwrap_or_default()));
        let mut cmd = None;
        egui::SidePanel::left("filetree_tree").resizable(true).default_width(300.0).show_inside(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.find).hint_text("🔍 Find file…").desired_width(f32::INFINITY));
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                if self.find.trim().is_empty() {
                    self.dir_ui(ui, module, tree_rev, "", &palette, &mut cmd, 0);
                } else {
                    let m = module.clone();
                    let files = self.all_files.request(ui.ctx(), tree_rev, move || m.ls_tree_recursive(tree_rev).unwrap_or_default());
                    match files {
                        None => {
                            ui.spinner();
                        }
                        Some(files) => {
                            let needle = self.find.to_lowercase();
                            let hits: Vec<String> = files.iter().filter(|f| f.to_lowercase().contains(&needle)).take(500).cloned().collect();
                            for f in hits {
                                if ui.selectable_label(self.selected.as_deref() == Some(f.as_str()), &f).clicked() {
                                    self.selected = Some(f.clone());
                                }
                            }
                        }
                    }
                }
            });
        });

        if let Some(path) = self.selected.clone() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&path).strong());
                if ui.small_button("Blame").clicked() {
                    cmd = Some(DiffCommand::Blame { file: path.clone(), rev: tree_rev });
                }
                if ui.small_button("History").clicked() {
                    cmd = Some(DiffCommand::FileHistory(path.clone()));
                }
                if ui.small_button("Save as…").clicked() {
                    cmd = Some(DiffCommand::SaveAs { rev: tree_rev, file: path.clone() });
                }
            });
            let m = module.clone();
            let p = path.clone();
            let content = self
                .content
                .request(ui.ctx(), (tree_rev, path.clone()), move || match m.get_file_bytes(tree_rev, &p) {
                    Ok(b) => content_from_bytes(&b),
                    Err(e) => ViewerContent::Empty(e.to_string()),
                })
                .cloned()
                .unwrap_or(ViewerContent::Empty("Loading…".into()));
            self.viewer.ui(ui, &content, show_line_numbers, &[]);
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Select a file").italics().color(palette.muted));
            });
        }
        cmd
    }

    #[allow(clippy::too_many_arguments)]
    fn dir_ui(&mut self, ui: &mut Ui, module: &GitModule, rev: ObjectId, path: &str, palette: &Palette, cmd: &mut Option<DiffCommand>, depth: usize) {
        let items = match self.tree.value.as_ref().filter(|_| self.tree.key == Some(rev)) {
            Some(tree) => Some(tree.get(path).cloned().unwrap_or_default()),
            None => {
                let m = module.clone();
                let p = path.to_string();
                let loader = self.dirs.entry(path.to_string()).or_default();
                loader
                    .request(ui.ctx(), (rev, path.to_string()), move || {
                        let mut items = m.ls_tree(rev, &p).unwrap_or_default();
                        sort_items(&mut items);
                        Arc::new(items)
                    })
                    .cloned()
            }
        };
        let Some(items) = items else {
            ui.spinner();
            return;
        };
        for item in items.iter() {
            let full = if path.is_empty() { item.name.clone() } else { format!("{path}/{}", item.name) };
            match item.object_type {
                GitObjectType::Tree => {
                    let open = self.reveal.as_deref().map(|r| r.starts_with(&format!("{full}/")));
                    let mut header = CollapsingHeader::new(format!("🗀 {}", item.name)).id_salt(("ft", &full));
                    if let Some(true) = open {
                        header = header.open(Some(true));
                    }
                    let r = header.show(ui, |ui| self.dir_ui(ui, module, rev, &full, palette, cmd, depth + 1));
                    r.header_response.context_menu(|ui| {
                        if ui.button("File history").clicked() {
                            *cmd = Some(DiffCommand::FileHistory(full.clone()));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button("Filter commits by this path").clicked() {
                            *cmd = Some(DiffCommand::FilterPath(full.clone()));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                    });
                }
                _ => {
                    let icon = if item.object_type == GitObjectType::Commit { "⊞" } else { "🗋" };
                    let selected = self.selected.as_deref() == Some(full.as_str());
                    let r = ui.selectable_label(selected, format!("{icon} {}", item.name));
                    if self.reveal.as_deref() == Some(full.as_str()) {
                        r.scroll_to_me(Some(egui::Align::Center));
                        self.selected = Some(full.clone());
                        self.reveal = None;
                    }
                    if r.clicked() {
                        self.selected = Some(full.clone());
                    }
                    if r.double_clicked() {
                        *cmd = Some(DiffCommand::Blame { file: full.clone(), rev });
                    }
                    r.context_menu(|ui| {
                        if ui.button("Blame").clicked() {
                            *cmd = Some(DiffCommand::Blame { file: full.clone(), rev });
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button("File history").clicked() {
                            *cmd = Some(DiffCommand::FileHistory(full.clone()));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button("Open this revision (temp file)").clicked() {
                            *cmd = Some(DiffCommand::OpenRevisionFile { rev, file: full.clone() });
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button("Open working directory file").clicked() {
                            *cmd = Some(DiffCommand::OpenWorkFile(full.clone()));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button("Save as…").clicked() {
                            *cmd = Some(DiffCommand::SaveAs { rev, file: full.clone() });
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button("Reset file to this revision…").clicked() {
                            *cmd = Some(DiffCommand::ResetFileTo { rev, files: vec![full.clone()] });
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button("Filter commits by this path").clicked() {
                            *cmd = Some(DiffCommand::FilterPath(full.clone()));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button("Copy path").clicked() {
                            *cmd = Some(DiffCommand::CopyPaths(vec![full.clone()]));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: GitObjectType, name: &str) -> GitItem {
        GitItem { mode: if kind == GitObjectType::Tree { 40000 } else { 100644 }, object_type: kind, object_id: ObjectId::ZERO, name: name.to_string() }
    }

    #[test]
    fn build_tree_groups_entries_by_directory() {
        let tree = build_tree(vec![
            item(GitObjectType::Blob, "b.txt"),
            item(GitObjectType::Tree, "src"),
            item(GitObjectType::Blob, "src/main.rs"),
            item(GitObjectType::Tree, "src/empty"),
            item(GitObjectType::Blob, "A.md"),
        ]);
        let names = |p: &str| tree[p].iter().map(|i| i.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(""), ["src", "A.md", "b.txt"]);
        assert_eq!(names("src"), ["empty", "main.rs"]);
        assert!(tree["src/empty"].is_empty());
    }
}
