//! Port of `GraphRenderer` and `SegmentRenderer`.
//!
//! Instead of drawing into a GDI+ `Graphics`, the renderer emits [`Primitive`]s with
//! coordinates relative to the top-left corner of the row's graph cell. The UI layer
//! paints them (clipped to the cell).

use std::collections::HashSet;

use gitext_core::ObjectId;

use crate::graph::{RevisionGraph, RevisionGraphConfig, MAX_LANES};
use crate::model::{LaneInfo, LaneSharing, Row, SegIdx, Store};

const NO_LANE: i32 = -10;

/// How non-relative revisions are drawn (`RevisionGraphDrawStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum RevisionGraphDrawStyle {
    #[default]
    Normal,
    DrawNonRelativesGray,
    HighlightSelected,
}

/// Color of a primitive: a lane color index or the gray non-relative color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Brush {
    Lane(i32),
    NonRelative,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Point { x, y }
    }
}

/// A drawing operation.
#[derive(Debug, Clone, PartialEq)]
pub enum Primitive {
    Line { from: Point, to: Point, brush: Brush, width: f32, anti_alias: bool },
    Bezier { points: [Point; 4], brush: Brush, width: f32 },
    /// Revision node: filled square (when the revision has refs) or circle.
    Node { center: Point, size: f32, square: bool, outline: bool, brush: Brush },
}

/// Pixel metrics (`GraphRenderer.LaneWidth` etc. before DPI scaling).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub lane_width: f32,
    pub lane_line_width: f32,
    pub node_dimension: f32,
    pub row_height: f32,
}

impl Default for Metrics {
    fn default() -> Self {
        Metrics { lane_width: 16.0, lane_line_width: 2.0, node_dimension: 10.0, row_height: 22.0 }
    }
}

#[derive(Debug, Clone, Copy)]
struct SegmentLanesInfo {
    start_lane: i32,
    center_lane: i32,
    end_lane: i32,
    primary_end_lane: i32,
    is_the_revision_lane: bool,
    draw_from_start: bool,
    draw_to_end: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct DiagonalSegmentInfo {
    draw_from_start: bool,
    draw_to_end: bool,
    draw_center_to_start_perpendicularly: bool,
    draw_center: bool,
    draw_center_perpendicularly: bool,
    draw_center_to_end_perpendicularly: bool,
    horizontal_offset: f32,
}

/// Renders one row of the graph. Rows `index-2..=index+2` must be cached.
pub fn draw_row(
    graph: &RevisionGraph,
    index: i64,
    metrics: &Metrics,
    draw_style: RevisionGraphDrawStyle,
    head_id: ObjectId,
    hover_highlighted: Option<&HashSet<ObjectId>>,
) -> Vec<Primitive> {
    let mut out = Vec::new();
    let store = &graph.store;
    let config = graph.config();
    let Some(current_row) = graph.row(index) else {
        return out;
    };
    let previous_row = graph.row(index - 1);
    let next_row = graph.row(index + 1);

    let lane_width = metrics.lane_width;
    let row_height = metrics.row_height;
    let center_y = (row_height / 2.0).floor();
    let start_y = center_y - row_height;
    let end_y = center_y + row_height;

    let mut current_row_revision_lane_info: Option<LaneInfo> = None;

    let is_hovered = |id: &ObjectId| hover_highlighted.map(|h| h.contains(id));
    let mut segments: Vec<SegIdx> = current_row.segments.iter().rev().copied().collect();
    segments.sort_by_key(|&s| {
        let seg = &store.segments[s];
        let child = &store.nodes[seg.child];
        let parent = &store.nodes[seg.parent];
        (
            child.is_relative,
            is_hovered(&child.object_id) == Some(true) || is_hovered(&parent.object_id) == Some(true),
        )
    });

    let skip_secondary = !matches!(draw_style, RevisionGraphDrawStyle::DrawNonRelativesGray | RevisionGraphDrawStyle::HighlightSelected);
    let lane_x = |lane: i32| ((lane as f32 + 0.5) * lane_width).floor();

    for segment in segments {
        let lanes = get_lanes_info(
            store,
            segment,
            previous_row,
            current_row,
            next_row,
            skip_secondary,
            config.merge_graph_lanes_having_common_parent,
            Some(&mut current_row_revision_lane_info),
        );
        if !lanes.draw_from_start && !lanes.draw_to_end {
            continue;
        }

        let p_start = Point::new(lane_x(lanes.start_lane), start_y);
        let p_center = Point::new(lane_x(lanes.center_lane), center_y);
        let p_end = Point::new(lane_x(lanes.end_lane), end_y);

        let seg = &store.segments[segment];
        let child = &store.nodes[seg.child];
        let brush = brush_for_lane_info(seg.lane_info, child.is_relative, draw_style, is_hovered(&child.object_id));
        let mut renderer = SegmentRenderer::new(&config, metrics, brush, &mut out);

        if config.render_graph_with_diagonals {
            let previous_info = || {
                let lanes = get_lanes_info(store, segment, graph.row(index - 2), previous_row.unwrap(), Some(current_row), skip_secondary, config.merge_graph_lanes_having_common_parent, None);
                get_diagonal_segment_info(&lanes, config.merge_graph_lanes_having_common_parent, metrics)
            };
            let next_info = || {
                let lanes = get_lanes_info(store, segment, Some(current_row), next_row.unwrap(), graph.row(index + 2), skip_secondary, config.merge_graph_lanes_having_common_parent, None);
                get_diagonal_segment_info(&lanes, config.merge_graph_lanes_having_common_parent, metrics)
            };
            let current = get_diagonal_segment_info(&lanes, config.merge_graph_lanes_having_common_parent, metrics);
            draw_segment_with_diagonals(&mut renderer, p_start, p_center, p_end, previous_info, current, next_info);
        } else {
            if lanes.draw_from_start {
                renderer.draw_to(p_start, true);
            }
            renderer.draw_to(p_center, true);
            if lanes.draw_to_end {
                renderer.draw_to(p_end, true);
            }
        }
    }

    let revision_lane = current_row.get_current_revision_lane(store);
    if revision_lane < MAX_LANES {
        let node = &store.nodes[current_row.revision];
        let square = node.revision.as_ref().is_some_and(|r| !r.refs.is_empty());
        let outline = node.object_id == head_id;
        let brush = brush_for_lane_info(current_row_revision_lane_info, node.is_relative, draw_style, is_hovered(&node.object_id));
        out.push(Primitive::Node {
            center: Point::new(lane_x(revision_lane), center_y),
            size: metrics.node_dimension,
            square,
            outline,
            brush,
        });
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn get_lanes_info(
    store: &Store,
    segment: SegIdx,
    previous_row: Option<&Row>,
    current_row: &Row,
    next_row: Option<&Row>,
    skip_secondary_shared_segments: bool,
    merge_graph_lanes_having_common_parent: bool,
    set_lane_info: Option<&mut Option<LaneInfo>>,
) -> SegmentLanesInfo {
    let current_lane = current_row.get_lane_for_segment(store, segment);
    let mut start_lane = NO_LANE;
    let mut end_lane = NO_LANE;

    if skip_secondary_shared_segments && current_lane.sharing == LaneSharing::Entire {
        return SegmentLanesInfo {
            start_lane,
            center_lane: NO_LANE,
            end_lane,
            primary_end_lane: end_lane,
            is_the_revision_lane: false,
            draw_from_start: false,
            draw_to_end: false,
        };
    }

    let center_lane = current_lane.index;
    let mut is_the_revision_lane = true;
    let seg = &store.segments[segment];
    let lane_for_row = |row: Option<&Row>| -> i32 {
        row.map(|r| r.get_lane_for_segment(store, segment).index).filter(|&l| l >= 0).unwrap_or(NO_LANE)
    };
    if seg.parent == current_row.revision {
        start_lane = lane_for_row(previous_row);
        if let Some(set) = set_lane_info {
            *set = seg.lane_info;
        }
    } else if seg.child == current_row.revision {
        end_lane = lane_for_row(next_row);
        if let Some(set) = set_lane_info {
            *set = seg.lane_info;
        }
    } else {
        start_lane = lane_for_row(previous_row);
        end_lane = lane_for_row(next_row);
        is_the_revision_lane = false;
    }

    let primary_end_lane = end_lane;
    if current_lane.sharing == LaneSharing::DifferentStart && merge_graph_lanes_having_common_parent && skip_secondary_shared_segments {
        end_lane = NO_LANE;
    }

    let within = |a: i32| a <= MAX_LANES || center_lane <= MAX_LANES;
    SegmentLanesInfo {
        start_lane,
        center_lane,
        end_lane,
        primary_end_lane,
        is_the_revision_lane,
        draw_from_start: start_lane >= 0 && center_lane >= 0 && within(start_lane),
        draw_to_end: end_lane >= 0 && center_lane >= 0 && within(end_lane),
    }
}

fn get_diagonal_segment_info(lanes: &SegmentLanesInfo, merge_common_parent: bool, metrics: &Metrics) -> DiagonalSegmentInfo {
    let draw_from_start = lanes.draw_from_start;
    let draw_to_end = lanes.draw_to_end;
    let is_the_revision_lane = lanes.is_the_revision_lane;
    let start_shift = lanes.center_lane - lanes.start_lane;
    let mut end_shift = lanes.end_lane - lanes.center_lane;
    let start_is_diagonal = start_shift.abs() == 1;
    let end_is_diagonal = end_shift.abs() == 1;
    let is_bow_of_diagonals = start_is_diagonal && end_is_diagonal && -start_shift.signum() == end_shift.signum();
    let bow_offset = (metrics.lane_width / 6.0).floor();
    let junction_bow_offset = if merge_common_parent { metrics.lane_line_width } else { bow_offset };
    let mut horizontal_offset = if is_bow_of_diagonals { -(start_shift.signum() as f32) * junction_bow_offset } else { 0.0 };

    let mut draw_center_to_start_perpendicularly = draw_from_start && (start_shift == 0 || (!start_is_diagonal && !is_the_revision_lane));
    let mut draw_center_to_end_perpendicularly = draw_to_end && (end_shift == 0 || (!end_is_diagonal && !is_the_revision_lane));
    let draw_center_perpendicularly = is_bow_of_diagonals;
    let mut draw_center = draw_center_perpendicularly
        || !draw_from_start
        || !draw_to_end
        || (!draw_center_to_start_perpendicularly && !draw_center_to_end_perpendicularly);

    if lanes.end_lane < 0 && lanes.primary_end_lane >= 0 && start_shift != 0 {
        end_shift = lanes.primary_end_lane - lanes.center_lane;
        let same_direction = end_shift.signum() == start_shift.signum();
        if start_is_diagonal {
            let end_delta = end_shift.abs();
            if !same_direction || end_delta > 1 {
                draw_center_to_end_perpendicularly = true;
                draw_center = false;
                let offset = if end_delta != 1 || same_direction { (metrics.lane_line_width / 3.0).floor() } else { bow_offset };
                horizontal_offset = -(start_shift.signum() as f32) * offset;
            }
        } else if end_shift.abs() == 1 {
            draw_center_to_start_perpendicularly = false;
            if !same_direction {
                horizontal_offset = -(start_shift.signum() as f32) * (metrics.lane_line_width * 2.0 / 3.0).floor();
            }
        } else {
            draw_center_to_start_perpendicularly = false;
        }
    }

    DiagonalSegmentInfo {
        draw_from_start,
        draw_to_end,
        draw_center_to_start_perpendicularly,
        draw_center,
        draw_center_perpendicularly,
        draw_center_to_end_perpendicularly,
        horizontal_offset,
    }
}

fn draw_segment_with_diagonals(
    r: &mut SegmentRenderer,
    p_start: Point,
    p_center: Point,
    p_end: Point,
    previous: impl FnOnce() -> DiagonalSegmentInfo,
    current: DiagonalSegmentInfo,
    next: impl FnOnce() -> DiagonalSegmentInfo,
) {
    let half_perpendicular_height = (r.metrics.row_height / 6.0).floor();

    if current.draw_from_start {
        let previous = previous();
        let start_x = p_start.x + previous.horizontal_offset;
        if previous.draw_center_to_end_perpendicularly {
            r.draw_to(Point::new(start_x, p_start.y + half_perpendicular_height), true);
        } else if previous.draw_center {
            r.draw_to(Point::new(start_x, p_start.y), previous.draw_center_perpendicularly);
        } else {
            r.draw_to(Point::new(start_x, p_start.y - half_perpendicular_height), true);
        }
    }

    let center_x = p_center.x + current.horizontal_offset;
    if current.draw_center_to_start_perpendicularly {
        r.draw_to(Point::new(center_x, p_center.y - half_perpendicular_height), true);
    }
    if current.draw_center {
        r.draw_to(Point::new(center_x, p_center.y), current.draw_center_perpendicularly);
    }
    if current.draw_center_to_end_perpendicularly {
        r.draw_to(Point::new(center_x, p_center.y + half_perpendicular_height), true);
    }

    if current.draw_to_end {
        let next = next();
        let end_x = p_end.x + next.horizontal_offset;
        if next.draw_center_to_start_perpendicularly {
            r.draw_to(Point::new(end_x, p_end.y - half_perpendicular_height), true);
        } else if next.draw_center {
            r.draw_to(Point::new(end_x, p_end.y), next.draw_center_perpendicularly);
        } else {
            r.draw_to(Point::new(end_x, p_end.y + half_perpendicular_height), true);
        }
    }
}

fn brush_for_lane_info(lane_info: Option<LaneInfo>, is_relative: bool, style: RevisionGraphDrawStyle, is_hover_highlighted: Option<bool>) -> Brush {
    if let Some(li) = lane_info {
        if is_hover_highlighted != Some(false)
            && (is_hover_highlighted == Some(true)
                || is_relative
                || !matches!(style, RevisionGraphDrawStyle::DrawNonRelativesGray | RevisionGraphDrawStyle::HighlightSelected))
        {
            return Brush::Lane(li.color);
        }
    }
    Brush::NonRelative
}

/// Port of `SegmentRenderer`: draws a poly-curve point by point.
struct SegmentRenderer<'a> {
    config: &'a RevisionGraphConfig,
    metrics: &'a Metrics,
    brush: Brush,
    out: &'a mut Vec<Primitive>,
    from_perpendicularly: bool,
    from_point: Option<Point>,
}

impl<'a> SegmentRenderer<'a> {
    fn new(config: &'a RevisionGraphConfig, metrics: &'a Metrics, brush: Brush, out: &'a mut Vec<Primitive>) -> Self {
        SegmentRenderer { config, metrics, brush, out, from_perpendicularly: true, from_point: None }
    }

    fn draw_to(&mut self, to: Point, to_perpendicularly: bool) {
        if let Some(from) = self.from_point {
            self.draw(from, to, self.from_perpendicularly, to_perpendicularly);
        }
        self.from_point = Some(to);
        self.from_perpendicularly = to_perpendicularly;
    }

    fn line(&mut self, from: Point, to: Point, anti_alias: bool) {
        self.out.push(Primitive::Line { from, to, brush: self.brush, width: self.metrics.lane_line_width, anti_alias });
    }

    fn bezier(&mut self, e0: Point, c0: Point, c1: Point, e1: Point) {
        self.out.push(Primitive::Bezier { points: [e0, c0, c1, e1], brush: self.brush, width: self.metrics.lane_line_width });
    }

    fn draw(&mut self, from: Point, to: Point, from_perp: bool, to_perp: bool) {
        if from.x == to.x {
            self.line(from, to, false);
            return;
        }
        let mut e0 = from;
        let mut e1 = to;
        let height = to.y - from.y;
        let width = to.x - from.x;
        let single_lane = width.abs() <= self.metrics.lane_width;
        let cell_shift = Point::new(width.signum() * self.metrics.lane_width, self.metrics.row_height);
        let scale = |f: f32| Point::new(f * cell_shift.x, f * cell_shift.y);
        let add = |a: Point, b: Point| Point::new(a.x + b.x, a.y + b.y);
        let sub = |a: Point, b: Point| Point::new(a.x - b.x, a.y - b.y);

        if !from_perp && !to_perp && single_lane {
            self.line(e0, e1, true);
            return;
        }

        let mut c0 = e0;
        let mut c1 = e1;
        const DIAGONAL_FRACTION_CURVE: f32 = 0.25;
        let perpendicular_offset = DIAGONAL_FRACTION_CURVE * cell_shift.y;

        if from_perp && to_perp {
            if self.config.render_graph_with_diagonals && single_lane {
                c0.y += perpendicular_offset;
                c1.y -= perpendicular_offset;
                let mid = Point::new(0.5 * (e0.x + e1.x), 0.5 * (e0.y + e1.y));
                let shift = scale(DIAGONAL_FRACTION_CURVE);
                self.bezier(e0, c0, sub(mid, shift), mid);
                self.bezier(e1, c1, add(mid, shift), mid);
                return;
            }
            c0.y = 0.5 * (from.y + to.y);
            c1.y = c0.y;
        } else if single_lane {
            let fraction_straight = if height < cell_shift.y { 2.0 / 5.0 } else { 0.5 };
            if from_perp {
                let end = add(e1, scale(-fraction_straight));
                self.line(e1, end, true);
                e1 = end;
                c1 = sub(e1, scale(DIAGONAL_FRACTION_CURVE));
                c0.y += perpendicular_offset;
            } else {
                let end = add(e0, scale(fraction_straight));
                self.line(e0, end, true);
                e0 = end;
                c0 = add(e0, scale(DIAGONAL_FRACTION_CURVE));
                c1.y -= perpendicular_offset;
            }
        } else {
            let fraction_straight = 1.0 / 6.0;
            if from_perp {
                c0.y += perpendicular_offset;
            } else {
                let end = add(e0, scale(fraction_straight));
                self.line(e0, end, true);
                e0 = end;
                c0 = add(end, scale(fraction_straight));
            }
            if to_perp {
                c1.y -= perpendicular_offset;
            } else {
                let end = add(e1, scale(-fraction_straight));
                self.line(e1, end, true);
                e1 = end;
                c1 = add(end, scale(-fraction_straight));
            }
        }
        self.bezier(e0, c0, c1, e1);
    }
}
