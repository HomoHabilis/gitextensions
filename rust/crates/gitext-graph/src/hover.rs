//! Port of `HoverHighlightCalculator`: ancestry highlighting while hovering a ref label.
//! The debouncing is left to the UI; [`HoverHighlight::set`] computes synchronously.

use std::collections::HashSet;

use gitext_core::{GitRef, ObjectId};

use crate::graph::RevisionGraph;
use crate::model::NodeIdx;

/// Visible row range of the revision grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleRowRange {
    pub from_index: usize,
    pub count: usize,
}

/// Set of commit ids to highlight while a ref label is hovered.
#[derive(Debug, Default)]
pub struct HoverHighlight {
    highlighted_ids: Option<HashSet<ObjectId>>,
    is_dirty: bool,
}

impl HoverHighlight {
    pub fn highlighted_ids(&self) -> Option<&HashSet<ObjectId>> {
        self.highlighted_ids.as_ref()
    }

    pub fn is_dirty(&self) -> bool {
        self.is_dirty
    }

    /// Returns the dirty state and resets it.
    pub fn consume_is_dirty(&mut self) -> bool {
        std::mem::take(&mut self.is_dirty)
    }

    pub fn clear(&mut self) {
        if self.highlighted_ids.take().is_some() {
            self.is_dirty = true;
        }
    }

    /// Updates the highlight to the ancestry of `git_ref` (at `row_index`) and its tracked
    /// remote / tracking local branch. Returns `false` if nothing changed (C#: cancelled).
    pub fn set(&mut self, graph: &mut RevisionGraph, git_ref: Option<&GitRef>, row_index: i64, visible: VisibleRowRange) -> bool {
        let Some(git_ref) = git_ref.filter(|_| row_index >= 0) else {
            let had = self.highlighted_ids.is_some();
            self.clear();
            return had;
        };
        let row_index = row_index as usize;
        let Some(hovered) = graph.get_node_for_row(row_index) else {
            return false;
        };

        let max_children_above = 50.max(visible.count);
        let mut visible_ids: HashSet<ObjectId> = HashSet::with_capacity(max_children_above + 2 * visible.count);
        let mut ancestor_ids: HashSet<ObjectId> = HashSet::new();

        fn add_id_and_parents(graph: &mut RevisionGraph, row: usize, visible_ids: &mut HashSet<ObjectId>) -> Option<NodeIdx> {
            let rev = graph.get_node_for_row(row)?;
            visible_ids.insert(graph.store.nodes[rev].object_id);
            if let Some(graph_row) = graph.row(row as i64) {
                for &s in &graph_row.segments {
                    visible_ids.insert(graph.store.nodes[graph.store.segments[s].parent].object_id);
                }
            }
            Some(rev)
        }

        let is_in_branch_group = |r: &GitRef| git_ref.is_tracking_remote(Some(r)) || r.is_tracking_remote(Some(git_ref));
        let has_group_ref = |graph: &RevisionGraph, n: NodeIdx| {
            graph.store.nodes[n].revision.as_ref().is_some_and(|rev| rev.refs.iter().any(&is_in_branch_group))
        };

        if add_id_and_parents(graph, row_index, &mut visible_ids).is_none() {
            return false;
        }
        let check_other_refs =
            (git_ref.is_remote() || (git_ref.is_head() && !git_ref.merge_with.is_empty())) && !has_group_ref(graph, hovered);

        let mut below_rev = None;
        let visible_to = (visible.from_index + visible.count) as i64 - 1;
        let mut row = row_index as i64 + 1;
        while row <= visible_to {
            let Some(rev) = add_id_and_parents(graph, row as usize, &mut visible_ids) else {
                return false;
            };
            if check_other_refs && has_group_ref(graph, rev) {
                below_rev = Some(rev);
            }
            row += 1;
        }

        walk_ancestors(graph, hovered, &mut ancestor_ids, &visible_ids);
        if let Some(below) = below_rev {
            walk_ancestors(graph, below, &mut ancestor_ids, &visible_ids);
        } else if check_other_refs {
            let search_from = visible.from_index.saturating_sub(max_children_above);
            let mut row = row_index as i64 - 1;
            while row >= search_from as i64 {
                let Some(rev) = add_id_and_parents(graph, row as usize, &mut visible_ids) else {
                    return false;
                };
                if has_group_ref(graph, rev) {
                    walk_ancestors(graph, rev, &mut ancestor_ids, &visible_ids);
                    break;
                }
                row -= 1;
            }
        }

        let new_ids = (!ancestor_ids.is_empty()).then_some(ancestor_ids);
        if new_ids == self.highlighted_ids {
            return false;
        }
        self.highlighted_ids = new_ids;
        self.is_dirty = true;
        true
    }
}

fn walk_ancestors(graph: &RevisionGraph, revision: NodeIdx, result: &mut HashSet<ObjectId>, visible_ids: &HashSet<ObjectId>) {
    let mut stack = vec![revision];
    let mut visited = HashSet::new();
    while let Some(current) = stack.pop() {
        let id = graph.store.nodes[current].object_id;
        if !visited.insert(id) || result.contains(&id) {
            continue;
        }
        if visible_ids.contains(&id) {
            result.insert(id);
        }
        stack.extend(graph.store.parents(current));
    }
}

#[cfg(test)]
mod tests {
    //! Ported from GitUI.Tests/UserControls/RevisionGrid/Graph/HoverHighlightCalculatorTests.cs
    use super::*;
    use crate::graph::RevisionGraphConfig;
    use gitext_core::GitRevision;

    const TIP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const PARENT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const ROOT: &str = "cccccccccccccccccccccccccccccccccccccccc";
    const OTHER: &str = "dddddddddddddddddddddddddddddddddddddddd";

    fn id(s: &str) -> ObjectId {
        ObjectId::parse(s).unwrap()
    }

    fn branch(name: &str) -> GitRef {
        GitRef::from_complete_name(ObjectId::random(), format!("refs/heads/{name}"))
    }

    fn revision(i: &str, parents: &[&str], refs: Vec<GitRef>) -> GitRevision {
        let mut r = GitRevision::new(id(i)).with_parents(parents.iter().map(|p| id(p)).collect());
        r.refs = refs;
        r
    }

    fn row_for(graph: &mut RevisionGraph, rows: usize, i: &str) -> i64 {
        (0..rows).find(|&r| { let n = graph.get_node_for_row(r); n.map(|n| graph.store.nodes[n].object_id) == Some(id(i)) }).unwrap() as i64
    }

    fn set(ids: &[&str]) -> HashSet<ObjectId> {
        ids.iter().map(|s| id(s)).collect()
    }

    #[test]
    fn should_include_tip_and_ancestors_within_visible_range() {
        let mut g = RevisionGraph::new(RevisionGraphConfig::default());
        let main = branch("main");
        g.add(revision(TIP, &[PARENT], vec![main.clone()]));
        g.add(revision(OTHER, &[ROOT], vec![branch("feature")]));
        g.add(revision(PARENT, &[ROOT], vec![]));
        g.add(revision(ROOT, &[], vec![]));
        g.cache_to(3, 3);
        let mut h = HoverHighlight::default();
        let row = row_for(&mut g, 4, TIP);
        h.set(&mut g, Some(&main), row, VisibleRowRange { from_index: 0, count: 4 });
        assert_eq!(h.highlighted_ids(), Some(&set(&[TIP, PARENT, ROOT])));
    }

    #[test]
    fn should_include_one_ancestor_outside_visible_range() {
        let mut g = RevisionGraph::new(RevisionGraphConfig::default());
        let main = branch("main");
        g.add(revision(TIP, &[PARENT], vec![main.clone()]));
        g.add(revision(PARENT, &[ROOT], vec![]));
        g.add(revision(ROOT, &[], vec![]));
        g.cache_to(2, 2);
        let mut h = HoverHighlight::default();
        let row = row_for(&mut g, 3, TIP);
        h.set(&mut g, Some(&main), row, VisibleRowRange { from_index: 0, count: 2 });
        assert_eq!(h.highlighted_ids(), Some(&set(&[TIP, PARENT, ROOT])));
    }

    #[test]
    fn should_clear_and_not_mark_dirty_when_unchanged() {
        let mut g = RevisionGraph::new(RevisionGraphConfig::default());
        let main = branch("main");
        g.add(revision(TIP, &[PARENT], vec![main.clone()]));
        g.add(revision(PARENT, &[], vec![]));
        g.cache_to(1, 1);
        let range = VisibleRowRange { from_index: 0, count: 2 };
        let mut h = HoverHighlight::default();
        let row = row_for(&mut g, 2, TIP);
        assert!(h.set(&mut g, Some(&main), row, range));
        assert!(h.consume_is_dirty());
        assert!(!h.is_dirty());
        // unchanged: "cancelled", not dirty
        assert!(!h.set(&mut g, Some(&main), row, range));
        assert!(!h.is_dirty());
        // clear
        h.set(&mut g, None, -1, range);
        assert!(h.highlighted_ids().is_none());
        assert!(h.is_dirty());
    }
}
