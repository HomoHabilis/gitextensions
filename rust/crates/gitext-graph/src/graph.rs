//! Port of `GitUI.UserControls.RevisionGrid.Graph.RevisionGraph`.

use std::collections::HashMap;

use gitext_core::{GitRevision, ObjectId};

use crate::model::{Lane, LaneSharing, NodeIdx, Row, SegIdx, Store};

/// Maximum number of lanes drawn.
pub const MAX_LANES: i32 = 40;
const ORDER_SEGMENTS_LOOK_AHEAD: usize = 50;
const STRAIGHTEN_LANES_LOOK_AHEAD: i32 = 20;

/// Port of `RevisionGraphConfig` (the graph related app settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevisionGraphConfig {
    pub merge_graph_lanes_having_common_parent: bool,
    pub render_graph_with_diagonals: bool,
    pub straighten_graph_diagonals: bool,
    pub straighten_graph_segments_limit: i32,
}

impl Default for RevisionGraphConfig {
    /// Defaults match `AppSettings` defaults.
    fn default() -> Self {
        RevisionGraphConfig {
            merge_graph_lanes_having_common_parent: true,
            render_graph_with_diagonals: true,
            straighten_graph_diagonals: true,
            straighten_graph_segments_limit: 80,
        }
    }
}

impl RevisionGraphConfig {
    pub fn reduce_graph_crossings(&self) -> bool {
        !self.merge_graph_lanes_having_common_parent
    }
}

/// Contains all structures needed to render the revision graph.
#[derive(Debug, Default)]
pub struct RevisionGraph {
    config: RevisionGraphConfig,
    pub store: Store,
    /// Revisions which can be displayed (loaded from the log), by id.
    revision_by_object_id: HashMap<ObjectId, NodeIdx>,
    /// Displayable revisions in insertion order (stable tie-break for sorting).
    displayable: Vec<NodeIdx>,
    /// Revisions seen as parents only.
    incomplete_revision_by_object_id: HashMap<ObjectId, NodeIdx>,
    loading_completed: bool,
    max_score: i32,
    ordered_nodes_cache: Vec<NodeIdx>,
    ordered_nodes_cache_invalid: bool,
    ordered_row_cache: Vec<Row>,
    ordered_row_cache_invalid_from_score: i32,
    pub only_first_parent: bool,
    pub head_id: ObjectId,
}

struct MoveLaneBy {
    row: usize,
    lane: i32,
    by: i32,
}

impl RevisionGraph {
    pub fn new(config: RevisionGraphConfig) -> Self {
        RevisionGraph {
            config,
            ordered_nodes_cache_invalid: true,
            ordered_row_cache_invalid_from_score: i32::MAX,
            ..Default::default()
        }
    }

    pub fn config(&self) -> RevisionGraphConfig {
        self.config
    }

    fn straighten_diagonals_look_ahead(&self) -> i32 {
        if self.config.straighten_graph_diagonals { STRAIGHTEN_LANES_LOOK_AHEAD / 2 } else { 0 }
    }

    fn straighten_look_ahead(&self) -> i32 {
        2 * (self.straighten_diagonals_look_ahead() + STRAIGHTEN_LANES_LOOK_AHEAD)
    }

    /// Clears the graph, applying a (possibly new) configuration.
    pub fn clear(&mut self, config: RevisionGraphConfig) {
        let only_first_parent = self.only_first_parent;
        *self = RevisionGraph::new(config);
        self.only_first_parent = only_first_parent;
    }

    pub fn loading_completed(&mut self) {
        self.loading_completed = true;
    }

    pub fn is_loading_completed(&self) -> bool {
        self.loading_completed
    }

    /// Number of displayable revisions.
    pub fn count(&self) -> usize {
        self.displayable.len()
    }

    pub fn contains(&self, id: &ObjectId) -> bool {
        self.revision_by_object_id.contains_key(id)
    }

    pub fn node(&self, idx: NodeIdx) -> &crate::model::Node {
        &self.store.nodes[idx]
    }

    /// Index of the last row with finally straightened lanes.
    pub fn get_cached_count(&mut self) -> usize {
        let cached_count = self.ordered_row_cache.len();
        if cached_count == 0 {
            return 0;
        }
        self.build_ordered_nodes_cache(usize::MAX);
        if self.is_row_cache_dirty() {
            return 0;
        }
        if self.loading_completed && cached_count == self.count() {
            cached_count
        } else {
            (cached_count as i64 - self.straighten_look_ahead() as i64).max(0) as usize
        }
    }

    /// Builds the ordered node cache up to `current_row_index` and the row cache up to
    /// `last_to_cache_row_index` (plus look-ahead for straightening).
    pub fn cache_to(&mut self, current_row_index: usize, last_to_cache_row_index: usize) {
        let look_ahead = self.straighten_look_ahead() as usize;
        let mut current_row_index = current_row_index.saturating_add(look_ahead);
        let mut last_to_cache_row_index = last_to_cache_row_index.saturating_add(look_ahead) as i64;
        if self.loading_completed {
            let max_row_index = self.count() as i64 - 1;
            current_row_index = (current_row_index as i64).min(max_row_index).max(0) as usize;
            last_to_cache_row_index = last_to_cache_row_index.min(max_row_index);
        }
        self.build_ordered_nodes_cache(current_row_index);
        self.build_ordered_row_cache(last_to_cache_row_index);
    }

    pub fn is_row_relative(&mut self, row: usize) -> bool {
        self.get_node_for_row(row).is_some_and(|n| self.store.nodes[n].is_relative)
    }

    pub fn try_get_node(&self, id: &ObjectId) -> Option<NodeIdx> {
        self.revision_by_object_id.get(id).copied()
    }

    pub fn try_get_row_index(&mut self, id: &ObjectId) -> Option<usize> {
        let node = self.try_get_node(id)?;
        if !self.loading_completed {
            self.build_ordered_nodes_cache(0);
            if let Some(i) = self.ordered_nodes_cache.iter().position(|&n| n == node) {
                return Some(i);
            }
        }
        self.build_ordered_nodes_cache(usize::MAX);
        self.ordered_nodes_cache.iter().position(|&n| n == node)
    }

    pub fn get_node_for_row(&mut self, row: usize) -> Option<NodeIdx> {
        self.build_ordered_nodes_cache(row);
        self.ordered_nodes_cache.get(row).copied()
    }

    /// The revision displayed at `row`, if loaded.
    pub fn get_revision_for_row(&mut self, row: usize) -> Option<&GitRevision> {
        let n = self.get_node_for_row(row)?;
        self.store.nodes[n].revision.as_ref()
    }

    /// Port of `GetSegmentsForRow`. Returns `None` if the row is not (validly) cached.
    pub fn get_segments_for_row(&mut self, row: i64) -> Option<&Row> {
        if row < 0 {
            return None;
        }
        self.build_ordered_nodes_cache(row as usize);
        if self.is_row_cache_dirty() {
            return None;
        }
        self.ordered_row_cache.get(row as usize)
    }

    /// Validates the row cache (like `GetSegmentsForRow`) and returns whether `row` is available
    /// via [`RevisionGraph::row`].
    pub fn validate_row(&mut self, row: i64) -> bool {
        self.get_segments_for_row(row).is_some()
    }

    /// Row lookup without cache validation (use after `cache_to` in the same frame).
    pub fn row(&self, row: i64) -> Option<&Row> {
        if row < 0 {
            return None;
        }
        self.ordered_row_cache.get(row as usize)
    }

    /// Rows currently cached and valid.
    pub fn valid_row_count(&mut self) -> usize {
        self.build_ordered_nodes_cache(usize::MAX);
        if self.is_row_cache_dirty() { 0 } else { self.ordered_row_cache.len() }
    }

    pub fn highlight_branch(&mut self, id: &ObjectId) {
        for n in &mut self.store.nodes {
            n.is_relative = false;
        }
        if let Some(node) = self.try_get_node(id) {
            self.store.make_relative(node);
        }
    }

    /// Adds a revision from the git log, including segments to its parents.
    /// Returns whether the row cache was invalidated.
    pub fn add(&mut self, revision: GitRevision) -> bool {
        let mut min_existing_score;
        let id = revision.object_id;
        let node = if let Some(node) = self.incomplete_revision_by_object_id.remove(&id) {
            min_existing_score = self.store.nodes[node].score;
            self.max_score += 1;
            self.store.nodes[node].score = self.max_score;
            node
        } else {
            self.max_score += 1;
            let node = self.store.add_node(id, self.max_score);
            min_existing_score = self.max_score;
            node
        };

        if self.head_id == id {
            self.store.nodes[node].is_relative = true;
        }

        let parents: Vec<ObjectId> = revision.parent_ids.clone().unwrap_or_default();
        self.store.nodes[node].revision = Some(revision);

        for parent_id in parents {
            let parent = if let Some(&parent) = self.incomplete_revision_by_object_id.get(&parent_id) {
                self.max_score += 1;
                self.store.nodes[parent].score = self.max_score;
                parent
            } else if let Some(&parent) = self.revision_by_object_id.get(&parent_id) {
                min_existing_score = min_existing_score.min(self.store.nodes[parent].score);
                self.max_score += 1;
                self.max_score = self.store.ensure_score_is_above(parent, self.max_score);
                parent
            } else {
                self.max_score += 1;
                let parent = self.store.add_node(parent_id, self.max_score);
                self.incomplete_revision_by_object_id.insert(parent_id, parent);
                parent
            };
            self.store.add_parent(node, parent);
            if self.only_first_parent {
                break;
            }
        }

        if self.revision_by_object_id.insert(id, node).is_none() {
            self.displayable.push(node);
        }
        self.mark_cache_as_invalid_if_needed(min_existing_score)
    }

    /// Inserts the artificial work tree and index revisions before the first of `parents`
    /// found in the graph.
    pub fn insert(&mut self, work_tree_rev: GitRevision, index_rev: GitRevision, parents: &[ObjectId]) -> bool {
        let mut insert_score = i32::MIN;
        for parent_id in parents {
            if let Some(parent) = self.try_get_node(parent_id) {
                const INSERT_RANGE: i32 = 2;
                let limit_score = self.store.nodes[parent].score;
                insert_score = limit_score - INSERT_RANGE;
                for &n in &self.displayable {
                    let node = &mut self.store.nodes[n];
                    if node.score < limit_score {
                        node.score -= INSERT_RANGE;
                    }
                }
                break;
            }
        }

        let wt_id = work_tree_rev.object_id;
        let idx_id = index_rev.object_id;
        let wt = self.store.add_node(wt_id, insert_score);
        self.store.nodes[wt].revision = Some(work_tree_rev);
        let ix = self.store.add_node(idx_id, insert_score + 1);
        self.store.nodes[ix].revision = Some(index_rev);
        self.store.add_parent(wt, ix);
        self.revision_by_object_id.insert(wt_id, wt);
        self.revision_by_object_id.insert(idx_id, ix);
        self.displayable.push(wt);
        self.displayable.push(ix);
        self.mark_cache_as_invalid_if_needed(insert_score)
    }

    fn is_row_cache_dirty(&mut self) -> bool {
        let Some(last) = self.ordered_row_cache.last() else {
            self.ordered_row_cache_invalid_from_score = i32::MAX;
            return false;
        };
        if self.ordered_row_cache_invalid_from_score <= self.store.nodes[last.revision].score
            || self.ordered_row_cache.len() > self.ordered_nodes_cache.len()
        {
            return true;
        }
        let index_to_compare = self.ordered_row_cache.len() - 1;
        self.ordered_row_cache[index_to_compare].revision != self.ordered_nodes_cache[index_to_compare]
            || self.ordered_row_cache[0].revision != self.ordered_nodes_cache[0]
    }

    fn build_ordered_row_cache(&mut self, mut last_to_cache_row_index: i64) {
        let order_segments = self.config.reduce_graph_crossings();
        let ordered_nodes_count = self.ordered_nodes_cache.len() as i64;
        let last_ordered_node_index = ordered_nodes_count - 1;
        let mut loading_completed = self.loading_completed;
        if self.is_row_cache_dirty() {
            self.ordered_row_cache_invalid_from_score = i32::MAX;
            self.ordered_row_cache.clear();
        }

        let max_last = last_ordered_node_index
            - if loading_completed || !order_segments { 0 } else { ORDER_SEGMENTS_LOOK_AHEAD as i64 };
        if last_to_cache_row_index > max_last {
            last_to_cache_row_index = max_last;
            loading_completed = false;
        }

        let start_index = self.ordered_row_cache.len() as i64;
        if start_index > last_to_cache_row_index {
            return;
        }

        let merge = self.config.merge_graph_lanes_having_common_parent;
        for next_index in start_index as usize..=last_to_cache_row_index as usize {
            let revision = self.ordered_nodes_cache[next_index];
            let mut revision_start_segments: Vec<SegIdx> = self.store.nodes[revision].start_segments.clone();
            if order_segments {
                revision_start_segments = order(&self.store, &revision_start_segments, &self.ordered_nodes_cache, next_index);
            }

            let segments: Vec<SegIdx>;
            if next_index == 0 {
                segments = revision_start_segments.clone();
                let mut prev_segment = None;
                for &start_segment in &revision_start_segments {
                    let li = self.store.new_lane_info(start_segment, prev_segment, None);
                    self.store.segments[start_segment].lane_info = Some(li);
                    prev_segment = Some(start_segment);
                }
            } else {
                let previous_row = &self.ordered_row_cache[next_index - 1];
                let prev_revision = previous_row.revision;
                let prev_segments = previous_row.segments.clone();
                let mut segs = Vec::with_capacity(prev_segments.len() + revision_start_segments.len());
                let mut start_segments_added = false;
                let prev_count = prev_segments.len();
                for prev_idx in 0..prev_count {
                    let segment = prev_segments[prev_idx];
                    if self.store.segments[segment].parent == prev_revision {
                        continue;
                    }
                    segs.push(segment);

                    if revision == self.store.segments[segment].parent {
                        let mut prev_segment = segment;
                        let next_segment = ((prev_idx + 1)..prev_count).map(|i| prev_segments[i]).find(|&s| {
                            let p = self.store.segments[s].parent;
                            p != prev_revision && p != revision
                        });

                        if !start_segments_added {
                            start_segments_added = true;
                            segs.extend_from_slice(&revision_start_segments);
                        }

                        let segment_lane_info = self.store.segments[segment].lane_info;
                        for (i, &start_segment) in revision_start_segments.iter().enumerate() {
                            if i == 0 {
                                let current = self.store.segments[start_segment].lane_info;
                                let replace = match current {
                                    None => true,
                                    Some(cur) => match segment_lane_info {
                                        Some(sli) => self.store.lane_info_start_score(&cur) > self.store.lane_info_start_score(&sli),
                                        None => false,
                                    },
                                };
                                if replace {
                                    self.store.segments[start_segment].lane_info = segment_lane_info;
                                }
                            } else if self.store.segments[start_segment].lane_info.is_none() {
                                let li = match segment_lane_info {
                                    None => self.store.new_lane_info(start_segment, Some(prev_segment), next_segment),
                                    Some(derived) => self.store.new_lane_info_derived(start_segment, Some(prev_segment), next_segment, derived),
                                };
                                self.store.segments[start_segment].lane_info = Some(li);
                            }
                            prev_segment = start_segment;
                        }
                    }
                }

                if !start_segments_added {
                    let mut prev_segment = segs.last().copied();
                    segs.extend_from_slice(&revision_start_segments);
                    for &start_segment in &revision_start_segments {
                        let li = self.store.new_lane_info(start_segment, prev_segment, None);
                        self.store.segments[start_segment].lane_info = Some(li);
                        prev_segment = Some(start_segment);
                    }
                }
                segments = segs;
            }

            self.ordered_row_cache.push(Row::new(revision, segments, merge));
        }

        let loading_completed = loading_completed && last_to_cache_row_index == last_ordered_node_index;
        let start_index = start_index as i32;
        let last = last_to_cache_row_index as i32;
        let straighten_lanes_start_index = 1.max(start_index - STRAIGHTEN_LANES_LOOK_AHEAD);
        let straighten_lanes_last_index = if loading_completed { last - 1 } else { last - STRAIGHTEN_LANES_LOOK_AHEAD };
        let limit = self.config.straighten_graph_segments_limit;
        straighten_lanes(&self.store, &self.ordered_row_cache, straighten_lanes_start_index, straighten_lanes_last_index, last, limit);

        let diag = self.straighten_diagonals_look_ahead();
        if diag > 0 {
            let start = 1.max(start_index - STRAIGHTEN_LANES_LOOK_AHEAD - diag);
            let last_straighten = if loading_completed { last - 1 } else { last - STRAIGHTEN_LANES_LOOK_AHEAD - diag };
            straighten_diagonals(&self.store, &self.ordered_row_cache, start, last_straighten, last, diag, limit);
        }
    }

    fn build_ordered_nodes_cache(&mut self, current_row_index: usize) {
        let count = self.count();
        if !self.ordered_nodes_cache_invalid
            && self.ordered_nodes_cache.len() as i64 > (count as i64 - 1).min(current_row_index.min(i64::MAX as usize) as i64)
        {
            return;
        }
        self.ordered_nodes_cache_invalid = false;
        let mut nodes = self.displayable.clone();
        let store = &self.store;
        nodes.sort_by_key(|&n| store.nodes[n].score);
        self.ordered_nodes_cache = nodes;
    }

    fn mark_cache_as_invalid_if_needed(&mut self, min_score: i32) -> bool {
        if !self.ordered_nodes_cache_invalid {
            if let Some(&last) = self.ordered_nodes_cache.last() {
                if min_score <= self.store.nodes[last].score {
                    self.ordered_nodes_cache_invalid = true;
                }
            }
        }
        self.ordered_row_cache_invalid_from_score = min_score;
        self.ordered_nodes_cache_invalid
    }

    /// Brute force topo order validation (test accessor in C#).
    pub fn validate_topo_order(&self) -> bool {
        for &n in &self.displayable {
            let node = &self.store.nodes[n];
            if self.store.parents(n).any(|p| self.store.nodes[p].score <= node.score) {
                return false;
            }
            if node.children.iter().any(|&c| node.score <= self.store.nodes[c].score) {
                return false;
            }
        }
        true
    }
}

/// Port of the local `Order` function: orders the start segments of a revision to reduce crossings.
fn order(store: &Store, segments: &[SegIdx], ordered_nodes: &[NodeIdx], next_index: usize) -> Vec<SegIdx> {
    let end_index = (next_index + ORDER_SEGMENTS_LOOK_AHEAD).min(ordered_nodes.len());
    let get_row_index = |revision: NodeIdx| -> i32 {
        for index in next_index + 1..end_index {
            if ordered_nodes[index] == revision {
                return (index - next_index) as i32;
            }
        }
        i32::MAX
    };

    fn is_ancestor_of(store: &Store, ancestor: NodeIdx, child: NodeIdx, stop_row: i32, get_row_index: &dyn Fn(NodeIdx) -> i32) -> bool {
        if store.parents(child).any(|p| p == ancestor) {
            return true;
        }
        for parent in store.parents(child) {
            if get_row_index(parent) < stop_row && is_ancestor_of(store, ancestor, parent, stop_row, get_row_index) {
                return true;
            }
        }
        false
    }

    let score = |segment: SegIdx, row: i32| -> i64 {
        let parent = &store.nodes[store.segments[segment].parent];
        let grand_parent_count = parent.parent_count();
        let row = row as i64;
        if grand_parent_count == 0 {
            row
        } else if grand_parent_count >= 2 {
            -2_000_000_000 + row
        } else if parent.children.len() >= 2 {
            -1_000_000_000 + row
        } else {
            row
        }
    };

    let compare = |a: SegIdx, b: SegIdx| -> std::cmp::Ordering {
        let pa = store.segments[a].parent;
        let pb = store.segments[b].parent;
        let row_a = get_row_index(pa);
        let row_b = get_row_index(pb);
        if row_a != i32::MAX && row_b != i32::MAX {
            if row_a > row_b && is_ancestor_of(store, pa, pb, row_a, &get_row_index) {
                return std::cmp::Ordering::Less;
            } else if row_b > row_a && is_ancestor_of(store, pb, pa, row_b, &get_row_index) {
                return std::cmp::Ordering::Greater;
            }
        }
        score(a, row_a).cmp(&score(b, row_b))
    };

    // Stable insertion sort which tolerates non-transitive comparers (like LINQ OrderBy).
    let mut result: Vec<SegIdx> = Vec::with_capacity(segments.len());
    for &s in segments {
        let mut pos = result.len();
        while pos > 0 && compare(s, result[pos - 1]) == std::cmp::Ordering::Less {
            pos -= 1;
        }
        result.insert(pos, s);
    }
    result
}

fn straighten_lanes(store: &Store, rows: &[Row], start_index: i32, last_straighten_index: i32, last_look_ahead_index: i32, limit: i32) {
    let mut go_back_limit = 1;
    let mut current_index = start_index;
    while current_index <= last_straighten_index {
        go_back_limit = go_back_limit.max(current_index - STRAIGHTEN_LANES_LOOK_AHEAD);
        let current_row = &rows[current_index as usize];
        if current_row.segments.len() as i32 > limit {
            current_index += 1;
            continue;
        }
        let mut moved = false;
        let previous_row = &rows[current_index as usize - 1];
        for &segment in current_row.segments.iter().take(MAX_LANES as usize) {
            let current_row_lane = current_row.get_lane_for_segment(store, segment);
            if current_row_lane.sharing != LaneSharing::ExclusiveOrPrimary {
                continue;
            }
            let current_lane = current_row_lane.index;
            let previous_lane = previous_row.get_lane_for_segment(store, segment).index;
            if previous_lane <= current_lane {
                continue;
            }
            let straightened_current_lane = current_lane + 1;
            let mut look_ahead_lane = current_lane;
            let mut segment_or_ancestor = current_row.first_parent_or_self(store, segment);
            let mut look_ahead_index = current_index + 1;
            while look_ahead_lane == current_lane
                && look_ahead_index <= (current_index + STRAIGHTEN_LANES_LOOK_AHEAD).min(last_look_ahead_index)
            {
                let look_ahead_row = &rows[look_ahead_index as usize];
                look_ahead_lane = look_ahead_row.get_lane_for_segment(store, segment_or_ancestor).index;
                if look_ahead_lane == straightened_current_lane
                    || (look_ahead_lane > straightened_current_lane && previous_lane == straightened_current_lane)
                {
                    for move_index in current_index..look_ahead_index {
                        rows[move_index as usize].move_lanes_right(store, current_lane);
                    }
                    moved = true;
                    break;
                }
                segment_or_ancestor = look_ahead_row.first_parent_or_self(store, segment_or_ancestor);
                look_ahead_index += 1;
            }
            if moved {
                break;
            }
        }
        current_index = if moved { (current_index - STRAIGHTEN_LANES_LOOK_AHEAD).max(go_back_limit) } else { current_index + 1 };
    }
}

fn straighten_diagonals(
    store: &Store,
    rows: &[Row],
    start_index: i32,
    last_straighten_index: i32,
    last_look_ahead_index: i32,
    look_ahead: i32,
    limit: i32,
) {
    let lane_of = |row: i32, segment: SegIdx| -> Lane { rows[row as usize].get_lane_for_segment(store, segment) };
    let mut move_lane_by: Vec<MoveLaneBy> = Vec::with_capacity(look_ahead as usize);
    let mut go_back_limit = 1;
    let mut current_index = start_index;
    while current_index <= last_straighten_index {
        go_back_limit = go_back_limit.max(current_index - look_ahead);
        let current_last_look_ahead_index = (current_index + look_ahead).min(last_look_ahead_index);
        let current_row = &rows[current_index as usize];
        if current_row.segments.len() as i32 > limit {
            current_index += 1;
            continue;
        }

        let mut moved = false;
        for &segment in current_row.segments.iter().take(MAX_LANES as usize) {
            let current_row_lane = current_row.get_lane_for_segment(store, segment);
            if current_row_lane.sharing != LaneSharing::ExclusiveOrPrimary {
                continue;
            }
            let current_lane = current_row_lane.index;
            let previous_lane = lane_of(current_index - 1, segment).index;

            let is_prev_lane_diagonal = |diagonal_delta: i32| -> bool {
                if current_index < 2 {
                    return false;
                }
                let prev_prev_lane = lane_of(current_index - 2, segment).index;
                prev_prev_lane >= 0 && prev_prev_lane == previous_lane + diagonal_delta
            };

            // Unfold one-lane shift to diagonal
            if current_lane == previous_lane - 1 && current_index + 2 <= current_last_look_ahead_index {
                let mut segment_or_ancestor = current_row.first_parent_or_self(store, segment);
                let next_row = &rows[current_index as usize + 1];
                let next_lane = next_row.get_lane_for_segment(store, segment_or_ancestor).index;
                if next_lane == current_lane {
                    segment_or_ancestor = next_row.first_parent_or_self(store, segment_or_ancestor);
                    let end_lane = lane_of(current_index + 2, segment_or_ancestor).index;
                    if end_lane >= 0 && end_lane == next_lane - 1 && !is_prev_lane_diagonal(1) {
                        current_row.move_lanes_right(store, current_lane);
                        moved = true;
                        break;
                    }
                }
            }

            let mut turn_multi_lane_crossing_into_diagonal = |diagonal_delta: i32| -> bool {
                move_lane_by.clear();
                let mut segment_or_ancestor = segment;
                let mut diagonal_lane = if previous_lane >= 0 { previous_lane } else { current_lane };
                for look_ahead_index in current_index..=current_last_look_ahead_index {
                    diagonal_lane += diagonal_delta;
                    let end_row = &rows[look_ahead_index as usize];
                    let end_lane = end_row.get_lane_for_segment(store, segment_or_ancestor);
                    let move_by = diagonal_lane - end_lane.index;
                    let last_chance = end_lane.sharing == LaneSharing::DifferentStart;
                    if move_by < 0 || end_lane.index < 0 || !(end_lane.sharing == LaneSharing::ExclusiveOrPrimary || last_chance) {
                        return false;
                    }
                    if move_by >= 2 && move_lane_by.len() == 2 && look_ahead_index == current_index + 3 && move_lane_by[1].by == 1 {
                        let m = &move_lane_by[0];
                        rows[m.row].move_lanes_right_by(store, m.lane, m.by);
                        return true;
                    }
                    if move_by == 0 && !move_lane_by.is_empty() {
                        for m in &move_lane_by {
                            rows[m.row].move_lanes_right_by(store, m.lane, m.by);
                        }
                        return true;
                    }
                    if last_chance {
                        return false;
                    }
                    if move_by > 0 {
                        move_lane_by.push(MoveLaneBy { row: look_ahead_index as usize, lane: end_lane.index, by: move_by });
                    }
                    segment_or_ancestor = end_row.first_parent_or_self(store, segment_or_ancestor);
                }
                false
            };

            moved = turn_multi_lane_crossing_into_diagonal(1) || turn_multi_lane_crossing_into_diagonal(-1);
            if moved {
                break;
            }

            // Join multi-lane crossings
            let delta_prev = previous_lane - current_lane;
            if previous_lane >= 0 && delta_prev.abs() >= 1 {
                let mut segment_or_ancestor = current_row.first_parent_or_self(store, segment);
                let next_row = &rows[current_index as usize + 1];
                let next_lane = next_row.get_lane_for_segment(store, segment_or_ancestor).index;
                let delta_next = current_lane - next_lane;
                let mut is_next_lane_diagonal = || -> bool {
                    if current_index + 2 > current_last_look_ahead_index {
                        return false;
                    }
                    segment_or_ancestor = next_row.first_parent_or_self(store, segment_or_ancestor);
                    let next_next_lane = lane_of(current_index + 2, segment_or_ancestor).index;
                    next_next_lane >= 0 && next_next_lane == next_lane - delta_next.signum()
                };
                if next_lane >= 0
                    && delta_next.signum() == delta_prev.signum()
                    && (delta_next + delta_prev).abs() >= 3
                    && !is_prev_lane_diagonal(delta_prev.signum())
                    && !is_next_lane_diagonal()
                {
                    let move_by = if delta_next < 0 { -delta_next } else { delta_prev };
                    current_row.move_lanes_right_by(store, current_lane, move_by);
                    moved = true;
                    break;
                }
            }
        }

        current_index = if moved { (current_index - look_ahead).max(go_back_limit) } else { current_index + 1 };
    }
}
