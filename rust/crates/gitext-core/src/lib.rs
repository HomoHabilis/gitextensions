//! Git command layer for the Git Extensions Rust port.
//!
//! This crate is the Rust counterpart of the C# `GitCommands`, `GitExtUtils`
//! and `GitExtensions.Extensibility` assemblies: it knows how to run git,
//! parse its output and persist application settings. It has no UI dependency.

pub mod git_ref;
pub mod object_id;
pub mod revision;

pub use git_ref::{GitRef, GitRefType};
pub use object_id::ObjectId;
pub use revision::GitRevision;
pub mod summary;
#[doc(hidden)]
pub mod testing;
