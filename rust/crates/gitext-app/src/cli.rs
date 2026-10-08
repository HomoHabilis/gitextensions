//! Command line parsing (port of `GitUICommands.RunCommand` / `FormCommandlineHelp`).

use std::path::{Path, PathBuf};

use crate::app::StartCommand;

pub struct ParsedArgs {
    pub repo: Option<PathBuf>,
    pub command: StartCommand,
}

pub const HELP: &str = "Git Extensions (Rust port)

Usage: gitext [command] [arguments]

Commands:
  browse [path]            Open the repository browser (default)
  commit                   Open the commit dialog
  clone [url]              Clone a repository
  init [dir]               Create a new repository
  blame <file> [line]      Blame a file
  filehistory <file>       Show the history of a file
  pull | push | stash      Open the pull, push or stash dialog
  merge [branch]           Merge a branch into the current branch
  rebase [branch]          Rebase the current branch
  checkout | branch | tag  Checkout, create branch, create tag
  remotes | reflog         Manage remotes, view the reflog
  archive | cleanup        Archive a revision, clean the working directory
  applypatch [file]        Apply a patch
  mergeconflicts           Solve merge conflicts
  gitignore                Edit .gitignore
  settings | about         Settings, about
  --help                   Show this help

The repository is the current directory unless a path is given to 'browse'.";

fn repo_for(dir: &Path) -> Option<PathBuf> {
    gitext_core::GitModule::open(dir).ok().map(|m| m.work_dir().to_path_buf())
}

/// Parses `args` (without the program name).
pub fn parse(args: &[String], cwd: &Path) -> Result<ParsedArgs, String> {
    let mut it = args.iter().map(String::as_str);
    let first = it.next();
    let rest: Vec<String> = it.map(str::to_string).collect();
    let arg = |i: usize| rest.get(i).cloned();
    let cwd_repo = repo_for(cwd);
    let command = match first {
        None => return Ok(ParsedArgs { repo: cwd_repo, command: StartCommand::Browse }),
        Some("--help" | "-h" | "help" | "/?") => return Err(HELP.to_string()),
        Some("--version" | "version") => return Err(format!("Git Extensions {}", env!("CARGO_PKG_VERSION"))),
        Some("browse") => {
            let path = rest.iter().find(|a| !a.starts_with('-')).map(PathBuf::from);
            let repo = match path {
                Some(p) => repo_for(&p).or(Some(p)),
                None => cwd_repo.clone(),
            };
            return Ok(ParsedArgs { repo, command: StartCommand::Browse });
        }
        Some("commit") => StartCommand::Commit,
        Some("clone") => StartCommand::Clone(arg(0)),
        Some("init") => StartCommand::Init(arg(0)),
        Some("blame") => StartCommand::Blame(arg(0).ok_or("blame: missing file")?, arg(1)),
        Some("filehistory") => StartCommand::FileHistory(arg(0).ok_or("filehistory: missing file")?),
        Some("settings") => StartCommand::Settings,
        Some("about") => StartCommand::About,
        Some("pull") => StartCommand::Pull,
        Some("push") => StartCommand::Push,
        Some("stash") => StartCommand::Stash,
        Some("merge") => StartCommand::Merge(arg(0).map(|a| a.trim_start_matches("--branch=").to_string())),
        Some("rebase") => StartCommand::Rebase(arg(0).map(|a| a.trim_start_matches("--branch=").to_string())),
        Some("checkout" | "checkoutbranch") => StartCommand::Checkout,
        Some("branch") => StartCommand::Branch,
        Some("tag") => StartCommand::Tag,
        Some("remotes") => StartCommand::Remotes,
        Some("reflog") => StartCommand::Reflog,
        Some("archive") => StartCommand::Archive,
        Some("applypatch") => StartCommand::ApplyPatch(arg(0)),
        Some("cleanup") => StartCommand::Cleanup,
        Some("mergeconflicts" | "mergetool") => StartCommand::MergeConflicts,
        Some("gitignore") => StartCommand::GitIgnore,
        Some("searchfile") => StartCommand::Search,
        Some(other) => {
            // `gitext <path>`: open the repository at path
            let p = PathBuf::from(other);
            if p.is_dir() {
                return Ok(ParsedArgs { repo: repo_for(&p).or(Some(p)), command: StartCommand::Browse });
            }
            return Err(format!("Unknown command '{other}'.\n\n{HELP}"));
        }
    };
    let needs_repo = !matches!(command, StartCommand::Clone(_) | StartCommand::Init(_) | StartCommand::Settings | StartCommand::About);
    if needs_repo && cwd_repo.is_none() {
        return Err(format!("'{}' is not a git repository.", cwd.display()));
    }
    Ok(ParsedArgs { repo: cwd_repo, command })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_commands() {
        let tmp = std::env::temp_dir();
        assert!(parse(&s(&["--help"]), &tmp).is_err());
        let p = parse(&s(&["clone", "https://x/y.git"]), &tmp).unwrap();
        assert!(matches!(p.command, StartCommand::Clone(Some(ref u)) if u == "https://x/y.git"));
        assert!(matches!(parse(&s(&["settings"]), &tmp).unwrap().command, StartCommand::Settings));
        assert!(parse(&s(&["bogus-command-xyz"]), &tmp).is_err());
    }

    #[test]
    fn repo_commands_need_a_repository() {
        let dir = tempfile::tempdir().unwrap();
        assert!(parse(&s(&["commit"]), dir.path()).is_err());
        gitext_core::GitModule::init(dir.path(), false, false).unwrap();
        let p = parse(&s(&["commit"]), dir.path()).unwrap();
        assert!(matches!(p.command, StartCommand::Commit));
        assert!(p.repo.is_some());
        let p = parse(&s(&["blame", "a.txt", "3"]), dir.path()).unwrap();
        assert!(matches!(p.command, StartCommand::Blame(ref f, Some(ref l)) if f == "a.txt" && l == "3"));
    }
}
