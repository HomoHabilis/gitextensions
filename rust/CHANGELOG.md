# Changelog

The notes of each release come from the section of its version below (`## <version>`), followed
by the list of commits since the previous release. Add a section before tagging a release;
without one, the release notes only list the commits.

## 0.1.0

First release of the cross-platform Rust port of Git Extensions, for Linux, macOS and Windows.

### Features

- **Browse window:** the revision grid with the Git Extensions revision graph (lanes, curved
  diagonals, ref labels, hover highlighting of a branch's ancestry), the repository tree of
  branches, remotes, tags, stashes and submodules, and the commit, diff and file tree tabs.
- **Commit window:** staging by file, hunk or line, amend, templates, message history, commit
  and push.
- **Dialogs:** branches and tags, checkout, merge, rebase (also interactive), cherry-pick,
  revert, reset, push, pull, remotes, stash, clone, init, submodules, worktrees, blame, file
  history, reflog, compare, bisect, archive, patches, cleanup, merge conflicts, `.gitignore`,
  verify database, settings.
- **Diff and merge tools:** configured like Git Extensions, detected when installed, with a
  fallback to another installed tool.
- **WSL repositories** on Windows run with the git of the distro.
- **Windows:** an installer and the Explorer context menu (*Git Extensions* when
  right-clicking files, folders and the background of folders).
- **Light and dark themes**, following the system setting by default.
- **Command line:** `gitext <command>` opens a window directly, like `GitExtensions.exe`.
