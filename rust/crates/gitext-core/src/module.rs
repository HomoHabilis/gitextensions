//! Port of `GitCommands.GitModule`: operations on a git repository.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::args::{to_posix_path, GitArgs};
use crate::blame::{parse_git_blame, GitBlame};
use crate::commands;
use crate::exec::{ExecResult, Executable, GitError, GitResult};
use crate::git_ref::{ref_name, GitRef};
use crate::object_id::ObjectId;
use crate::revision::GitRevision;
use crate::revision_reader::{log_format, read_revisions};
use crate::status::{parse_diff_raw, parse_status_v2, GitItemStatus, StagedStatus};
use crate::tree::{self, GitItem};

/// The hash of the empty tree (diff against it for root commits).
pub const EMPTY_TREE_ID: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// A configured remote (port of `Remote`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Remote {
    pub name: String,
    pub fetch_url: String,
    pub push_urls: Vec<String>,
}

/// A worktree (port of `GitWorktree`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitWorktree {
    pub path: String,
    pub head_type: WorktreeHeadType,
    pub sha1: Option<String>,
    pub branch: Option<String>,
    pub is_main: bool,
    pub is_deleted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeHeadType {
    Bare,
    Branch,
    Detached,
}

/// A line of `git submodule status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmoduleInfo {
    pub path: String,
    pub commit: ObjectId,
    /// ' ' up to date, '-' not initialized, '+' different commit checked out, 'U' conflicts.
    pub status: char,
    pub describe: String,
}

/// An entry of the reflog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefLogItem {
    pub object_id: ObjectId,
    pub selector: String,
    pub subject: String,
}

/// Ahead/behind counts of a branch relative to its upstream.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AheadBehind {
    pub ahead: u32,
    pub behind: u32,
    pub gone: bool,
}

/// What the repository is in the middle of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RepoState {
    pub merging: bool,
    pub rebasing: bool,
    pub rebase_interactive: bool,
    pub applying_patch: bool,
    pub cherry_picking: bool,
    pub reverting: bool,
    pub bisecting: bool,
}

impl RepoState {
    pub fn in_progress(&self) -> bool {
        self.merging || self.rebasing || self.applying_patch || self.cherry_picking || self.reverting
    }

    pub fn description(&self) -> Option<&'static str> {
        if self.rebasing {
            Some(if self.rebase_interactive { "Interactive rebase in progress" } else { "Rebase in progress" })
        } else if self.applying_patch {
            Some("Applying patches (git am) in progress")
        } else if self.merging {
            Some("Merge in progress")
        } else if self.cherry_picking {
            Some("Cherry-pick in progress")
        } else if self.reverting {
            Some("Revert in progress")
        } else if self.bisecting {
            Some("Bisect in progress")
        } else {
            None
        }
    }
}

/// Options affecting how diffs are produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffOptions {
    pub ignore_whitespace: bool,
    pub ignore_whitespace_changes: bool,
    pub context_lines: Option<u32>,
    pub histogram: bool,
    pub word_diff: bool,
}

impl DiffOptions {
    fn extra_args(&self) -> Vec<String> {
        let mut a = Vec::new();
        if self.ignore_whitespace {
            a.push("--ignore-all-space".to_string());
        } else if self.ignore_whitespace_changes {
            a.push("--ignore-space-change".to_string());
        }
        if let Some(c) = self.context_lines {
            a.push(format!("--unified={c}"));
        }
        if self.histogram {
            a.push("--histogram".to_string());
        }
        if self.word_diff {
            a.push("--word-diff".to_string());
        }
        a
    }
}

/// A git repository (working directory).
#[derive(Debug, Clone)]
pub struct GitModule {
    work_dir: PathBuf,
    git_dir: PathBuf,
    is_bare: bool,
}

impl GitModule {
    /// Opens the repository containing `path` (searches parent directories).
    pub fn open(path: impl AsRef<Path>) -> GitResult<GitModule> {
        let path = path.as_ref();
        if !path.is_dir() {
            return Err(GitError::Invalid(format!("'{}' is not a directory", path.display())));
        }
        let exe = Executable::git(path);
        let out = exe.run(&GitArgs::new("rev-parse").args(["--is-bare-repository", "--absolute-git-dir", "--show-toplevel"]))?;
        if !out.success() {
            return Err(GitError::Invalid(format!("'{}' is not a git repository", path.display())));
        }
        let text = out.stdout_str();
        let mut lines = text.lines();
        let is_bare = lines.next() == Some("true");
        let git_dir = PathBuf::from(lines.next().unwrap_or_default());
        let work_dir = if is_bare { git_dir.clone() } else { PathBuf::from(lines.next().unwrap_or_default()) };
        Ok(GitModule { work_dir: normalize_path(&work_dir), git_dir: normalize_path(&git_dir), is_bare })
    }

    /// Whether `path` is inside a git repository.
    /// Port of `GitModule.IsValidGitWorkingDir`: a `.git` directory or file, or a bare
    /// repository layout. Only checks the file system (it is called for every parent folder).
    pub fn is_valid_git_working_dir(path: impl AsRef<Path>) -> bool {
        let p = path.as_ref();
        if p.as_os_str().is_empty() {
            return false;
        }
        p.join(".git").exists() || (p.join("info").is_dir() && p.join("objects").is_dir() && p.join("refs").is_dir())
    }

    /// Initialises a new repository (port of `FormInit`).
    pub fn init(path: impl AsRef<Path>, bare: bool, shared: bool) -> GitResult<ExecResult> {
        let path = path.as_ref();
        std::fs::create_dir_all(path)?;
        let exe = Executable::git(path);
        exe.run_checked(&GitArgs::new("init").arg_if(bare, "--bare").arg_if(shared, "--shared=all"))
    }

    pub fn work_dir(&self) -> &Path {
        &self.work_dir
    }

    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    pub fn is_bare(&self) -> bool {
        self.is_bare
    }

    /// A short name for the repository (directory name).
    pub fn name(&self) -> String {
        self.work_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| self.work_dir.display().to_string())
    }

    pub fn git(&self) -> Executable {
        Executable::git(&self.work_dir)
    }

    pub fn run(&self, args: &GitArgs) -> GitResult<ExecResult> {
        self.git().run(args)
    }

    pub fn run_checked(&self, args: &GitArgs) -> GitResult<ExecResult> {
        self.git().run_checked(args)
    }

    fn output(&self, args: &GitArgs) -> GitResult<String> {
        self.git().output(args)
    }

    // ---------------------------------------------------------------- revisions

    /// Resolves a revision expression; zero if invalid.
    pub fn rev_parse(&self, expr: &str) -> ObjectId {
        if expr.trim().is_empty() || expr.len() > 260 {
            return ObjectId::ZERO;
        }
        if let Some(id) = ObjectId::try_parse(expr) {
            return id;
        }
        self.run(&GitArgs::new("rev-parse").arg("--quiet").arg("--verify").arg(format!("{expr}~0")))
            .ok()
            .filter(|r| r.success())
            .and_then(|r| ObjectId::try_parse(r.stdout_str().trim()))
            .unwrap_or_default()
    }

    /// The checked out commit (zero for an unborn branch).
    pub fn head_id(&self) -> ObjectId {
        self.rev_parse("HEAD")
    }

    /// Name of the current branch; `None` when detached.
    pub fn current_branch(&self) -> Option<String> {
        let r = self.run(&GitArgs::new("symbolic-ref").arg("--quiet").arg("--short").arg("HEAD")).ok()?;
        r.success().then(|| r.stdout_str().trim().to_string()).filter(|s| !s.is_empty())
    }

    /// Branch name or `(no branch)` when detached (port of `GetSelectedBranch`).
    pub fn selected_branch_display(&self) -> String {
        self.current_branch().unwrap_or_else(|| "(no branch)".to_string())
    }

    pub fn is_detached_head(&self) -> bool {
        self.current_branch().is_none()
    }

    /// Reads a single revision with full message.
    pub fn get_revision(&self, rev: &str, has_notes: bool) -> GitResult<Option<GitRevision>> {
        if let Some(id) = ObjectId::try_parse(rev) {
            if id.is_artificial() {
                return Err(GitError::Invalid("artificial revision".into()));
            }
        }
        let args = GitArgs::new("log").arg("-z").arg("-1").arg(format!("--pretty=format:{}", log_format(false, has_notes))).arg(rev).arg("--");
        let out = self.run(&args)?;
        if !out.success() {
            return Ok(None);
        }
        let mut parser = crate::revision_reader::RevisionParser::new(false, has_notes, 0);
        Ok(out.stdout.split(|&b| b == 0).next().and_then(|c| parser.try_parse_revision(c)))
    }

    pub fn get_parents(&self, id: ObjectId) -> Vec<ObjectId> {
        self.output(&GitArgs::new("rev-parse").arg(format!("{id}^@")))
            .map(|o| o.lines().filter_map(|l| ObjectId::try_parse(l.trim())).collect())
            .unwrap_or_default()
    }

    /// The commit message of `rev` (for amend).
    pub fn get_commit_message(&self, rev: &str) -> String {
        self.output(&GitArgs::new("log").arg("-1").arg("--format=%B").arg(rev)).map(|s| s.trim_end().to_string()).unwrap_or_default()
    }

    /// Port of `GetTagMessage`.
    pub fn get_tag_message(&self, tag: &str) -> Option<String> {
        let out = self.output(&GitArgs::new("cat-file").arg("-p").arg(tag)).ok()?;
        let body = out.split_once("\n\n").map(|(_, b)| b)?;
        let body = body.split("-----BEGIN PGP SIGNATURE-----").next().unwrap_or(body);
        let t = body.trim();
        (!t.is_empty()).then(|| t.to_string())
    }

    // ---------------------------------------------------------------- refs

    /// All refs with tracking information (port of `GetRefs`).
    pub fn get_refs(&self) -> GitResult<Vec<GitRef>> {
        let out = self.output(&GitArgs::new("for-each-ref").arg(
            "--format=%(objectname)%00%(*objectname)%00%(refname)%00%(upstream:remotename)%00%(upstream:remoteref)",
        ))?;
        let current = self.current_branch();
        let mut refs = parse_refs(&out);
        let upstream = refs.iter().find(|r| current.as_deref() == Some(r.name.as_str())).map(|r| (r.tracking_remote.clone(), r.merge_with.clone()));
        for r in &mut refs {
            r.is_selected = r.is_head() && current.as_deref() == Some(r.name.as_str());
            if let Some((remote, merge)) = &upstream {
                r.is_selected_head_merge_source = r.is_remote() && &r.remote == remote && &r.local_name() == merge;
            }
        }
        Ok(refs)
    }

    /// Refs grouped by commit (for the revision grid labels).
    pub fn get_refs_by_commit(&self) -> HashMap<ObjectId, Vec<GitRef>> {
        let mut map: HashMap<ObjectId, Vec<GitRef>> = HashMap::new();
        for r in self.get_refs().unwrap_or_default() {
            if ref_name::is_remote_head(&r.complete_name) {
                continue;
            }
            map.entry(r.object_id).or_default().push(r);
        }
        map
    }

    pub fn get_ahead_behind(&self) -> HashMap<String, AheadBehind> {
        let out = self.output(&GitArgs::new("for-each-ref").arg("--format=%(refname:short)%00%(upstream:track)").arg("refs/heads/")).unwrap_or_default();
        parse_ahead_behind(&out)
    }

    /// Branches containing `id` (port of `GetAllBranchesWhichContainGivenCommit`).
    pub fn branches_containing(&self, id: ObjectId, include_local: bool, include_remote: bool) -> Vec<String> {
        if !include_local && !include_remote {
            return Vec::new();
        }
        let args = GitArgs::new("branch")
            .arg_if(include_remote && include_local, "-a")
            .arg_if(include_remote && !include_local, "-r")
            .arg("--format=%(refname:short)")
            .arg("--contains")
            .arg(id.to_string());
        self.output(&args).map(|o| o.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty() && !l.ends_with("/HEAD")).collect()).unwrap_or_default()
    }

    pub fn tags_containing(&self, id: ObjectId) -> Vec<String> {
        self.output(&GitArgs::new("tag").arg("--contains").arg(id.to_string()))
            .map(|o| o.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// `git describe` (port of `GitDescribeProvider`): (closest tag, commits since).
    pub fn describe(&self, id: ObjectId) -> Option<String> {
        let out = self.output(&GitArgs::new("describe").arg("--tags").arg(id.to_string())).ok()?;
        Some(out.trim().to_string()).filter(|s| !s.is_empty())
    }

    pub fn get_stashes(&self) -> Vec<GitRevision> {
        let args = GitArgs::new("stash").arg("list").arg("-z").arg(format!("--pretty=format:{}", log_format(true, false)));
        read_revisions(&self.git(), &args, true).unwrap_or_default()
    }

    // ---------------------------------------------------------------- status / diff

    pub fn state(&self) -> RepoState {
        let g = &self.git_dir;
        RepoState {
            merging: g.join("MERGE_HEAD").exists(),
            rebasing: g.join("rebase-merge").is_dir() || (g.join("rebase-apply").is_dir() && !g.join("rebase-apply/applying").exists()),
            rebase_interactive: g.join("rebase-merge/interactive").exists(),
            applying_patch: g.join("rebase-apply/applying").exists(),
            cherry_picking: g.join("CHERRY_PICK_HEAD").exists(),
            reverting: g.join("REVERT_HEAD").exists(),
            bisecting: g.join("BISECT_LOG").exists(),
        }
    }

    /// Work tree and index changes (`git status --porcelain=2`).
    pub fn get_status(&self, untracked: commands::UntrackedFilesMode, no_locks: bool) -> GitResult<Vec<GitItemStatus>> {
        let args = commands::get_all_changed_files(true, untracked, commands::IgnoreSubmodulesMode::None, no_locks);
        let out = self.run(&args)?;
        if !out.success() {
            return Err(GitError::Failed { args: args.to_string(), exit_code: out.exit_code, stderr: out.stderr_str() });
        }
        Ok(parse_status_v2(&out.stdout_str()))
    }

    /// Changes between two revisions; handles the artificial work tree / index revisions
    /// (port of `GetDiffFilesWithSubmodulesStatus`). `first` = older revision.
    pub fn get_diff_files(&self, first: Option<ObjectId>, second: ObjectId) -> GitResult<Vec<GitItemStatus>> {
        if second == ObjectId::WORK_TREE && (first.is_none() || first == Some(ObjectId::INDEX)) {
            let all = self.get_status(commands::UntrackedFilesMode::All, true)?;
            return Ok(all.into_iter().filter(|s| s.staged == StagedStatus::WorkTree).collect());
        }
        if second == ObjectId::INDEX && (first.is_none() || first.is_some_and(|f| f == self.head_id())) {
            let all = self.get_status(commands::UntrackedFilesMode::No, true)?;
            return Ok(all.into_iter().filter(|s| s.staged == StagedStatus::Index).collect());
        }
        let staged = if second == ObjectId::WORK_TREE {
            StagedStatus::WorkTree
        } else if second == ObjectId::INDEX {
            StagedStatus::Index
        } else {
            StagedStatus::None
        };
        let mut args = GitArgs::with_config(&[("diff.ignoresubmodules", "none")], "diff");
        args.add_all(["--no-ext-diff", "--find-renames", "--find-copies", "-z", "--raw"]);
        let first_arg = match first {
            Some(f) if f == ObjectId::INDEX => None,
            Some(f) if f.is_zero() => Some(EMPTY_TREE_ID.to_string()),
            Some(f) => Some(f.to_string()),
            None => {
                let parents = self.get_parents(second);
                Some(parents.first().map(|p| p.to_string()).unwrap_or_else(|| EMPTY_TREE_ID.to_string()))
            }
        };
        if second == ObjectId::INDEX {
            args.add("--cached");
            args.add_opt(first_arg);
        } else if second == ObjectId::WORK_TREE {
            args.add_opt(first_arg);
        } else {
            args.add_opt(first_arg);
            args.add(second.to_string());
        }
        args.add("--");
        let out = self.run_checked(&args)?;
        let mut files = parse_diff_raw(&out.stdout_str(), staged);
        if second == ObjectId::WORK_TREE {
            // untracked files are not part of git-diff
            let untracked = self.get_status(commands::UntrackedFilesMode::All, true)?;
            files.extend(untracked.into_iter().filter(|s| s.staged == StagedStatus::WorkTree && s.is_new && !s.is_tracked));
        }
        Ok(files)
    }

    /// Port of `GetStagedStatus`: classify a revision pair.
    pub fn staged_status(first: Option<ObjectId>, second: ObjectId, parent_to_second: Option<ObjectId>) -> StagedStatus {
        if second == ObjectId::WORK_TREE && (first == Some(ObjectId::INDEX) || first.is_none()) {
            StagedStatus::WorkTree
        } else if second == ObjectId::INDEX && (first == parent_to_second || first.is_none()) {
            StagedStatus::Index
        } else if first.is_some_and(|f| f.is_artificial()) || second.is_artificial() {
            StagedStatus::Unknown
        } else {
            StagedStatus::None
        }
    }

    /// The unified diff for one file between two revisions (artificial ids supported).
    pub fn get_file_diff(&self, first: Option<ObjectId>, second: ObjectId, file: &str, old_file: Option<&str>, options: &DiffOptions) -> GitResult<String> {
        let extra = options.extra_args();
        let extra: Vec<&str> = extra.iter().map(String::as_str).collect();
        let args = if second == ObjectId::WORK_TREE && (first.is_none() || first == Some(ObjectId::INDEX)) {
            commands::get_current_changes(file, old_file, false, &extra, true)
        } else if second == ObjectId::INDEX && (first.is_none() || first.is_some_and(|f| f == self.head_id())) {
            commands::get_current_changes(file, old_file, true, &extra, true)
        } else {
            let mut a = GitArgs::with_config(&commands::DIFF_CONFIGS, "diff");
            a.add_all(["--no-ext-diff", "--find-renames", "--find-copies"]);
            a.add_all(extra.iter().copied());
            let first = match first {
                Some(f) if f == ObjectId::INDEX => None,
                Some(f) if !f.is_zero() => Some(f.to_string()),
                _ => Some(self.get_parents(second).first().map(|p| p.to_string()).unwrap_or_else(|| EMPTY_TREE_ID.to_string())),
            };
            if second == ObjectId::INDEX {
                a.add("--cached");
                a.add_opt(first);
            } else if second == ObjectId::WORK_TREE {
                a.add_opt(first);
            } else {
                a.add_opt(first);
                a.add(second.to_string());
            }
            a.add("--");
            a.add(to_posix_path(file));
            if let Some(o) = old_file.filter(|o| *o != file) {
                a.add(to_posix_path(o));
            }
            a
        };
        let out = self.run(&args)?;
        let mut text = out.stdout_str();
        if text.is_empty() && second == ObjectId::WORK_TREE {
            // Untracked file: show it as added
            let r = self.run(&GitArgs::with_config(&commands::DIFF_CONFIGS, "diff").args(["--no-ext-diff", "--no-index", "--"]).arg("/dev/null").arg(file).args(extra.iter().copied()))?;
            text = r.stdout_str();
        }
        Ok(text)
    }

    /// Combined diff of a merge commit (`git diff-tree --cc`).
    pub fn get_combined_diff(&self, id: ObjectId, file: &str) -> GitResult<String> {
        self.output(&GitArgs::with_config(&commands::DIFF_CONFIGS, "diff-tree").args(["--no-ext-diff", "--cc"]).arg(id.to_string()).arg("--").arg(file))
    }

    /// Full patch of a commit (`git show`).
    pub fn show_patch(&self, rev: &str) -> GitResult<String> {
        self.output(&GitArgs::with_config(&commands::DIFF_CONFIGS, "show").args(["--no-ext-diff", "--stat", "--patch", "-M", "-C"]).arg(rev))
    }

    /// The content of a file at a revision (work tree / index supported).
    pub fn get_file_bytes(&self, rev: ObjectId, file: &str) -> GitResult<Vec<u8>> {
        if rev == ObjectId::WORK_TREE {
            return Ok(std::fs::read(self.work_dir.join(file))?);
        }
        let spec = if rev == ObjectId::INDEX { format!(":{}", to_posix_path(file)) } else { format!("{rev}:{}", to_posix_path(file)) };
        Ok(self.run_checked(&GitArgs::new("show").arg(spec))?.stdout)
    }

    pub fn get_blob(&self, id: ObjectId) -> GitResult<Vec<u8>> {
        Ok(self.run_checked(&GitArgs::new("cat-file").arg("blob").arg(id.to_string()))?.stdout)
    }

    /// Tree entries at `rev` below `path` (empty = root).
    pub fn ls_tree(&self, rev: ObjectId, path: &str) -> GitResult<Vec<GitItem>> {
        let spec = if path.is_empty() { rev.to_string() } else { format!("{rev}:{}", to_posix_path(path)) };
        let out = self.output(&GitArgs::new("ls-tree").arg("-z").arg(spec))?;
        Ok(tree::parse(&out))
    }

    /// All files at `rev` (recursive, for "find file").
    pub fn ls_tree_recursive(&self, rev: ObjectId) -> GitResult<Vec<String>> {
        let out = self.output(&GitArgs::new("ls-tree").arg("-r").arg("-z").arg("--name-only").arg(rev.to_string()))?;
        Ok(out.split('\0').filter(|s| !s.is_empty()).map(str::to_string).collect())
    }

    pub fn blame(&self, rev: ObjectId, file: &str, detect_moves: bool, detect_copies: bool, ignore_whitespace: bool) -> GitResult<GitBlame> {
        let mut args = GitArgs::new("blame").arg("--porcelain");
        args.add_if(detect_moves, "-M");
        args.add_if(detect_copies, "-C");
        args.add_if(ignore_whitespace, "-w");
        if !rev.is_artificial() && !rev.is_zero() {
            args.add(rev.to_string());
        }
        args.add("--");
        args.add(to_posix_path(file));
        let out = self.run_checked(&args)?;
        Ok(parse_git_blame(&out.stdout_str()))
    }

    /// The line in the parent commit corresponding to `line` of `file` in `rev`.
    pub fn original_line_in_previous_commit(&self, rev: &GitRevision, file: &str, line: i64) -> i64 {
        let parent = rev.first_parent_id();
        if parent.is_zero() {
            return line;
        }
        let args = GitArgs::new("diff").args(["--no-ext-diff", "-U0"]).arg(parent.to_string()).arg(rev.object_id.to_string()).arg("--").arg(file);
        self.output(&args).map(|d| crate::blame::original_line_in_previous_commit(&d, line)).unwrap_or(line)
    }

    /// Unmerged files during a merge conflict.
    pub fn get_unmerged_files(&self) -> Vec<String> {
        let out = self.output(&GitArgs::new("diff").arg("--name-only").arg("--diff-filter=U").arg("-z")).unwrap_or_default();
        let mut v: Vec<String> = out.split('\0').filter(|s| !s.is_empty()).map(str::to_string).collect();
        v.dedup();
        v
    }

    // ---------------------------------------------------------------- staging

    pub fn stage_files(&self, files: &[&str]) -> GitResult<ExecResult> {
        if files.is_empty() {
            return Ok(ExecResult::default());
        }
        self.run_checked(&GitArgs::new("add").arg("--all").arg("--").args(files.iter().map(|f| to_posix_path(f))))
    }

    pub fn stage_all(&self) -> GitResult<ExecResult> {
        self.run_checked(&GitArgs::new("add").arg("--all"))
    }

    /// Unstages files (handles the unborn branch case).
    pub fn unstage_files(&self, files: &[&str]) -> GitResult<ExecResult> {
        if files.is_empty() {
            return Ok(ExecResult::default());
        }
        let paths: Vec<String> = files.iter().map(|f| to_posix_path(f)).collect();
        if self.head_id().is_zero() {
            return self.run_checked(&GitArgs::new("rm").arg("--cached").arg("-r").arg("--quiet").arg("--").args(paths));
        }
        self.run_checked(&GitArgs::new("reset").arg("--quiet").arg("HEAD").arg("--").args(paths))
    }

    pub fn unstage_all(&self) -> GitResult<ExecResult> {
        if self.head_id().is_zero() {
            return self.run_checked(&GitArgs::new("rm").arg("--cached").arg("-r").arg("--quiet").arg("."));
        }
        self.run_checked(&GitArgs::new("reset").arg("--quiet").arg("HEAD").arg("--"))
    }

    /// Discards work tree changes of tracked files (port of `ResetFiles`).
    pub fn reset_files(&self, files: &[&str]) -> GitResult<ExecResult> {
        if files.is_empty() {
            return Ok(ExecResult::default());
        }
        self.run_checked(&GitArgs::new("checkout").arg("--").args(files.iter().map(|f| to_posix_path(f))))
    }

    /// Resets files to `rev` in both index and work tree.
    pub fn reset_files_to(&self, rev: &str, files: &[&str]) -> GitResult<ExecResult> {
        self.run_checked(&GitArgs::new("checkout").arg(rev).arg("--").args(files.iter().map(|f| to_posix_path(f))))
    }

    /// Removes files from index and work tree (port of `RemoveFiles`).
    pub fn remove_files(&self, files: &[&str], force: bool) -> GitResult<ExecResult> {
        if files.is_empty() {
            return Ok(ExecResult::default());
        }
        self.run_checked(&commands::remove(force, false, &files.iter().map(|f| f.as_ref()).collect::<Vec<&str>>()))
    }

    /// Deletes untracked files from disk.
    pub fn delete_untracked(&self, files: &[&str]) -> std::io::Result<()> {
        for f in files {
            let p = self.work_dir.join(f);
            if p.is_dir() {
                std::fs::remove_dir_all(p)?;
            } else if p.exists() {
                std::fs::remove_file(p)?;
            }
        }
        Ok(())
    }

    /// Applies a patch, e.g. to stage selected lines (`cached`) or reset them (`reverse`).
    pub fn apply_patch_text(&self, patch: &str, cached: bool, reverse: bool) -> GitResult<ExecResult> {
        let args = GitArgs::new("apply")
            .arg_if(cached, "--cached")
            .arg_if(reverse, "--reverse")
            .arg("--whitespace=nowarn")
            .arg("--recount")
            .arg("--unidiff-zero")
            .arg("-");
        let r = self.git().run_with_input(&args, Some(patch.as_bytes()))?;
        if !r.success() {
            return Err(GitError::Failed { args: args.to_string(), exit_code: r.exit_code, stderr: r.all_output() });
        }
        Ok(r)
    }

    pub fn assume_unchanged(&self, files: &[&str], value: bool) -> GitResult<ExecResult> {
        self.run_checked(&GitArgs::new("update-index").arg(if value { "--assume-unchanged" } else { "--no-assume-unchanged" }).arg("--").args(files.iter().copied()))
    }

    pub fn skip_worktree(&self, files: &[&str], value: bool) -> GitResult<ExecResult> {
        self.run_checked(&GitArgs::new("update-index").arg(if value { "--skip-worktree" } else { "--no-skip-worktree" }).arg("--").args(files.iter().copied()))
    }

    // ---------------------------------------------------------------- commit

    /// Path of the file where the commit dialog stores the message (`.git/COMMITMESSAGE`).
    pub fn commit_message_path(&self) -> PathBuf {
        self.git_dir.join("COMMITMESSAGE")
    }

    /// Commits with `message` (written to a file and passed with `-F`).
    pub fn commit(&self, message: &str, mut options: commands::CommitOptions) -> GitResult<ExecResult> {
        let path = self.git_dir.join("COMMITMESSAGE");
        std::fs::write(&path, message)?;
        options.commit_message_file = Some(path.display().to_string());
        let r = self.run(&commands::commit(&options))?;
        if !r.success() {
            return Err(GitError::Failed { args: "commit".into(), exit_code: r.exit_code, stderr: r.all_output() });
        }
        let _ = std::fs::remove_file(path);
        Ok(r)
    }

    /// Merge message prepared by git (`MERGE_MSG`), if any.
    pub fn merge_message(&self) -> Option<String> {
        std::fs::read_to_string(self.git_dir.join("MERGE_MSG")).ok().map(|m| {
            m.lines().filter(|l| !l.starts_with('#')).collect::<Vec<_>>().join("\n").trim().to_string()
        })
    }

    // ---------------------------------------------------------------- config

    pub fn get_config(&self, key: &str) -> Option<String> {
        let r = self.run(&GitArgs::new("config").arg("--get").arg(key)).ok()?;
        r.success().then(|| r.stdout_str().trim_end_matches(['\n', '\r']).to_string())
    }

    pub fn set_config(&self, key: &str, value: &str, global: bool) -> GitResult<ExecResult> {
        self.run_checked(&GitArgs::new("config").arg_if(global, "--global").arg_if(!global, "--local").arg(key).arg(value))
    }

    pub fn unset_config(&self, key: &str, global: bool) -> GitResult<ExecResult> {
        let r = self.run(&GitArgs::new("config").arg_if(global, "--global").arg_if(!global, "--local").arg("--unset-all").arg(key))?;
        Ok(r)
    }

    /// All effective config entries (`key`, `value`).
    pub fn list_config(&self) -> Vec<(String, String)> {
        let out = self.output(&GitArgs::new("config").arg("--list").arg("-z")).unwrap_or_default();
        out.split('\0')
            .filter(|s| !s.is_empty())
            .map(|e| match e.split_once('\n') {
                Some((k, v)) => (k.to_string(), v.to_string()),
                None => (e.to_string(), String::new()),
            })
            .collect()
    }

    pub fn user_name(&self) -> String {
        self.get_config("user.name").unwrap_or_default()
    }

    pub fn user_email(&self) -> String {
        self.get_config("user.email").unwrap_or_default()
    }

    // ---------------------------------------------------------------- remotes

    pub fn get_remotes(&self) -> GitResult<Vec<Remote>> {
        let out = self.output(&GitArgs::new("remote").arg("-v"))?;
        Ok(parse_remotes(&out))
    }

    pub fn get_remote_names(&self) -> Vec<String> {
        self.output(&GitArgs::new("remote")).map(|o| o.lines().map(str::to_string).filter(|l| !l.is_empty()).collect()).unwrap_or_default()
    }

    pub fn add_remote(&self, name: &str, url: &str) -> GitResult<ExecResult> {
        if name.trim().is_empty() {
            return Err(GitError::Invalid("Please enter a name.".into()));
        }
        self.run_checked(&GitArgs::new("remote").arg("add").arg(name.trim()).arg(to_posix_path(url.trim())))
    }

    pub fn remove_remote(&self, name: &str) -> GitResult<ExecResult> {
        self.run_checked(&GitArgs::new("remote").arg("rm").arg(name))
    }

    pub fn rename_remote(&self, old: &str, new: &str) -> GitResult<ExecResult> {
        self.run_checked(&GitArgs::new("remote").arg("rename").arg(old).arg(new))
    }

    pub fn set_remote_url(&self, name: &str, url: &str, push: bool) -> GitResult<ExecResult> {
        if push && url.trim().is_empty() {
            return self.unset_config(&format!("remote.{name}.pushurl"), false);
        }
        self.run_checked(&GitArgs::new("remote").arg("set-url").arg_if(push, "--push").arg(name).arg(to_posix_path(url.trim())))
    }

    pub fn prune_remote(&self, name: &str) -> GitResult<ExecResult> {
        self.run_checked(&GitArgs::new("remote").arg("prune").arg(name))
    }

    /// Branches on a remote (`ls-remote --heads`).
    pub fn ls_remote_heads(&self, remote: &str) -> GitResult<Vec<String>> {
        let out = self.output(&GitArgs::new("ls-remote").arg("--heads").arg(remote))?;
        Ok(out.lines().filter_map(|l| l.split('\t').nth(1)).map(|r| r.strip_prefix("refs/heads/").unwrap_or(r).to_string()).collect())
    }

    /// Port of `FetchCmd`.
    pub fn fetch_args(&self, remote: &str, remote_branch: &str, local_branch: &str, fetch_tags: Option<bool>, prune: bool, unshallow: bool) -> GitArgs {
        let mut a = GitArgs::with_config(&[("fetch.parallel", "0"), ("submodule.fetchjobs", "0")], "fetch");
        a.add("--progress");
        if remote.is_empty() {
            a.add("--all");
        } else {
            a.add(remote);
        }
        if !remote_branch.is_empty() {
            let local = if local_branch.is_empty() { String::new() } else { format!(":refs/heads/{local_branch}") };
            a.add(format!("+{remote_branch}{local}"));
        }
        match fetch_tags {
            Some(true) => a.add("--tags"),
            Some(false) => a.add("--no-tags"),
            None => &mut a,
        };
        a.add_if(prune, "--prune");
        a.add_if(unshallow && self.git_dir.join("shallow").exists(), "--unshallow");
        a
    }

    /// Port of `PullCmd`.
    pub fn pull_args(&self, remote: &str, remote_branch: &str, rebase: bool, fetch_tags: Option<bool>, autostash: bool, prune: bool) -> GitArgs {
        let mut a = GitArgs::new("pull");
        a.add("--progress");
        if rebase {
            a.add("--rebase");
            a.add_if(autostash, "--autostash");
        } else {
            a.add("--no-rebase");
        }
        if remote.is_empty() {
            a.add("--all");
        } else {
            a.add(remote);
            a.add_if(!remote_branch.is_empty(), format!("+{remote_branch}"));
        }
        match fetch_tags {
            Some(true) => a.add("--tags"),
            Some(false) => a.add("--no-tags"),
            None => &mut a,
        };
        a.add_if(prune, "--prune");
        a
    }

    // ---------------------------------------------------------------- submodules

    pub fn get_submodules(&self) -> Vec<SubmoduleInfo> {
        let out = self.output(&GitArgs::new("submodule").arg("status").arg("--recursive")).unwrap_or_default();
        parse_submodule_status(&out)
    }

    /// Paths of submodules configured in `.gitmodules` (port of `GetSubmodulesLocalPaths`).
    pub fn get_submodule_paths(&self) -> Vec<String> {
        if !self.work_dir.join(".gitmodules").exists() {
            return Vec::new();
        }
        let out = self.output(&GitArgs::new("config").args(["--file", ".gitmodules", "--get-regexp", r"submodule\..*\.path"])).unwrap_or_default();
        out.lines().filter_map(|l| l.split_once(' ').map(|(_, p)| p.trim().to_string())).filter(|p| !p.is_empty()).collect()
    }

    /// The superproject working directory, if this is a submodule.
    pub fn superproject(&self) -> Option<PathBuf> {
        let out = self.output(&GitArgs::new("rev-parse").arg("--show-superproject-working-tree")).ok()?;
        let p = out.trim();
        (!p.is_empty()).then(|| PathBuf::from(p))
    }

    // ---------------------------------------------------------------- worktrees

    pub fn get_worktrees(&self) -> Vec<GitWorktree> {
        let out = self.output(&GitArgs::new("worktree").arg("list").arg("--porcelain").arg("-z")).unwrap_or_default();
        parse_worktrees(&out)
    }

    // ---------------------------------------------------------------- reflog

    pub fn get_reflog(&self, git_ref: &str) -> Vec<RefLogItem> {
        let out = self.output(&GitArgs::new("reflog").arg("show").arg("-z").arg("--format=%H%x1f%gd%x1f%gs").arg(git_ref)).unwrap_or_default();
        parse_reflog(&out)
    }

    // ---------------------------------------------------------------- ignore / misc files

    pub fn read_work_file(&self, relative: &str) -> String {
        std::fs::read_to_string(self.work_dir.join(relative)).unwrap_or_default()
    }

    pub fn write_work_file(&self, relative: &str, content: &str) -> std::io::Result<()> {
        let p = self.work_dir.join(relative);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(p, content)
    }

    pub fn exclude_file_path(&self) -> PathBuf {
        self.git_dir.join("info").join("exclude")
    }

    /// Appends patterns to `.gitignore` (or `.git/info/exclude` when `local`).
    pub fn add_to_ignore(&self, patterns: &[&str], local: bool) -> std::io::Result<()> {
        let path = if local { self.exclude_file_path() } else { self.work_dir.join(".gitignore") };
        let mut content = std::fs::read_to_string(&path).unwrap_or_default();
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        for p in patterns {
            if !content.lines().any(|l| l.trim() == p.trim()) {
                content.push_str(p);
                content.push('\n');
            }
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)
    }

    /// Files that would be ignored by `patterns` (preview in the add-to-gitignore dialog).
    pub fn files_matching_ignore(&self, patterns: &[&str]) -> Vec<String> {
        let mut args = GitArgs::new("ls-files").arg("-z").arg("--others").arg("--ignored");
        for p in patterns.iter().filter(|p| !p.trim().is_empty()) {
            args.add(format!("--exclude={}", p.trim()));
        }
        self.output(&args).map(|o| o.split('\0').filter(|s| !s.is_empty()).map(str::to_string).collect()).unwrap_or_default()
    }

    /// Lost objects (`git fsck --lost-found`) for the verify dialog: (id, kind).
    pub fn lost_objects(&self) -> Vec<(ObjectId, String)> {
        let out = self.run(&GitArgs::new("fsck").arg("--lost-found").arg("--no-reflogs").arg("--unreachable")).map(|r| r.all_output()).unwrap_or_default();
        out.lines()
            .filter_map(|l| {
                let mut parts = l.split_whitespace();
                let first = parts.next()?;
                if first != "unreachable" && first != "dangling" {
                    return None;
                }
                let kind = parts.next()?.to_string();
                let id = ObjectId::try_parse(parts.next()?)?;
                Some((id, kind))
            })
            .collect()
    }

    /// Sparse checkout patterns.
    pub fn sparse_checkout_patterns(&self) -> Option<String> {
        if self.get_config("core.sparseCheckout").as_deref() != Some("true") {
            return None;
        }
        Some(std::fs::read_to_string(self.git_dir.join("info").join("sparse-checkout")).unwrap_or_default())
    }

    /// GPG signature status of a commit (`%G?` and `%GS`).
    pub fn gpg_info(&self, id: ObjectId) -> Option<String> {
        let out = self.output(&GitArgs::new("log").arg("-1").arg("--format=%G?%n%GS%n%GK").arg(id.to_string())).ok()?;
        let mut l = out.lines();
        let status = l.next()?;
        let signer = l.next().unwrap_or_default();
        let key = l.next().unwrap_or_default();
        let s = match status {
            "G" => "Good signature",
            "B" => "Bad signature",
            "U" => "Good signature, unknown validity",
            "X" => "Good signature, expired",
            "Y" => "Good signature, expired key",
            "R" => "Good signature, revoked key",
            "E" => "Signature cannot be checked (missing key)",
            _ => return None,
        };
        Some(format!("{s}\nSigner: {signer}\nKey: {key}"))
    }

    /// Recent commit messages of the current user (for the commit message history menu).
    pub fn recent_commit_messages(&self, count: usize) -> Vec<String> {
        let author = self.user_email();
        let mut args = GitArgs::new("log").arg("-z").arg(format!("-n{count}")).arg("--format=%B");
        if !author.is_empty() {
            args.add(format!("--author={author}"));
        }
        self.output(&args).map(|o| o.split('\0').map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).collect()).unwrap_or_default()
    }

    /// Number of commits on HEAD (for the status bar).
    pub fn commit_count(&self, rev: &str) -> usize {
        self.output(&GitArgs::new("rev-list").arg("--count").arg(rev)).ok().and_then(|o| o.trim().parse().ok()).unwrap_or(0)
    }

    /// Files touched by a commit range (port of `GetRevisionsFromRange` file listing).
    pub fn is_ancestor(&self, ancestor: ObjectId, descendant: ObjectId) -> bool {
        self.run(&GitArgs::new("merge-base").arg("--is-ancestor").arg(ancestor.to_string()).arg(descendant.to_string())).map(|r| r.success()).unwrap_or(false)
    }

    pub fn merge_base(&self, a: &str, b: &str) -> Option<ObjectId> {
        self.output(&GitArgs::new("merge-base").arg(a).arg(b)).ok().and_then(|o| ObjectId::try_parse(o.trim()))
    }
}

fn normalize_path(p: &Path) -> PathBuf {
    // git returns forward slashes on Windows; use native separators.
    PathBuf::from(p.to_string_lossy().replace('/', std::path::MAIN_SEPARATOR_STR))
}

/// Parses the custom `for-each-ref` output of [`GitModule::get_refs`].
pub fn parse_refs(output: &str) -> Vec<GitRef> {
    let mut refs = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.split('\0').collect();
        if parts.len() < 3 {
            continue;
        }
        let id = if parts[1].is_empty() { parts[0] } else { parts[1] };
        let Some(id) = ObjectId::try_parse(id) else {
            continue;
        };
        let mut r = GitRef::from_complete_name(id, parts[2]);
        if r.is_head() && parts.len() >= 5 && !parts[4].is_empty() {
            r = r.with_tracking(parts[3], parts[4]);
        }
        refs.push(r);
    }
    refs
}

/// Parses `%(refname:short)%00%(upstream:track)` (port of `AheadBehindDataProvider`).
pub fn parse_ahead_behind(output: &str) -> HashMap<String, AheadBehind> {
    let mut map = HashMap::new();
    for line in output.lines() {
        let Some((branch, track)) = line.split_once('\0') else {
            continue;
        };
        let track = track.trim_matches(['[', ']']);
        let mut ab = AheadBehind::default();
        if track.is_empty() {
            continue;
        }
        for part in track.split(", ") {
            if let Some(n) = part.strip_prefix("ahead ") {
                ab.ahead = n.parse().unwrap_or(0);
            } else if let Some(n) = part.strip_prefix("behind ") {
                ab.behind = n.parse().unwrap_or(0);
            } else if part == "gone" {
                ab.gone = true;
            }
        }
        map.insert(branch.to_string(), ab);
    }
    map
}

/// Parses `git remote -v` (port of `GetRemotesAsync`).
pub fn parse_remotes(output: &str) -> Vec<Remote> {
    let mut remotes: Vec<Remote> = Vec::new();
    for line in output.lines() {
        let Some((name, rest)) = line.split_once('\t') else {
            continue;
        };
        let rest = rest.trim();
        let (url, kind) = if let Some(i) = rest.rfind(" (fetch)") {
            (&rest[..i], "fetch")
        } else if let Some(i) = rest.rfind(" (push)") {
            (&rest[..i], "push")
        } else {
            continue;
        };
        let url = to_posix_path(url);
        let idx = match remotes.iter().position(|r| r.name == name) {
            Some(i) => i,
            None => {
                remotes.push(Remote { name: name.to_string(), ..Default::default() });
                remotes.len() - 1
            }
        };
        if kind == "fetch" {
            remotes[idx].fetch_url = url;
        } else {
            remotes[idx].push_urls.push(url);
        }
    }
    remotes
}

/// Parses `git submodule status`.
pub fn parse_submodule_status(output: &str) -> Vec<SubmoduleInfo> {
    output
        .lines()
        .filter_map(|l| {
            if l.len() < 42 {
                return None;
            }
            let status = l.chars().next()?;
            let commit = ObjectId::try_parse(&l[1..41])?;
            let rest = l[42..].trim();
            let (path, describe) = match rest.rfind(" (") {
                Some(i) if rest.ends_with(')') => (&rest[..i], rest[i + 2..rest.len() - 1].to_string()),
                _ => (rest, String::new()),
            };
            Some(SubmoduleInfo { path: path.to_string(), commit, status, describe })
        })
        .collect()
}

/// Parses `git worktree list --porcelain -z` (port of `GetWorktrees`).
pub fn parse_worktrees(output: &str) -> Vec<GitWorktree> {
    let mut result = Vec::new();
    let mut current: Option<GitWorktree> = None;
    for item in output.split('\0') {
        if item.is_empty() {
            if let Some(w) = current.take() {
                result.push(w);
            }
            continue;
        }
        if let Some(path) = item.strip_prefix("worktree ") {
            let native = normalize_path(Path::new(path)).display().to_string();
            let is_deleted = !Path::new(path).exists();
            current = Some(GitWorktree {
                path: native,
                head_type: WorktreeHeadType::Detached,
                sha1: None,
                branch: None,
                is_main: result.is_empty(),
                is_deleted,
            });
        } else if let Some(w) = current.as_mut() {
            if let Some(h) = item.strip_prefix("HEAD ") {
                w.sha1 = Some(h.to_string());
            } else if let Some(b) = item.strip_prefix("branch ") {
                w.head_type = WorktreeHeadType::Branch;
                w.branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
            } else if item == "detached" {
                w.head_type = WorktreeHeadType::Detached;
            } else if item == "bare" {
                w.head_type = WorktreeHeadType::Bare;
            }
        }
    }
    if let Some(w) = current {
        result.push(w);
    }
    result
}

/// Parses the reflog format of [`GitModule::get_reflog`].
pub fn parse_reflog(output: &str) -> Vec<RefLogItem> {
    output
        .split('\0')
        .filter_map(|e| {
            let mut p = e.trim_matches('\n').split('\u{1f}');
            let id = ObjectId::try_parse(p.next()?)?;
            Some(RefLogItem { object_id: id, selector: p.next().unwrap_or_default().to_string(), subject: p.next().unwrap_or_default().to_string() })
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod test_repo {
    //! A temporary repository for integration tests (port of `ReferenceRepository`).
    use super::*;

    pub struct TestRepo {
        pub dir: tempfile::TempDir,
        pub module: GitModule,
    }

    impl TestRepo {
        pub fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let exe = Executable::git(dir.path());
            exe.run_checked(&GitArgs::new("init").arg("-q").arg("-b").arg("master")).unwrap();
            for (k, v) in [("user.name", "Test User"), ("user.email", "test@example.com"), ("commit.gpgsign", "false"), ("core.autocrlf", "false")] {
                exe.run_checked(&GitArgs::new("config").arg(k).arg(v)).unwrap();
            }
            let module = GitModule::open(dir.path()).unwrap();
            TestRepo { dir, module }
        }

        pub fn write(&self, name: &str, content: &str) {
            let p = self.dir.path().join(name);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(p, content).unwrap();
        }

        #[allow(dead_code)]
        pub fn git(&self, args: &[&str]) -> String {
            self.module.output(&GitArgs::empty().args(args.iter().copied())).unwrap()
        }

        pub fn commit_file(&self, name: &str, content: &str, message: &str) -> ObjectId {
            self.write(name, content);
            self.module.stage_files(&[name]).unwrap();
            self.module.commit(message, Default::default()).unwrap();
            self.module.head_id()
        }
    }
}

#[cfg(test)]
mod tests {
    //! Ported from GitCommands.Tests/GitModuleTests.cs, GitModuleTests_Remotes.cs,
    //! Git/GitModuleWorktreeTests.cs and Git/AheadBehindDataProviderTests.cs, plus
    //! integration tests against real temporary repositories.
    use super::test_repo::TestRepo;
    use super::*;

    #[test]
    fn get_remotes_should_parse_correctly_configured_remotes() {
        let lines = [
            "RussKie\tgit://github.com/RussKie/gitextensions.git (fetch)",
            "RussKie\tgit://github.com/RussKie/gitextensions.git (push)",
            "origin\tgit@github.com:drewnoakes/gitextensions.git (fetch)",
            "origin\tgit@github.com:drewnoakes/gitextensions.git (push)",
            "upstream\tgit@github.com:gitextensions/gitextensions.git (fetch)",
            "upstream\tgit@github.com:gitextensions/gitextensions.git (push)",
            "asymmetrical\thttps://github.com/gitextensions/fetch.git (fetch)",
            "asymmetrical\thttps://github.com/gitextensions/push.git (push)",
            "with-space\tc:\\Bare Repo (fetch)",
            "with-space\tc:\\Bare Repo (push)",
            "multi\tgit@github.com:drewnoakes/gitextensions.git (fetch)",
            "multi\tgit@github.com:drewnoakes/gitextensions.git (push)",
            "multi\tgit@gitlab.com:drewnoakes/gitextensions.git (push)",
            "ignoreunknown\tgit@github.com:drewnoakes/gitextensions.git (unknownType)",
            "ignorenotab git@github.com:drewnoakes/gitextensions.git (fetch)",
            "ignoremissingtype\tgit@gitlab.com:drewnoakes/gitextensions.git",
            "git@gitlab.com:drewnoakes/gitextensions.git",
            "with_option\thttps://github.com/flannelhead/jsmn-stream.git (fetch) [blob:none]",
            "with_option\thttps://github.com/flannelhead/jsmn-stream.git (push) [ignored]",
        ];
        let remotes = parse_remotes(&lines.join("\n"));
        assert_eq!(remotes.len(), 7);
        assert_eq!(remotes[0].name, "RussKie");
        assert_eq!(remotes[0].fetch_url, "git://github.com/RussKie/gitextensions.git");
        assert_eq!(remotes[0].push_urls, ["git://github.com/RussKie/gitextensions.git"]);
        assert_eq!(remotes[3].fetch_url, "https://github.com/gitextensions/fetch.git");
        assert_eq!(remotes[3].push_urls, ["https://github.com/gitextensions/push.git"]);
        assert_eq!(remotes[4].fetch_url, "c:/Bare Repo");
        assert_eq!(remotes[5].push_urls.len(), 2);
        assert_eq!(remotes[5].push_urls[1], "git@gitlab.com:drewnoakes/gitextensions.git");
        assert_eq!(remotes[6].name, "with_option");
        assert_eq!(remotes[6].fetch_url, "https://github.com/flannelhead/jsmn-stream.git");
        assert_eq!(remotes[6].push_urls, ["https://github.com/flannelhead/jsmn-stream.git"]);
    }

    fn wt(parts: &[&str]) -> String {
        parts.join("\0")
    }

    #[test]
    fn get_worktrees_parsing() {
        let w = parse_worktrees(&wt(&["worktree C:/repos/main", "HEAD abc1234abc1234abc1234abc1234abc1234abc12", "branch refs/heads/master", "", ""]));
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].head_type, WorktreeHeadType::Branch);
        assert_eq!(w[0].sha1.as_deref(), Some("abc1234abc1234abc1234abc1234abc1234abc12"));
        assert_eq!(w[0].branch.as_deref(), Some("master"));
        assert!(w[0].is_deleted);

        let w = parse_worktrees(&wt(&["worktree C:/repos/detached", "HEAD def5678def5678def5678def5678def5678def56", "detached", "", ""]));
        assert_eq!(w[0].head_type, WorktreeHeadType::Detached);
        assert!(w[0].branch.is_none());

        let w = parse_worktrees(&wt(&["worktree C:/repos/bare", "bare", "", ""]));
        assert_eq!(w[0].head_type, WorktreeHeadType::Bare);
        assert!(w[0].sha1.is_none());

        let w = parse_worktrees(&wt(&[
            "worktree C:/repos/main",
            "HEAD aaaa1234aaaa1234aaaa1234aaaa1234aaaa1234",
            "branch refs/heads/master",
            "",
            "worktree C:/repos/feature",
            "HEAD bbbb5678bbbb5678bbbb5678bbbb5678bbbb5678",
            "branch refs/heads/feature/my-feature",
            "",
            "worktree C:/repos/hotfix",
            "HEAD cccc9012cccc9012cccc9012cccc9012cccc9012",
            "branch refs/heads/hotfix",
            "",
            "",
        ]));
        assert_eq!(w.len(), 3);
        assert_eq!(w[1].branch.as_deref(), Some("feature/my-feature"));
        assert_eq!(w.iter().map(|w| w.is_main).collect::<Vec<_>>(), [true, false, false]);

        let w = parse_worktrees(&wt(&["worktree C:/my repos/work tree", "HEAD abc1234abc1234abc1234abc1234abc1234abc12", "branch refs/heads/main", "", ""]));
        assert!(w[0].path.contains("my repos") && w[0].path.ends_with("work tree"));
        assert!(parse_worktrees("").is_empty());
    }

    #[test]
    fn ahead_behind_parsing() {
        let m = parse_ahead_behind("master\0[ahead 2, behind 3]\nfeature\0[ahead 1]\nold\0[gone]\nnone\0\n");
        assert_eq!(m["master"], AheadBehind { ahead: 2, behind: 3, gone: false });
        assert_eq!(m["feature"].ahead, 1);
        assert!(m["old"].gone);
        assert!(!m.contains_key("none"));
    }

    #[test]
    fn submodule_status_parsing() {
        let s = parse_submodule_status(" 1111111111111111111111111111111111111111 sub/a (v1.0)\n-2222222222222222222222222222222222222222 sub b\n");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].path, "sub/a");
        assert_eq!(s[0].describe, "v1.0");
        assert_eq!(s[1].status, '-');
        assert_eq!(s[1].path, "sub b");
    }

    #[test]
    fn get_staged_status() {
        let (a, b) = (ObjectId::random(), ObjectId::random());
        assert_eq!(GitModule::staged_status(Some(ObjectId::INDEX), ObjectId::WORK_TREE, None), StagedStatus::WorkTree);
        assert_eq!(GitModule::staged_status(Some(a), ObjectId::INDEX, Some(a)), StagedStatus::Index);
        assert_eq!(GitModule::staged_status(Some(b), ObjectId::INDEX, Some(a)), StagedStatus::Unknown);
        assert_eq!(GitModule::staged_status(Some(a), b, Some(a)), StagedStatus::None);
        assert_eq!(GitModule::staged_status(Some(ObjectId::WORK_TREE), b, None), StagedStatus::Unknown);
    }

    #[test]
    fn rev_parse_should_return_zero_if_invalid() {
        let repo = TestRepo::new();
        assert!(repo.module.rev_parse("").is_zero());
        assert!(repo.module.rev_parse(" ").is_zero());
        assert!(repo.module.rev_parse(&"a".repeat(261)).is_zero());
        assert!(repo.module.rev_parse("no-such-ref").is_zero());
        let id = ObjectId::random();
        assert_eq!(repo.module.rev_parse(&id.to_string()), id);
    }

    #[test]
    fn is_valid_git_working_dir_checks_the_file_system() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        assert!(!GitModule::is_valid_git_working_dir(""));
        assert!(!GitModule::is_valid_git_working_dir(p));
        // a sub folder of a repository is not a working dir itself (as in the C# version)
        std::fs::create_dir_all(p.join("repo/.git")).unwrap();
        std::fs::create_dir_all(p.join("repo/sub")).unwrap();
        assert!(GitModule::is_valid_git_working_dir(p.join("repo")));
        assert!(!GitModule::is_valid_git_working_dir(p.join("repo/sub")));
        // worktrees and submodules have a .git file
        std::fs::create_dir_all(p.join("wt")).unwrap();
        std::fs::write(p.join("wt/.git"), "gitdir: ../repo/.git/worktrees/wt\n").unwrap();
        assert!(GitModule::is_valid_git_working_dir(p.join("wt")));
        // bare repository layout
        for d in ["bare/info", "bare/objects", "bare/refs"] {
            std::fs::create_dir_all(p.join(d)).unwrap();
        }
        assert!(GitModule::is_valid_git_working_dir(p.join("bare")));
    }

    #[test]
    fn repository_workflow() {
        let repo = TestRepo::new();
        let m = &repo.module;
        assert!(GitModule::is_valid_git_working_dir(repo.dir.path()));
        assert!(m.head_id().is_zero());
        assert_eq!(m.current_branch().as_deref(), Some("master"));

        // status of a new file, staging, committing
        repo.write("a.txt", "one\n");
        let status = m.get_status(commands::UntrackedFilesMode::All, false).unwrap();
        assert_eq!(status.len(), 1);
        assert!(status[0].is_new && !status[0].is_tracked);
        m.stage_files(&["a.txt"]).unwrap();
        let status = m.get_status(commands::UntrackedFilesMode::All, false).unwrap();
        assert_eq!(status[0].staged, StagedStatus::Index);
        m.unstage_files(&["a.txt"]).unwrap();
        assert_eq!(m.get_status(commands::UntrackedFilesMode::All, false).unwrap()[0].staged, StagedStatus::WorkTree);
        m.stage_all().unwrap();
        m.commit("first commit\n\nbody", Default::default()).unwrap();
        let first = m.head_id();
        assert!(!first.is_zero());
        assert_eq!(m.get_commit_message("HEAD"), "first commit\n\nbody");
        let rev = m.get_revision("HEAD", false).unwrap().unwrap();
        assert_eq!(rev.subject, "first commit");
        assert_eq!(rev.author, "Test User");

        // modify, diff work tree, reset changes
        repo.write("a.txt", "one\ntwo\n");
        let files = m.get_diff_files(Some(ObjectId::INDEX), ObjectId::WORK_TREE).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].is_changed);
        let diff = m.get_file_diff(Some(ObjectId::INDEX), ObjectId::WORK_TREE, "a.txt", None, &DiffOptions::default()).unwrap();
        assert!(diff.contains("+two"));
        m.reset_files(&["a.txt"]).unwrap();
        assert!(m.get_status(commands::UntrackedFilesMode::All, false).unwrap().is_empty());

        // second commit; diff between commits; root commit diff
        let second = repo.commit_file("b/c.txt", "c\n", "second");
        let files = m.get_diff_files(Some(first), second).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "b/c.txt");
        assert!(files[0].is_new);
        let root_files = m.get_diff_files(None, first).unwrap();
        assert_eq!(root_files[0].name, "a.txt");
        assert_eq!(m.get_parents(second), vec![first]);
        assert!(m.is_ancestor(first, second));

        // tree & file content
        let items = m.ls_tree(second, "").unwrap();
        assert_eq!(items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["a.txt", "b"]);
        assert_eq!(m.ls_tree(second, "b").unwrap()[0].name, "c.txt");
        assert_eq!(m.get_file_bytes(second, "b/c.txt").unwrap(), b"c\n");
        assert_eq!(m.ls_tree_recursive(second).unwrap(), ["a.txt", "b/c.txt"]);

        // blame
        let blame = m.blame(second, "a.txt", false, false, false).unwrap();
        assert_eq!(blame.lines.len(), 1);
        assert_eq!(blame.lines[0].commit.object_id, first);

        // branches & refs
        m.run_checked(&commands::branch("feature", first, false)).unwrap();
        let refs = m.get_refs().unwrap();
        assert!(refs.iter().any(|r| r.name == "feature" && r.object_id == first && !r.is_selected));
        assert!(refs.iter().any(|r| r.name == "master" && r.is_selected));
        m.run_checked(&commands::create_tag("v1", first, commands::TagOperation::Lightweight, "", None, false).unwrap()).unwrap();
        let by_commit = m.get_refs_by_commit();
        assert_eq!(by_commit[&first].len(), 2);
        assert_eq!(m.branches_containing(first, true, false), ["feature", "master"]);
        assert_eq!(m.tags_containing(first), ["v1"]);
        m.run_checked(&commands::rename_branch("feature", "feature2")).unwrap();
        let f2 = m.get_refs().unwrap().into_iter().find(|r| r.name == "feature2").unwrap();
        m.run_checked(&commands::delete_branch(&[&f2], true).unwrap()).unwrap();
        assert!(!m.get_refs().unwrap().iter().any(|r| r.name.starts_with("feature")));

        // stash
        repo.write("a.txt", "stashed\n");
        m.run_checked(&commands::stash_save(false, false, Some("my stash"), &[])).unwrap();
        let stashes = m.get_stashes();
        assert_eq!(stashes.len(), 1);
        assert!(stashes[0].subject.contains("my stash"));
        assert_eq!(stashes[0].reflog_selector.as_deref(), Some("refs/stash@{0}"));

        // checkout detached
        m.run_checked(&commands::checkout(&first.to_string(), commands::LocalChangesAction::DontChange)).unwrap();
        assert!(m.is_detached_head());
        assert_eq!(m.selected_branch_display(), "(no branch)");

        // reflog
        let reflog = m.get_reflog("HEAD");
        assert!(reflog.len() >= 3);
        assert_eq!(reflog[0].object_id, first);
        assert!(reflog[0].selector.starts_with("HEAD@{"));
    }

    #[test]
    fn remotes_add_rename_remove() {
        // Ported from GitModuleTests_Remotes AddRemote/RenameRemote/RemoveRemote using a real repo
        let repo = TestRepo::new();
        let m = &repo.module;
        assert!(m.add_remote("", "url").is_err());
        m.add_remote("origin", "https://example.com/repo.git").unwrap();
        assert_eq!(m.get_remote_names(), ["origin"]);
        let remotes = m.get_remotes().unwrap();
        assert_eq!(remotes[0].fetch_url, "https://example.com/repo.git");
        m.set_remote_url("origin", "https://example.com/push.git", true).unwrap();
        assert_eq!(m.get_remotes().unwrap()[0].push_urls, ["https://example.com/push.git"]);
        m.rename_remote("origin", "upstream").unwrap();
        assert_eq!(m.get_remote_names(), ["upstream"]);
        m.remove_remote("upstream").unwrap();
        assert!(m.get_remote_names().is_empty());
    }

    #[test]
    fn fetch_and_pull_args() {
        let repo = TestRepo::new();
        let m = &repo.module;
        assert_eq!(
            m.fetch_args("remote", "remotebranch", "localbranch", Some(false), false, false).to_string(),
            "-c fetch.parallel=0 -c submodule.fetchjobs=0 fetch --progress remote +remotebranch:refs/heads/localbranch --no-tags"
        );
        assert_eq!(m.fetch_args("", "", "", None, true, false).to_string(), "-c fetch.parallel=0 -c submodule.fetchjobs=0 fetch --progress --all --prune");
        assert_eq!(m.pull_args("origin", "main", true, None, true, false).to_string(), "pull --progress --rebase --autostash origin +main");
        assert_eq!(m.pull_args("origin", "", false, Some(true), false, true).to_string(), "pull --progress --no-rebase origin --tags --prune");
    }

    #[test]
    fn partial_staging_with_patch() {
        let repo = TestRepo::new();
        let m = &repo.module;
        repo.commit_file("f.txt", "a\nb\nc\nd\n", "init");
        repo.write("f.txt", "a\nB\nc\nD\n");
        let diff = m.get_file_diff(Some(ObjectId::INDEX), ObjectId::WORK_TREE, "f.txt", None, &DiffOptions::default()).unwrap();
        let lines = crate::patch::parse_diff_lines(&diff);
        let selected: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| l.text == "-b" || l.text == "+B").map(|(i, _)| i).collect();
        let patch = crate::patch::create_partial_patch(&diff, &selected, false).unwrap();
        m.apply_patch_text(&patch, true, false).unwrap();
        let staged = m.get_file_diff(None, ObjectId::INDEX, "f.txt", None, &DiffOptions::default()).unwrap();
        assert!(staged.contains("+B") && !staged.contains("+D"));
        let unstaged = m.get_file_diff(Some(ObjectId::INDEX), ObjectId::WORK_TREE, "f.txt", None, &DiffOptions::default()).unwrap();
        assert!(unstaged.contains("+D") && !unstaged.contains("+B"));

        // unstage it again
        let lines = crate::patch::parse_diff_lines(&staged);
        let selected: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| l.text == "-b" || l.text == "+B").map(|(i, _)| i).collect();
        let patch = crate::patch::create_partial_patch(&staged, &selected, true).unwrap();
        m.apply_patch_text(&patch, true, true).unwrap();
        assert!(m.get_file_diff(None, ObjectId::INDEX, "f.txt", None, &DiffOptions::default()).unwrap().is_empty());
    }

    #[test]
    fn merge_conflict_state() {
        let repo = TestRepo::new();
        let m = &repo.module;
        let base = repo.commit_file("f.txt", "base\n", "base");
        m.run_checked(&commands::branch("other", base, true)).unwrap();
        repo.commit_file("f.txt", "other\n", "other");
        m.run_checked(&commands::checkout("master", Default::default())).unwrap();
        repo.commit_file("f.txt", "master\n", "master");
        let r = m.run(&commands::merge_branch("other", &commands::MergeOptions::default())).unwrap();
        assert!(!r.success());
        assert!(m.state().merging);
        assert_eq!(m.get_unmerged_files(), ["f.txt"]);
        let status = m.get_status(commands::UntrackedFilesMode::All, false).unwrap();
        assert!(status.iter().any(|s| s.is_unmerged));
        m.run_checked(&commands::abort_merge()).unwrap();
        assert!(!m.state().merging);
    }

    #[test]
    fn ignore_files() {
        let repo = TestRepo::new();
        let m = &repo.module;
        repo.write("x.log", "");
        repo.write("y.txt", "");
        assert_eq!(m.files_matching_ignore(&["*.log"]), ["x.log"]);
        m.add_to_ignore(&["*.log"], false).unwrap();
        m.add_to_ignore(&["*.log"], false).unwrap();
        assert_eq!(m.read_work_file(".gitignore"), "*.log\n");
        let status = m.get_status(commands::UntrackedFilesMode::All, false).unwrap();
        assert!(status.iter().all(|s| s.name != "x.log"));
        m.add_to_ignore(&["y.txt"], true).unwrap();
        assert!(std::fs::read_to_string(m.exclude_file_path()).unwrap().contains("y.txt"));
    }

    #[test]
    fn parse_refs_with_tracking() {
        let id = ObjectId::random();
        let tag_target = ObjectId::random();
        let out = format!(
            "{id}\0\0refs/heads/master\0origin\0refs/heads/main\n{id}\0{tag_target}\0refs/tags/v1\0\0\n{id}\0\0refs/remotes/origin/main\0\0\n"
        );
        let refs = parse_refs(&out);
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0].tracking_remote, "origin");
        assert_eq!(refs[0].merge_with, "main");
        assert!(refs[0].is_tracking_remote(Some(&refs[2])));
        assert_eq!(refs[1].object_id, tag_target);
    }

    #[test]
    fn get_tag_message() {
        let repo = TestRepo::new();
        let id = repo.commit_file("a", "a", "c");
        let msg = repo.module.git_dir().join("TAGMSG");
        std::fs::write(&msg, "Tag message\n\nline 2\n").unwrap();
        repo.module
            .run_checked(&commands::create_tag("v1", id, commands::TagOperation::Annotate, "", Some(&msg.display().to_string()), false).unwrap())
            .unwrap();
        assert_eq!(repo.module.get_tag_message("v1").as_deref(), Some("Tag message\n\nline 2"));
        assert_eq!(repo.module.get_refs().unwrap().iter().find(|r| r.is_tag()).unwrap().object_id, id);
    }
}
