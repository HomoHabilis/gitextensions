//! Repository data shown by the browse window, loaded in the background
//! (refs, status, remotes, stashes, submodules, worktrees).

use std::collections::HashMap;

use gitext_core::commands::UntrackedFilesMode;
use gitext_core::module::{AheadBehind, GitWorktree, Remote, RepoState, SubmoduleInfo};
use gitext_core::status::{GitItemStatus, StagedStatus};
use gitext_core::{GitModule, GitRef, GitRevision, ObjectId};

#[derive(Debug, Clone, Default)]
pub struct RepoData {
    pub refs: Vec<GitRef>,
    pub refs_by_commit: HashMap<ObjectId, Vec<GitRef>>,
    pub current_branch: Option<String>,
    pub head: ObjectId,
    pub state: RepoState,
    pub remotes: Vec<Remote>,
    pub stashes: Vec<GitRevision>,
    pub submodules: Vec<SubmoduleInfo>,
    pub worktrees: Vec<GitWorktree>,
    pub ahead_behind: HashMap<String, AheadBehind>,
    pub status: Vec<GitItemStatus>,
    pub user_name: String,
    pub user_email: String,
    pub is_bare: bool,
    pub superproject: Option<std::path::PathBuf>,
    pub error: Option<String>,
}

impl RepoData {
    /// Loads everything (runs on a background thread).
    pub fn load(module: &GitModule, show_untracked: bool) -> RepoData {
        let mut d = RepoData { is_bare: module.is_bare(), ..Default::default() };
        d.head = module.head_id();
        d.current_branch = module.current_branch();
        match module.get_refs() {
            Ok(r) => d.refs = r,
            Err(e) => d.error = Some(e.to_string()),
        }
        for r in &d.refs {
            if gitext_core::git_ref::ref_name::is_remote_head(&r.complete_name) {
                continue;
            }
            d.refs_by_commit.entry(r.object_id).or_default().push(r.clone());
        }
        d.state = module.state();
        d.remotes = module.get_remotes().unwrap_or_default();
        d.stashes = module.get_stashes();
        d.submodules = module.get_submodules();
        d.worktrees = module.get_worktrees();
        d.ahead_behind = module.get_ahead_behind();
        if !d.is_bare {
            let mode = if show_untracked { UntrackedFilesMode::All } else { UntrackedFilesMode::No };
            d.status = module.get_status(mode, true).unwrap_or_default();
        }
        d.user_name = module.user_name();
        d.user_email = module.user_email();
        d.superproject = module.superproject();
        d
    }

    pub fn local_branches(&self) -> impl Iterator<Item = &GitRef> {
        self.refs.iter().filter(|r| r.is_head())
    }

    pub fn remote_branches(&self) -> impl Iterator<Item = &GitRef> {
        self.refs.iter().filter(|r| r.is_remote() && !gitext_core::git_ref::ref_name::is_remote_head(&r.complete_name))
    }

    pub fn tags(&self) -> impl Iterator<Item = &GitRef> {
        self.refs.iter().filter(|r| r.is_tag() && !r.is_dereference)
    }

    pub fn current_ref(&self) -> Option<&GitRef> {
        let name = self.current_branch.as_deref()?;
        self.local_branches().find(|r| r.name == name)
    }

    pub fn remote_names(&self) -> Vec<String> {
        self.remotes.iter().map(|r| r.name.clone()).collect()
    }

    /// Branch names (local first, then remote), for combo boxes.
    pub fn branch_names(&self, include_remote: bool) -> Vec<String> {
        let mut v: Vec<String> = self.local_branches().map(|r| r.name.clone()).collect();
        if include_remote {
            v.extend(self.remote_branches().map(|r| r.name.clone()));
        }
        v
    }

    pub fn work_tree_changes(&self) -> usize {
        self.status.iter().filter(|s| s.staged == StagedStatus::WorkTree).count()
    }

    pub fn index_changes(&self) -> usize {
        self.status.iter().filter(|s| s.staged == StagedStatus::Index).count()
    }

    pub fn has_conflicts(&self) -> bool {
        self.status.iter().any(|s| s.is_unmerged)
    }

    /// Default remote for the current branch (`GetCurrentRemote`).
    pub fn current_remote(&self) -> Option<String> {
        self.current_ref()
            .map(|r| r.tracking_remote.clone())
            .filter(|r| !r.is_empty())
            .or_else(|| self.remotes.iter().find(|r| r.name == "origin").map(|r| r.name.clone()))
            .or_else(|| self.remotes.first().map(|r| r.name.clone()))
    }
}
