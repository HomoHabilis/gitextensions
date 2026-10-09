# Contributing to Git Extensions (Rust port)

Contributions are welcome: ideas, bug reports, bug fixes and new features.

## Reporting issues

Search the [issue tracker](https://github.com/HomoHabilis/gitextensions/issues?q=), including
closed issues, before creating a new one. Include the steps to reproduce, screenshots, and
the versions of Git Extensions (*Help → About*), git and your operating system.

Problems that also happen in the original Windows application can be reported to
[gitextensions/gitextensions](https://github.com/gitextensions/gitextensions/issues).

## Pull requests

The code is under [`rust/`](rust); [rust/README.md](rust/README.md) describes its layout.
Before submitting, run the tests from `rust/`:

```sh
cargo test --workspace
```

The `rust` GitHub Actions workflow runs the tests and builds the binaries on Linux, macOS and
Windows for every pull request that changes `rust/`.

To make a pull request easy to review:

- Keep it focused on one change.
- Use clear commits. Fixup and squash commits are rejected by the `Git Checks` workflow, so
  squash them before merging.
- Describe the change and why it should be made.
- Add tests for parsing and repository operations (`gitext-core`) and for the revision graph
  (`gitext-graph`).
- Follow the style of the surrounding code. The code is not formatted with `cargo fmt`, so
  do not reformat lines you do not change.

When behaviour should match the original application, compare with the C# code in
[gitextensions/gitextensions](https://github.com/gitextensions/gitextensions) or in this
repository's history before the C# code was removed.

## Conduct

Please review our [code of conduct](CODE_OF_CONDUCT.md).
