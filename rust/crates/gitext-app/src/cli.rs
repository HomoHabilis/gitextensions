//! Command line parsing (port of `GitUICommands.RunCommand` / `FormCommandlineHelp`).

use std::path::{Path, PathBuf};

use crate::app::StartCommand;

pub struct ParsedArgs {
    pub repo: Option<PathBuf>,
    pub command: StartCommand,
}

/// Why the command line does not start the application.
#[derive(Debug)]
pub enum CliExit {
    /// `--help`, `--version`: print the text.
    Info(String),
    /// An invalid command line.
    Error(String),
    /// A repository command run outside of a repository (`dir`).
    NotARepository(PathBuf),
}

pub const HELP: &str = "Git Extensions (Rust port)

Usage: gitext [-C <path>] [command] [arguments]

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
  difftool <file>          Open the changes of a file with the diff tool
  reset [path...]          Reset (discard) the changes of files or folders
  addfiles [path]          Add files to the index
  viewdiff                 Compare two revisions
  checkoutrevision         Checkout a revision
  shellext install|uninstall
                           Add or remove the Windows Explorer context menu
  settings | about         Settings, about
  --help                   Show this help

The repository is the current directory unless a path is given to 'browse' or with -C.
-C <path> runs in the directory <path>, or for a file in its directory, and the file is the
default for the commands that take a file (used by the Explorer context menu).";

fn repo_for(dir: &Path) -> Option<PathBuf> {
    gitext_core::GitModule::open(dir).ok().map(|m| m.work_dir().to_path_buf())
}

/// `path` relative to the repository `repo` with `/` separators (`""` for the repository
/// itself), or `path` unchanged when it is not inside the repository.
fn repo_relative(repo: Option<&Path>, cwd: &Path, path: &str) -> String {
    let Some(repo) = repo else { return path.to_string() };
    let full = cwd.join(path);
    let (Ok(full), Ok(repo)) = (std::fs::canonicalize(&full), std::fs::canonicalize(repo)) else { return path.to_string() };
    match full.strip_prefix(&repo) {
        Ok(rel) => rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"),
        Err(_) => path.to_string(),
    }
}

/// Parses `args` (without the program name).
pub fn parse(args: &[String], cwd: &Path) -> Result<ParsedArgs, CliExit> {
    let mut cwd = cwd.to_path_buf();
    let mut args = args;
    // -C <path>: the directory to run in; a file selects that file
    let mut selected: Option<PathBuf> = None;
    if let [flag, path, rest @ ..] = args {
        if flag == "-C" || flag == "--path" {
            let path = cwd.join(crate::shell_ext::clean_shell_path(path));
            if path.is_file() {
                cwd = path.parent().map(Path::to_path_buf).unwrap_or(cwd);
            } else if path.is_dir() {
                cwd = path.clone();
            } else {
                return Err(CliExit::Error(format!("'{}' does not exist.", path.display())));
            }
            selected = Some(path);
            args = rest;
        }
    }
    let cwd = cwd.as_path();
    let mut it = args.iter().map(String::as_str);
    let first = it.next();
    let rest: Vec<String> = it.map(str::to_string).collect();
    let arg = |i: usize| rest.get(i).cloned();
    let cwd_repo = repo_for(cwd);
    let relative = |p: &str| repo_relative(cwd_repo.as_deref(), cwd, p);
    let selected_relative = selected.as_ref().map(|p| relative(&p.to_string_lossy()));
    // the file argument, or the selected file (-C)
    let file_arg = |i: usize, command: &str| -> Result<String, CliExit> {
        arg(i)
            .map(|a| relative(&a))
            .or_else(|| selected_relative.clone().filter(|p| !p.is_empty()))
            .ok_or_else(|| CliExit::Error(format!("{command}: missing file")))
    };
    // the path arguments, or the selected path; `[]` for the whole repository
    let paths_arg = || -> Vec<String> {
        let paths: Vec<String> = if rest.is_empty() { selected_relative.iter().cloned().collect() } else { rest.iter().map(|a| relative(a)).collect() };
        paths.into_iter().filter(|p| !p.is_empty() && p != ".").collect()
    };
    let command = match first {
        None => return Ok(ParsedArgs { repo: cwd_repo, command: StartCommand::Browse }),
        Some("--help" | "-h" | "help" | "/?") => return Err(CliExit::Info(HELP.to_string())),
        Some("--version" | "version") => return Err(CliExit::Info(format!("Git Extensions {}", env!("CARGO_PKG_VERSION")))),
        Some("browse" | "openrepo") => {
            let path = rest.iter().find(|a| !a.starts_with('-')).map(|p| cwd.join(p));
            let repo = match path {
                Some(p) => repo_for(&p).or(Some(p)),
                None => cwd_repo.clone().or_else(|| selected.is_some().then(|| cwd.to_path_buf())),
            };
            return Ok(ParsedArgs { repo, command: StartCommand::Browse });
        }
        Some("shellext") => match arg(0).as_deref() {
            Some("install" | "register") => StartCommand::ShellExt(true),
            Some("uninstall" | "unregister") => StartCommand::ShellExt(false),
            _ => return Err(CliExit::Error("Usage: gitext shellext install|uninstall".into())),
        },
        Some("commit") => StartCommand::Commit,
        Some("clone") => StartCommand::Clone(arg(0)),
        // the Explorer menu creates the repository in the selected folder
        Some("init") => StartCommand::Init(arg(0).or_else(|| selected.is_some().then(|| cwd.display().to_string()))),
        Some("blame") => StartCommand::Blame(file_arg(0, "blame")?, arg(1)),
        Some("filehistory") => StartCommand::FileHistory(file_arg(0, "filehistory")?),
        Some("difftool") => StartCommand::DiffTool(file_arg(0, "difftool")?),
        Some("reset" | "revert") => StartCommand::ResetChanges(paths_arg()),
        Some("add" | "addfiles") => {
            let paths = paths_arg();
            StartCommand::AddFiles(if paths.is_empty() { ".".into() } else { paths.join(" ") })
        }
        Some("viewdiff") => StartCommand::ViewDiff,
        Some("settings") => StartCommand::Settings,
        Some("about") => StartCommand::About,
        Some("pull") => StartCommand::Pull,
        Some("push") => StartCommand::Push,
        Some("stash") => StartCommand::Stash,
        Some("merge") => StartCommand::Merge(arg(0).map(|a| a.trim_start_matches("--branch=").to_string())),
        Some("rebase") => StartCommand::Rebase(arg(0).map(|a| a.trim_start_matches("--branch=").to_string())),
        Some("checkout" | "checkoutbranch") => StartCommand::Checkout,
        Some("checkoutrevision") => StartCommand::CheckoutRevision,
        Some("branch") => StartCommand::Branch,
        Some("tag") => StartCommand::Tag,
        Some("remotes") => StartCommand::Remotes,
        Some("reflog") => StartCommand::Reflog,
        Some("archive") => StartCommand::Archive,
        Some("applypatch" | "apply") => StartCommand::ApplyPatch(arg(0).or_else(|| selected.as_ref().filter(|p| p.is_file()).map(|p| p.display().to_string()))),
        Some("cleanup") => StartCommand::Cleanup,
        Some("mergeconflicts" | "mergetool") => StartCommand::MergeConflicts,
        Some("gitignore") => StartCommand::GitIgnore,
        Some("searchfile") => StartCommand::Search,
        Some(other) => {
            // `gitext <path>`: open the repository at path
            let p = cwd.join(other);
            if p.is_dir() {
                return Ok(ParsedArgs { repo: repo_for(&p).or(Some(p)), command: StartCommand::Browse });
            }
            return Err(CliExit::Error(format!("Unknown command '{other}'.\n\n{HELP}")));
        }
    };
    let needs_repo = !matches!(command, StartCommand::Clone(_) | StartCommand::Init(_) | StartCommand::Settings | StartCommand::About | StartCommand::ShellExt(_));
    if needs_repo && cwd_repo.is_none() {
        return Err(CliExit::NotARepository(cwd.to_path_buf()));
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

    #[test]
    fn explorer_path_selects_a_file_or_folder() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        gitext_core::GitModule::init(&repo, false, false).unwrap();
        let file = repo.join("src").join("main.rs");
        std::fs::write(&file, "fn main() {}").unwrap();
        let elsewhere = std::env::temp_dir();
        let f = file.display().to_string();

        let p = parse(&s(&["-C", &f, "filehistory"]), &elsewhere).unwrap();
        assert!(matches!(p.command, StartCommand::FileHistory(ref x) if x == "src/main.rs"));
        assert!(p.repo.is_some());
        let p = parse(&s(&["-C", &f, "difftool"]), &elsewhere).unwrap();
        assert!(matches!(p.command, StartCommand::DiffTool(ref x) if x == "src/main.rs"));
        let p = parse(&s(&["-C", &f, "reset"]), &elsewhere).unwrap();
        assert!(matches!(p.command, StartCommand::ResetChanges(ref x) if x == &["src/main.rs".to_string()]));
        // a folder: its path; the repository itself: everything
        let src = repo.join("src").display().to_string();
        let p = parse(&s(&["-C", &src, "addfiles"]), &elsewhere).unwrap();
        assert!(matches!(p.command, StartCommand::AddFiles(ref x) if x == "src"));
        let p = parse(&s(&["-C", &repo.display().to_string(), "reset"]), &elsewhere).unwrap();
        assert!(matches!(p.command, StartCommand::ResetChanges(ref x) if x.is_empty()));
        let p = parse(&s(&["-C", &src, "commit"]), &elsewhere).unwrap();
        assert!(matches!(p.command, StartCommand::Commit));

        // outside of a repository
        let outside = dir.path().display().to_string();
        assert!(matches!(parse(&s(&["-C", &outside, "commit"]), &elsewhere), Err(CliExit::NotARepository(_))));
        let p = parse(&s(&["-C", &outside, "init"]), &elsewhere).unwrap();
        assert!(matches!(p.command, StartCommand::Init(Some(ref d)) if Path::new(d) == dir.path()));
        assert!(matches!(parse(&s(&["-C", "/does/not/exist", "commit"]), &elsewhere), Err(CliExit::Error(_))));
    }
}
