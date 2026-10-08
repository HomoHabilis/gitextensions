//! Port of `GitCommands.GitRef` and `GitCommands.GitRefName`.

use crate::object_id::ObjectId;

pub mod ref_name {
    //! Port of `GitRefName`: helpers for ref name manipulation.
    use crate::object_id::ObjectId;

    pub const REFS_PREFIX: &str = "refs/";
    pub const REFS_TAGS_PREFIX: &str = "refs/tags/";
    pub const REFS_HEADS_PREFIX: &str = "refs/heads/";
    pub const REFS_REMOTES_PREFIX: &str = "refs/remotes/";
    pub const REFS_BISECT_PREFIX: &str = "refs/bisect/";
    pub const REFS_BISECT_GOOD_PREFIX: &str = "refs/bisect/good";
    pub const REFS_BISECT_BAD_PREFIX: &str = "refs/bisect/bad";
    pub const REFS_STASH_PREFIX: &str = "refs/stash";
    pub const REFS_NOTES_PREFIX: &str = "refs/notes/commits";
    pub const TAG_DEREFERENCE_SUFFIX: &str = "^{}";

    /// Remote name of a full `refs/remotes/<remote>/...` ref, empty otherwise.
    pub fn get_remote_name(ref_name: &str) -> String {
        let Some(after) = ref_name.strip_prefix(REFS_REMOTES_PREFIX) else {
            return String::new();
        };
        after.split('/').next().unwrap_or_default().to_string()
    }

    /// Remote name for a full ref or an abbreviated `remote/branch` name.
    pub fn get_remote_name_from<'a>(ref_name: &str, remotes: impl IntoIterator<Item = &'a str>) -> String {
        if ref_name.starts_with(REFS_PREFIX) {
            return get_remote_name(ref_name);
        }
        for remote in remotes {
            if ref_name.starts_with(remote) && ref_name.len() > remote.len() && ref_name.as_bytes()[remote.len()] == b'/' {
                return remote.to_string();
            }
        }
        String::new()
    }

    /// Branch part of a `refs/remotes/<remote>/<branch>` ref.
    pub fn get_remote_branch(ref_name: &str) -> String {
        if ref_name.len() <= REFS_REMOTES_PREFIX.len() {
            return String::new();
        }
        match ref_name[REFS_REMOTES_PREFIX.len()..].find('/') {
            Some(i) => ref_name[REFS_REMOTES_PREFIX.len() + i + 1..].to_string(),
            None => String::new(),
        }
    }

    /// `master` > `refs/heads/master`; full refs and hashes are returned as-is.
    pub fn get_full_branch_name(branch: &str) -> String {
        let branch = branch.trim();
        if branch.is_empty() || branch.starts_with(REFS_PREFIX) || ObjectId::is_valid(branch) {
            return branch.to_string();
        }
        format!("{REFS_HEADS_PREFIX}{branch}")
    }

    /// `branch` > `refs/remotes/<remote>/branch`; full refs and hashes are returned as-is.
    pub fn get_full_remote_name(branch: &str, remote: &str) -> String {
        let branch = branch.trim();
        if branch.is_empty() || branch.starts_with(REFS_PREFIX) || ObjectId::is_valid(branch) {
            return branch.to_string();
        }
        format!("{REFS_REMOTES_PREFIX}{remote}/{branch}")
    }

    /// Whether the ref is `refs/remotes/<remote>/HEAD`.
    pub fn is_remote_head(ref_name: &str) -> bool {
        let Some(rest) = ref_name.strip_prefix(REFS_REMOTES_PREFIX) else {
            return false;
        };
        let Some(remote) = rest.strip_suffix("/HEAD") else {
            return false;
        };
        !remote.is_empty() && !remote.contains('/')
    }
}

/// Kind of a reference, determined from its complete name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GitRefType {
    Other,
    Head,
    Remote,
    Tag,
    Bisect,
    BisectGood,
    BisectBad,
    Stash,
}

impl GitRefType {
    pub fn determine(complete_name: &str) -> Self {
        use ref_name::*;
        if complete_name.starts_with(REFS_HEADS_PREFIX) {
            GitRefType::Head
        } else if complete_name.starts_with(REFS_TAGS_PREFIX) {
            GitRefType::Tag
        } else if complete_name.starts_with(REFS_REMOTES_PREFIX) {
            GitRefType::Remote
        } else if complete_name.starts_with(REFS_STASH_PREFIX) {
            GitRefType::Stash
        } else if complete_name.starts_with(REFS_BISECT_GOOD_PREFIX) {
            GitRefType::BisectGood
        } else if complete_name.starts_with(REFS_BISECT_BAD_PREFIX) {
            GitRefType::BisectBad
        } else if complete_name.starts_with(REFS_BISECT_PREFIX) {
            GitRefType::Bisect
        } else {
            GitRefType::Other
        }
    }
}

/// A git reference (branch, remote branch, tag, stash, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRef {
    pub object_id: ObjectId,
    pub complete_name: String,
    pub name: String,
    /// Remote name for remote refs, else empty.
    pub remote: String,
    pub kind: GitRefType,
    pub is_dereference: bool,
    /// For local branches: the upstream branch name (`branch.<name>.merge` without `refs/heads/`).
    pub merge_with: String,
    /// For local branches: the upstream remote (`branch.<name>.remote`).
    pub tracking_remote: String,
    /// Whether this is the currently checked out branch.
    pub is_selected: bool,
    /// Whether this is the upstream of the currently checked out branch.
    pub is_selected_head_merge_source: bool,
}

impl GitRef {
    pub fn new(object_id: ObjectId, complete_name: impl Into<String>, remote: impl Into<String>) -> Self {
        let complete_name = complete_name.into();
        let kind = GitRefType::determine(&complete_name);
        let is_dereference = complete_name.ends_with(ref_name::TAG_DEREFERENCE_SUFFIX);
        let name = Self::parse_name_with(&complete_name, kind, is_dereference);
        GitRef {
            object_id,
            complete_name,
            name,
            remote: remote.into(),
            kind,
            is_dereference,
            merge_with: String::new(),
            tracking_remote: String::new(),
            is_selected: false,
            is_selected_head_merge_source: false,
        }
    }

    /// Creates a ref, deriving the remote name from `refs/remotes/<remote>/...`.
    pub fn from_complete_name(object_id: ObjectId, complete_name: impl Into<String>) -> Self {
        let complete_name = complete_name.into();
        let remote = ref_name::get_remote_name(&complete_name);
        Self::new(object_id, complete_name, remote)
    }

    /// Local branch with upstream tracking information.
    pub fn with_tracking(mut self, tracking_remote: impl Into<String>, merge_with: impl Into<String>) -> Self {
        self.tracking_remote = tracking_remote.into();
        let merge: String = merge_with.into();
        self.merge_with = merge.strip_prefix(ref_name::REFS_HEADS_PREFIX).unwrap_or(&merge).to_string();
        self
    }

    pub fn is_head(&self) -> bool {
        self.kind == GitRefType::Head
    }
    pub fn is_remote(&self) -> bool {
        self.kind == GitRefType::Remote
    }
    pub fn is_tag(&self) -> bool {
        self.kind == GitRefType::Tag
    }
    pub fn is_stash(&self) -> bool {
        self.kind == GitRefType::Stash
    }
    pub fn is_bisect(&self) -> bool {
        self.kind == GitRefType::Bisect
    }
    pub fn is_bisect_good(&self) -> bool {
        self.kind == GitRefType::BisectGood
    }
    pub fn is_bisect_bad(&self) -> bool {
        self.kind == GitRefType::BisectBad
    }

    /// Name without the remote prefix (for remote refs).
    pub fn local_name(&self) -> String {
        Self::compute_local_name(self.is_remote(), &self.remote, &self.name)
    }

    pub fn guid(&self) -> Option<String> {
        (!self.object_id.is_zero()).then(|| self.object_id.to_string())
    }

    /// Whether this local branch tracks the given remote branch.
    pub fn is_tracking_remote(&self, remote: Option<&GitRef>) -> bool {
        match remote {
            Some(remote) => {
                self.is_head()
                    && remote.is_remote()
                    && self.merge_with == remote.local_name()
                    && self.tracking_remote == remote.remote
            }
            None => false,
        }
    }

    pub fn compute_local_name(is_remote: bool, remote: &str, name: &str) -> String {
        if !is_remote
            || remote.is_empty()
            || name.len() <= remote.len()
            || name.as_bytes()[remote.len()] != b'/'
            || !name.starts_with(remote)
        {
            return name.to_string();
        }
        name[remote.len() + 1..].to_string()
    }

    pub fn parse_name(complete_name: &str) -> String {
        let kind = GitRefType::determine(complete_name);
        let is_dereference = kind == GitRefType::Tag && complete_name.ends_with(ref_name::TAG_DEREFERENCE_SUFFIX);
        Self::parse_name_with(complete_name, kind, is_dereference)
    }

    fn parse_name_with(complete_name: &str, kind: GitRefType, is_dereference: bool) -> String {
        use ref_name::*;
        let name = match kind {
            GitRefType::Head => &complete_name[REFS_HEADS_PREFIX.len()..],
            GitRefType::Remote => &complete_name[REFS_REMOTES_PREFIX.len()..],
            GitRefType::Tag => {
                let end = complete_name.len() - if is_dereference { TAG_DEREFERENCE_SUFFIX.len() } else { 0 };
                &complete_name[REFS_TAGS_PREFIX.len()..end.max(REFS_TAGS_PREFIX.len())]
            }
            _ => complete_name.find("refs/").map(|i| &complete_name[i + 5..]).unwrap_or(complete_name),
        };
        if name.is_empty() { complete_name.to_string() } else { name.to_string() }
    }

    /// Names which occur for several refs (e.g. a branch and a tag with the same name).
    pub fn get_ambiguous_ref_names<'a>(refs: impl IntoIterator<Item = &'a GitRef>) -> std::collections::HashSet<String> {
        let mut seen = std::collections::HashSet::new();
        let mut ambiguous = std::collections::HashSet::new();
        for r in refs {
            if !seen.insert(r.name.clone()) {
                ambiguous.insert(r.name.clone());
            }
        }
        ambiguous
    }
}

impl std::fmt::Display for GitRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.complete_name)
    }
}

#[cfg(test)]
mod tests {
    //! Ported from GitCommands.Tests/Git/GitRefTests.cs and GitRefNameTests.cs
    use super::ref_name::*;
    use super::*;

    fn local_tracking(remote_branch: &str, remote: &str) -> GitRef {
        GitRef::new(ObjectId::random(), "refs/heads/local_branch", "")
            .with_tracking(remote, format!("refs/heads/{remote_branch}"))
    }

    fn remote_ref(branch: &str, remote: &str) -> GitRef {
        GitRef::new(ObjectId::random(), format!("refs/remotes/{remote}/{branch}"), remote)
    }

    #[test]
    fn is_tracking_remote() {
        assert!(local_tracking("remote_branch", "origin").is_tracking_remote(Some(&remote_ref("remote_branch", "origin"))));
        assert!(!local_tracking("remote_branch", "origin").is_tracking_remote(None));
        assert!(!local_tracking("remote_branch", "origin").is_tracking_remote(Some(&remote_ref("remote_branch", "upstream"))));
        assert!(!local_tracking("one_remote_branch", "origin").is_tracking_remote(Some(&remote_ref("another_remote_branch", "origin"))));
        assert!(!remote_ref("a_remote_branch", "origin").is_tracking_remote(Some(&remote_ref("a_remote_branch", "origin"))));
        assert!(!local_tracking("a", "origin").is_tracking_remote(Some(&local_tracking("a", "origin"))));
        let untracked = GitRef::new(ObjectId::random(), "refs/heads/local_branch", "");
        assert!(!untracked.is_tracking_remote(Some(&local_tracking("a_remote_branch", "origin"))));
    }

    #[test]
    fn local_name() {
        let r = GitRef::new(ObjectId::random(), "refs/remotes/origin/local_branch", "origin");
        assert_eq!(r.local_name(), "local_branch");
        // git-svn: remote not a prefix of name
        let r = GitRef::new(ObjectId::random(), "refs/remotes/a_short_name", "Remote_longer_than_Name");
        assert_eq!(r.local_name(), "a_short_name");
    }

    #[test]
    fn parse_name_and_type() {
        let t = GitRef::from_complete_name(ObjectId::random(), "refs/tags/v1.0^{}");
        assert!(t.is_tag() && t.is_dereference);
        assert_eq!(t.name, "v1.0");
        assert_eq!(GitRef::parse_name("refs/stash"), "stash");
        assert_eq!(GitRef::parse_name("refs/bisect/bad"), "bisect/bad");
        assert_eq!(GitRefType::determine("refs/bisect/good-abc"), GitRefType::BisectGood);
        let r = GitRef::from_complete_name(ObjectId::random(), "refs/remotes/upstream/feature/x");
        assert_eq!(r.remote, "upstream");
        assert_eq!(r.local_name(), "feature/x");
    }

    #[test]
    fn get_full_branch_name_test() {
        assert_eq!(get_full_branch_name(""), "");
        assert_eq!(get_full_branch_name("    "), "");
        assert_eq!(get_full_branch_name("4e0f0fe3f6add43557913c354de02560b8faec32"), "4e0f0fe3f6add43557913c354de02560b8faec32");
        assert_eq!(get_full_branch_name("master"), "refs/heads/master");
        assert_eq!(get_full_branch_name(" master "), "refs/heads/master");
        assert_eq!(get_full_branch_name("refs/heads/master"), "refs/heads/master");
        assert_eq!(get_full_branch_name("refs/heads/release/2.48"), "refs/heads/release/2.48");
        assert_eq!(get_full_branch_name("refs/tags/my-tag"), "refs/tags/my-tag");
        assert_eq!(get_full_branch_name("refs/foo"), "refs/foo");
        assert_eq!(get_full_remote_name("x", "origin"), "refs/remotes/origin/x");
    }

    #[test]
    fn get_remote_name_test() {
        assert_eq!(get_remote_name("refs/remotes/foo/master"), "foo");
        assert_eq!(get_remote_name("refs/tags/1.0.0"), "");
        let remotes = ["foo", "bar"];
        assert_eq!(get_remote_name_from("foo/master", remotes), "foo");
        assert_eq!(get_remote_name_from("food/master", remotes), "");
        assert_eq!(get_remote_name_from("refs/tags/1.0.0", remotes), "");
    }

    #[test]
    fn get_remote_branch_test() {
        assert_eq!(get_remote_branch("refs/remotes/foo/master"), "master");
        assert_eq!(get_remote_branch("refs/remotes/foo/tmp/master"), "tmp/master");
        assert_eq!(get_remote_branch("refs/remotes/foo"), "");
        assert_eq!(get_remote_branch("short"), "");
    }

    #[test]
    fn is_remote_head_test() {
        assert!(is_remote_head("refs/remotes/origin/HEAD"));
        assert!(is_remote_head("refs/remotes/upstream/HEAD"));
        for s in [
            "refs/remotes/ori/gin/HEAD",
            "refs/remotes//HEAD",
            "refs/remotes/HEAD",
            "refs/origin/HEAD",
            "ref/remotes/origin/HEAD",
            "remotes/origin/HEAD",
            "wat/refs/remotes/origin/HEAD",
            "refs/remotes/origin/HEADZ",
            "refs/remotes/origin/HEAD/wat",
            "refs/remotes/origin/HEAD  ",
            "  refs/remotes/origin/HEAD",
        ] {
            assert!(!is_remote_head(s), "{s}");
        }
    }
}
