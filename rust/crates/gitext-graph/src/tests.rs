//! Ported from GitUI.Tests/UserControls/RevisionGrid/Graph/RevisionGraphTests.cs and
//! RevisionGraphRowTests.cs. The ASCII-art snapshots are the `*.verified.txt` files of the
//! original C# test-suite (copied to `rust/testdata/graph`), proving an identical layout.

use std::collections::HashMap;

use gitext_core::testing::verify;
use gitext_core::{GitRevision, ObjectId};

use crate::graph::{RevisionGraph, RevisionGraphConfig, MAX_LANES};
use crate::model::{Row, Store};

const LOOK_AHEAD: usize = 20 * 2;

/// Test defaults of the C# `[SetUp]`: no lane merging, no diagonal straightening.
fn config(merge: bool, straighten_diagonals: bool) -> RevisionGraphConfig {
    RevisionGraphConfig {
        merge_graph_lanes_having_common_parent: merge,
        straighten_graph_diagonals: straighten_diagonals,
        ..RevisionGraphConfig::default()
    }
}

fn revisions() -> Vec<GitRevision> {
    //     Commit1
    //        |
    //     Commit2
    //    /       \
    // Commit3     |
    //   |       Commit4
    //    \       /
    //     Commit5
    //        |
    //       ...
    let ids: Vec<ObjectId> = (0..6 + LOOK_AHEAD).map(|_| ObjectId::random()).collect();
    let mut result = vec![
        GitRevision::new(ids[0]).with_parents(vec![ids[1]]),
        GitRevision::new(ids[1]).with_parents(vec![ids[2], ids[3]]),
        GitRevision::new(ids[2]).with_parents(vec![ids[4]]),
        GitRevision::new(ids[3]).with_parents(vec![ids[4]]),
    ];
    for i in 4..ids.len() - 1 {
        result.push(GitRevision::new(ids[i]).with_parents(vec![ids[i + 1]]));
    }
    result.push(GitRevision::new(*ids.last().unwrap()).with_parents(vec![]));
    result
}

fn setup(merge: bool, finish_loading: bool, revisions: Vec<GitRevision>) -> RevisionGraph {
    let mut graph = RevisionGraph::new(config(merge, false));
    for revision in revisions {
        if graph.count() == 0 {
            graph.head_id = revision.object_id;
        }
        graph.add(revision);
    }
    if finish_loading {
        graph.loading_completed();
        let last = graph.count() - 1;
        graph.cache_to(last, last);
    }
    graph
}

/// Creates a graph from commit specs listed oldest to newest: `id:parent1,parent2 ...`.
fn create_graph(commit_specs: &str, config: RevisionGraphConfig) -> RevisionGraph {
    let mut commits: Vec<GitRevision> = Vec::new();
    let mut by_id: HashMap<String, ObjectId> = HashMap::new();
    for spec in commit_specs.split(' ').map(str::trim).filter(|s| !s.is_empty()) {
        let mut parts = spec.split(':');
        let id = parts.next().unwrap().to_string();
        let mut commit = GitRevision::new(ObjectId::random()).with_subject(id.clone());
        if let Some(parents) = parts.next() {
            commit.parent_ids = Some(parents.split(',').map(|p| by_id[p]).collect());
        }
        by_id.insert(id, commit.object_id);
        commits.push(commit);
    }
    let mut graph = RevisionGraph::new(config);
    for commit in commits.into_iter().rev() {
        graph.add(commit);
    }
    graph.loading_completed();
    let last = graph.count() - 1;
    graph.cache_to(last, last);
    graph
}

fn create_graph_top_down(commit_specs: &str, config: RevisionGraphConfig) -> RevisionGraph {
    let reversed: Vec<&str> = commit_specs.split(' ').rev().collect();
    create_graph(&reversed.join(" "), config)
}

/// ASCII art of the graph with the same layout as the GUI (port of `AsciiGraphFor`).
fn ascii_graph_for(graph: &mut RevisionGraph) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut row_index = 0i64;
    loop {
        assert!(graph.validate_row(row_index), "row must be cached");
        let store: &Store = &graph.store;
        let row: &Row = graph.row(row_index).unwrap();
        let lane_count = row.get_lane_count(store) as usize;
        let mut line: Vec<char> = vec![' '; lane_count * 2 + 1];
        for &segment in &row.segments {
            line[row.get_lane_for_segment(store, segment).index as usize * 2] = '|';
        }
        let subject = store.nodes[row.revision].revision.as_ref().map(|r| r.subject.clone());
        let marker = match subject.as_deref() {
            Some(s) if s.chars().count() == 1 => s.chars().next().unwrap(),
            _ => '*',
        };
        line[row.get_current_revision_lane(store) as usize * 2] = marker;
        lines.push(line.iter().collect::<String>().trim_end().to_string());

        let Some(next_row) = graph.row(row_index + 1) else {
            break;
        };

        let width = row.get_lane_count(store).max(next_row.get_lane_count(store)) as usize * 2 + 1;
        let mut line: Vec<char> = vec![' '; width];
        let mut actions: Vec<(usize, i32)> = Vec::new();
        for &segment in &row.segments {
            let from_pos = row.get_lane_for_segment(store, segment).index * 2;
            let to_pos = next_row.get_lane_for_segment(store, segment).index * 2;
            if to_pos == -2 {
                continue;
            }
            if to_pos == from_pos {
                actions.push((from_pos as usize, 0));
            } else if to_pos == from_pos + 2 {
                actions.push((from_pos as usize, 1));
            } else if to_pos == from_pos - 2 {
                actions.push((from_pos as usize, -1));
            } else if to_pos > from_pos {
                line[from_pos as usize + 1] = '`';
                line[to_pos as usize] = 'ˎ';
                for pos in from_pos + 2..to_pos {
                    line[pos as usize] = '-';
                }
            } else {
                line[from_pos as usize - 1] = '´';
                line[to_pos as usize] = ',';
                for pos in to_pos + 1..from_pos - 1 {
                    line[pos as usize] = '-';
                }
            }
        }
        for (from_pos, kind) in actions {
            match kind {
                0 => line[from_pos] = '|',
                1 => line[from_pos + 1] = if line[from_pos + 1] == '/' { 'X' } else { '\\' },
                _ => line[from_pos - 1] = if line[from_pos - 1] == '\\' { 'X' } else { '/' },
            }
        }
        lines.push(line.iter().collect::<String>().trim_end().to_string());
        row_index += 1;
    }
    lines.join("\n")
}

fn verify_graph(graph: &mut RevisionGraph, snapshot: &str) {
    let actual = ascii_graph_for(graph);
    verify(&format!("graph/{snapshot}.verified.txt"), &actual);
}

fn verify_top_down(commit_specs: &str, test: &str, index: usize, config: RevisionGraphConfig) {
    let mut graph = create_graph_top_down(commit_specs, config);
    verify_graph(&mut graph, &format!("{test}.{index}"));
}

#[test]
fn should_be_able_to_cache_graph_to() {
    for merge in [false, true] {
        let mut g = setup(merge, false, revisions());
        assert_eq!(g.get_cached_count(), 0);
        g.cache_to(4, 2);
        assert_eq!(g.get_cached_count(), if merge { 3 } else { 0 });
        g.cache_to(4, 4);
        assert_eq!(g.get_cached_count(), if merge { 5 } else { 0 });
        g.cache_to(400, 400);
        assert_eq!(g.get_cached_count(), if merge { 6 } else { 0 });
        g.loading_completed();
        assert_eq!(g.get_cached_count(), if merge { 6 + LOOK_AHEAD } else { 0 });
        g.cache_to(400, 400);
        assert_eq!(g.get_cached_count(), 6 + LOOK_AHEAD);
    }
}

#[test]
fn should_be_able_to_clear() {
    for merge in [false, true] {
        let mut g = setup(merge, false, revisions());
        assert_eq!(g.count(), 6 + LOOK_AHEAD);
        g.clear(config(merge, false));
        assert_eq!(g.count(), 0);
    }
}

#[test]
fn should_be_able_to_highlight_branch() {
    for merge in [false, true] {
        let mut g = setup(merge, false, revisions());
        let n = g.count();
        g.cache_to(n, n);
        assert!(g.is_row_relative(0));
        assert!(g.is_row_relative(1));
        assert!(g.is_row_relative(4));
        let id = { let n = g.get_node_for_row(1).unwrap(); g.store.nodes[n].object_id };
        g.highlight_branch(&id);
        assert!(!g.is_row_relative(0));
        assert!(g.is_row_relative(1));
        assert!(g.is_row_relative(4));
    }
}

#[test]
fn should_be_able_to_get_lane_count() {
    for merge in [false, true] {
        let mut g = setup(merge, false, revisions());
        if !merge {
            g.loading_completed();
        }
        let n = g.count();
        g.cache_to(n, n);
        for (row, expected) in [(0, 1), (1, 1), (2, 2), (3, 2), (4, 1), (5, 1)] {
            assert!(g.validate_row(row));
            let r = g.row(row).unwrap();
            assert_eq!(r.get_lane_count(&g.store), expected, "merge={merge} row={row}");
        }
    }
}

#[test]
fn should_reorder_in_topo_order() {
    for merge in [false, true] {
        let mut g = setup(merge, false, revisions());
        let n = g.count();
        g.cache_to(n, n);
        assert!(g.validate_topo_order());

        let commit1 = ObjectId::random();
        let commit2 = ObjectId::random();
        let row4 = { let n = g.get_node_for_row(4).unwrap(); g.store.nodes[n].object_id };
        g.add(GitRevision::new(commit2).with_parents(vec![row4])); // dangling
        let n = g.count();
        g.cache_to(n, n);
        assert!(g.validate_topo_order());

        g.add(GitRevision::new(commit1).with_parents(vec![commit2])); // connecting commit
        let n = g.count();
        g.cache_to(n, n);
        assert!(g.validate_topo_order());

        let top = { let n = g.get_node_for_row(0).unwrap(); g.store.nodes[n].object_id };
        g.add(GitRevision::new(ObjectId::random()).with_parents(vec![top])); // new head
        let n = g.count();
        g.cache_to(n, n);
        assert!(g.validate_topo_order());
    }
}

#[test]
fn cache_empty_graph() {
    // https://github.com/gitextensions/gitextensions/issues/6193
    for merge in [false, true] {
        let mut g = setup(merge, false, revisions());
        g.clear(config(merge, false));
        g.cache_to(100, 100);
        g.cache_to(100, 100);
    }
}

#[test]
fn detached_single_revision() {
    // https://github.com/gitextensions/gitextensions/issues/6210
    for merge in [false, true] {
        let mut g = RevisionGraph::new(config(merge, false));
        let (c1, c2, c3) = (ObjectId::random(), ObjectId::random(), ObjectId::random());
        g.add(GitRevision::new(c1).with_parents(vec![c3]));
        g.add(GitRevision::new(c2));
        g.add(GitRevision::new(c3));
        g.loading_completed();
        let n = g.count();
        g.cache_to(n, n);
        assert!(g.validate_row(1));
        assert_eq!(g.row(1).unwrap().get_current_revision_lane(&g.store), 1);
        verify_graph(&mut g, &format!("RevisionGraphTests.DetachedSingleRevision_mergeGraphLanesHavingCommonParent={}", cap(merge)));
    }
}

fn cap(b: bool) -> &'static str {
    if b { "True" } else { "False" }
}

fn verify_both(name: &str, specs: &str) {
    for merge in [false, true] {
        let mut g = create_graph(specs, config(merge, false));
        verify_graph(&mut g, &format!("RevisionGraphTests.{name}_mergeGraphLanesHavingCommonParent={}", cap(merge)));
    }
}

#[test]
fn segments_are_straightened() {
    verify_both("SegmentsAreStraightened", " 1  2:1  3:1  4:1,3  5:4  6:5  7:5,6  8:7,2 ");
}

#[test]
fn segments_with_commits_are_straightened() {
    verify_both("SegmentsWithCommitsAreStraightened", " 1  2:1  3:1  4:1,3  5:2  6:5  7:4  8:4,7  9:8,6 ");
}

#[test]
fn segments_with_outgoing_secondary_merges_are_not_straightened() {
    verify_both("SegmentsWithOutgoingSecondaryMergesAreNotStraightened", " 1  2:1  3:1  4:1,3  5:2  6:4,5  7:6  8:6,7  9:8,5 ");
}

#[test]
fn segments_with_incoming_merges_are_straightened() {
    verify_both("SegmentsWithIncomingMergesAreStraightened", " 1  2:1  3:1  4:1,3  5:2,4  6:4  7:4,6  8:7,5 ");
}

#[test]
fn segments_are_straightened_although_this_causes_width_increase() {
    verify_both("SegmentsAreStraightenedAlthoughThisCausesWidthIncrease", " 1  2:1  3:1  4:1  5:1,4  6:2  7:2,6  8:5  9:5,8,3,7 ");
}

#[test]
fn segments_with_outgoing_primary_merges_are_straightened() {
    verify_both("SegmentsWithOutgoingPrimaryMergesAreStraightened", " 1  2:1  3:1  6:1  7:1,6  8:3,2  9:7  10:7,9,8 ");
}

#[test]
fn segments_are_not_straightened_if_this_causes_a_shift_for_primary_segment() {
    verify_both(
        "SegmentsAreNotStraightenedIfThisCausesAShiftForPrimarySegment",
        " 1  a:1  b:1  2:1  3:1  4:1  5:4,1  6:3  7:5,6  8:7,2,6  c:8  d:8  e:8  9:8,e,d,c,b,a ",
    );
}

const GRAPH_WITH_MULTI_LANE_CROSSINGS: &str =
    "0:C,1,2,3,4,5,6,7,8,9,A,B 1:R 2:R 3:R 4:C 5:C 6:C 7:R 8:C 9:R A:C B:R C:D D:E E:F F:G G:H,K,R H:I,R I:J,R J:R K:R R";

#[test]
fn segments_are_not_straightened_over_multi_lane_crossings() {
    let mut g = create_graph_top_down(GRAPH_WITH_MULTI_LANE_CROSSINGS, config(true, false));
    verify_graph(&mut g, "RevisionGraphTests.SegmentsAreNotStraightenedOverMultiLaneCrossings");
}

#[test]
fn segments_are_not_straightened_over_multi_lane_crossings_no_merge() {
    let mut g = create_graph_top_down(GRAPH_WITH_MULTI_LANE_CROSSINGS, config(false, false));
    verify_graph(&mut g, "RevisionGraphTests.SegmentsAreNotStraightenedOverMultiLaneCrossings_NoMergeGraphLanesHavingCommonParent");
}

#[test]
fn segments_are_not_straightened_over_multi_lane_crossings_no_merge_straighten_diagonals() {
    let mut g = create_graph_top_down(GRAPH_WITH_MULTI_LANE_CROSSINGS, config(false, true));
    verify_graph(
        &mut g,
        "RevisionGraphTests.SegmentsAreNotStraightenedOverMultiLaneCrossings_NoMergeGraphLanesHavingCommonParent_StraightenGraphDiagonals",
    );
}

#[test]
fn turn_multi_lane_crossings_into_diagonals() {
    for specs in [
        "R 5:R 4:R 3:R 2:R,5,4 1:2 0:1,3",
        "R 7:R 6:R 5:R 4:R 3:R,7,6,5 2:3 1:2 0:1,4",
        "R 8:R 7:R 6:R 5:R 4:R 3:R,8,7,6,5 2:3 1:2 0:1,4",
    ] {
        let mut g = create_graph(specs, config(false, true));
        let name = specs.replace(':', "-");
        verify_graph(&mut g, &format!("RevisionGraphTests.TurnMultiLaneCrossingsIntoDiagonals_commitSpecs={name}"));
    }
}

#[test]
fn unfold_one_lane_shifts_to_diagonals() {
    for (i, specs) in [
        "0:1,4 1:2 2:R,3 3:R 4:R R",
        "0:1,R 1:2,R 2:4,3 3:R 4:5,8 5:6 6:7,R 7:R,R 8:9 9:R R",
        "0:D,1,2,3,4,5,6,B,8,9,7 1:D 2:D 3:E 4:E 5:C 6:C 7:G 8:9 9:A A:B B:C C:F D:R E:F F:R G:H H:R R",
        "0:4 1:3,2 2:6 3:8,B 4:5 5:A,6 6:7 7:8 8:C,R 9:A A:B,R B:R C:R R",
    ]
    .iter()
    .enumerate()
    {
        verify_top_down(specs, "UnfoldOneLaneShiftsToDiagonals", i + 1, config(false, true));
    }
}

#[test]
fn do_not_unfold_one_lane_shift_followed_by_diagonal() {
    for (i, specs) in [
        "0:1,5 1:2   2:R,3 3:R,4 4:R   5:R R",
        "0:1,R 1:2,R 2:4,3 3:R   4:5,R 5:6   6:7   7:8,R 8:9 9:R R",
        "0:1,R 1:2,R 2:4,3 3:R 4:5,8 5:6,R 6:7,R 7:R,R 8:9 9:R R",
        "0:5,1,2,3,4 1:5 2:5 3:6 4:8 5:R 6:7 7:8,B 8:9 9:R,A A:R   B:R     R",
        "0:5,1,2,3,4 1:5 2:5 3:6 4:8 5:R 6:7 7:8,B 8:9 9:R,A A:R,C B:R C:R R",
    ]
    .iter()
    .enumerate()
    {
        verify_top_down(specs, "DoNotUnfoldOneLaneShiftFollowedByDiagonal", i + 1, config(false, true));
    }
}

#[test]
fn join_multi_lane_crossings() {
    for (i, specs) in [
        "0:3,2 1:5 2:5 3:5 4:R 5:R R",
        "0:3,2 1:5 2:5 3:5 4:6 5:6 6:7,8 7:R 8:R R",
        "0:5,4 1:7 2:6 3:R 4:7 5:7 6:R 7:R R",
        "0:R,3 1:2 2:8,7 3:4,5,6 4:R 5:R 6:R 7:8 8:R R",
    ]
    .iter()
    .enumerate()
    {
        verify_top_down(specs, "JoinMultiLaneCrossings", i + 1, config(false, true));
    }
}

#[test]
fn do_not_join_multi_lane_crossings() {
    for (i, specs) in ["0:6 1:6 2:6 3:6 4:R 5:7 6:R 7:R R", "0:1,4,R 1:2,R 2:3,R 3:R 4:R R"].iter().enumerate() {
        verify_top_down(specs, "DoNotJoinMultiLaneCrossings", i + 1, config(false, true));
    }
}

#[test]
fn move_visible_and_invisible_lanes_right() {
    for move_first_lane in [false, true] {
        let base = ObjectId::random();
        let mut lane_commits = Vec::new();
        let mut revs = Vec::new();
        for lane in 0..MAX_LANES as usize + 10 {
            let id = ObjectId::random();
            lane_commits.push(id);
            revs.push(GitRevision::new(id).with_subject(format!("{}", lane % 10)).with_parents(vec![base]));
        }
        revs.push(GitRevision::new(base).with_subject("B").with_parents(vec![]));
        let mut g = setup(false, true, revs);

        for (lane, id) in lane_commits.iter().enumerate() {
            let index = g.try_get_row_index(id).unwrap() as i64;
            assert!(g.validate_row(index));
            let row = g.row(index).unwrap();
            let initial = lane as i32 + 1;
            assert_eq!(row.get_lane_count(&g.store), initial);
            row.move_lanes_right(&g.store, if move_first_lane { 0 } else { lane as i32 });
            assert_eq!(row.get_lane_count(&g.store), initial + 1);
        }
        verify_graph(&mut g, &format!("RevisionGraphTests.MoveVisibleAndInvisibleLanesRight_moveFirstLane={}", cap(move_first_lane)));
    }
}

// ---- RevisionGraphRowTests ----

struct RowFixture {
    store: Store,
    segment: usize,
    segment1: usize,
    segment2: usize,
    revision: usize,
}

fn row_fixture() -> RowFixture {
    let mut store = Store::default();
    let p = store.add_node(ObjectId::INDEX, 0);
    let c = store.add_node(ObjectId::WORK_TREE, 0);
    let p1 = store.add_node(ObjectId::random(), 0);
    let c1 = store.add_node(ObjectId::random(), 0);
    let p2 = store.add_node(ObjectId::random(), 0);
    let c2 = store.add_node(ObjectId::random(), 0);
    store.segments.push(crate::model::Segment::new(p, c));
    store.segments.push(crate::model::Segment::new(p1, c1));
    store.segments.push(crate::model::Segment::new(p2, c2));
    RowFixture { store, segment: 0, segment1: 1, segment2: 2, revision: c }
}

#[test]
fn move_lanes_right_should_do_nothing_if_empty() {
    for from_lane in [-1, 0, 1] {
        let f = row_fixture();
        let row = Row::new(f.revision, vec![], true);
        assert_eq!(row.get_lane_count(&f.store), 1);
        row.move_lanes_right(&f.store, from_lane);
        assert_eq!(row.get_lane_count(&f.store), 1);
        assert!(row.segments.is_empty());
    }
}

#[test]
fn move_lanes_right_should_move_single_segment() {
    for (from_lane, expected) in [(-1, 1), (0, 1), (1, 0), (2, 0)] {
        let f = row_fixture();
        let row = Row::new(f.revision, vec![f.segment], true);
        assert_eq!(row.get_lane_count(&f.store), 1);
        row.move_lanes_right(&f.store, from_lane);
        assert_eq!(row.get_lane_count(&f.store), if from_lane >= 1 { 1 } else { 2 });
        assert_eq!(row.get_lane_for_segment(&f.store, f.segment).index, expected);
    }
}

#[test]
fn move_lanes_right_should_move_segments() {
    for (from_lane, e0, e1, e2) in [(-1, 1, 2, 3), (0, 1, 2, 3), (1, 0, 2, 3), (2, 0, 1, 3), (3, 0, 1, 2), (4, 0, 1, 2)] {
        let f = row_fixture();
        let row = Row::new(f.revision, vec![f.segment, f.segment1, f.segment2], true);
        assert_eq!(row.get_lane_count(&f.store), 3);
        row.move_lanes_right(&f.store, from_lane);
        assert_eq!(row.get_lane_count(&f.store), if from_lane >= 3 { 3 } else { 4 });
        assert_eq!(row.get_lane_for_segment(&f.store, f.segment).index, e0);
        assert_eq!(row.get_lane_for_segment(&f.store, f.segment1).index, e1);
        assert_eq!(row.get_lane_for_segment(&f.store, f.segment2).index, e2);
    }
}

#[test]
fn move_lanes_right_should_move_segments_twice() {
    for (from1, from2, e0, e1, e2) in [
        (-1, 4, 1, 2, 3),
        (0, 1, 2, 3, 4),
        (0, 2, 1, 3, 4),
        (0, 3, 1, 2, 4),
        (0, 4, 1, 2, 3),
        (1, 0, 1, 2, 3),
        (1, 1, 0, 3, 4),
        (1, 2, 0, 3, 4),
        (1, 3, 0, 2, 4),
        (1, 4, 0, 2, 3),
        (2, 0, 1, 2, 3),
        (2, 1, 0, 2, 3),
        (2, 2, 0, 1, 4),
        (2, 3, 0, 1, 4),
        (2, 4, 0, 1, 3),
        (3, 0, 1, 2, 3),
        (3, 1, 0, 2, 3),
        (3, 2, 0, 1, 3),
        (3, 3, 0, 1, 2),
        (4, 3, 0, 1, 2),
    ] {
        let f = row_fixture();
        let row = Row::new(f.revision, vec![f.segment, f.segment1, f.segment2], true);
        assert_eq!(row.get_lane_count(&f.store), 3);
        row.move_lanes_right(&f.store, from1);
        row.move_lanes_right(&f.store, from2);
        assert_eq!(row.get_lane_count(&f.store), e2 + 1, "{from1},{from2}");
        assert_eq!(row.get_lane_for_segment(&f.store, f.segment).index, e0, "{from1},{from2}");
        assert_eq!(row.get_lane_for_segment(&f.store, f.segment1).index, e1, "{from1},{from2}");
        assert_eq!(row.get_lane_for_segment(&f.store, f.segment2).index, e2, "{from1},{from2}");
    }
}

#[test]
fn render_emits_nodes_and_segments_for_every_row() {
    use crate::render::*;
    let mut g = create_graph(" 1  2:1  3:1  4:1,3  5:4  6:5  7:5,6  8:7,2 ", RevisionGraphConfig::default());
    let n = g.count() as i64;
    let head = { let n = g.get_node_for_row(0).unwrap(); g.store.nodes[n].object_id };
    for row in 0..n {
        let prims = draw_row(&g, row, &Metrics::default(), RevisionGraphDrawStyle::Normal, head, None);
        let nodes: Vec<_> = prims.iter().filter(|p| matches!(p, Primitive::Node { .. })).collect();
        assert_eq!(nodes.len(), 1, "row {row}");
        if let Primitive::Node { outline, .. } = nodes[0] {
            assert_eq!(*outline, row == 0);
        }
        assert!(prims.len() > 1, "row {row} must have segments");
    }
}
