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
pub mod args;
pub mod exec;
pub mod revision_reader;

pub use args::GitArgs;
pub use exec::{Executable, GitError, GitResult};
pub mod commands;
pub mod status;
pub mod patch;
pub mod blame;
pub mod tree;
pub mod branch_name;
pub mod module;
pub use module::GitModule;
pub mod app_title;
pub mod commit_message;
pub mod repo_history;
pub mod settings;
pub mod file_tree;
pub mod url_util;
