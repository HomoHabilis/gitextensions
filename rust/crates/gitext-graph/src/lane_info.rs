//! Port of `LaneNodeLocator`, `LaneInfoProvider` and `BranchFinder`: tooltip text for a lane.

use std::sync::OnceLock;

use regex::Regex;

use crate::graph::RevisionGraph;
use crate::model::{NodeIdx, Store};

pub const NO_INFO_TEXT: &str = "Sorry, this commit seems to be not loaded.";
pub const BODY_NOT_LOADED: &str = "\n\nFull message text is not present in older commits.\nSelect this commit to populate the full message.";

/// Result of [`find_prev_node`]: the node, whether the location is at the node, and the child of
/// the lane's segment.
pub type PrevNode = (Option<NodeIdx>, bool, Option<NodeIdx>);

/// Port of `LaneNodeLocator.FindPrevNode`: the revision a lane at `row_index` leads to.
pub fn find_prev_node(graph: &RevisionGraph, row_index: i64, lane: i32) -> PrevNode {
    const NOT_FOUND: PrevNode = (None, false, None);
    if row_index < 0 || lane < 0 {
        return NOT_FOUND;
    }
    let Some(row) = graph.row(row_index) else {
        return NOT_FOUND;
    };
    if row.get_current_revision_lane(&graph.store) == lane {
        return (Some(row.revision), true, None);
    }
    let segments = row.get_segments_for_index(&graph.store, lane);
    match segments.first() {
        Some(&first) => {
            let seg = &graph.store.segments[first];
            debug_assert!(
                segments.iter().all(|&s| graph.store.segments[s].parent == seg.parent),
                "All segments for a lane should have the same parent."
            );
            (Some(seg.parent), false, Some(seg.child))
        }
        None => NOT_FOUND,
    }
}

/// Port of `LaneInfoProvider.GetLaneInfo`.
pub fn get_lane_info(graph: &RevisionGraph, row_index: i64, lane: i32) -> String {
    let (node, is_at_node, single_child) = find_prev_node(graph, row_index, lane);
    lane_info_text(&graph.store, node, is_at_node, single_child)
}

/// Formats the lane info for a located node (testable without a row cache).
pub fn lane_info_text(store: &Store, node: Option<NodeIdx>, is_at_node: bool, single_child: Option<NodeIdx>) -> String {
    let Some(node) = node else {
        return String::new();
    };
    let Some(revision) = store.nodes[node].revision.as_ref() else {
        return NO_INFO_TEXT.to_string();
    };
    let mut text = String::new();
    if let Some(child) = single_child {
        let c = &store.nodes[child];
        text.push_str(&format!(
            "{}: {}\n|\n",
            c.object_id.to_short_string(),
            c.revision.as_ref().map(|r| r.subject.as_str()).unwrap_or_default()
        ));
    }
    if !revision.is_artificial() {
        if is_at_node {
            text.push_str("* ");
        }
        text.push_str(&revision.guid());
        text.push('\n');
        let branch = BranchFinder::new(store, node);
        if let Some(committed_to) = branch.committed_to.as_deref().filter(|s| !s.trim().is_empty()) {
            text.push_str(&format!("\nBranch: {committed_to}"));
            if let Some(with) = branch.merged_with.as_deref().filter(|s| !s.trim().is_empty()) {
                text.push_str(&format!(" (merged with {with})"));
            }
        }
        text.push('\n');
    }
    match revision.body() {
        Some(body) if revision.has_multi_line_message || !body.is_empty() => {
            text.push_str(&gitext_core::summary::build_summary(Some(body)).unwrap_or_default());
        }
        _ => {
            text.push_str(&revision.subject);
            if revision.has_multi_line_message {
                text.push_str(BODY_NOT_LOADED);
            }
        }
    }
    text
}

fn merge_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^merged? (pull request (?<pr>.*) from )?(.*branch |tag )?'?(?<with>[^ ']*[^ '.])'?( of [^ ]*[^ .])?( into (?<into>.*[^.]))?\.?$").unwrap()
    })
}

/// Parses a merge commit subject into (into, with).
pub fn parse_merge_message(subject: &str, append_pull_request: bool) -> (Option<String>, Option<String>) {
    let Some(m) = merge_regex().captures(subject) else {
        return (None, None);
    };
    let into = m.name("into").map(|g| g.as_str().to_string()).unwrap_or_else(|| "master".to_string());
    let mut with = m.name("with").map(|g| g.as_str().to_string()).unwrap_or_else(|| "?".to_string());
    if append_pull_request {
        if let Some(pr) = m.name("pr") {
            with.push_str(&format!(" by pull request {}", pr.as_str()));
        }
    }
    (Some(into), Some(with))
}

/// Port of `BranchFinder`: guesses the branch a commit was committed to.
pub struct BranchFinder {
    pub committed_to: Option<String>,
    pub merged_with: Option<String>,
}

impl BranchFinder {
    pub fn new(store: &Store, mut node: NodeIdx) -> Self {
        let mut f = BranchFinder { committed_to: None, merged_with: None };
        let mut parent: Option<NodeIdx> = None;
        loop {
            if f.check_for_merge(store, node, parent) || f.find_branch(store, node) {
                break;
            }
            // the first child (children are stored in insertion order here)
            let Some(&first_child) = store.nodes[node].children.first() else {
                break;
            };
            if store.nodes[first_child].revision.is_none() {
                break;
            }
            parent = Some(node);
            node = first_child;
        }
        f
    }

    fn find_branch(&mut self, store: &Store, node: NodeIdx) -> bool {
        let Some(rev) = store.nodes[node].revision.as_ref() else {
            return false;
        };
        for r in &rev.refs {
            if r.is_head() || r.is_remote() || r.is_stash() {
                self.committed_to = Some(r.name.clone());
                return true;
            }
        }
        false
    }

    fn check_for_merge(&mut self, store: &Store, node: NodeIdx, parent: Option<NodeIdx>) -> bool {
        let Some(rev) = store.nodes[node].revision.as_ref() else {
            return false;
        };
        let is_the_first_branch = parent.is_none() || store.parents(node).next() == parent;
        let (into, with) = parse_merge_message(&rev.subject, is_the_first_branch);
        if into.is_some() {
            self.committed_to = if is_the_first_branch { into } else { with.clone() };
        }
        if self.merged_with.is_none() {
            self.merged_with = Some(with.unwrap_or_default());
        }
        self.committed_to.is_some()
    }
}

#[cfg(test)]
mod tests {
    //! Ported from GitUI.Tests/UserControls/RevisionGrid/Graph/LaneInfoProviderTests.cs and
    //! LaneNodeLocatorTests.cs (using real graphs instead of mocks).
    use super::*;
    use crate::graph::RevisionGraphConfig;
    use gitext_core::{GitRef, GitRevision, ObjectId};

    #[test]
    fn merge_subjects_are_decoded() {
        let pr = " by pull request #1234";
        let cases = [
            ("Merge Branch xxx", "xxx", "master"),
            ("merge branch xxx", "xxx", "master"),
            ("merged branch xxx", "xxx", "master"),
            ("merge pull request #1234 from xxx", &*format!("xxx{pr}"), "master"),
            ("merge tag yyy of remote/branch", "yyy", "master"),
            ("merge tag yyy", "yyy", "master"),
            ("merge tag 'yyy'", "yyy", "master"),
            ("merge branch 'xxx'", "xxx", "master"),
            ("merge remote tracking branch xxx", "xxx", "master"),
            ("merge the branch xxx", "xxx", "master"),
            ("merge branch xxx into zzz", "xxx", "zzz"),
            ("Merged branch xxx.", "xxx", "master"),
            ("Merged branch xx.x.", "xx.x", "master"),
            ("Merged branch xxx into zzz.", "xxx", "zzz"),
            ("Merged branch xxx into zz.z.", "xxx", "zz.z"),
            ("Merged tag yyy.", "yyy", "master"),
            ("Merged tag yy.y.", "yy.y", "master"),
            ("Merged tag yyy of remote/branch.", "yyy", "master"),
            ("Merged tag yyy of remote/branc.h.", "yyy", "master"),
            ("Merged tag yyy of remote/branch into zzz.", "yyy", "zzz"),
            ("Merged tag yyy of remote/branch into zz.z.", "yyy", "zz.z"),
        ];
        for (subject, with, into) in cases {
            let (i, w) = parse_merge_message(subject, true);
            assert_eq!(w.as_deref(), Some(with), "{subject}");
            assert_eq!(i.as_deref(), Some(into), "{subject}");
        }
        assert_eq!(parse_merge_message("special merge", true), (None, None));
    }

    fn rev(id: ObjectId, subject: &str, parents: Vec<ObjectId>) -> GitRevision {
        let mut r = GitRevision::new(id).with_subject(subject).with_parents(parents);
        r.author = "John Doe".into();
        r
    }

    fn build(revisions: Vec<GitRevision>) -> RevisionGraph {
        let mut g = RevisionGraph::new(RevisionGraphConfig::default());
        for r in revisions {
            g.add(r);
        }
        g.loading_completed();
        let n = g.count();
        g.cache_to(n, n);
        g
    }

    #[test]
    fn find_prev_node_returns_not_found_for_invalid_input() {
        let g = build(vec![]);
        assert_eq!(find_prev_node(&g, 0, -1), (None, false, None));
        assert_eq!(find_prev_node(&g, -1, 0), (None, false, None));
        assert_eq!(find_prev_node(&g, 100, 0), (None, false, None));
    }

    #[test]
    fn find_prev_node_at_node_and_on_crossing_lane() {
        // 1 <- 2, 1 <- 3 (two branches)
        let (a, b, c) = (ObjectId::random(), ObjectId::random(), ObjectId::random());
        let g = build(vec![rev(c, "c", vec![a]), rev(b, "b", vec![a]), rev(a, "a", vec![])]);
        let (node, at, child) = find_prev_node(&g, 0, 0);
        assert_eq!(g.store.nodes[node.unwrap()].object_id, c);
        assert!(at);
        assert!(child.is_none());
        // row 1: lane 0 is segment c->a crossing, lane 1 is node b
        let (node, at, child) = find_prev_node(&g, 1, 0);
        assert_eq!(g.store.nodes[node.unwrap()].object_id, a);
        assert!(!at);
        assert_eq!(g.store.nodes[child.unwrap()].object_id, c);
        assert_eq!(find_prev_node(&g, 1, 7), (None, false, None));
    }

    #[test]
    fn get_lane_info_texts() {
        let g = build(vec![]);
        assert_eq!(get_lane_info(&g, 0, 0), "");

        let mut store = Store::default();
        let n = store.add_node(ObjectId::WORK_TREE, 0);
        assert_eq!(lane_info_text(&store, Some(n), false, None), NO_INFO_TEXT);

        let id = ObjectId::parse("a48da1aba59a65b2a7f0df7e3512817caf16819f").unwrap();
        let real = store.add_node(id, 1);
        let mut r = rev(id, "fix: bugs", vec![]);
        r.set_body(Some("fix: bugs\n\nall bugs fixed".into()));
        r.has_multi_line_message = true;
        store.nodes[real].revision = Some(r);
        assert_eq!(lane_info_text(&store, Some(real), true, None), format!("* {id}\n\nfix: bugs\n\nall bugs fixed"));

        // branch detection via refs of a child
        let child_id = ObjectId::random();
        let child = store.add_node(child_id, 0);
        let mut cr = rev(child_id, "child", vec![id]);
        cr.refs = vec![GitRef::from_complete_name(child_id, "refs/heads/feature")];
        store.nodes[child].revision = Some(cr);
        store.add_parent(child, real);
        assert_eq!(
            lane_info_text(&store, Some(real), false, Some(child)),
            format!("{}: child\n|\n{id}\n\nBranch: feature\nfix: bugs\n\nall bugs fixed", child_id.to_short_string())
        );

        // merge commit
        let mid = ObjectId::random();
        let m = store.add_node(mid, 2);
        let mut mr = rev(mid, "merge remote tracking branch upstream/branch", vec![]);
        mr.has_multi_line_message = true;
        store.nodes[m].revision = Some(mr);
        assert_eq!(
            lane_info_text(&store, Some(m), false, None),
            format!("{mid}\n\nBranch: master (merged with upstream/branch)\nmerge remote tracking branch upstream/branch{BODY_NOT_LOADED}")
        );
    }
}
