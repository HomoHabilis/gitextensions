# Working on this repository

The application is the Rust port of Git Extensions in `rust/` (an egui desktop app). The
original C# code was removed; it is only in the history and in
[gitextensions/gitextensions](https://github.com/gitextensions/gitextensions).

## Definition of done

Run from `rust/` and fix everything they report before pushing (CI runs the same):

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets --locked -- -D warnings`
3. `cargo test --workspace --locked`

CI also runs clippy and the tests on Windows and macOS, so code behind `cfg(windows)` or
`cfg(target_os = "macos")` is checked there; check Windows code locally with
`cargo clippy --target x86_64-pc-windows-gnu` (needs `gcc-mingw-w64-x86-64`).

## Guardrails

The lints in `[workspace.lints]` of `rust/Cargo.toml` are deliberate. Do not weaken or remove
them. When one must be silenced, use `#[allow(lint, reason = "...")]` on the smallest item that
needs it; a crate-wide or reasonless `allow` is not accepted.

- **No `unwrap()` outside tests.** A panic closes the whole window. Handle the `None`/`Err`
  (`let ... else`, `?`, a fallback the user can see), or, when it truly cannot happen, use
  `expect("...")` stating the invariant. Lock a `Mutex` with
  `.lock().unwrap_or_else(PoisonError::into_inner)`.
- **No `unsafe`** without an `#[allow(unsafe_code, reason)]` and a `// SAFETY:` comment.
- **Errors the user should see are shown, not dropped.** `let _ =` is fine for best-effort work
  (a repaint request, a send to a closed channel), not for a git command the user asked for.
- **Read-only parameters are borrowed** (`&str`, `&[T]`, `&Path`), not `String`/`Vec`/`PathBuf`.
- Cloning a small value in the UI code is fine (egui rebuilds the UI each frame). Cloning a whole
  diff, file list or commit list each frame is not: share it with `Arc`.

## Tests

- Every behaviour that can be checked by a test is checked by one, including views, which can
  be run in a headless `egui::Context`. Run each new test at least once before pushing.
- What needs a real window is checked by hand on Linux under Xvfb (the app also needs
  `libxkbcommon-x11-0`). Only what can differ on Windows (process launching, paths, the shell,
  the cost of a git call) is left to a check on Windows.
- `UPDATE_SNAPSHOTS=1 cargo test` rewrites the approval files after an intended change.

## Performance

Speed is the main goal of the port, mostly on Windows where each git process costs ~150 ms.
Count git calls, not only milliseconds: a change must not add a git call to a click or a
frame. Before and after a change to a view or to the loading of commits, run:

- `cargo test --release -p gitext-app frame_cost -- --ignored --nocapture` (add `BENCH_TREE=1`
  for the tree view): per-frame cost of a big diff.
- `GITEXT_BENCH_REPO=<repo with 2000+ commits> cargo test --release -p gitext-app load_latency -- --ignored --nocapture`:
  time and git calls to show a commit.
- `GITEXT_PROFILE=1` prints the slow frames of the running app.

## Translations

UI strings go through `tr("...")`/`trf(...)`. When strings change, run
`rust/crates/gitext-app/translations/update.py`.
