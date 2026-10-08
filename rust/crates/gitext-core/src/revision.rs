//! Port of `GitUIPluginInterfaces.GitRevision`.

use crate::git_ref::GitRef;
use crate::object_id::ObjectId;
use chrono::{DateTime, Local, TimeZone};

pub const WORK_TREE_GUID: &str = "1111111111111111111111111111111111111111";
pub const INDEX_GUID: &str = "2222222222222222222222222222222222222222";
pub const COMBINED_DIFF_GUID: &str = "3333333333333333333333333333333333333333";

/// A commit as shown in the revision grid.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GitRevision {
    pub object_id: ObjectId,
    pub refs: Vec<GitRef>,
    /// `None` when not populated.
    pub parent_ids: Option<Vec<ObjectId>>,
    pub tree_id: ObjectId,
    pub author: String,
    pub author_email: String,
    pub author_unix_time: i64,
    pub committer: String,
    pub committer_email: String,
    pub commit_unix_time: i64,
    pub subject: String,
    body: Option<String>,
    pub has_multi_line_message: bool,
    /// `None` = not loaded, empty = no notes.
    pub notes: Option<String>,
    pub is_autostash: bool,
    /// Reflog selector such as `refs/stash@{0}` for stashes.
    pub reflog_selector: Option<String>,
}

impl GitRevision {
    /// # Panics
    /// If `object_id` is zero.
    pub fn new(object_id: ObjectId) -> Self {
        assert!(!object_id.is_zero(), "ObjectId must not be the default (zero) value.");
        GitRevision { object_id, ..Default::default() }
    }

    pub fn with_parents(mut self, parents: Vec<ObjectId>) -> Self {
        self.parent_ids = Some(parents);
        self
    }

    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = subject.into();
        self
    }

    pub fn guid(&self) -> String {
        self.object_id.to_string()
    }

    /// Full commit message without notes; falls back to the subject for single line messages.
    pub fn body(&self) -> Option<&str> {
        match &self.body {
            Some(b) => Some(b),
            None if !self.has_multi_line_message => Some(&self.subject),
            None => None,
        }
    }

    pub fn set_body(&mut self, body: Option<String>) {
        self.body = body;
    }

    pub fn is_artificial(&self) -> bool {
        self.object_id.is_artificial()
    }

    pub fn is_stash(&self) -> bool {
        self.reflog_selector.is_some()
    }

    pub fn has_parent(&self) -> bool {
        self.parent_ids.as_ref().is_some_and(|p| !p.is_empty())
    }

    pub fn parents(&self) -> &[ObjectId] {
        self.parent_ids.as_deref().unwrap_or(&[])
    }

    pub fn first_parent_id(&self) -> ObjectId {
        self.parents().first().copied().unwrap_or_default()
    }

    pub fn author_date(&self) -> Option<DateTime<Local>> {
        unix_to_local(self.author_unix_time)
    }

    pub fn commit_date(&self) -> Option<DateTime<Local>> {
        unix_to_local(self.commit_unix_time)
    }
}

fn unix_to_local(t: i64) -> Option<DateTime<Local>> {
    if t == 0 {
        return None;
    }
    Local.timestamp_opt(t, 0).single()
}

impl std::fmt::Display for GitRevision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.object_id.to_short_string(), self.subject)
    }
}

/// Creates the artificial work tree revision.
pub fn work_tree_revision(head: ObjectId, subject: &str) -> GitRevision {
    let mut r = GitRevision::new(ObjectId::WORK_TREE).with_parents(vec![ObjectId::INDEX]);
    r.subject = subject.to_string();
    let _ = head;
    r
}

/// Creates the artificial index revision with `head` as parent.
pub fn index_revision(head: ObjectId, subject: &str) -> GitRevision {
    let parents = if head.is_zero() { vec![] } else { vec![head] };
    let mut r = GitRevision::new(ObjectId::INDEX).with_parents(parents);
    r.subject = subject.to_string();
    r
}

#[cfg(test)]
mod tests {
    //! Ported from GitCommands.Tests/GitRevisionTests.cs
    use super::*;

    #[test]
    fn should_validate_full_sha1_correctly() {
        use crate::object_id::is_full_sha1_hash;
        assert!(is_full_sha1_hash("0000000000000000000000000000000000000000"));
        assert!(is_full_sha1_hash("1111111111111111111111111111111111111111"));
        assert!(is_full_sha1_hash("0123456789abcdefa0123456789abcdefa012345"));
        assert!(!is_full_sha1_hash("0123456789ABCDEFA0123456789ABCDEFA012345"));
        assert!(!is_full_sha1_hash("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"));
        assert!(!is_full_sha1_hash("00000000000000000000000000000000000000000"));
        assert!(!is_full_sha1_hash("000000000000000000000000000000000000000"));
        assert!(!is_full_sha1_hash("0000000000000000000000000000000000000000 "));
        assert!(!is_full_sha1_hash(" 0000000000000000000000000000000000000000"));
    }

    #[test]
    fn ctor_should_throw_if_zero() {
        assert!(std::panic::catch_unwind(|| GitRevision::new(ObjectId::ZERO)).is_err());
    }

    #[test]
    fn body_falls_back_to_subject() {
        let mut r = GitRevision::new(ObjectId::random()).with_subject("subj");
        assert_eq!(r.body(), Some("subj"));
        r.has_multi_line_message = true;
        assert_eq!(r.body(), None);
        r.set_body(Some("subj\n\nbody".into()));
        assert_eq!(r.body(), Some("subj\n\nbody"));
    }

    #[test]
    fn artificial_and_parents() {
        let r = index_revision(ObjectId::random(), "Commit index");
        assert!(r.is_artificial());
        assert!(r.has_parent());
        let w = work_tree_revision(ObjectId::random(), "Working directory");
        assert_eq!(w.first_parent_id(), ObjectId::INDEX);
        assert!(!GitRevision::new(ObjectId::random()).has_parent());
        assert!(GitRevision::new(ObjectId::random()).first_parent_id().is_zero());
    }
}
