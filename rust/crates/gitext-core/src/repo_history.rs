//! Port of `UserRepositoryHistory` (`LocalRepositoryManager`, `Repository`): recent and
//! favourite repositories shown on the dashboard and in the "Open recent" menus.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RepositoryAnchor {
    /// Pinned to the top.
    AnchoredInTop,
    /// Always kept in the recent list.
    AnchoredInRecent,
    #[default]
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    pub path: String,
    #[serde(default)]
    pub anchor: RepositoryAnchor,
    #[serde(default)]
    pub category: Option<String>,
}

impl Repository {
    pub fn new(path: impl Into<String>) -> Self {
        Repository { path: path.into(), anchor: RepositoryAnchor::None, category: None }
    }

    /// Port of `GetParentPath`.
    pub fn parent_path(&self) -> String {
        let p = Path::new(&self.path);
        if self.path.starts_with("\\\\") || !p.is_dir() {
            return String::new();
        }
        p.parent().map(|p| p.display().to_string()).unwrap_or_else(|| self.path.clone())
    }

    pub fn name(&self) -> String {
        Path::new(self.path.trim_end_matches(['/', '\\'])).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| self.path.clone())
    }
}

/// The persisted repository history.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryHistory {
    #[serde(default)]
    pub recent: Vec<Repository>,
    #[serde(default)]
    pub favourites: Vec<Repository>,
    #[serde(skip)]
    pub max_recent: usize,
}

fn same_path(a: &str, b: &str) -> bool {
    let n = |s: &str| s.trim_end_matches(['/', '\\']).replace('\\', "/");
    if cfg!(windows) { n(a).eq_ignore_ascii_case(&n(b)) } else { n(a) == n(b) }
}

impl RepositoryHistory {
    pub fn default_path() -> Option<PathBuf> {
        crate::settings::config_dir().map(|d| d.join("repositories.json"))
    }

    pub fn load(path: &Path, max_recent: usize) -> Self {
        let mut h: RepositoryHistory = std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        h.max_recent = max_recent;
        h.recent = trim(h.recent, max_recent);
        h
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self).unwrap_or_default())
    }

    /// Adds `path` as the most recent repository. Returns whether the list changed.
    pub fn add_as_most_recent(&mut self, path: &str) -> bool {
        if path.trim().is_empty() {
            return false;
        }
        if self.recent.first().is_some_and(|r| same_path(&r.path, path)) {
            return false;
        }
        let repo = match self.recent.iter().position(|r| same_path(&r.path, path)) {
            Some(i) => self.recent.remove(i),
            None => Repository::new(path),
        };
        self.recent.insert(0, repo);
        if self.max_recent > 0 {
            self.recent = trim(std::mem::take(&mut self.recent), self.max_recent);
        }
        true
    }

    pub fn remove_recent(&mut self, path: &str) -> bool {
        let before = self.recent.len();
        self.recent.retain(|r| !same_path(&r.path, path));
        before != self.recent.len()
    }

    pub fn remove_favourite(&mut self, path: &str) -> bool {
        let before = self.favourites.len();
        self.favourites.retain(|r| !same_path(&r.path, path));
        before != self.favourites.len()
    }

    /// Adds/moves a repository to the favourites with a category.
    pub fn assign_category(&mut self, path: &str, category: Option<&str>) {
        match category.filter(|c| !c.trim().is_empty()) {
            None => {
                self.remove_favourite(path);
            }
            Some(c) => match self.favourites.iter_mut().find(|r| same_path(&r.path, path)) {
                Some(r) => r.category = Some(c.to_string()),
                None => self.favourites.push(Repository { path: path.to_string(), anchor: RepositoryAnchor::None, category: Some(c.to_string()) }),
            },
        }
    }

    pub fn set_anchor(&mut self, path: &str, anchor: RepositoryAnchor) {
        if let Some(r) = self.recent.iter_mut().find(|r| same_path(&r.path, path)) {
            r.anchor = anchor;
        }
    }

    pub fn categories(&self) -> Vec<String> {
        let mut c: Vec<String> = self.favourites.iter().filter_map(|r| r.category.clone()).collect();
        c.sort();
        c.dedup();
        c
    }

    /// Removes repositories for which `is_invalid` returns true.
    pub fn remove_invalid(&mut self, is_invalid: impl Fn(&str) -> bool) -> bool {
        let before = self.recent.len() + self.favourites.len();
        self.recent.retain(|r| !is_invalid(&r.path));
        self.favourites.retain(|r| !is_invalid(&r.path));
        before != self.recent.len() + self.favourites.len()
    }

    /// Recent repositories split into pinned (top) and other (port of `RecentRepoSplitter`).
    pub fn split_recent(&self, sort_alphabetically: bool) -> (Vec<&Repository>, Vec<&Repository>) {
        let mut top: Vec<&Repository> = self.recent.iter().filter(|r| r.anchor == RepositoryAnchor::AnchoredInTop).collect();
        let mut rest: Vec<&Repository> = self.recent.iter().filter(|r| r.anchor != RepositoryAnchor::AnchoredInTop).collect();
        if sort_alphabetically {
            top.sort_by_key(|r| r.name().to_lowercase());
            rest.sort_by_key(|r| r.name().to_lowercase());
        }
        (top, rest)
    }
}

/// Keeps all anchored repositories and fills the remaining slots in order.
fn trim(repos: Vec<Repository>, size: usize) -> Vec<Repository> {
    if size == 0 || repos.len() <= size {
        return repos;
    }
    let anchored = repos.iter().filter(|r| r.anchor != RepositoryAnchor::None).count();
    let mut free = size.saturating_sub(anchored);
    repos
        .into_iter()
        .filter(|r| {
            if r.anchor != RepositoryAnchor::None {
                true
            } else if free > 0 {
                free -= 1;
                true
            } else {
                false
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! Ported from GitCommands.Tests/UserRepositoryHistory/LocalRepositoryManagerTests.cs
    use super::*;

    fn history(paths: &[&str]) -> RepositoryHistory {
        RepositoryHistory { recent: paths.iter().map(|p| Repository::new(*p)).collect(), favourites: vec![], max_recent: 0 }
    }

    const ADD: &str = "path to add\\";

    #[test]
    fn add_as_most_recent() {
        let mut h = history(&["path1\\", "path3\\", "path4\\", "path5\\"]);
        assert!(h.add_as_most_recent(ADD));
        assert_eq!(h.recent.len(), 5);
        assert_eq!(h.recent[0].path, ADD);

        let mut h = history(&["path1\\", "path3\\", "path4\\", ADD, "path5\\"]);
        h.add_as_most_recent(ADD);
        assert_eq!(h.recent.len(), 5);
        assert_eq!(h.recent[0].path, ADD);

        let mut h = history(&["path1\\", "path3\\", ADD, "path4\\", ADD, "path5\\"]);
        h.add_as_most_recent(ADD);
        assert_eq!(h.recent.len(), 6);
        assert_eq!(h.recent[0].path, ADD);
        assert_eq!(h.recent[4].path, ADD);

        let mut h = history(&[ADD, "path1\\", "path3\\"]);
        assert!(!h.add_as_most_recent(ADD));
    }

    #[test]
    fn trim_history_per_settings() {
        let mut h = history(&["path1", "path2", "path3", "path4", "path5", "path6", "path7"]);
        h.recent = trim(h.recent, 3);
        assert_eq!(h.recent.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), ["path1", "path2", "path3"]);
    }

    #[test]
    fn keep_anchored_repositories_when_trimming() {
        let mut h = history(&["path1", "path2", "path3", "path4", "path5", "path6"]);
        h.recent[4].anchor = RepositoryAnchor::AnchoredInTop;
        h.recent[5].anchor = RepositoryAnchor::AnchoredInRecent;
        let t = trim(h.recent, 3);
        assert_eq!(t.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), ["path1", "path5", "path6"]);

        let mut h = history(&["path1", "path2", "path3", "path4"]);
        for r in &mut h.recent[1..] {
            r.anchor = RepositoryAnchor::AnchoredInTop;
        }
        let t = trim(h.recent, 2);
        assert_eq!(t.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), ["path2", "path3", "path4"]);
    }

    #[test]
    fn remove_and_invalid() {
        let mut h = history(&["a", "b", "c"]);
        assert!(h.remove_recent("b"));
        assert!(!h.remove_recent("x"));
        h.assign_category("a", Some("Work"));
        assert_eq!(h.categories(), ["Work"]);
        assert!(!h.remove_favourite("x"));
        assert!(h.remove_invalid(|p| p == "a"));
        assert!(h.favourites.is_empty());
        assert_eq!(h.recent.len(), 1);
        assert!(!h.remove_invalid(|_| false));
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("repos.json");
        let mut h = history(&["a", "b"]);
        h.set_anchor("b", RepositoryAnchor::AnchoredInTop);
        h.save(&file).unwrap();
        let l = RepositoryHistory::load(&file, 10);
        assert_eq!(l.recent, h.recent);
        let (top, rest) = l.split_recent(false);
        assert_eq!(top[0].path, "b");
        assert_eq!(rest[0].path, "a");
    }
}
