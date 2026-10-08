//! Revision graph layout engine for the Git Extensions Rust port.
//!
//! A faithful port of `GitUI.UserControls.RevisionGrid.Graph`: it assigns lanes to the
//! commits loaded from `git log`, straightens lanes and diagonals and emits drawing
//! primitives for each row, so that the graph looks exactly like the one of Git Extensions.

pub mod graph;
pub mod hover;
pub mod lane_info;
pub mod model;
pub mod render;

pub use graph::{RevisionGraph, RevisionGraphConfig, MAX_LANES};
pub use model::{Lane, LaneSharing, NodeIdx, Row, SegIdx, Store};
pub use render::{draw_row, Brush, Metrics, Point, Primitive, RevisionGraphDrawStyle};

#[cfg(test)]
mod tests;
