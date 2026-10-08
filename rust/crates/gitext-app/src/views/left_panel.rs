//! Port of `RepoObjectsTree` (left panel): branches, remotes, tags, stashes, submodules
//! and worktrees with their context menus.

use std::collections::BTreeMap;

use egui::{CollapsingHeader, RichText, Ui};
use gitext_core::{GitRef, ObjectId};

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
}

#[derive(Default)]
pub struct LeftPanel {
    pub filter: String,
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

impl LeftPanel {
    pub fn ui(&mut self, ui: &mut Ui, data: &RepoData, selected: Option<ObjectId>) -> Option<LeftCommand> {
        let palette = Palette::for_ui(ui);
        let mut cmd = None;
        ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("🔍 Search branches, tags…").desired_width(f32::INFINITY));
        ui.add_space(2.0);
        let filter = self.filter.to_lowercase();
        let matches = |s: &str| filter.is_empty() || s.to_lowercase().contains(&filter);

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // Branches
            let locals: Vec<&GitRef> = data.local_branches().filter(|r| matches(&r.name)).collect();
            let header = CollapsingHeader::new(RichText::new(format!("🔀 Branches ({})", locals.len())).strong()).default_open(true).show(ui, |ui| {
                let mut tree = PathTree::default();
                for r in &locals {
                    tree.insert(&r.name, r);
                }
                self.branch_tree(ui, &tree, "local", data, selected, &palette, &mut cmd, false);
            });
            header.header_response.context_menu(|ui| {
                if ui.button("Create branch…").clicked() {
                    cmd = Some(LeftCommand::CreateBranch(data.head));
                    ui.close_kind(egui::UiKind::Menu);
                }
            });

            // Remotes
            let header = CollapsingHeader::new(RichText::new(format!("☁ Remotes ({})", data.remotes.len())).strong()).default_open(true).show(ui, |ui| {
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
                        if ui.button(format!("Fetch {}", remote.name)).clicked() {
                            cmd = Some(LeftCommand::FetchRemote(remote.name.clone()));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button(format!("Prune {}", remote.name)).clicked() {
                            cmd = Some(LeftCommand::PruneRemote(remote.name.clone()));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button("Manage remotes…").clicked() {
                            cmd = Some(LeftCommand::ManageRemotes);
                            ui.close_kind(egui::UiKind::Menu);
                        }
                    });
                }
            });
            header.header_response.context_menu(|ui| {
                if ui.button("Manage remotes…").clicked() {
                    cmd = Some(LeftCommand::ManageRemotes);
                    ui.close_kind(egui::UiKind::Menu);
                }
            });

            // Tags
            let tags: Vec<&GitRef> = data.tags().filter(|r| matches(&r.name)).collect();
            CollapsingHeader::new(RichText::new(format!("🏷 Tags ({})", tags.len())).strong()).default_open(false).show(ui, |ui| {
                for t in tags {
                    let r = ui.selectable_label(selected == Some(t.object_id), &t.name);
                    if r.clicked() {
                        cmd = Some(LeftCommand::Select(t.object_id));
                    }
                    r.context_menu(|ui| {
                        if ui.button("Checkout…").clicked() {
                            cmd = Some(LeftCommand::Checkout(t.name.clone()));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button("Create branch here…").clicked() {
                            cmd = Some(LeftCommand::CreateBranch(t.object_id));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button("Merge into current branch…").clicked() {
                            cmd = Some(LeftCommand::Merge(t.name.clone()));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button("Push tag…").clicked() {
                            cmd = Some(LeftCommand::PushTag(t.name.clone()));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button("Delete tag…").clicked() {
                            cmd = Some(LeftCommand::DeleteTag(t.name.clone()));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                    });
                }
            });

            // Stashes
            let header = CollapsingHeader::new(RichText::new(format!("☰ Stashes ({})", data.stashes.len())).strong()).default_open(false).show(ui, |ui| {
                for s in &data.stashes {
                    let name = s.reflog_selector.clone().unwrap_or_default().trim_start_matches("refs/").to_string();
                    let r = ui.selectable_label(selected == Some(s.object_id), format!("{name}: {}", s.subject));
                    if r.clicked() {
                        cmd = Some(LeftCommand::Select(s.object_id));
                    }
                    r.context_menu(|ui| {
                        if ui.button("Apply").clicked() {
                            cmd = Some(LeftCommand::ApplyStash(name.clone()));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button("Pop").clicked() {
                            cmd = Some(LeftCommand::PopStash(name.clone()));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                        if ui.button("Drop…").clicked() {
                            cmd = Some(LeftCommand::DropStash(name.clone()));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                    });
                }
            });
            header.header_response.context_menu(|ui| {
                if ui.button("Stash changes").clicked() {
                    cmd = Some(LeftCommand::CreateStash);
                    ui.close_kind(egui::UiKind::Menu);
                }
            });

            // Submodules
            if !data.submodules.is_empty() {
                let header = CollapsingHeader::new(RichText::new(format!("⊞ Submodules ({})", data.submodules.len())).strong()).default_open(false).show(ui, |ui| {
                    for s in &data.submodules {
                        let status = match s.status {
                            '-' => " (not initialized)",
                            '+' => " (modified)",
                            'U' => " (conflicts)",
                            _ => "",
                        };
                        let r = ui.selectable_label(false, format!("{}{status}", s.path)).on_hover_text(format!("{} {}", s.commit.to_short_string(), s.describe));
                        if r.double_clicked() {
                            cmd = Some(LeftCommand::OpenSubmodule(s.path.clone()));
                        }
                        r.context_menu(|ui| {
                            if ui.button("Open").clicked() {
                                cmd = Some(LeftCommand::OpenSubmodule(s.path.clone()));
                                ui.close_kind(egui::UiKind::Menu);
                            }
                            if ui.button("Update").clicked() {
                                cmd = Some(LeftCommand::UpdateSubmodule(s.path.clone()));
                                ui.close_kind(egui::UiKind::Menu);
                            }
                            if ui.button("Synchronize").clicked() {
                                cmd = Some(LeftCommand::SyncSubmodule(s.path.clone()));
                                ui.close_kind(egui::UiKind::Menu);
                            }
                        });
                    }
                });
                header.header_response.context_menu(|ui| {
                    if ui.button("Manage submodules…").clicked() {
                        cmd = Some(LeftCommand::ManageSubmodules);
                        ui.close_kind(egui::UiKind::Menu);
                    }
                });
            }

            // Worktrees
            if data.worktrees.len() > 1 {
                let header = CollapsingHeader::new(RichText::new(format!("🗐 Worktrees ({})", data.worktrees.len())).strong()).default_open(false).show(ui, |ui| {
                    for w in &data.worktrees {
                        let label = format!(
                            "{}{}{}",
                            w.path,
                            w.branch.as_ref().map(|b| format!(" [{b}]")).unwrap_or_default(),
                            if w.is_main { " (main)" } else { "" }
                        );
                        let r = ui.selectable_label(false, RichText::new(label).color(if w.is_deleted { palette.error } else { ui.visuals().text_color() }));
                        if r.double_clicked() {
                            cmd = Some(LeftCommand::OpenWorktree(w.path.clone()));
                        }
                        r.context_menu(|ui| {
                            if ui.button("Open").clicked() {
                                cmd = Some(LeftCommand::OpenWorktree(w.path.clone()));
                                ui.close_kind(egui::UiKind::Menu);
                            }
                            if !w.is_main && ui.button("Remove…").clicked() {
                                cmd = Some(LeftCommand::RemoveWorktree(w.path.clone()));
                                ui.close_kind(egui::UiKind::Menu);
                            }
                        });
                    }
                });
                header.header_response.context_menu(|ui| {
                    if ui.button("Manage worktrees…").clicked() {
                        cmd = Some(LeftCommand::ManageWorktrees);
                        ui.close_kind(egui::UiKind::Menu);
                    }
                });
            }
        });
        cmd
    }

    #[allow(clippy::too_many_arguments)]
    fn branch_tree(&self, ui: &mut Ui, tree: &PathTree, salt: &str, data: &RepoData, selected: Option<ObjectId>, palette: &Palette, cmd: &mut Option<LeftCommand>, remote: bool) {
        for (folder, sub) in &tree.children {
            CollapsingHeader::new(RichText::new(format!("🗀 {folder}")).color(palette.muted)).id_salt((salt, folder, sub.leaves.len())).default_open(true).show(ui, |ui| {
                self.branch_tree(ui, sub, &format!("{salt}/{folder}"), data, selected, palette, cmd, remote);
            });
        }
        for (label, r) in &tree.leaves {
            let is_current = !remote && data.current_branch.as_deref() == Some(r.name.as_str());
            let mut text = RichText::new(*label);
            if is_current {
                text = text.strong().color(palette.lanes[2]);
            }
            let r_ui = ui.horizontal(|ui| {
                let resp = ui.selectable_label(selected == Some(r.object_id), text);
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
                            s.push_str("(gone)");
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
                resp.on_hover_text(format!("{}\ntracking {}/{}", r.complete_name, r.tracking_remote, r.merge_with))
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
                    if ui.button("Checkout as local branch…").clicked() {
                        *cmd = Some(LeftCommand::CheckoutRemote(name.clone()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                } else if !is_current && ui.button("Checkout").clicked() {
                    *cmd = Some(LeftCommand::Checkout(name.clone()));
                    ui.close_kind(egui::UiKind::Menu);
                }
                if !is_current {
                    if ui.button(format!("Merge into {current}…")).clicked() {
                        *cmd = Some(LeftCommand::Merge(name.clone()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if ui.button(format!("Rebase {current} on this…")).clicked() {
                        *cmd = Some(LeftCommand::Rebase(name.clone()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                }
                if ui.button("Create branch here…").clicked() {
                    *cmd = Some(LeftCommand::CreateBranch(r.object_id));
                    ui.close_kind(egui::UiKind::Menu);
                }
                if ui.button("Show only this branch").clicked() {
                    *cmd = Some(LeftCommand::FilterBranch(r.complete_name.clone()));
                    ui.close_kind(egui::UiKind::Menu);
                }
                ui.separator();
                if remote {
                    if ui.button("Delete remote branch…").clicked() {
                        *cmd = Some(LeftCommand::DeleteRemoteBranch(name.clone()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                } else {
                    if ui.button("Push…").clicked() {
                        *cmd = Some(LeftCommand::Push(name.clone()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if is_current && ui.button("Pull…").clicked() {
                        *cmd = Some(LeftCommand::Pull);
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if ui.button("Set upstream…").clicked() {
                        *cmd = Some(LeftCommand::SetUpstream(name.clone()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if ui.button("Rename…").clicked() {
                        *cmd = Some(LeftCommand::Rename(name.clone()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if !is_current && ui.button("Delete…").clicked() {
                        *cmd = Some(LeftCommand::Delete(r.complete_name.clone()));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                }
            });
        }
    }
}
