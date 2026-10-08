//! Port of `ArgumentBuilder` / `GitArgumentBuilder`.
//!
//! Unlike the C# version (which builds one Windows command line string), arguments are kept
//! as an argv vector and passed to the process without any shell, which is correct on all
//! platforms. [`std::fmt::Display`] renders a readable, shell-like command line for logs.

use std::fmt;

/// A list of command line arguments for git.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitArgs {
    args: Vec<String>,
}

impl GitArgs {
    /// Starts arguments for the git sub-command `command` (e.g. `"log"`).
    pub fn new(command: &str) -> Self {
        let mut a = GitArgs::default();
        a.add(command);
        a
    }

    /// Starts git arguments with `-c key=value` configuration overrides before the command.
    pub fn with_config(configs: &[(&str, &str)], command: &str) -> Self {
        let mut a = GitArgs::default();
        for (k, v) in configs {
            a.add("-c");
            a.add(format!("{k}={v}"));
        }
        a.add(command);
        a
    }

    pub fn empty() -> Self {
        GitArgs::default()
    }

    /// Adds an argument; empty strings are ignored (as `null`/empty in C#).
    pub fn add(&mut self, arg: impl Into<String>) -> &mut Self {
        let s = arg.into();
        if !s.is_empty() {
            self.args.push(s);
        }
        self
    }

    /// Adds an argument which may be empty (e.g. an empty commit message).
    pub fn add_raw(&mut self, arg: impl Into<String>) -> &mut Self {
        self.args.push(arg.into());
        self
    }

    /// Adds `arg` if `condition` is true.
    pub fn add_if(&mut self, condition: bool, arg: impl Into<String>) -> &mut Self {
        if condition {
            self.add(arg);
        }
        self
    }

    /// Adds an optional argument.
    pub fn add_opt(&mut self, arg: Option<impl Into<String>>) -> &mut Self {
        if let Some(a) = arg {
            self.add(a);
        }
        self
    }

    pub fn add_all<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for a in args {
            self.add(a);
        }
        self
    }

    /// Builder style [`GitArgs::add`].
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.add(arg);
        self
    }

    /// Builder style [`GitArgs::add_if`].
    pub fn arg_if(mut self, condition: bool, arg: impl Into<String>) -> Self {
        self.add_if(condition, arg);
        self
    }

    /// Builder style [`GitArgs::add_all`].
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.add_all(args);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.args.is_empty()
    }

    pub fn as_slice(&self) -> &[String] {
        &self.args
    }

    /// Prefixes the arguments with `--no-optional-locks` (for background status queries).
    pub fn no_locks(mut self, no_locks: bool) -> Self {
        if no_locks {
            self.args.insert(0, "--no-optional-locks".to_string());
        }
        self
    }
}

/// Quotes an argument for display if needed.
pub fn quote_for_display(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".to_string();
    }
    if arg.chars().any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '|' | '&' | ';' | '$' | '`')) {
        format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        arg.to_string()
    }
}

impl fmt::Display for GitArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for a in &self.args {
            if !first {
                f.write_str(" ")?;
            }
            first = false;
            f.write_str(&quote_for_display(a))?;
        }
        Ok(())
    }
}

impl<S: Into<String>> FromIterator<S> for GitArgs {
    fn from_iter<T: IntoIterator<Item = S>>(iter: T) -> Self {
        let mut a = GitArgs::default();
        a.add_all(iter);
        a
    }
}

/// Converts a path to use forward slashes (git always accepts them).
pub fn to_posix_path(path: &str) -> String {
    path.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    //! Ported from GitExtensions.Extensibility.Tests/ArgumentBuilderTests.cs and
    //! GitExtUtils.Tests/GitArgumentBuilderTests.cs (adapted to argv semantics).
    use super::*;

    fn test(expected: &str, args: &[Option<&str>]) {
        let mut b = GitArgs::empty();
        for a in args.iter().flatten() {
            b.add(*a);
        }
        assert_eq!(b.to_string(), expected);
    }

    #[test]
    fn adds_simple_parameters() {
        test("", &[]);
        test("foo", &[Some("foo")]);
        test("foo bar", &[Some("foo"), Some("bar")]);
        test("foo bar", &[Some("foo"), None, Some("bar")]);
        test("foo bar", &[Some("foo"), Some(""), Some("bar")]);
        test("", &[None]);
    }

    #[test]
    fn is_empty() {
        let mut b = GitArgs::empty();
        assert!(b.is_empty());
        b.add("test");
        assert!(!b.is_empty());
    }

    #[test]
    fn add_conditional() {
        let a = GitArgs::new("log").arg_if(true, "-z").arg_if(false, "--all").arg("HEAD");
        assert_eq!(a.to_string(), "log -z HEAD");
        assert_eq!(a.as_slice(), ["log", "-z", "HEAD"]);
    }

    #[test]
    fn config_items_precede_command() {
        let a = GitArgs::with_config(&[("rebase.autosquash", "false")], "rebase");
        assert_eq!(a.to_string(), "-c rebase.autosquash=false rebase");
        assert_eq!(GitArgs::new("status").no_locks(true).to_string(), "--no-optional-locks status");
    }

    #[test]
    fn display_quotes_arguments_with_spaces() {
        let a = GitArgs::new("commit").arg("--author=a b <c@d>").arg("-F").arg("path with space");
        assert_eq!(a.to_string(), "commit \"--author=a b <c@d>\" -F \"path with space\"");
        assert_eq!(a.as_slice()[1], "--author=a b <c@d>");
    }

    #[test]
    fn posix_path() {
        assert_eq!(to_posix_path("hello\\world.patch"), "hello/world.patch");
    }
}
