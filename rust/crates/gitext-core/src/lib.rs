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
pub mod args;
pub mod exec;
pub mod revision_reader;
pub mod summary;
#[doc(hidden)]
pub mod testing;

pub use args::GitArgs;
pub use exec::{Executable, GitError, GitResult};
pub mod blame;
pub mod branch_name;
pub mod commands;
pub mod module;
pub mod patch;
pub mod status;
pub mod tree;
pub use module::GitModule;
pub mod app_title;
pub mod commit_message;
pub mod diff_tools;
pub mod file_tree;
pub mod inline_diff;
pub mod repo_history;
pub mod settings;
pub mod url_util;
pub mod wsl;
