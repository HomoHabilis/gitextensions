# Git Extensions – Rust port

A cross-platform (Linux, macOS, Windows) port of Git Extensions, written in Rust with
[egui](https://github.com/emilk/egui). It keeps the features, the window layout and the
revision graph drawing of the WinForms application. Colors, icons and fonts are new: there is a
light and a dark theme, and by default the app follows the system setting.

## Building and running

```sh
cd rust
cargo run --release -p gitext-app            # opens the repository in the current directory
cargo run --release -p gitext-app -- --help  # all command-line verbs
```

The binary is called `gitext`. It needs `git` on the `PATH`; a different executable can be set
in *Settings → Git*.

### Linux

One build runs on Ubuntu 22.04, 24.04 and newer, as long as it is built on the oldest
release it must run on. A binary needs the glibc of the machine it was built on or newer, and
the CI builds on Ubuntu 22.04 (glibc 2.35). A binary built on Ubuntu 24.04 also runs on 22.04.
It prints `weak version 'GLIBC_2.39' not found` at startup, which is harmless: the Rust standard
library falls back when the newer functions are missing. Build on 22.04 to avoid the message.

Runtime dependencies:

- **Graphics:** OpenGL (Mesa), plus `libxkbcommon-x11` (X11) or a Wayland compositor.
- **File dialogs:** zenity (GNOME) or kdialog (KDE) when installed, otherwise the
  xdg-desktop-portal. Without either, as on WSL, type the path in the *Open local repository*
  dialog, or install zenity: `sudo apt install zenity`.
- **Diff and merge tool:** Meld is recommended (`sudo apt install meld`) and is not bundled.

Nothing extra is needed on macOS or Windows.

### Windows installer and Explorer context menu

`installer/gitext.nsi` builds a Windows installer with [NSIS](https://nsis.sourceforge.io)
(the CI attaches it to the Windows build artifact):

```sh
cargo build --release -p gitext-app
makensis /DVERSION=0.1.0 installer/gitext.nsi   # writes installer/GitExtensions-0.1.0-setup.exe
```

It installs for the current user, without administrator rights, into
`%LOCALAPPDATA%\Programs\GitExtensions`, and adds a Start menu entry, the uninstaller and,
optionally, a desktop shortcut. Git for Windows is not bundled.

The installer also adds the **Git Extensions** menu to Explorer, like the shell extension of
Git Extensions. It has three versions:

- **Files:** open with difftool, file history, blame, reset file changes, add files, apply
  patch, open repository, commit, settings.
- **Folders:** open repository, commit, pull, push, view stash, view changes, checkout branch
  or revision, create branch, file history of the folder, reset changes, add files, clone,
  create new repository, settings.
- **Folder background:** the same, without the commands about the folder itself.

The original is a COM extension that Explorer loads, and it shows only the commands that fit
(the repository commands only inside a repository). This port registers static Explorer verbs
in `HKCU\Software\Classes` instead, which run `gitext -C <path> <command>`. Nothing is loaded
into Explorer, but the menu cannot know whether a path is inside a repository: a command used
outside of one offers to create a repository there. On Windows 11 the menu is under
*Show more options*, like the original.

Without the installer: `gitext shellext install` / `gitext shellext uninstall`, or
*Settings → General → Windows Explorer context menu*, which also chooses the menu items.

### Diff and merge tools

*Settings → Git → Diff and merge tools* configures them the way Git Extensions does:
`diff.guitool` / `merge.guitool` and `difftool.<tool>.path|cmd` / `mergetool.<tool>.path|cmd`
in the global git config.

- **Supported tools:** WinMerge, Meld, KDiff3, Beyond Compare, P4Merge, VS Code, the
  TortoiseGit tools, Sublime Merge, DiffMerge, Araxis, SemanticMerge and vsdiffmerge.
- **Detect** finds them in the usual install locations and on the `PATH`.
- **Browse…** picks the executable, and the command line is filled in for the selected tool.

When the configured tool is not installed, another installed tool is used instead, so a
WinMerge configuration falls back to Meld on Linux. When no tool is installed, a dialog
explains how to install one.

### WSL repositories (Windows)

Repositories under `\\wsl$\<distro>\…` or `\\wsl.localhost\<distro>\…` are run with the git
of the distro (`wsl -d <distro> --cd <dir> --exec git …`), as Git Extensions does. This is much
faster than Windows git on WSL files, and avoids its "detected dubious ownership" refusal. Git
must be installed in the distro, and *Settings → Git* can turn this off.

For other repositories that git refuses as "owned by someone else", the app offers to add them
to `safe.directory`. Diff and merge tools of WSL repositories are those configured in the
distro.

The settings are stored as JSON in the platform config directory (`~/.config/GitExtensions`
on Linux, `%APPDATA%\GitExtensions` on Windows, `~/Library/Application Support/GitExtensions`
on macOS). Set `GITEXT_CONFIG_DIR` to use another directory.

Set `GITEXT_PROFILE=1` to print the frames that take longer than 16 ms, with the time spent in
each part of the window, to stderr. This helps when reporting slowness in a large repository.

## Layout

| Crate | Contents | C# origin |
|---|---|---|
| `crates/gitext-core` | Object ids, refs, revisions, `git log` parsing, status/diff/patch/blame parsing, building git command lines, running git, repository operations, settings, recent repositories | `GitCommands`, `GitExtensions.Extensibility`, `GitUIPluginInterfaces`, the parsing parts of `GitUI` |
| `crates/gitext-graph` | Revision graph model, lane assignment, straightening, curves and the row renderer (it emits drawing primitives) | `GitUI/UserControls/RevisionGrid/Graph` |
| `crates/gitext-app` | The application (`gitext`): windows, views and dialogs | `GitUI`, `GitExtensions` |

## Feature mapping

**Browse window (`FormBrowse`).** It has the menus (Start, Dashboard, Repository, Commands,
View, Tools, Help), the toolbar and a branch/revision filter bar with "first parent" and
toggles for remotes, tags, stashes, reflog and artificial commits. Below that are three areas:

- **Left panel (repo object tree).** Branches grouped into folders with ahead/behind counts,
  remotes, tags, stashes and submodules. Each has a context menu: checkout, merge, rebase,
  rename, delete, push, pull, fetch, prune, set upstream, apply/pop/drop stash, and so on.
- **Revision grid with graph.** Lanes, merges, curved or straight diagonals, colored lanes, and
  ref labels for local, remote, tag and stash refs. It shows the working-directory and index
  pseudo-commits and grays out non-relatives (or highlights the selected branch). Hovering a
  label highlights its ancestry, and commits by the current author are highlighted. It has
  quick search, keyboard navigation and multi-selection.
  - The full context menu covers: checkout, create branch or tag, reset, merge, rebase,
    interactive rebase, cherry-pick, revert, copy, navigate, compare, archive and format-patch.
- **Bottom tabs.**
  - *Commit*: the commit info, with parents, children, describe and containing branches/tags.
  - *Diff*: file list and diff, with a parent selector for merge commits.
  - *File tree*: browse a revision, find a file, open blame or history, save as.

The status bar shows the current branch and its tracking state.

**Dashboard.** Recent repositories (with a filter), plus open, clone and create.

**Commit window (`FormCommit`).** It has unstaged and staged lists, flat or as a tree, with a
filter. It supports staging and unstaging by file, hunk or selected lines, and resetting files
or lines. You also get:

- Amend, templates, message history, sign-off/no-verify/author options, and commit & push.
- Stash and reset all.

**Dialogs.** Each is a separate window:

- Branches: create, checkout and delete branches; create and delete tags.
- Integrating changes: merge, rebase (including interactive), cherry-pick and revert.
- Reset (soft, mixed, hard, keep, merge).
- Remote operations: push (branches, tags, force with lease), pull (merge, rebase or fetch
  only, with autostash), and manage remotes.
- Local changes and sources: stash, clone, init, submodules, worktrees.
- History tools: blame (with "blame previous revision"), file history, reflog, compare
  commits, bisect, go to commit.
- Patches and files: archive, apply/format patch, view patch, clean working directory,
  `.gitignore` editor.
- Recovery and checks: merge conflict resolution (ours, theirs, merge tool), verify database
  (lost objects).
- App windows: git command log, settings, about.

The `gitext <verb>` command line opens the matching window directly, like `GitExtensions.exe
<verb>` does.

## Tests

The tests of the original repository were ported where the code under test was ported. They
use the original approval files, copied to `testdata/`, so they show that the Rust code produces
the same output as the C# code:

- **Revision graph.** The 39 `RevisionGraphTests` scenarios produce exactly the original
  `*.verified.txt` ASCII graph snapshots (`crates/gitext-graph/src/tests.rs`).
- **`git log` parsing.** `RevisionReaderTests` runs on the original binary log captures and
  compares against `*.verified.json`.
- **Status and diff parsing.** The `GitCommandHelpers` status (porcelain v2) and diff-raw
  parsing tests.
- **Patches.** Patch parsing and partial patch creation (`PatchProcessorTest`, `PatchManagerTest`),
  including the large and binary patches.
- **Other ported tests.** Blame parsing (`GitBlameParserTests`), the revision summary builder,
  the file tree builder and the commands/arguments tests (`GitCommandsTests`, `ArgumentBuilder`
  tests). There are also branch name normalisation, object id, ref, URL and title tests.
- **Repository operations.** These run against a temporary repository created with the real
  `git`. They cover stage, commit, branch, tag, stash, refs and status.

Run them with:

```sh
cargo test --workspace
```

Set `UPDATE_SNAPSHOTS=1` to rewrite the approval files after an intended change.

The `rust` GitHub Actions workflow runs the tests and builds the binary on Linux, macOS and
Windows.

## Why a new port

Before porting, I searched the forks of `gitextensions/gitextensions` for an existing Rust,
cross-platform port, and none exists. Searches covered forks with Rust as language, forks
containing a `Cargo.toml`, and the web. The only related project is
[t4-git-ui](https://github.com/toperux/t4-git-ui), an independent Tauri + React application
in the style of Git Extensions. It is not a fork and not a port.
