# Parity plan: Rust port vs upstream Git Extensions

What the port still lacks compared with upstream Git Extensions (the .NET app at commit
`16461a8^1`), split into must-have, nice-to-have and dropped features. Ticked items are
done. The detailed gap report the plan comes from is in the project files
(`feature-gap/upstream-vs-rust.md`), and the plan with its discussion is the
"Rust port parity plan" document.

Blocks are ordered by how often a daily user hits the gap, then by effort.
Effort: S = one focused pull request, M = a short series, L = a feature-sized project.

## Must-have

### M1. Settings that do nothing (S), done in #16

Every option in Settings now changes something or is gone. Settings were kept when they
needed no plugin or new dependency.

- [x] Show whitespace
- [x] Ref sort by / order
- [x] Force-with-lease by default when pushing after an amend
- [x] Default action for local changes on checkout (also used by the left panel checkout)
- [x] Follow renames in file history
- [x] Refresh the commit dialog when the window gets the focus
- [x] Script "ask for confirmation" and "run in the background"
- [x] Commit details position (right of the graph, also in the View menu)
- [x] File manager command
- [x] Removed: autostash on checkout (same as default action = Stash)
- [x] Removed: current branch only (same as the branch filter)
- [x] Removed: word wrap in diffs (upstream has none)
- [x] Removed: diff tool (tools are set in git config)
- [x] Removed: show git command line (the process dialog always shows it)
- [x] Removed: superproject refs (no superproject support yet)

### M2. Diff viewer toolbar (M), done in #16

- [x] Previous / next change
- [x] Ignore whitespace: all (-w), amount (-b), at end of line
- [x] Show whitespace toggle
- [x] Context lines - / +
- [x] Show entire file
- [x] Go to line (Ctrl+G)
- [x] Copy old version / copy new version of the selection
- [x] Large file guard (over 5 MB, shown on request)

Also fixed in #16 while testing: script arguments keep quoted words, the commit message
panel no longer grows on an error, the Git settings page reads git in the background,
a refresh keeps the selected commit's details and diff, no console flash when Windows
opens a folder.

### M3. Intra-line highlighting (M), PR A

- [ ] Highlight the changed words inside modified lines (upstream `MarkInlineDifferences`)

### M4. Commit-list actions (M), PR B

- [ ] Undo last commit
- [ ] Reword commit
- [ ] Edit commit
- [ ] Reset another branch
- [ ] Compare to BASE / select as BASE
- [ ] Compare with current branch
- [ ] Compare to branch
- [ ] Open selected commits with the difftool

### M5. Commit dialog gaps (S), PR C

- [ ] Reset unstaged changes
- [ ] Stash staged changes
- [ ] Commit & push button
- [ ] Create branch
- [ ] Unresolved-conflicts banner
- [ ] Ln/Col indicator
- [ ] Show ignored / assumed unchanged / skip-worktree toggles

### M6. File list actions (M), PR C

- [ ] Rename / move
- [ ] Delete
- [ ] Stop tracking
- [ ] Open with
- [ ] Copy path variants (relative, POSIX, WSL)
- [ ] Cherry-pick file changes
- [ ] Remember first / second file for a diff

### M7. Main window gaps (S), PR B

- [ ] Repository switcher dropdown
- [ ] Update / synchronize all submodules in the Repository menu
- [ ] Empty-repository screen
- [ ] Advanced revision filter dialog (since / until, limit, simplify merges, hide merges)

### M8. Left panel actions (M), PR B

- [ ] Fetch & checkout / merge / rebase on remote branches
- [ ] Open remote URL
- [ ] Copy names
- [ ] Rebase on tag
- [ ] Create / prune worktree from a node
- [ ] Keyboard focus with Delete / F2

### M9. Search tools (M), PR A

- [ ] Find in commit files (git grep)
- [ ] Find file in a revision
- [ ] Alternative diff views: git word diff, difftastic, range-diff

### M10. Avatars (M), PR B

- [ ] Author images in the grid and commit details (Gravatar or initials), with a disk cache

### M11. Syntax highlighting (M), PR A

- [ ] Language-aware colouring of the diff and file view, with an on/off toggle

### Fixes requested with the must-haves

- [ ] While a repository loads, don't say "This repository does not contain any commits" (PR B)
- [ ] Stash dialog: show the working directory changes when no stash is selected, stash
      selected files, refresh button (PR C)
- [ ] Message and confirmation boxes with long content scroll instead of growing past the
      screen (PR C)

## Nice-to-have

### N1. Navigation history (S)

- [ ] Back / forward through selected commits on Alt+Left / Alt+Right as upstream (parent /
      child stay on Ctrl+P / Ctrl+N, and the port's Alt+Left/Right alias for them goes)
- [ ] Select in left panel

### N2. Image and binary preview (M)

- [ ] Show images side by side or old / new
- [ ] Treat-as-text toggle
- [ ] Encoding choice

### N3. Commit message helpers (M)

- [ ] Spell check
- [ ] Auto-complete of paths and symbols
- [ ] Conventional-commit prefix menu (Ctrl+T)
- [ ] "Only my messages" in the history

### N4. Commit details extras (M)

- [ ] Clickable issue links (revision links settings)
- [ ] GPG indicator in the grid
- [ ] Remote branches containing the commit
- [ ] Annotated tag messages
- [ ] Notes editor

### N5. Built-in replacements for popular plugins (M)

- [ ] Background fetch
- [ ] Delete merged / unused branches
- [ ] Find large files

### N6. User scripts upgrade (M)

- [ ] More variables (upstream has about 30)
- [ ] Events (before / after commit, pull, push)
- [ ] Toolbar placement
- [ ] Prompts

### N7. Settings breadth (L)

- [ ] Configurable hotkeys page
- [ ] Confirmations page
- [ ] Branch sorting / prioritised branches
- [ ] Settings-source header (effective / local / global)
- [ ] Checklist page

### N8. Smaller dialog extras (M)

- [ ] Git command log: save, word wrap, cache tab
- [ ] Verify: unreachable, save to lost-found, remove dangling
- [ ] Reflog ref chooser
- [ ] Go-to-commit branch and tag lists
- [ ] Push "select tracked"
- [ ] Delete tag from chosen remotes
- [ ] Archive of changed files
- [ ] Dashboard "remove missing repositories"
- [ ] Conflicts dialog leftovers: open base / local / remote with…, save them as, file history,
      submodule conflict dialog

### N9. Translations (L)

- [ ] Translatable UI strings and a language setting, reusing the upstream translations where
      the strings still match

## Dropped

Kept here so the decision is not made again.

| Upstream feature | Why it is dropped | Replacement in the port |
|---|---|---|
| Plugin system (API, plugin settings pages) | Large API surface for a handful of plugins | Useful plugins built in (N5) |
| Gource, statistics (GitImpact, GitStatistics), release notes generator, proxy switcher, auto-compile submodules, create local branches plugins | Rarely used, or one git command away | None, or a user script |
| GitHub integration (pull requests, fork and clone, add upstream remote) | Not wanted (decided 2026-10-14) | None |
| Build server integration (AppVeyor, Azure DevOps, GitHub Actions, GitLab, Jenkins, TeamCity) | Six provider integrations to maintain | None |
| Embedded console (ConEmu, Mintty) and Console tab | Windows-only console hosts | External terminal |
| PuTTY / Pageant / plink, SSH key prompt, built-in askpass helper | OpenSSH ships with Git for Windows and every Linux | git and the system askpass |
| Visual Studio integration, Git GUI / gitk launchers | Tied to other tools | Open in editor, external tools |
| COM shell extension DLL | Already replaced | Static Explorer menu entries |
| Windows jump list, taskbar thumbnail buttons and progress | Windows-only polish | None |
| Update checker, changelog window, donate dialog | Tied to upstream release channels | A link to the project's releases page |
| Bug reporter (NBug) and telemetry | Tied to upstream services | "Report an issue" link |
| Fix-HOME dialog, Win32 interop helpers, WinForms controls | Not needed with egui and Rust | None |
| Replace in the diff viewer, custom colour and theme files | The viewer is read-only; built-in light and dark palettes | None |

## Decisions

- Translations are nice-to-have (N9), not dropped.
- GitHub integration and all build-server integration are dropped.
- Back / forward navigation (N1) goes on Alt+Left / Alt+Right as upstream; parent / child stay
  on Ctrl+P / Ctrl+N.
- Search tools and avatars are must-have (M9, M10); syntax highlighting is must-have, last (M11).
- M1 kept a setting when it needed no plugin or new dependency, and deleted the rest.
- The remaining must-haves are split into three pull requests that touch separate files:
  A = diff viewer (M3, M9, M11), B = commit list and main window (M4, M7, M8, M10),
  C = dialogs and file lists (M5, M6).
