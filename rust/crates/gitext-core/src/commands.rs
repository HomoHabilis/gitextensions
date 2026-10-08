//! Port of `GitCommands.Git.Commands`: argument builders for git operations.

use crate::args::{to_posix_path, GitArgs};
use crate::git_ref::{ref_name, GitRef};
use crate::object_id::ObjectId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum LocalChangesAction {
    #[default]
    DontChange,
    Merge,
    Reset,
    Stash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CheckoutNewBranchMode {
    #[default]
    DontCreate,
    Create,
    Reset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ForcePushOptions {
    #[default]
    DoNotForce,
    Force,
    ForceWithLease,
}

impl ForcePushOptions {
    fn arg(self) -> &'static str {
        match self {
            ForcePushOptions::DoNotForce => "",
            ForcePushOptions::Force => "-f",
            ForcePushOptions::ForceWithLease => "--force-with-lease",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CleanMode {
    #[default]
    OnlyNonIgnored,
    OnlyIgnored,
    All,
}

impl CleanMode {
    fn arg(self) -> &'static str {
        match self {
            CleanMode::OnlyNonIgnored => "",
            CleanMode::OnlyIgnored => "-X",
            CleanMode::All => "-x",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    ResetIndex,
    Soft,
    Mixed,
    Keep,
    Merge,
    Hard,
}

impl ResetMode {
    fn arg(self) -> &'static str {
        match self {
            ResetMode::ResetIndex => "",
            ResetMode::Soft => "--soft",
            ResetMode::Mixed => "--mixed",
            ResetMode::Keep => "--keep",
            ResetMode::Merge => "--merge",
            ResetMode::Hard => "--hard",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitBisectOption {
    Good,
    Bad,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum UntrackedFilesMode {
    #[default]
    Default,
    No,
    Normal,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum IgnoreSubmodulesMode {
    #[default]
    None,
    Default,
    Untracked,
    Dirty,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagOperation {
    Lightweight,
    Annotate,
    SignWithDefaultKey,
    SignWithSpecificKey,
}

impl TagOperation {
    pub fn can_provide_message(self) -> bool {
        !matches!(self, TagOperation::Lightweight)
    }
}

/// `-c protocol.file.allow=always`, needed to add local submodules.
pub const ALLOW_FILE_CONFIG: (&str, &str) = ("protocol.file.allow", "always");

fn git_with_configs(configs: &[(&str, &str)], command: &str) -> GitArgs {
    GitArgs::with_config(configs, command)
}

pub fn abort_mailbox() -> GitArgs {
    GitArgs::new("am").arg("--3way").arg("--abort")
}
pub fn resolved_mailbox() -> GitArgs {
    GitArgs::new("am").arg("--3way").arg("--resolved")
}
pub fn skip_mailbox() -> GitArgs {
    GitArgs::new("am").arg("--3way").arg("--skip")
}
pub fn abort_merge() -> GitArgs {
    GitArgs::new("merge").arg("--abort")
}
pub fn continue_merge() -> GitArgs {
    GitArgs::new("merge").arg("--continue")
}
pub fn abort_rebase() -> GitArgs {
    GitArgs::new("rebase").arg("--abort")
}
pub fn continue_rebase() -> GitArgs {
    GitArgs::new("rebase").arg("--continue")
}
pub fn skip_rebase() -> GitArgs {
    GitArgs::new("rebase").arg("--skip")
}
pub fn edit_todo_rebase() -> GitArgs {
    GitArgs::new("rebase").arg("--edit-todo")
}
pub fn start_bisect() -> GitArgs {
    GitArgs::new("bisect").arg("start")
}
pub fn stop_bisect() -> GitArgs {
    GitArgs::new("bisect").arg("reset")
}

pub fn continue_bisect(option: GitBisectOption, ids: &[ObjectId]) -> GitArgs {
    let o = match option {
        GitBisectOption::Good => "good",
        GitBisectOption::Bad => "bad",
        GitBisectOption::Skip => "skip",
    };
    GitArgs::new("bisect").arg(o).args(ids.iter().map(|i| i.to_string()))
}

pub fn add_submodule(remote_path: &str, local_path: &str, branch: Option<&str>, force: bool, allow_file: bool) -> GitArgs {
    let configs: &[(&str, &str)] = if allow_file { &[ALLOW_FILE_CONFIG] } else { &[] };
    let mut a = git_with_configs(configs, "submodule");
    a.add("add");
    a.add_if(force, "-f");
    if let Some(b) = branch.map(str::trim).filter(|b| !b.is_empty()) {
        a.add("-b");
        a.add(b);
    }
    a.add(to_posix_path(remote_path));
    a.add(to_posix_path(local_path));
    a
}

pub fn apply_diff_patch(ignore_whitespace: bool, patch_file: &str) -> GitArgs {
    GitArgs::new("apply").arg_if(ignore_whitespace, "--ignore-whitespace").arg(to_posix_path(patch_file))
}

pub fn apply_mailbox_patch(sign_off: bool, ignore_whitespace: bool, patch_file: Option<&str>) -> GitArgs {
    let mut a = GitArgs::new("am");
    a.add("--3way");
    a.add_if(sign_off, "--signoff");
    a.add_if(ignore_whitespace, "--ignore-whitespace");
    a.add_opt(patch_file.map(to_posix_path));
    a
}

/// `checkout -b name <id>` or `branch name <id>`.
pub fn branch(branch_name: &str, object_id: ObjectId, checkout: bool) -> GitArgs {
    let mut a = GitArgs::new(if checkout { "checkout" } else { "branch" });
    a.add_if(checkout, "-b");
    a.add(branch_name.trim());
    if !object_id.is_zero() {
        a.add(object_id.to_string());
    }
    a
}

pub fn checkout(branch_or_revision: &str, changes: LocalChangesAction) -> GitArgs {
    GitArgs::new("checkout")
        .arg_if(changes == LocalChangesAction::Merge, "--merge")
        .arg_if(changes == LocalChangesAction::Reset, "--force")
        .arg(branch_or_revision)
}

/// Port of `Commands.CheckoutBranch`.
pub fn checkout_branch(
    branch_name: &str,
    remote: bool,
    local_changes: LocalChangesAction,
    new_branch_mode: CheckoutNewBranchMode,
    new_branch_name: Option<&str>,
) -> GitArgs {
    let mut a = GitArgs::new("checkout");
    a.add_if(local_changes == LocalChangesAction::Merge, "--merge");
    a.add_if(local_changes == LocalChangesAction::Reset, "--force");
    if remote && new_branch_mode == CheckoutNewBranchMode::Create {
        a.add("-b");
        a.add(new_branch_name.unwrap_or_default());
    }
    if remote && new_branch_mode == CheckoutNewBranchMode::Reset {
        a.add("-B");
        a.add(new_branch_name.unwrap_or_default());
    }
    a.add_if(remote && new_branch_mode == CheckoutNewBranchMode::Create, "--track");
    a.add(branch_name);
    a
}

pub fn cherry_pick(commit_id: ObjectId, commit: bool, extra: &[&str]) -> GitArgs {
    GitArgs::new("cherry-pick").arg_if(!commit, "--no-commit").args(extra.iter().copied()).arg(commit_id.to_string())
}

/// Port of `Commands.Clean`. `paths`/`excludes` are already split arguments.
pub fn clean(mode: CleanMode, dry_run: bool, directories: bool, paths: &[&str], excludes: &[&str]) -> GitArgs {
    GitArgs::new("clean")
        .arg(mode.arg())
        .arg_if(directories, "-d")
        .arg(if dry_run { "--dry-run" } else { "-f" })
        .args(paths.iter().copied())
        .args(excludes.iter().copied())
}

pub fn clean_submodules(mode: CleanMode, dry_run: bool, directories: bool, paths: &[&str]) -> GitArgs {
    GitArgs::new("submodule")
        .args(["foreach", "--recursive", "git", "clean"])
        .arg(mode.arg())
        .arg_if(directories, "-d")
        .arg(if dry_run { "--dry-run" } else { "-f" })
        .args(paths.iter().copied())
}

/// Port of `Commands.Clone`. `branch`: `None` = `--no-checkout`, `Some("")` = remote HEAD.
pub fn clone(
    from_path: &str,
    to_path: &str,
    central: bool,
    init_submodules: bool,
    branch: Option<&str>,
    depth: Option<u32>,
    is_single_branch: Option<bool>,
) -> GitArgs {
    let mut from = from_path.trim().to_string();
    if std::path::Path::new(&from).exists() {
        from = to_posix_path(&from);
    }
    let mut a = GitArgs::new("clone");
    a.add("-v");
    a.add_if(central, "--bare");
    a.add_if(init_submodules, "--recurse-submodules");
    if let Some(d) = depth {
        a.add("--depth");
        a.add(d.to_string());
    }
    a.add_if(is_single_branch == Some(true), "--single-branch");
    a.add_if(is_single_branch == Some(false), "--no-single-branch");
    a.add("--progress");
    match branch {
        None => {
            a.add("--no-checkout");
        }
        Some(b) if !b.is_empty() => {
            a.add("--branch");
            a.add(b);
        }
        _ => {}
    }
    a.add(from);
    a.add(to_posix_path(to_path.trim()));
    a
}

/// Options of `git commit`.
#[derive(Debug, Clone, Default)]
pub struct CommitOptions {
    pub amend: bool,
    pub sign_off: bool,
    pub author: String,
    pub commit_message_file: Option<String>,
    pub no_verify: bool,
    pub gpg_sign: Option<bool>,
    pub gpg_key_id: String,
    pub allow_empty: bool,
    pub reset_author: bool,
}

pub fn commit(o: &CommitOptions) -> GitArgs {
    let mut a = GitArgs::new("commit");
    a.add_if(o.amend, "--amend");
    a.add_if(o.no_verify, "--no-verify");
    a.add_if(o.sign_off, "--signoff");
    let author = o.author.trim().trim_matches('"');
    a.add_if(!author.is_empty(), format!("--author={author}"));
    a.add_if(o.gpg_sign == Some(false), "--no-gpg-sign");
    a.add_if(o.gpg_sign == Some(true) && o.gpg_key_id.trim().is_empty(), "--gpg-sign");
    a.add_if(o.gpg_sign == Some(true) && !o.gpg_key_id.trim().is_empty(), format!("--gpg-sign={}", o.gpg_key_id));
    if let Some(f) = &o.commit_message_file {
        a.add("-F");
        a.add(to_posix_path(f));
    }
    a.add_if(o.allow_empty, "--allow-empty");
    a.add_if(o.reset_author && o.amend, "--reset-author");
    a
}

pub fn create_orphan(new_branch_name: &str, start_point: ObjectId) -> GitArgs {
    let mut a = GitArgs::new("checkout").arg("--orphan").arg(new_branch_name);
    if !start_point.is_zero() {
        a.add(start_point.to_string());
    }
    a
}

/// Port of `Commands.CreateTag`.
pub fn create_tag(
    tag_name: &str,
    object_id: ObjectId,
    operation: TagOperation,
    sign_key_id: &str,
    message_file: Option<&str>,
    force: bool,
) -> Result<GitArgs, String> {
    if object_id.is_artificial() {
        return Err("A valid, non-artificial revision is required for tagging.".into());
    }
    if tag_name.trim().is_empty() {
        return Err("TagName is required.".into());
    }
    if operation.can_provide_message() && message_file.is_none_or(|f| f.trim().is_empty()) {
        return Err("TagMessageFileName is required.".into());
    }
    if operation == TagOperation::SignWithSpecificKey && sign_key_id.trim().is_empty() {
        return Err("SignKeyId is required.".into());
    }
    let mut a = GitArgs::new("tag");
    a.add_if(force, "-f");
    match operation {
        TagOperation::Lightweight => {}
        TagOperation::Annotate => {
            a.add("-a");
        }
        TagOperation::SignWithDefaultKey => {
            a.add("-s");
        }
        TagOperation::SignWithSpecificKey => {
            a.add("-u");
            a.add(sign_key_id);
        }
    }
    if operation.can_provide_message() {
        a.add("-F");
        a.add(to_posix_path(message_file.unwrap()));
    }
    a.add(tag_name.trim());
    a.add("--");
    a.add(object_id.to_string());
    Ok(a)
}

/// Port of `Commands.DeleteBranch`.
pub fn delete_branch(branches: &[&GitRef], force: bool) -> Result<GitArgs, String> {
    if branches.is_empty() {
        return Err("At least one branch is required.".into());
    }
    let has_remote = branches.iter().any(|b| b.is_remote());
    let has_non_remote = branches.iter().any(|b| !b.is_remote());
    Ok(GitArgs::new("branch")
        .arg("--delete")
        .arg_if(force, "--force")
        .arg_if(has_remote && has_non_remote, "--all")
        .arg_if(has_remote && !has_non_remote, "--remotes")
        .args(branches.iter().map(|b| b.name.clone())))
}

pub fn delete_remote_branches(remote: &str, branch_local_names: &[&str]) -> GitArgs {
    GitArgs::new("push").arg(remote).args(branch_local_names.iter().map(|b| format!(":refs/heads/{b}")))
}

pub fn delete_tag(tag_name: &str) -> GitArgs {
    GitArgs::new("tag").arg("-d").arg(tag_name)
}

/// Port of `Commands.GetAllChangedFiles` (`git status --porcelain=2 -z`).
pub fn get_all_changed_files(
    exclude_ignored_files: bool,
    untracked_files: UntrackedFilesMode,
    ignore_submodules: IgnoreSubmodulesMode,
    no_locks: bool,
) -> GitArgs {
    let mut a = GitArgs::with_config(&[("diff.ignoresubmodules", "none")], "status");
    a.add("--porcelain=2");
    a.add("-z");
    a.add(match untracked_files {
        UntrackedFilesMode::Default => "--untracked-files",
        UntrackedFilesMode::No => "--untracked-files=no",
        UntrackedFilesMode::Normal => "--untracked-files=normal",
        UntrackedFilesMode::All => "--untracked-files=all",
    });
    a.add_if(!exclude_ignored_files, "--ignored");
    a.add(match ignore_submodules {
        IgnoreSubmodulesMode::None => "",
        IgnoreSubmodulesMode::Default => "--ignore-submodules",
        IgnoreSubmodulesMode::Untracked => "--ignore-submodules=untracked",
        IgnoreSubmodulesMode::Dirty => "--ignore-submodules=dirty",
        IgnoreSubmodulesMode::All => "--ignore-submodules=all",
    });
    a.no_locks(no_locks)
}

/// Common `-c` options for diffs (stable, machine parsable output).
pub const DIFF_CONFIGS: [(&str, &str); 6] = [
    ("color.ui", "never"),
    ("diff.submodule", "short"),
    ("diff.noprefix", "false"),
    ("diff.mnemonicprefix", "false"),
    ("diff.ignoresubmodules", "none"),
    ("core.safecrlf", "false"),
];

/// Port of `Commands.GetCurrentChanges`: diff of work tree or index for a file.
pub fn get_current_changes(file_name: &str, old_file_name: Option<&str>, staged: bool, extra_diff_arguments: &[&str], no_locks: bool) -> GitArgs {
    let mut a = GitArgs::with_config(&DIFF_CONFIGS, "diff");
    a.add("--no-ext-diff");
    a.add("--find-renames");
    a.add("--find-copies");
    a.add_all(extra_diff_arguments.iter().copied());
    a.add_if(staged, "--cached");
    a.add("--");
    a.add(to_posix_path(file_name));
    if staged {
        a.add_opt(old_file_name.map(to_posix_path));
    }
    a.no_locks(no_locks)
}

/// Which refs to list with `for-each-ref`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RefsFilter {
    pub heads: bool,
    pub remotes: bool,
    pub tags: bool,
}

impl RefsFilter {
    pub const NO_FILTER: RefsFilter = RefsFilter { heads: false, remotes: false, tags: false };
    pub const ALL: RefsFilter = RefsFilter { heads: true, remotes: true, tags: true };
    pub const HEADS: RefsFilter = RefsFilter { heads: true, remotes: false, tags: false };
    pub const REMOTES: RefsFilter = RefsFilter { heads: false, remotes: true, tags: false };
    pub const TAGS: RefsFilter = RefsFilter { heads: false, remotes: false, tags: true };

    fn is_no_filter(&self) -> bool {
        !self.heads && !self.remotes && !self.tags
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum GitRefsSortBy {
    #[default]
    Default,
    Authordate,
    Committerdate,
    Creatordate,
    Objectsize,
    Refname,
    VersionRefname,
}

impl GitRefsSortBy {
    pub const ALL: [GitRefsSortBy; 7] = [
        GitRefsSortBy::Default,
        GitRefsSortBy::Authordate,
        GitRefsSortBy::Committerdate,
        GitRefsSortBy::Creatordate,
        GitRefsSortBy::Objectsize,
        GitRefsSortBy::Refname,
        GitRefsSortBy::VersionRefname,
    ];

    fn key(self) -> &'static str {
        match self {
            GitRefsSortBy::Default => "",
            GitRefsSortBy::Authordate => "authordate",
            GitRefsSortBy::Committerdate => "committerdate",
            GitRefsSortBy::Creatordate => "creatordate",
            GitRefsSortBy::Objectsize => "objectsize",
            GitRefsSortBy::Refname => "refname",
            GitRefsSortBy::VersionRefname => "version:refname",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum GitRefsSortOrder {
    Ascending,
    #[default]
    Descending,
}

/// Port of `Commands.GetRefs`.
pub fn get_refs(filter: RefsFilter, no_locks: bool, sort_by: GitRefsSortBy, sort_order: GitRefsSortOrder, count: usize) -> GitArgs {
    let has_tags = filter.is_no_filter() || filter.tags;
    let mut a = GitArgs::new("for-each-ref");
    if sort_by != GitRefsSortBy::Default {
        let order = if sort_order == GitRefsSortOrder::Ascending { "" } else { "-" };
        if has_tags {
            let deref_key = if sort_by == GitRefsSortBy::VersionRefname { "refname" } else { sort_by.key() };
            a.add(format!("--sort={order}*{deref_key}"));
        }
        a.add(format!("--sort={order}{}", sort_by.key()));
    }
    a.add(if has_tags {
        "--format=%(if)%(authordate)%(then)%(objectname) %(refname)%(else)%(*objectname) %(*refname)%(end)"
    } else {
        "--format=%(objectname) %(refname)"
    });
    a.add_if(count > 0, format!("--count={count}"));
    a.add_if(filter.heads, "refs/heads/");
    a.add_if(filter.remotes, "refs/remotes/");
    a.add_if(filter.tags, "refs/tags/");
    a.no_locks(no_locks)
}

/// Options of `git merge`.
#[derive(Debug, Clone, Default)]
pub struct MergeOptions {
    pub allow_fast_forward: bool,
    pub squash: bool,
    pub no_commit: bool,
    pub strategy: String,
    pub allow_unrelated_histories: bool,
    pub merge_commit_file_path: Option<String>,
    pub log: Option<i32>,
}

pub fn merge_branch(branch: &str, o: &MergeOptions) -> GitArgs {
    let mut a = GitArgs::new("merge");
    a.add(if o.allow_fast_forward { "--ff" } else { "--no-ff" });
    a.add_if(!o.strategy.is_empty(), format!("--strategy={}", o.strategy));
    a.add_if(o.squash, "--squash");
    a.add_if(o.no_commit, "--no-commit");
    a.add_if(o.allow_unrelated_histories, "--allow-unrelated-histories");
    if let Some(f) = o.merge_commit_file_path.as_deref().filter(|f| !f.trim().is_empty()) {
        a.add("-F");
        a.add(to_posix_path(f));
    }
    if let Some(log) = o.log.filter(|l| *l > 0) {
        a.add(format!("--log={log}"));
    }
    a.add("--no-edit");
    a.add(branch);
    a
}

pub fn merged_branches(include_remote: bool, full_refname: bool, commit: Option<&str>) -> GitArgs {
    let mut a = GitArgs::new("branch");
    a.add_if(full_refname, "--format=%(refname)");
    a.add_if(include_remote, "-a");
    a.add("--merged");
    a.add_opt(commit.map(str::trim).filter(|c| !c.is_empty()));
    a
}

fn recurse_submodules(recursive: u8) -> &'static str {
    match recursive {
        1 => "--recurse-submodules=check",
        2 => "--recurse-submodules=on-demand",
        _ => "",
    }
}

/// Port of `Commands.Push`.
pub fn push(remote: &str, from_branch: &str, to_branch: Option<&str>, force: ForcePushOptions, track: bool, recursive_submodules: u8) -> GitArgs {
    let to_branch = to_branch.map(ref_name::get_full_branch_name).filter(|b| !b.is_empty());
    let from_branch = if from_branch.is_empty() && to_branch.is_some() { "HEAD" } else { from_branch };
    let mut a = GitArgs::new("push");
    a.add(force.arg());
    a.add_if(track, "-u");
    a.add(recurse_submodules(recursive_submodules));
    a.add("--progress");
    a.add(to_posix_path(remote).trim());
    match to_branch {
        None => a.add(from_branch),
        Some(to) => a.add(format!("{from_branch}:{to}")),
    };
    a
}

pub fn push_all(remote: &str, force: ForcePushOptions, track: bool, recursive_submodules: u8) -> GitArgs {
    GitArgs::new("push")
        .arg(force.arg())
        .arg_if(track, "-u")
        .arg(recurse_submodules(recursive_submodules))
        .arg("--progress")
        .arg("--all")
        .arg(to_posix_path(remote).trim())
}

/// A local > remote branch mapping for [`push_multiple`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitPushAction {
    pub local: String,
    pub remote: String,
    pub force: bool,
    pub delete: bool,
}

pub fn push_multiple(remote: &str, actions: &[GitPushAction]) -> GitArgs {
    let mut a = GitArgs::new("push").arg("--progress").arg(to_posix_path(remote));
    for act in actions {
        if act.delete {
            a.add(format!(":{}", ref_name::get_full_branch_name(&act.remote)));
        } else {
            a.add(format!(
                "{}{}:{}",
                if act.force { "+" } else { "" },
                ref_name::get_full_branch_name(&act.local),
                ref_name::get_full_branch_name(&act.remote)
            ));
        }
    }
    a
}

pub fn push_tag(path: &str, tag: &str, all: bool, force: ForcePushOptions) -> GitArgs {
    if !all && tag.trim().is_empty() {
        return GitArgs::empty();
    }
    let mut a = GitArgs::new("push").arg(force.arg()).arg("--progress").arg(to_posix_path(path).trim());
    if all {
        a.add("--tags");
    } else {
        a.add("tag");
        a.add(tag.replace(' ', ""));
    }
    a
}

/// Port of `Commands.RebaseOptions`.
#[derive(Debug, Clone, Default)]
pub struct RebaseOptions {
    pub branch_name: String,
    pub interactive: bool,
    pub preserve_merges: bool,
    pub auto_squash: bool,
    pub auto_stash: bool,
    pub ignore_date: bool,
    pub committer_date_is_author_date: bool,
    pub support_rebase_merges: bool,
    pub update_refs: Option<bool>,
    pub from: Option<String>,
    pub on_to: Option<String>,
}

pub fn rebase(o: &RebaseOptions) -> Result<GitArgs, String> {
    if o.from.is_none() != o.on_to.is_none() {
        return Err("For arguments \"From\" and \"OnTo\", either both must have values, or neither may.".into());
    }
    let mut a = GitArgs::with_config(&[("rebase.autosquash", "false")], "rebase");
    if o.ignore_date {
        a.add("--ignore-date");
    } else if o.committer_date_is_author_date {
        a.add("--committer-date-is-author-date");
    } else {
        if o.interactive {
            a.add("-i");
            a.add(if o.auto_squash { "--autosquash" } else { "--no-autosquash" });
        }
        if o.preserve_merges {
            a.add(if o.support_rebase_merges { "--rebase-merges" } else { "--preserve-merges" });
        }
    }
    if let Some(u) = o.update_refs {
        a.add(if u { "--update-refs" } else { "--no-update-refs" });
    }
    a.add_if(o.auto_stash, "--autostash");
    if let Some(onto) = &o.on_to {
        a.add("--onto");
        a.add(onto.as_str());
    }
    a.add_opt(o.from.clone());
    a.add(o.branch_name.as_str());
    Ok(a)
}

pub fn remove(force: bool, is_recursive: bool, files: &[&str]) -> GitArgs {
    GitArgs::new("rm")
        .arg_if(force, "--force")
        .arg_if(is_recursive, "-r")
        .arg_if(files.is_empty(), ".")
        .args(files.iter().copied())
}

pub fn rename_branch(name: &str, new_name: &str) -> GitArgs {
    GitArgs::new("branch").arg("-m").arg(name).arg(new_name)
}

pub fn reset(mode: ResetMode, commit: Option<&str>, file: Option<&str>, quiet: bool) -> Result<GitArgs, String> {
    if mode == ResetMode::ResetIndex && commit.is_none_or(|c| c.trim().is_empty()) {
        return Err("reset to index requires a tree-ish parameter".into());
    }
    Ok(GitArgs::new("reset")
        .arg(mode.arg())
        .arg_if(quiet, "--quiet")
        .arg(commit.unwrap_or_default())
        .arg("--")
        .arg(file.map(to_posix_path).unwrap_or_default()))
}

pub fn revert(commit_id: ObjectId, auto_commit: bool, parent_index: u32) -> GitArgs {
    let mut a = GitArgs::new("revert").arg_if(!auto_commit, "--no-commit");
    if parent_index > 0 {
        a.add("-m");
        a.add(parent_index.to_string());
    }
    a.arg(commit_id.to_string())
}

/// Port of `Commands.StashSave`.
pub fn stash_save(untracked: bool, keep_index: bool, message: Option<&str>, selected_files: &[&str]) -> GitArgs {
    let files: Vec<&str> = selected_files.iter().copied().filter(|f| !f.trim().is_empty()).collect();
    let is_partial = !selected_files.is_empty();
    let message = message.filter(|m| !m.trim().is_empty());
    let mut a = GitArgs::new("stash");
    a.add(if is_partial { "push" } else { "save" });
    a.add_if(untracked, "-u");
    a.add_if(keep_index, "--keep-index");
    a.add_if(is_partial && message.is_some(), "-m");
    a.add_opt(message);
    a.add_if(is_partial, "--");
    if is_partial {
        a.add_all(files);
    }
    a
}

pub fn submodule_sync(name: Option<&str>) -> GitArgs {
    GitArgs::new("submodule").arg("sync").arg(name.map(str::trim).unwrap_or_default())
}

pub fn submodule_update(names: &[&str], allow_file: bool) -> GitArgs {
    let configs: &[(&str, &str)] = if allow_file { &[ALLOW_FILE_CONFIG] } else { &[] };
    git_with_configs(configs, "submodule")
        .arg("update")
        .arg("--init")
        .arg("--recursive")
        .args(names.iter().map(|n| n.trim()))
}

pub fn update_ref(git_ref: &str, target: ObjectId) -> GitArgs {
    GitArgs::new("update-ref").arg(git_ref).arg(target.to_string())
}

#[cfg(test)]
mod tests {
    //! Ported from GitCommands.Tests/Git/CommandsTests.cs. Expected values are adapted from
    //! the quoted Windows command line to the argv rendering of [`GitArgs`].
    use super::*;

    fn s(a: GitArgs) -> String {
        a.to_string()
    }

    #[test]
    fn abort_merge_cmd() {
        assert_eq!(s(abort_merge()), "merge --abort");
        assert_eq!(s(continue_merge()), "merge --continue");
    }

    #[test]
    fn add_submodule_cmd() {
        for (config, allow) in [("", false), ("-c protocol.file.allow=always ", true)] {
            assert_eq!(s(add_submodule("remotepath", "localpath", Some("branch"), false, allow)), format!("{config}submodule add -b branch remotepath localpath"));
            assert_eq!(s(add_submodule("remotepath", "localpath", None, false, allow)), format!("{config}submodule add remotepath localpath"));
            assert_eq!(s(add_submodule("remotepath", "localpath", Some("branch"), true, allow)), format!("{config}submodule add -f -b branch remotepath localpath"));
            assert_eq!(s(add_submodule("remote\\path", "local\\path", Some("branch"), true, allow)), format!("{config}submodule add -f -b branch remote/path local/path"));
        }
    }

    #[test]
    fn apply_patch_cmds() {
        assert_eq!(s(apply_diff_patch(false, "hello\\world.patch")), "apply hello/world.patch");
        assert_eq!(s(apply_diff_patch(true, "hello\\world.patch")), "apply --ignore-whitespace hello/world.patch");
        for (sign_off, ws, file, expected) in [
            (false, false, Some("hello\\world.patch"), "am --3way hello/world.patch"),
            (false, true, Some("hello\\world.patch"), "am --3way --ignore-whitespace hello/world.patch"),
            (true, false, Some("hello\\world.patch"), "am --3way --signoff hello/world.patch"),
            (true, true, Some("hello\\world.patch"), "am --3way --signoff --ignore-whitespace hello/world.patch"),
            (true, true, None, "am --3way --signoff --ignore-whitespace"),
        ] {
            assert_eq!(s(apply_mailbox_patch(sign_off, ws, file)), expected);
        }
    }

    #[test]
    fn branch_cmd() {
        let id = ObjectId::random();
        assert_eq!(s(branch("branch", id, true)), format!("checkout -b branch {id}"));
        assert_eq!(s(branch("branch", id, false)), format!("branch branch {id}"));
        assert_eq!(s(branch("branch", ObjectId::ZERO, true)), "checkout -b branch");
    }

    #[test]
    fn checkout_cmd() {
        assert_eq!(s(checkout("branch", LocalChangesAction::DontChange)), "checkout branch");
        assert_eq!(s(checkout("branch", LocalChangesAction::Merge)), "checkout --merge branch");
        assert_eq!(s(checkout("branch", LocalChangesAction::Reset)), "checkout --force branch");
        assert_eq!(s(checkout("branch", LocalChangesAction::Stash)), "checkout branch");
    }

    #[test]
    fn checkout_branch_cmd() {
        // Ported from GitCheckoutBranchCmdTest.cs
        assert_eq!(s(checkout_branch("branch", false, LocalChangesAction::DontChange, CheckoutNewBranchMode::DontCreate, None)), "checkout branch");
        assert_eq!(s(checkout_branch("branch", false, LocalChangesAction::Merge, CheckoutNewBranchMode::DontCreate, None)), "checkout --merge branch");
        assert_eq!(s(checkout_branch("branch", false, LocalChangesAction::Reset, CheckoutNewBranchMode::DontCreate, None)), "checkout --force branch");
        assert_eq!(
            s(checkout_branch("origin/branch", true, LocalChangesAction::DontChange, CheckoutNewBranchMode::Create, Some("newBranch"))),
            "checkout -b newBranch --track origin/branch"
        );
        assert_eq!(
            s(checkout_branch("origin/branch", true, LocalChangesAction::DontChange, CheckoutNewBranchMode::Reset, Some("newBranch"))),
            "checkout -B newBranch origin/branch"
        );
    }

    #[test]
    fn clean_cmd() {
        for (mode, dry, dirs, paths, excludes, expected) in [
            (CleanMode::OnlyNonIgnored, true, false, &[][..], &[][..], "clean --dry-run"),
            (CleanMode::OnlyNonIgnored, false, false, &[], &[], "clean -f"),
            (CleanMode::OnlyNonIgnored, false, true, &[], &[], "clean -d -f"),
            (CleanMode::OnlyNonIgnored, false, false, &["path1"], &[], "clean -f path1"),
            (CleanMode::OnlyNonIgnored, false, false, &["path1"], &["--exclude=excludes"], "clean -f path1 --exclude=excludes"),
            (CleanMode::OnlyNonIgnored, false, false, &["path1", "path2"], &[], "clean -f path1 path2"),
            (
                CleanMode::OnlyNonIgnored,
                false,
                false,
                &["path1", "path2"],
                &["--exclude=exclude1", "--exclude=exclude2"],
                "clean -f path1 path2 --exclude=exclude1 --exclude=exclude2",
            ),
            (CleanMode::OnlyIgnored, false, false, &[], &[], "clean -X -f"),
            (CleanMode::All, false, false, &[], &[], "clean -x -f"),
        ] {
            assert_eq!(s(clean(mode, dry, dirs, paths, excludes)), expected);
        }
        assert_eq!(s(clean_submodules(CleanMode::All, false, true, &["paths"])), "submodule foreach --recursive git clean -x -d -f paths");
        assert_eq!(s(clean_submodules(CleanMode::OnlyNonIgnored, true, false, &[])), "submodule foreach --recursive git clean --dry-run");
    }

    #[test]
    fn clone_cmd() {
        assert_eq!(s(clone("from", "to", false, false, Some(""), None, None)), "clone -v --progress from to");
        assert_eq!(s(clone("from/path", "to/path", false, false, Some(""), None, None)), "clone -v --progress from/path to/path");
        assert_eq!(s(clone("from", "to", true, false, Some(""), None, None)), "clone -v --bare --progress from to");
        assert_eq!(s(clone("from", "to", false, true, Some(""), None, None)), "clone -v --recurse-submodules --progress from to");
        assert_eq!(s(clone("from", "to", false, false, Some(""), Some(2), None)), "clone -v --depth 2 --progress from to");
        assert_eq!(s(clone("from", "to", false, false, Some(""), None, Some(true))), "clone -v --single-branch --progress from to");
        assert_eq!(s(clone("from", "to", false, false, Some(""), None, Some(false))), "clone -v --no-single-branch --progress from to");
        assert_eq!(s(clone("from", "to", false, false, Some("branch"), None, None)), "clone -v --progress --branch branch from to");
        assert_eq!(s(clone("from", "to", false, false, None, None, None)), "clone -v --progress --no-checkout from to");
    }

    fn commit_with(amend: bool, sign_off: bool, author: &str, file: bool, no_verify: bool, gpg: Option<bool>, key: &str) -> String {
        s(commit(&CommitOptions {
            amend,
            sign_off,
            author: author.into(),
            commit_message_file: file.then(|| "COMMITMESSAGE".to_string()),
            no_verify,
            gpg_sign: gpg,
            gpg_key_id: key.into(),
            ..Default::default()
        }))
    }

    #[test]
    fn commit_cmd() {
        assert_eq!(commit_with(false, false, "", true, false, None, ""), "commit -F COMMITMESSAGE");
        assert_eq!(commit_with(true, false, "", true, false, None, ""), "commit --amend -F COMMITMESSAGE");
        assert_eq!(commit_with(false, true, "", true, false, None, ""), "commit --signoff -F COMMITMESSAGE");
        assert_eq!(commit_with(false, false, "foo", true, false, None, ""), "commit --author=foo -F COMMITMESSAGE");
        assert_eq!(commit_with(false, false, "", false, false, None, ""), "commit");
        assert_eq!(commit_with(false, false, "", true, true, None, ""), "commit --no-verify -F COMMITMESSAGE");
        assert_eq!(commit_with(false, false, "", true, false, Some(false), ""), "commit --no-gpg-sign -F COMMITMESSAGE");
        assert_eq!(commit_with(false, false, "", true, false, Some(true), ""), "commit --gpg-sign -F COMMITMESSAGE");
        assert_eq!(commit_with(false, false, "", true, false, Some(true), "key"), "commit --gpg-sign=key -F COMMITMESSAGE");
        assert_eq!(commit_with(false, false, "", false, false, Some(true), "      "), "commit --gpg-sign");
        assert_eq!(
            commit_with(true, true, "", true, true, Some(true), "12345678"),
            "commit --amend --no-verify --signoff --gpg-sign=12345678 -F COMMITMESSAGE"
        );
    }

    #[test]
    fn commit_cmd_should_trim_author() {
        for input in ["  \"author <author@mail.com>\"  ", "\"author <author@mail.com>\"", "author <author@mail.com>"] {
            let a = commit(&CommitOptions { author: input.into(), commit_message_file: Some("COMMITMESSAGE".into()), ..Default::default() });
            assert_eq!(a.as_slice(), ["commit", "--author=author <author@mail.com>", "-F", "COMMITMESSAGE"]);
        }
    }

    #[test]
    fn continue_bisect_cmd() {
        let (a, b) = (ObjectId::random(), ObjectId::random());
        assert_eq!(s(continue_bisect(GitBisectOption::Good, &[])), "bisect good");
        assert_eq!(s(continue_bisect(GitBisectOption::Bad, &[])), "bisect bad");
        assert_eq!(s(continue_bisect(GitBisectOption::Skip, &[])), "bisect skip");
        assert_eq!(s(continue_bisect(GitBisectOption::Good, &[a, b])), format!("bisect good {a} {b}"));
    }

    #[test]
    fn get_all_changed_files_cmd() {
        use IgnoreSubmodulesMode as I;
        use UntrackedFilesMode as U;
        let p = "-c diff.ignoresubmodules=none status --porcelain=2 -z";
        assert_eq!(s(get_all_changed_files(true, U::Default, I::Default, false)), format!("{p} --untracked-files --ignore-submodules"));
        assert_eq!(s(get_all_changed_files(false, U::Default, I::Default, false)), format!("{p} --untracked-files --ignored --ignore-submodules"));
        assert_eq!(s(get_all_changed_files(true, U::No, I::Default, false)), format!("{p} --untracked-files=no --ignore-submodules"));
        assert_eq!(s(get_all_changed_files(true, U::Normal, I::Default, false)), format!("{p} --untracked-files=normal --ignore-submodules"));
        assert_eq!(s(get_all_changed_files(true, U::All, I::Default, false)), format!("{p} --untracked-files=all --ignore-submodules"));
        assert_eq!(s(get_all_changed_files(true, U::Default, I::None, false)), format!("{p} --untracked-files"));
        assert_eq!(s(get_all_changed_files(true, U::Default, I::Untracked, false)), format!("{p} --untracked-files --ignore-submodules=untracked"));
        assert_eq!(s(get_all_changed_files(true, U::Default, I::Dirty, false)), format!("{p} --untracked-files --ignore-submodules=dirty"));
        assert_eq!(s(get_all_changed_files(true, U::Default, I::All, false)), format!("{p} --untracked-files --ignore-submodules=all"));
        assert_eq!(
            s(get_all_changed_files(true, U::Default, I::Default, true)),
            format!("--no-optional-locks {p} --untracked-files --ignore-submodules")
        );
    }

    #[test]
    fn get_current_changes_cmd() {
        let p = "-c color.ui=never -c diff.submodule=short -c diff.noprefix=false -c diff.mnemonicprefix=false -c diff.ignoresubmodules=none -c core.safecrlf=false diff --no-ext-diff --find-renames --find-copies";
        assert_eq!(s(get_current_changes("new", Some("old"), true, &["extra"], false)), format!("{p} extra --cached -- new old"));
        assert_eq!(s(get_current_changes("new", Some("old"), false, &["extra"], false)), format!("{p} extra -- new"));
        assert_eq!(s(get_current_changes("new", Some("old"), true, &["extra"], true)), format!("--no-optional-locks {p} extra --cached -- new old"));
    }

    #[test]
    fn get_refs_cmd() {
        let format = " --format=\"%(if)%(authordate)%(then)%(objectname) %(refname)%(else)%(*objectname) %(*refname)%(end)\"";
        let format_no_tag = " \"--format=%(objectname) %(refname)\"";
        // In argv rendering, the whole format argument is quoted.
        let format = format.replace(" --format=\"", " \"--format=");
        for sort_by in GitRefsSortBy::ALL {
            for sort_order in [GitRefsSortOrder::Ascending, GitRefsSortOrder::Descending] {
                let (sc, scr) = if sort_by == GitRefsSortBy::Default {
                    (String::new(), String::new())
                } else {
                    let key = sort_by.key();
                    let deref = if sort_by == GitRefsSortBy::VersionRefname { "refname" } else { key };
                    let order = if sort_order == GitRefsSortOrder::Ascending { "" } else { "-" };
                    (format!(" --sort={order}{key}"), format!(" --sort={order}*{deref}"))
                };
                let t = |f, nl, c, e: String| assert_eq!(s(get_refs(f, nl, sort_by, sort_order, c)), e);
                t(RefsFilter::ALL, false, 0, format!("for-each-ref{scr}{sc}{format} refs/heads/ refs/remotes/ refs/tags/"));
                t(RefsFilter::TAGS, false, 0, format!("for-each-ref{scr}{sc}{format} refs/tags/"));
                t(RefsFilter::HEADS, false, 0, format!("for-each-ref{sc}{format_no_tag} refs/heads/"));
                t(RefsFilter::HEADS, false, 100, format!("for-each-ref{sc}{format_no_tag} --count=100 refs/heads/"));
                t(RefsFilter::HEADS, true, 0, format!("--no-optional-locks for-each-ref{sc}{format_no_tag} refs/heads/"));
                t(RefsFilter::REMOTES, false, 0, format!("for-each-ref{sc}{format_no_tag} refs/remotes/"));
                t(RefsFilter::NO_FILTER, true, 0, format!("--no-optional-locks for-each-ref{scr}{sc}{format}"));
            }
        }
    }

    #[test]
    fn merge_branch_cmd() {
        let m = |ff, squash, no_commit, unrelated, file: Option<&str>, log: Option<i32>| {
            s(merge_branch(
                "branch",
                &MergeOptions {
                    allow_fast_forward: ff,
                    squash,
                    no_commit,
                    strategy: String::new(),
                    allow_unrelated_histories: unrelated,
                    merge_commit_file_path: file.map(str::to_string),
                    log,
                },
            ))
        };
        assert_eq!(m(false, false, false, false, None, None), "merge --no-ff --no-edit branch");
        assert_eq!(m(true, true, true, true, None, None), "merge --ff --squash --no-commit --allow-unrelated-histories --no-edit branch");
        assert_eq!(m(true, false, false, false, None, None), "merge --ff --no-edit branch");
        for blank in ["", "   ", "\t", "\n"] {
            assert_eq!(m(false, true, false, false, Some(blank), None), "merge --no-ff --squash --no-edit branch");
        }
        assert_eq!(m(false, true, false, false, Some("foo"), None), "merge --no-ff --squash -F foo --no-edit branch");
        assert_eq!(m(false, true, false, false, Some("D:\\myrepo\\.git\\file"), None), "merge --no-ff --squash -F D:/myrepo/.git/file --no-edit branch");
        assert_eq!(m(true, true, false, false, None, Some(-1)), "merge --ff --squash --no-edit branch");
        assert_eq!(m(true, true, false, false, None, Some(0)), "merge --ff --squash --no-edit branch");
        assert_eq!(m(true, true, false, false, None, Some(5)), "merge --ff --squash --log=5 --no-edit branch");
    }

    #[test]
    fn merged_branches_cmd() {
        for include_remote in [true, false] {
            for full in [true, false] {
                for commit in [None, Some(""), Some("HEAD"), Some("1234567890")] {
                    let format_arg = if full { " --format=%(refname)" } else { "" };
                    let remote_arg = if include_remote { " -a" } else { "" };
                    let commit_arg = commit.filter(|c| !c.is_empty()).map(|c| format!(" {c}")).unwrap_or_default();
                    assert_eq!(s(merged_branches(include_remote, full, commit)), format!("branch{format_arg}{remote_arg} --merged{commit_arg}"));
                }
            }
        }
    }

    #[test]
    fn push_cmds() {
        use ForcePushOptions::*;
        assert_eq!(s(push_all("remote", DoNotForce, false, 0)), "push --progress --all remote");
        assert_eq!(s(push_all("remote", Force, false, 0)), "push -f --progress --all remote");
        assert_eq!(s(push_all("remote", ForceWithLease, false, 0)), "push --force-with-lease --progress --all remote");
        assert_eq!(s(push_all("remote", DoNotForce, true, 0)), "push -u --progress --all remote");
        assert_eq!(s(push_all("remote", DoNotForce, false, 1)), "push --recurse-submodules=check --progress --all remote");
        assert_eq!(s(push_all("remote", DoNotForce, false, 2)), "push --recurse-submodules=on-demand --progress --all remote");

        assert_eq!(s(push("remote", "from-branch", None, DoNotForce, false, 0)), "push --progress remote from-branch");
        let to = Some("to-branch");
        assert_eq!(s(push("remote", "from-branch", to, DoNotForce, false, 0)), "push --progress remote from-branch:refs/heads/to-branch");
        assert_eq!(s(push("remote", "from-branch", to, Force, false, 0)), "push -f --progress remote from-branch:refs/heads/to-branch");
        assert_eq!(s(push("remote", "from-branch", to, ForceWithLease, false, 0)), "push --force-with-lease --progress remote from-branch:refs/heads/to-branch");
        assert_eq!(s(push("remote", "from-branch", to, DoNotForce, true, 0)), "push -u --progress remote from-branch:refs/heads/to-branch");
        assert_eq!(s(push("remote", "from-branch", to, DoNotForce, false, 1)), "push --recurse-submodules=check --progress remote from-branch:refs/heads/to-branch");
        assert_eq!(s(push("remote", "from-branch", to, DoNotForce, false, 2)), "push --recurse-submodules=on-demand --progress remote from-branch:refs/heads/to-branch");
        assert_eq!(s(push("remote", "", to, DoNotForce, false, 0)), "push --progress remote HEAD:refs/heads/to-branch");
    }

    #[test]
    fn push_tag_cmd() {
        use ForcePushOptions::*;
        assert_eq!(s(push_tag("path", "tag", false, DoNotForce)), "push --progress path tag tag");
        assert_eq!(s(push_tag("path", " tag ", false, DoNotForce)), "push --progress path tag tag");
        assert_eq!(s(push_tag("path\\path", " tag ", false, DoNotForce)), "push --progress path/path tag tag");
        assert_eq!(s(push_tag("path", "tag", true, DoNotForce)), "push --progress path --tags");
        assert_eq!(s(push_tag("path", "tag", true, Force)), "push -f --progress path --tags");
        assert_eq!(s(push_tag("path", "tag", true, ForceWithLease)), "push --force-with-lease --progress path --tags");
        assert_eq!(s(push_tag("path", "", false, DoNotForce)), "");
    }

    #[test]
    fn rebase_cmd_throws_if_from_or_onto_null() {
        for (from, onto) in [(None, Some("onto")), (Some("from"), None)] {
            let o = RebaseOptions { branch_name: "branch".into(), from: from.map(Into::into), on_to: onto.map(Into::into), ..Default::default() };
            assert!(rebase(&o).is_err());
        }
    }

    #[test]
    #[allow(clippy::type_complexity)]
    fn rebase_cmd() {
        let p = "-c rebase.autosquash=false rebase";
        let cases: [(bool, bool, bool, bool, bool, bool, Option<bool>, &str); 15] = [
            (false, false, false, false, false, false, None, "branch"),
            (true, false, false, false, false, false, None, "-i --no-autosquash branch"),
            (false, true, false, false, false, false, None, "--rebase-merges branch"),
            (false, false, true, false, false, false, None, "branch"),
            (false, false, false, true, false, false, None, "--autostash branch"),
            (true, false, true, false, false, false, None, "-i --autosquash branch"),
            (false, false, false, false, true, false, None, "--ignore-date branch"),
            (false, false, false, false, false, true, None, "--committer-date-is-author-date branch"),
            (false, false, false, true, true, false, None, "--ignore-date --autostash branch"),
            (false, false, false, true, false, true, None, "--committer-date-is-author-date --autostash branch"),
            (true, true, true, true, true, false, None, "--ignore-date --autostash branch"),
            (true, true, true, true, false, true, None, "--committer-date-is-author-date --autostash branch"),
            (true, true, true, true, false, false, None, "-i --autosquash --rebase-merges --autostash branch"),
            (false, false, false, false, false, false, Some(false), "--no-update-refs branch"),
            (false, false, false, false, false, false, Some(true), "--update-refs branch"),
        ];
        for (interactive, preserve, squash, stash, ignore_date, cdate, update_refs, expected) in cases {
            let o = RebaseOptions {
                branch_name: "branch".into(),
                interactive,
                preserve_merges: preserve,
                auto_squash: squash,
                auto_stash: stash,
                ignore_date,
                committer_date_is_author_date: cdate,
                support_rebase_merges: true,
                update_refs,
                ..Default::default()
            };
            assert_eq!(s(rebase(&o).unwrap()), format!("{p} {expected}"));
        }
        let o = RebaseOptions { branch_name: "branch".into(), from: Some("from".into()), on_to: Some("onto".into()), ..Default::default() };
        assert_eq!(s(rebase(&o).unwrap()), format!("{p} --onto onto from branch"));
        let o = RebaseOptions { ignore_date: true, ..o };
        assert_eq!(s(rebase(&o).unwrap()), format!("{p} --ignore-date --onto onto from branch"));
    }

    #[test]
    fn remove_cmd() {
        assert_eq!(s(remove(true, true, &[])), "rm --force -r .");
        assert_eq!(s(remove(false, true, &[])), "rm -r .");
        assert_eq!(s(remove(true, false, &[])), "rm --force .");
        assert_eq!(s(remove(true, true, &["a", "b", "c"])), "rm --force -r a b c");
    }

    #[test]
    fn rename_branch_cmd() {
        assert_eq!(s(rename_branch("foo", "far")), "branch -m foo far");
    }

    #[test]
    fn reset_cmd() {
        for hash in [None, Some(""), Some("\t")] {
            assert!(reset(ResetMode::ResetIndex, hash, Some("file.txt"), true).is_err());
        }
        use ResetMode::*;
        let cases = [
            (ResetIndex, Some("tree-ish"), None, "reset --quiet tree-ish --"),
            (ResetIndex, Some("tree-ish"), Some("file.txt"), "reset --quiet tree-ish -- file.txt"),
            (Soft, None, None, "reset --soft --quiet --"),
            (Mixed, None, None, "reset --mixed --quiet --"),
            (Hard, None, None, "reset --hard --quiet --"),
            (Merge, None, None, "reset --merge --quiet --"),
            (Keep, None, None, "reset --keep --quiet --"),
            (Soft, Some("tree-ish"), None, "reset --soft --quiet tree-ish --"),
            (Hard, Some("tree-ish"), None, "reset --hard --quiet tree-ish --"),
            (Keep, None, Some("file.txt"), "reset --keep --quiet -- file.txt"),
            (Mixed, Some("tree-ish"), Some("file.txt"), "reset --mixed --quiet tree-ish -- file.txt"),
        ];
        for (mode, commit, file, expected) in cases {
            assert_eq!(s(reset(mode, commit, file, true).unwrap()), expected);
        }
    }

    #[test]
    fn revert_cmd() {
        let id = ObjectId::random();
        assert_eq!(s(revert(id, true, 0)), format!("revert {id}"));
        assert_eq!(s(revert(id, false, 0)), format!("revert --no-commit {id}"));
        assert_eq!(s(revert(id, true, 1)), format!("revert -m 1 {id}"));
    }

    #[test]
    fn stash_save_cmd() {
        assert_eq!(s(stash_save(false, false, None, &[])), "stash save");
        assert_eq!(s(stash_save(true, false, None, &[])), "stash save -u");
        assert_eq!(s(stash_save(false, true, None, &[])), "stash save --keep-index");
        assert_eq!(s(stash_save(false, false, Some("message"), &[])), "stash save message");
        assert_eq!(s(stash_save(false, false, None, &["a", "b"])), "stash push -- a b");
        assert_eq!(s(stash_save(false, false, Some("test message"), &["a", "b"])), "stash push -m \"test message\" -- a b");
        for m in [None, Some(""), Some(" "), Some("\t")] {
            assert_eq!(s(stash_save(false, false, m, &["a", "b"])), "stash push -- a b");
            assert_eq!(s(stash_save(false, false, m, &[])), "stash save");
        }
        assert_eq!(s(stash_save(false, false, None, &["", "a"])), "stash push -- a");
        assert_eq!(s(stash_save(false, false, Some("test message"), &[])), "stash save \"test message\"");
    }

    #[test]
    fn submodule_sync_cmd() {
        assert_eq!(s(submodule_sync(Some("foo"))), "submodule sync foo");
        assert_eq!(s(submodule_sync(Some(""))), "submodule sync");
        assert_eq!(s(submodule_sync(None)), "submodule sync");
    }

    #[test]
    fn update_ref_cmd() {
        let id = ObjectId::parse("2111111111111111111111111111111111111111").unwrap();
        assert_eq!(s(update_ref("mybranch", id)), "update-ref mybranch 2111111111111111111111111111111111111111");
    }

    #[test]
    fn create_tag_cmd() {
        // Ported from CommandsTests.CreateTag.cs
        let id = ObjectId::random();
        assert_eq!(s(create_tag("tagname", id, TagOperation::Lightweight, "", None, false).unwrap()), format!("tag tagname -- {id}"));
        assert_eq!(s(create_tag("tagname", id, TagOperation::Annotate, "", Some("c:/.git/TAGMESSAGE"), false).unwrap()), format!("tag -a -F c:/.git/TAGMESSAGE tagname -- {id}"));
        assert_eq!(s(create_tag("tagname", id, TagOperation::SignWithDefaultKey, "", Some("c:/.git/TAGMESSAGE"), false).unwrap()), format!("tag -s -F c:/.git/TAGMESSAGE tagname -- {id}"));
        assert_eq!(s(create_tag("tagname", id, TagOperation::SignWithSpecificKey, "abc123", Some("c:/.git/TAGMESSAGE"), false).unwrap()), format!("tag -u abc123 -F c:/.git/TAGMESSAGE tagname -- {id}"));
        assert_eq!(s(create_tag("tagname", id, TagOperation::Lightweight, "", None, true).unwrap()), format!("tag -f tagname -- {id}"));
        assert!(create_tag("tagname", ObjectId::WORK_TREE, TagOperation::Lightweight, "", None, false).is_err());
        assert!(create_tag(" ", id, TagOperation::Lightweight, "", None, false).is_err());
        assert!(create_tag("t", id, TagOperation::Annotate, "", None, false).is_err());
        assert!(create_tag("t", id, TagOperation::SignWithSpecificKey, "", Some("f"), false).is_err());
    }

    #[test]
    fn delete_branch_cmd() {
        // Ported from CommandsTests.DeleteBranch.cs
        let local = GitRef::from_complete_name(ObjectId::random(), "refs/heads/local");
        let remote = GitRef::from_complete_name(ObjectId::random(), "refs/remotes/origin/remote");
        assert_eq!(s(delete_branch(&[&local], false).unwrap()), "branch --delete local");
        assert_eq!(s(delete_branch(&[&local], true).unwrap()), "branch --delete --force local");
        assert_eq!(s(delete_branch(&[&remote], false).unwrap()), "branch --delete --remotes origin/remote");
        assert_eq!(s(delete_branch(&[&local, &remote], false).unwrap()), "branch --delete --all local origin/remote");
        assert!(delete_branch(&[], false).is_err());
        assert_eq!(s(delete_remote_branches("origin", &["a", "b"])), "push origin :refs/heads/a :refs/heads/b");
    }
}
