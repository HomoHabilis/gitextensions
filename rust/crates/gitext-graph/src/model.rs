//! Nodes, segments, lanes and rows: port of `RevisionGraphRevision`, `RevisionGraphSegment`,
//! `Lane`, `LaneSharing`, `LaneInfo` and `RevisionGraphRow`.
//!
//! The C# implementation uses object references between revisions and segments; here all
//! nodes and segments live in a [`Store`] arena and reference each other by index.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use gitext_core::{GitRevision, ObjectId};

/// Index of a node in [`Store::nodes`].
pub type NodeIdx = usize;
/// Index of a segment in [`Store::segments`].
pub type SegIdx = usize;

/// Number of distinct lane colors (`RevisionGraphLaneColor.PresetGraphBrushes.Count`).
pub const LANE_COLOR_COUNT: i32 = 8;

/// Port of `RevisionGraphLaneColor.GetColorForLane`.
pub fn color_for_lane(seed: i32) -> i32 {
    (seed.unsigned_abs() % LANE_COLOR_COUNT as u32) as i32
}

/// A revision (node) in the graph.
#[derive(Debug)]
pub struct Node {
    pub object_id: ObjectId,
    /// Used to order the revisions in topo-order.
    pub score: i32,
    pub revision: Option<GitRevision>,
    /// Part of the highlighted branch (ancestor of the selected/checked out revision).
    pub is_relative: bool,
    /// Segments to the parents, in parent order.
    pub start_segments: Vec<SegIdx>,
    /// Children, in the order they were added.
    pub children: Vec<NodeIdx>,
}

impl Node {
    pub fn new(object_id: ObjectId, score: i32) -> Self {
        Node { object_id, score, revision: None, is_relative: false, start_segments: Vec::new(), children: Vec::new() }
    }

    pub fn parent_count(&self) -> usize {
        self.start_segments.len()
    }
}

/// Port of `LaneInfo`: color and origin of a lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneInfo {
    pub color: i32,
    pub start_revision: NodeIdx,
}

/// The connection between a child and a parent revision. May span multiple rows.
#[derive(Debug)]
pub struct Segment {
    pub parent: NodeIdx,
    pub child: NodeIdx,
    pub lane_info: Option<LaneInfo>,
    /// Remembers whether this segment has already shared a lane with another segment.
    pub(crate) is_secondary_shared_lane_at_least_since_score: Cell<i32>,
}

impl Segment {
    pub fn new(parent: NodeIdx, child: NodeIdx) -> Self {
        Segment { parent, child, lane_info: None, is_secondary_shared_lane_at_least_since_score: Cell::new(i32::MAX) }
    }
}

/// Arena of nodes and segments.
#[derive(Debug, Default)]
pub struct Store {
    pub nodes: Vec<Node>,
    pub segments: Vec<Segment>,
}

impl Store {
    pub fn add_node(&mut self, object_id: ObjectId, score: i32) -> NodeIdx {
        self.nodes.push(Node::new(object_id, score));
        self.nodes.len() - 1
    }

    /// Port of `RevisionGraphRevision.Parents`.
    pub fn parents(&self, node: NodeIdx) -> impl Iterator<Item = NodeIdx> + '_ {
        self.nodes[node].start_segments.iter().map(|&s| self.segments[s].parent)
    }

    /// Port of `LaneInfo.StartScore`.
    pub fn lane_info_start_score(&self, lane_info: &LaneInfo) -> i32 {
        self.nodes[lane_info.start_revision].score
    }

    /// Port of `RevisionGraphRevision.AddParent`.
    pub fn add_parent(&mut self, child: NodeIdx, parent: NodeIdx) {
        debug_assert!(self.nodes[parent].score > self.nodes[child].score, "Parent score must be higher than for the child.");
        if self.nodes[child].is_relative {
            self.make_relative(parent);
        }
        self.nodes[parent].children.push(child);
        self.segments.push(Segment::new(parent, child));
        let seg = self.segments.len() - 1;
        self.nodes[child].start_segments.push(seg);
    }

    /// Test helper mirroring `RevisionGraphRevision.TestAccessor.AddParent`.
    pub fn add_parent_ensuring_score(&mut self, child: NodeIdx, parent: NodeIdx) {
        let min = self.nodes[child].score + 1;
        self.ensure_score_is_above(parent, min);
        self.add_parent(child, parent);
    }

    /// Port of `RevisionGraphRevision.MakeRelative`.
    pub fn make_relative(&mut self, node: NodeIdx) {
        if self.nodes[node].is_relative {
            return;
        }
        if self.nodes[node].parent_count() == 0 {
            self.nodes[node].is_relative = true;
            return;
        }
        let mut stack = vec![node];
        while let Some(rev) = stack.pop() {
            self.nodes[rev].is_relative = true;
            for i in 0..self.nodes[rev].start_segments.len() {
                let parent = self.segments[self.nodes[rev].start_segments[i]].parent;
                if !self.nodes[parent].is_relative {
                    stack.push(parent);
                }
            }
        }
    }

    /// Port of `RevisionGraphRevision.EnsureScoreIsAbove`. Returns the max score assigned.
    pub fn ensure_score_is_above(&mut self, node: NodeIdx, minimal_score: i32) -> i32 {
        if minimal_score <= self.nodes[node].score {
            return self.nodes[node].score;
        }
        self.nodes[node].score = minimal_score;
        if self.nodes[node].parent_count() == 0 {
            return minimal_score;
        }
        let mut max_score = minimal_score;
        let mut stack = vec![node];
        while let Some(rev) = stack.pop() {
            let rev_score = self.nodes[rev].score;
            let mut previous: Option<NodeIdx> = None;
            for i in 0..self.nodes[rev].start_segments.len() {
                let parent = self.segments[self.nodes[rev].start_segments[i]].parent;
                if self.nodes[parent].score > rev_score {
                    continue;
                }
                self.nodes[parent].score = rev_score + 1;
                max_score = max_score.max(rev_score + 1);
                match previous {
                    None => previous = Some(parent),
                    Some(prev) => {
                        if self.nodes[prev].start_segments.len() >= self.nodes[parent].start_segments.len() {
                            stack.push(prev);
                            previous = Some(parent);
                        } else {
                            stack.push(parent);
                        }
                    }
                }
            }
            if let Some(prev) = previous {
                stack.push(prev);
            }
        }
        max_score
    }

    /// Port of `LaneInfo(startSegment, segmentToTheLeft, segmentToTheRight)`.
    pub fn new_lane_info(&self, start_segment: SegIdx, left: Option<SegIdx>, right: Option<SegIdx>) -> LaneInfo {
        let seg = &self.segments[start_segment];
        let start_revision = seg.child;
        let seed = self.nodes[start_revision].object_id.hash_code() ^ self.nodes[seg.parent].object_id.hash_code();
        LaneInfo { start_revision, color: self.get_color(seed, left, right, None) }
    }

    /// Port of `LaneInfo(startSegment, segmentToTheLeft, segmentToTheRight, derivedFrom)`.
    pub fn new_lane_info_derived(&self, start_segment: SegIdx, left: Option<SegIdx>, right: Option<SegIdx>, derived_from: LaneInfo) -> LaneInfo {
        let start_revision = self.segments[start_segment].parent;
        let seed = self.nodes[start_revision].object_id.hash_code();
        LaneInfo { start_revision, color: self.get_color(seed, left, right, Some(derived_from.color)) }
    }

    fn get_color(&self, mut seed: i32, left: Option<SegIdx>, right: Option<SegIdx>, derived: Option<i32>) -> i32 {
        let left_color = left.and_then(|s| self.segments[s].lane_info).map(|l| l.color);
        let right_color = right.and_then(|s| self.segments[s].lane_info).map(|l| l.color);
        loop {
            let color = color_for_lane(seed);
            if Some(color) != left_color && Some(color) != right_color && Some(color) != derived {
                return color;
            }
            seed = seed.wrapping_add(1);
        }
    }
}

/// Port of `LaneSharing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneSharing {
    /// The segment uses the lane exclusively or is the initial user of the lane.
    ExclusiveOrPrimary,
    /// The segment entirely re-uses a lane with another one because they have the same parent.
    Entire,
    /// The segment partially re-uses a lane because they have the same parent.
    DifferentStart,
    /// The segment partially re-uses a lane because they have the same child.
    DifferentEnd,
}

/// Port of `Lane`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lane {
    pub index: i32,
    pub sharing: LaneSharing,
}

impl Lane {
    pub const NONE: Lane = Lane { index: -1, sharing: LaneSharing::ExclusiveOrPrimary };
}

#[derive(Debug)]
struct RowLanes {
    /// Lane per segment, parallel to [`Row::segments`].
    lanes: Vec<Lane>,
    position: HashMap<SegIdx, usize>,
    gaps: Vec<i32>,
    lane_count: i32,
    revision_lane: i32,
}

/// Port of `RevisionGraphRow`: the ordered segments crossing or connecting to one revision row.
#[derive(Debug)]
pub struct Row {
    pub revision: NodeIdx,
    pub segments: Vec<SegIdx>,
    merge_graph_lanes_having_common_parent: bool,
    lanes: RefCell<Option<RowLanes>>,
}

impl Row {
    pub fn new(revision: NodeIdx, segments: Vec<SegIdx>, merge_graph_lanes_having_common_parent: bool) -> Self {
        Row { revision, segments, merge_graph_lanes_having_common_parent, lanes: RefCell::new(None) }
    }

    /// Port of `BuildSegmentLanes`: lazily assigns lanes. The order in which rows are built
    /// matters (segment state is updated), exactly as in the C# implementation.
    fn build_segment_lanes(&self, store: &Store) {
        if self.lanes.borrow().is_some() {
            return;
        }
        let revision_score = store.nodes[self.revision].score;
        let mut lanes: Vec<Lane> = Vec::with_capacity(self.segments.len());
        let mut lane_count = 0;
        let mut revision_lane = -1;
        let mut has_start = false;
        let mut has_end = false;

        let secondary_sharing = |seg: &Segment| -> LaneSharing {
            let since = &seg.is_secondary_shared_lane_at_least_since_score;
            if revision_score > since.get() {
                return LaneSharing::Entire;
            }
            since.set(since.get().min(revision_score));
            LaneSharing::DifferentStart
        };

        for (i, &seg_idx) in self.segments.iter().enumerate() {
            let segment = &store.segments[seg_idx];
            let lane = if segment.child == self.revision {
                if revision_lane < 0 {
                    revision_lane = lane_count;
                    lane_count += 1;
                }
                segment.is_secondary_shared_lane_at_least_since_score.set(i32::MAX);
                let sharing = if !has_start {
                    has_start = true;
                    LaneSharing::ExclusiveOrPrimary
                } else {
                    LaneSharing::DifferentEnd
                };
                Lane { index: revision_lane, sharing }
            } else if segment.parent == self.revision {
                if revision_lane < 0 {
                    revision_lane = lane_count;
                    lane_count += 1;
                }
                let sharing = if !has_end {
                    has_end = true;
                    segment.is_secondary_shared_lane_at_least_since_score.set(i32::MAX);
                    LaneSharing::ExclusiveOrPrimary
                } else {
                    secondary_sharing(segment)
                };
                Lane { index: revision_lane, sharing }
            } else {
                let mut merged = None;
                if self.merge_graph_lanes_having_common_parent {
                    for (j, &other) in self.segments[..i].iter().enumerate() {
                        if lanes[j].index != revision_lane && store.segments[other].parent == segment.parent {
                            merged = Some(Lane { index: lanes[j].index, sharing: secondary_sharing(segment) });
                            break;
                        }
                    }
                }
                match merged {
                    Some(lane) => lane,
                    None => {
                        segment.is_secondary_shared_lane_at_least_since_score.set(i32::MAX);
                        let index = lane_count;
                        lane_count += 1;
                        Lane { index, sharing: LaneSharing::ExclusiveOrPrimary }
                    }
                }
            };
            lanes.push(lane);
        }

        if revision_lane < 0 {
            revision_lane = lane_count;
            lane_count += 1;
        }

        let position = self.segments.iter().enumerate().map(|(i, &s)| (s, i)).collect();
        *self.lanes.borrow_mut() = Some(RowLanes { lanes, position, gaps: Vec::new(), lane_count, revision_lane });
    }

    pub fn get_current_revision_lane(&self, store: &Store) -> i32 {
        self.build_segment_lanes(store);
        self.lanes.borrow().as_ref().unwrap().revision_lane.max(0)
    }

    pub fn get_lane_count(&self, store: &Store) -> i32 {
        self.build_segment_lanes(store);
        self.lanes.borrow().as_ref().unwrap().lane_count
    }

    pub fn get_segments_for_index(&self, store: &Store, index: i32) -> Vec<SegIdx> {
        self.build_segment_lanes(store);
        let lanes = self.lanes.borrow();
        let lanes = lanes.as_ref().unwrap();
        self.segments.iter().zip(&lanes.lanes).filter(|(_, l)| l.index == index).map(|(&s, _)| s).collect()
    }

    pub fn get_lane_for_segment(&self, store: &Store, segment: SegIdx) -> Lane {
        self.build_segment_lanes(store);
        let lanes = self.lanes.borrow();
        let lanes = lanes.as_ref().unwrap();
        lanes.position.get(&segment).map(|&i| lanes.lanes[i]).unwrap_or(Lane::NONE)
    }

    pub fn move_lanes_right_by(&self, store: &Store, mut from_lane: i32, mut by: i32) {
        while by > 0 {
            self.move_lanes_right(store, from_lane);
            by -= 1;
            from_lane += 1;
        }
    }

    pub fn move_lanes_right(&self, store: &Store, from_lane: i32) {
        self.build_segment_lanes(store);
        let mut guard = self.lanes.borrow_mut();
        let l = guard.as_mut().unwrap();
        let next_gap = l.gaps.iter().copied().filter(|&g| g > from_lane).min().unwrap_or(i32::MAX);
        if l.revision_lane >= from_lane && l.revision_lane < next_gap {
            l.revision_lane += 1;
        }
        let to_move: Vec<usize> =
            (0..l.lanes.len()).filter(|&i| l.lanes[i].index >= from_lane && l.lanes[i].index < next_gap).collect();
        if to_move.is_empty() {
            return;
        }
        if !l.gaps.contains(&from_lane) {
            l.gaps.push(from_lane);
        }
        if next_gap < i32::MAX {
            l.gaps.retain(|&g| g != next_gap);
        } else {
            l.lane_count += 1;
        }
        for i in to_move {
            l.lanes[i].index += 1;
        }
    }

    /// The segment leading to this row's first parent if this row is the parent row of
    /// `segment`; otherwise `segment`.
    pub fn first_parent_or_self(&self, store: &Store, segment: SegIdx) -> SegIdx {
        if store.segments[segment].parent != self.revision
            || self.get_lane_for_segment(store, segment).sharing != LaneSharing::ExclusiveOrPrimary
        {
            return segment;
        }
        self.segments.iter().copied().find(|&s| store.segments[s].child == self.revision).unwrap_or(segment)
    }
}
