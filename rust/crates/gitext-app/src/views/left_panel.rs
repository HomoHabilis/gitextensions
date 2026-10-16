//! Port of `RepoObjectsTree` (left panel): branches, remotes, tags, stashes, submodules
//! and worktrees with their context menus.

use std::collections::BTreeMap;

use egui::{CollapsingHeader, RichText, Ui};
use gitext_core::{GitRef, ObjectId};

use crate::i18n::{tr, trf};
use crate::repo::RepoData;
use crate::theme::Palette;

#[derive(Debug, Clone)]
pub enum LeftCommand {
    Select(ObjectId),
    Checkout(String),
    CheckoutRemote(String),
    Merge(String),
    Rebase(String),
    CreateBranch(ObjectId),
    Rename(String),
    Delete(String),
    DeleteRemoteBranch(String),
    FilterBranch(String),
    Push(String),
    Pull,
    FetchRemote(String),
    PruneRemote(String),
    ManageRemotes,
    DeleteTag(String),
    PushTag(String),
    ApplyStash(String),
    PopStash(String),
    DropStash(String),
    OpenSubmodule(String),
    UpdateSubmodule(String),
    SyncSubmodule(String),
    ManageSubmodules,
    OpenWorktree(String),
    RemoveWorktree(String),
    ManageWorktrees,
    SetUpstream(String),
    CreateStash,
    /// Fetch a remote branch (`remote/branch`), then run the next step (`RemoteBranchNode`).
    FetchRemoteBranch(String, FetchThen),
    /// "Fetch and prune" a remote (upstream "Fetch and prune" on a remote node).
    FetchPruneRemote(String),
    FetchAllRemotes,
    FetchPruneAllRemotes,
    OpenUrl(String),
    Copy(String),
    ShowInFolder(String),
    CreateStashStaged,
    ManageStashes,
    CreateWorktree,
    PruneWorktrees,
}

/// What follows the fetch of a remote branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchThen {
    Nothing,
    Checkout,
    Merge,
    Rebase,
    CreateBranch,
}

/// A node of the tree that can be selected and acted on with the keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// (name, complete name)
    Local(String, String),
    Remote(String),
    Tag(String),
    /// `stash@{n}`
    Stash(String),
    /// (path, is main)
    Worktree(String, bool),
}

#[derive(Default)]
pub struct LeftPanel {
    pub filter: String,
    /// The node selected in the tree (Delete, F2, Up/Down act on it while the panel has the
    /// keyboard).
    pub selected: Option<Node>,
    pub has_focus: bool,
    /// The selectable nodes in display order, rebuilt each frame (for Up/Down).
    order: Vec<(Node, Option<ObjectId>)>,
    scroll_to_selected: bool,
}

/// A node of the branch path tree (`feature/x` shows under `feature`).
#[derive(Default)]
struct PathTree<'a> {
    children: BTreeMap<String, PathTree<'a>>,
    leaves: Vec<(&'a str, &'a GitRef)>,
}

impl<'a> PathTree<'a> {
    fn insert(&mut self, path: &'a str, r: &'a GitRef) {
        match path.split_once('/') {
            Some((head, rest)) => self.children.entry(head.to_string()).or_default().insert(rest, r),
            None => self.leaves.push((path, r)),
        }
    }
}

/// A menu item that sets the command and closes the menu.
fn item(ui: &mut Ui, label: &str, cmd: &mut Option<LeftCommand>, c: LeftCommand) {
    if ui.button(label).clicked() {
        *cmd = Some(c);
        ui.close_kind(egui::UiKind::Menu);
    }
}

/// The "Copy to clipboard" submenu of a ref node.
fn copy_menu(ui: &mut Ui, name: &str, id: ObjectId, cmd: &mut Option<LeftCommand>) {
    ui.menu_button(tr("Copy to clipboard"), |ui| {
        item(ui, &format!("{}  {name}", tr("Name")), cmd, LeftCommand::Copy(name.to_string()));
        item(ui, &format!("{}  {}", tr("Commit hash"), id.to_short_string()), cmd, LeftCommand::Copy(id.to_string()));
    });
}

/// Whether a remote URL can be opened in a browser (`IsRemoteUrlUsingHttp`).
fn is_http_url(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();
    u.starts_with("http://") || u.starts_with("https://")
}

/// The command of the Delete key on a node (`ICanDelete`).
fn delete_command(node: &Node, data: &RepoData) -> Option<LeftCommand> {
    match node {
        Node::Local(name, complete) if data.current_branch.as_deref() != Some(name.as_str()) => Some(LeftCommand::Delete(complete.clone())),
        Node::Remote(name) => Some(LeftCommand::DeleteRemoteBranch(name.clone())),
        Node::Tag(name) => Some(LeftCommand::DeleteTag(name.clone())),
        Node::Stash(name) => Some(LeftCommand::DropStash(name.clone())),
        Node::Worktree(path, false) => Some(LeftCommand::RemoveWorktree(path.clone())),
        _ => None,
    }
}

impl LeftPanel {
    /// Selects `node` (clicked): the panel takes the keyboard.
    fn select_node(&mut self, node: Node) {
        self.selected = Some(node);
        self.has_focus = true;
    }

    fn is_selected(&self, node: &Node) -> bool {
        self.selected.as_ref() == Some(node)
    }

    /// A selectable row of the tree; returns its response, scrolled to when selected with the
    /// keyboard.
    fn node_label(&mut self, ui: &mut Ui, node: Node, id: Option<ObjectId>, highlight: bool, text: impl Into<egui::WidgetText>) -> egui::Response {
        let selected = self.is_selected(&node);
        let resp = ui.selectable_label(selected || highlight, text);
        if selected && std::mem::take(&mut self.scroll_to_selected) {
            resp.scroll_to_me(None);
        }
        if resp.clicked() || resp.secondary_clicked() {
            self.select_node(node.clone());
        }
        self.order.push((node, id));
        resp
    }

    /// The keys of the tree (`RepoObjectsTree` hotkeys: Delete, F2) and Up/Down.
    fn handle_keys(&mut self, ui: &Ui, data: &RepoData) -> Option<LeftCommand> {
        use crate::views::plain_key;
        if !self.has_focus || ui.ctx().wants_keyboard_input() {
            return None;
        }
        let ctx = ui.ctx();
        let current = self.selected.as_ref().and_then(|s| self.order.iter().position(|(n, _)| n == s));
        let mut target = None;
        if plain_key(ctx, egui::Key::ArrowDown) {
            target = Some(current.map_or(0, |c| (c + 1).min(self.order.len().saturating_sub(1))));
        } else if plain_key(ctx, egui::Key::ArrowUp) {
            target = Some(current.map_or(0, |c| c.saturating_sub(1)));
        }
        if let Some((node, id)) = target.and_then(|t| self.order.get(t)).cloned() {
            self.selected = Some(node);
            self.scroll_to_selected = true;
            return id.map(LeftCommand::Select);
        }
        let node = self.selected.clone()?;
        if plain_key(ctx, egui::Key::Delete) {
            return delete_command(&node, data);
        }
        if plain_key(ctx, egui::Key::F2) {
            if let Node::Local(name, _) = node {
                return Some(LeftCommand::Rename(name));
            }
        }
        None
    }

    pub fn ui(&mut self, ui: &mut Ui, data: &RepoData, selected: Option<ObjectId>) -> Option<LeftCommand> {
        let palette = Palette::for_ui(ui);
        let mut cmd = self.handle_keys(ui, data);
        self.order.clear();
        ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text(format!("🔍 {}", tr("Search branches, tags…"))).desired_width(f32::INFINITY));
        ui.add_space(2.0);
        let filter = self.filter.to_lowercase();
        let matches = |s: &str| filter.is_empty() || s.to_lowercase().contains(&filter);

        let scroll = egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // Branches
            let locals: Vec<&GitRef> = data.local_branches().filter(|r| matches(&r.name)).collect();
            let header =
                CollapsingHeader::new(RichText::new(format!("🔀 {}", trf("Branches ({0})", &[&locals.len()]))).strong()).default_open(true).show(ui, |ui| {
                    let mut tree = PathTree::default();
                    for r in &locals {
                        tree.insert(&r.name, r);
                    }
                    self.branch_tree(ui, &tree, "local", data, selected, &palette, &mut cmd, false);
                });
            header.header_response.context_menu(|ui| {
                item(ui, tr("Create branch…"), &mut cmd, LeftCommand::CreateBranch(data.head));
            });

            // Remotes
            let header = CollapsingHeader::new(RichText::new(format!("☁ {}", trf("Remotes ({0})", &[&data.remotes.len()]))).strong()).default_open(true).show(
                ui,
                |ui| {
                    for remote in &data.remotes {
                        let branches: Vec<&GitRef> = data.remote_branches().filter(|r| r.remote == remote.name && matches(&r.name)).collect();
                        let resp = CollapsingHeader::new(format!("🖧 {}", remote.name)).id_salt(("remote", &remote.name)).default_open(true).show(ui, |ui| {
                            let mut tree = PathTree::default();
                            for r in &branches {
                                let local = r.name.strip_prefix(&format!("{}/", remote.name)).unwrap_or(&r.name);
                                tree.insert(local, r);
                            }
                            self.branch_tree(ui, &tree, &remote.name, data, selected, &palette, &mut cmd, true);
                        });
                        resp.header_response.on_hover_text(&remote.fetch_url).context_menu(|ui| {
                            item(ui, tr("Fetch"), &mut cmd, LeftCommand::FetchRemote(remote.name.clone()));
                            item(ui, tr("Fetch and prune"), &mut cmd, LeftCommand::FetchPruneRemote(remote.name.clone()));
                            item(ui, tr("Prune"), &mut cmd, LeftCommand::PruneRemote(remote.name.clone()));
                            if is_http_url(&remote.fetch_url) {
                                item(ui, tr("Open remote URL"), &mut cmd, LeftCommand::OpenUrl(remote.fetch_url.trim().to_string()));
                            }
                            item(ui, tr("Copy URL"), &mut cmd, LeftCommand::Copy(remote.fetch_url.clone()));
                            ui.separator();
                            item(ui, tr("Manage remotes…"), &mut cmd, LeftCommand::ManageRemotes);
                        });
                    }
                },
            );
            header.header_response.context_menu(|ui| {
                item(ui, tr("Fetch all remotes"), &mut cmd, LeftCommand::FetchAllRemotes);
                item(ui, tr("Fetch and prune all remotes"), &mut cmd, LeftCommand::FetchPruneAllRemotes);
                ui.separator();
                item(ui, tr("Manage remotes…"), &mut cmd, LeftCommand::ManageRemotes);
            });

            // Tags
            let tags: Vec<&GitRef> = data.tags().filter(|r| matches(&r.name)).collect();
            CollapsingHeader::new(RichText::new(format!("🏷 {}", trf("Tags ({0})", &[&tags.len()]))).strong()).default_open(false).show(ui, |ui| {
                for t in tags {
                    let r = self.node_label(ui, Node::Tag(t.name.clone()), Some(t.object_id), selected == Some(t.object_id), &t.name);
                    if r.clicked() {
                        cmd = Some(LeftCommand::Select(t.object_id));
                    }
                    r.context_menu(|ui| {
                        let current = data.current_branch.clone().unwrap_or_default();
                        item(ui, tr("Checkout…"), &mut cmd, LeftCommand::Checkout(t.name.clone()));
                        item(ui, tr("Create branch here…"), &mut cmd, LeftCommand::CreateBranch(t.object_id));
                        item(ui, &trf("Merge into {0}…", &[&current]), &mut cmd, LeftCommand::Merge(t.name.clone()));
                        item(ui, &trf("Rebase {0} on this tag…", &[&current]), &mut cmd, LeftCommand::Rebase(t.name.clone()));
                        ui.separator();
                        item(ui, tr("Push tag…"), &mut cmd, LeftCommand::PushTag(t.name.clone()));
                        item(ui, &format!("{}  (Del)", tr("Delete tag…")), &mut cmd, LeftCommand::DeleteTag(t.name.clone()));
                        ui.separator();
                        copy_menu(ui, &t.name, t.object_id, &mut cmd);
                    });
                }
            });

            // Stashes
            let header = CollapsingHeader::new(RichText::new(format!("☰ {}", trf("Stashes ({0})", &[&data.stashes.len()]))).strong()).default_open(false).show(
                ui,
                |ui| {
                    for s in &data.stashes {
                        let name = s.reflog_selector.clone().unwrap_or_default().trim_start_matches("refs/").to_string();
                        let r =
                            self.node_label(ui, Node::Stash(name.clone()), Some(s.object_id), selected == Some(s.object_id), format!("{name}: {}", s.subject));
                        if r.clicked() {
                            cmd = Some(LeftCommand::Select(s.object_id));
                        }
                        r.context_menu(|ui| {
                            item(ui, tr("Apply"), &mut cmd, LeftCommand::ApplyStash(name.clone()));
                            item(ui, tr("Pop"), &mut cmd, LeftCommand::PopStash(name.clone()));
                            item(ui, &format!("{}  (Del)", tr("Drop…")), &mut cmd, LeftCommand::DropStash(name.clone()));
                        });
                    }
                },
            );
            header.header_response.context_menu(|ui| {
                item(ui, tr("Stash changes"), &mut cmd, LeftCommand::CreateStash);
                item(ui, tr("Stash staged changes"), &mut cmd, LeftCommand::CreateStashStaged);
                item(ui, tr("Manage stashes…"), &mut cmd, LeftCommand::ManageStashes);
            });

            // Submodules
            if !data.submodules.is_empty() {
                let header = CollapsingHeader::new(RichText::new(format!("⊞ {}", trf("Submodules ({0})", &[&data.submodules.len()]))).strong())
                    .default_open(false)
                    .show(ui, |ui| {
                        for s in &data.submodules {
                            let status = match s.status {
                                '-' => tr("(not initialized)"),
                                '+' => tr("(modified)"),
                                'U' => tr("(conflicts)"),
                                _ => "",
                            };
                            let status = if status.is_empty() { String::new() } else { format!(" {status}") };
                            let r = ui.selectable_label(false, format!("{}{status}", s.path)).on_hover_text(format!(
                                "{} {}",
                                s.commit.to_short_string(),
                                s.describe
                            ));
                            if r.double_clicked() {
                                cmd = Some(LeftCommand::OpenSubmodule(s.path.clone()));
                            }
                            r.context_menu(|ui| {
                                item(ui, tr("Open"), &mut cmd, LeftCommand::OpenSubmodule(s.path.clone()));
                                item(ui, tr("Update"), &mut cmd, LeftCommand::UpdateSubmodule(s.path.clone()));
                                item(ui, tr("Synchronize"), &mut cmd, LeftCommand::SyncSubmodule(s.path.clone()));
                            });
                        }
                    });
                header.header_response.context_menu(|ui| {
                    item(ui, tr("Manage submodules…"), &mut cmd, LeftCommand::ManageSubmodules);
                });
            }

            // Worktrees (always shown, as upstream: the root node creates and prunes them)
            if !data.is_bare || data.worktrees.len() > 1 {
                let header = CollapsingHeader::new(RichText::new(format!("🗐 {}", trf("Worktrees ({0})", &[&data.worktrees.len()]))).strong())
                    .default_open(false)
                    .show(ui, |ui| {
                        for w in &data.worktrees {
                            let label = format!(
                                "{}{}{}",
                                w.path,
                                w.branch.as_ref().map(|b| format!(" [{b}]")).unwrap_or_default(),
                                if w.is_main { format!(" {}", tr("(main)")) } else { String::new() }
                            );
                            let text = RichText::new(label).color(if w.is_deleted { palette.error } else { ui.visuals().text_color() });
                            let r = self.node_label(ui, Node::Worktree(w.path.clone(), w.is_main), None, false, text);
                            if r.double_clicked() {
                                cmd = Some(LeftCommand::OpenWorktree(w.path.clone()));
                            }
                            r.context_menu(|ui| {
                                ui.add_enabled_ui(!w.is_deleted, |ui| item(ui, tr("Open"), &mut cmd, LeftCommand::OpenWorktree(w.path.clone())));
                                if !w.is_main {
                                    item(ui, &format!("{}  (Del)", tr("Remove…")), &mut cmd, LeftCommand::RemoveWorktree(w.path.clone()));
                                }
                                item(ui, tr("Copy path"), &mut cmd, LeftCommand::Copy(w.path.clone()));
                                ui.add_enabled_ui(!w.is_deleted, |ui| item(ui, tr("Show in folder"), &mut cmd, LeftCommand::ShowInFolder(w.path.clone())));
                            });
                        }
                    });
                header.header_response.context_menu(|ui| {
                    item(ui, tr("Create worktree…"), &mut cmd, LeftCommand::CreateWorktree);
                    item(ui, tr("Prune worktrees"), &mut cmd, LeftCommand::PruneWorktrees);
                    item(ui, tr("Manage worktrees…"), &mut cmd, LeftCommand::ManageWorktrees);
                });
            }
        });
        // the panel keeps the keyboard until something else is clicked
        if ui.input(|i| i.pointer.any_pressed()) && !scroll.inner_rect.contains(ui.input(|i| i.pointer.interact_pos().unwrap_or_default())) {
            self.has_focus = false;
        }
        cmd
    }

    #[allow(clippy::too_many_arguments, reason = "the state the view reads and updates")]
    fn branch_tree(
        &mut self,
        ui: &mut Ui,
        tree: &PathTree,
        salt: &str,
        data: &RepoData,
        selected: Option<ObjectId>,
        palette: &Palette,
        cmd: &mut Option<LeftCommand>,
        remote: bool,
    ) {
        for (folder, sub) in &tree.children {
            CollapsingHeader::new(RichText::new(format!("🗀 {folder}")).color(palette.muted)).id_salt((salt, folder, sub.leaves.len())).default_open(true).show(
                ui,
                |ui| {
                    self.branch_tree(ui, sub, &format!("{salt}/{folder}"), data, selected, palette, cmd, remote);
                },
            );
        }
        for (label, r) in &tree.leaves {
            let is_current = !remote && data.current_branch.as_deref() == Some(r.name.as_str());
            let mut text = RichText::new(*label);
            if is_current {
                text = text.strong().color(palette.lanes[2]);
            }
            let node = if remote { Node::Remote(r.name.clone()) } else { Node::Local(r.name.clone(), r.complete_name.clone()) };
            let r_ui = ui.horizontal(|ui| {
                let resp = self.node_label(ui, node, Some(r.object_id), selected == Some(r.object_id), text);
                if !remote {
                    if let Some(ab) = data.ahead_behind.get(&r.name) {
                        let mut s = String::new();
                        if ab.ahead > 0 {
                            s.push_str(&format!("⬆{} ", ab.ahead));
                        }
                        if ab.behind > 0 {
                            s.push_str(&format!("⬇{}", ab.behind));
                        }
                        if ab.gone {
                            s.push_str(tr("(gone)"));
                        }
                        if !s.is_empty() {
                            ui.label(RichText::new(s.trim()).small().color(palette.muted));
                        }
                    }
                }
                resp
            });
            let resp = r_ui.inner;
            let resp = if !remote && !r.merge_with.is_empty() {
                resp.on_hover_text(format!("{}\n{}", r.complete_name, trf("tracking {0}/{1}", &[&r.tracking_remote, &r.merge_with])))
            } else {
                resp.on_hover_text(&r.complete_name)
            };
            if resp.clicked() {
                *cmd = Some(LeftCommand::Select(r.object_id));
            }
            if resp.double_clicked() {
                *cmd = Some(if remote { LeftCommand::CheckoutRemote(r.name.clone()) } else { LeftCommand::Checkout(r.name.clone()) });
            }
            let name = r.name.clone();
            resp.context_menu(|ui| {
                let current = data.current_branch.clone().unwrap_or_default();
                if remote {
                    item(ui, tr("Checkout as local branch…"), cmd, LeftCommand::CheckoutRemote(name.clone()));
                    ui.menu_button(tr("Fetch & …"), |ui| {
                        let fetch = |ui: &mut Ui, label: &str, then: FetchThen, cmd: &mut Option<LeftCommand>| {
                            item(ui, label, cmd, LeftCommand::FetchRemoteBranch(name.clone(), then))
                        };
                        fetch(ui, tr("Fetch"), FetchThen::Nothing, cmd);
                        fetch(ui, tr("Fetch & Checkout…"), FetchThen::Checkout, cmd);
                        if !current.is_empty() {
                            fetch(ui, &trf("Fetch & Merge into {0} (Pull)…", &[&current]), FetchThen::Merge, cmd);
                            fetch(ui, &trf("Fetch & Rebase {0}…", &[&current]), FetchThen::Rebase, cmd);
                        }
                        fetch(ui, tr("Fetch & Create branch…"), FetchThen::CreateBranch, cmd);
                    });
                } else if !is_current {
                    item(ui, tr("Checkout"), cmd, LeftCommand::Checkout(name.clone()));
                }
                if !is_current {
                    item(ui, &trf("Merge into {0}…", &[&current]), cmd, LeftCommand::Merge(name.clone()));
                    item(ui, &trf("Rebase {0} on this…", &[&current]), cmd, LeftCommand::Rebase(name.clone()));
                }
                item(ui, tr("Create branch here…"), cmd, LeftCommand::CreateBranch(r.object_id));
                item(ui, tr("Show only this branch"), cmd, LeftCommand::FilterBranch(r.complete_name.clone()));
                ui.separator();
                if remote {
                    item(ui, &format!("{}  (Del)", tr("Delete remote branch…")), cmd, LeftCommand::DeleteRemoteBranch(name.clone()));
                } else {
                    item(ui, tr("Push…"), cmd, LeftCommand::Push(name.clone()));
                    if is_current {
                        item(ui, tr("Pull…"), cmd, LeftCommand::Pull);
                    }
                    item(ui, tr("Set upstream…"), cmd, LeftCommand::SetUpstream(name.clone()));
                    item(ui, &format!("{}  (F2)", tr("Rename…")), cmd, LeftCommand::Rename(name.clone()));
                    if !is_current {
                        item(ui, &format!("{}  (Del)", tr("Delete…")), cmd, LeftCommand::Delete(r.complete_name.clone()));
                    }
                }
                ui.separator();
                copy_menu(ui, &name, r.object_id, cmd);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_key_commands() {
        let data = RepoData { current_branch: Some("main".into()), ..Default::default() };
        assert!(delete_command(&Node::Local("main".into(), "refs/heads/main".into()), &data).is_none(), "the current branch is not deleted");
        assert!(matches!(delete_command(&Node::Local("dev".into(), "refs/heads/dev".into()), &data), Some(LeftCommand::Delete(c)) if c == "refs/heads/dev"));
        assert!(matches!(delete_command(&Node::Remote("origin/dev".into()), &data), Some(LeftCommand::DeleteRemoteBranch(n)) if n == "origin/dev"));
        assert!(matches!(delete_command(&Node::Tag("v1".into()), &data), Some(LeftCommand::DeleteTag(n)) if n == "v1"));
        assert!(matches!(delete_command(&Node::Stash("stash@{0}".into()), &data), Some(LeftCommand::DropStash(_))));
        assert!(delete_command(&Node::Worktree("/r".into(), true), &data).is_none(), "the main worktree is not removed");
        assert!(matches!(delete_command(&Node::Worktree("/w".into(), false), &data), Some(LeftCommand::RemoveWorktree(_))));
    }

    #[test]
    fn http_urls_open_in_browser() {
        assert!(is_http_url("https://github.com/a/b.git"));
        assert!(is_http_url(" HTTP://host/x"));
        assert!(!is_http_url("git@github.com:a/b.git"));
        assert!(!is_http_url("/srv/repo.git"));
    }
}
