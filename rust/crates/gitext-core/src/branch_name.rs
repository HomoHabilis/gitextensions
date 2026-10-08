//! Port of `GitBranchNameNormaliser`: makes a string a valid git branch name
//! (see `git check-ref-format`).

use std::sync::OnceLock;

use regex::Regex;

/// Port of `GitBranchNameOptions`.
#[derive(Debug, Clone)]
pub struct GitBranchNameOptions {
    pub replacement_token: String,
    pub allow_trailing_slash: bool,
}

impl GitBranchNameOptions {
    pub fn new(replacement_token: &str) -> Self {
        GitBranchNameOptions { replacement_token: replacement_token.to_string(), allow_trailing_slash: false }
    }
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).unwrap())
}

/// Port of `PathUtil.IsValidPathChar`: whether `c` can be used in a branch name and a file path
/// (on all platforms).
pub fn is_valid_path_char(c: char) -> bool {
    c > ' ' && c < '~' && !matches!(c, '^' | ':' | '"' | '<' | '>' | '|')
}

/// Normalises `branch_name` into a valid ref name, replacing invalid parts.
pub fn normalise(branch_name: &str, options: &GitBranchNameOptions) -> String {
    if branch_name.trim().is_empty() {
        return String::new();
    }
    let mut b = rule10(branch_name, options);
    b = rule09(&b, options);
    b = rule08(&b, options);
    b = rule07(&b, options);
    b = rule05(&b, options);
    b = rule04(&b, options);
    b = rule03(&b, options);
    b = rule06(&b, options);
    rule01(&b, options)
}

/// No component may begin with '.' or end with '.lock'.
pub fn rule01(branch_name: &str, o: &GitBranchNameOptions) -> String {
    static PERIOD: OnceLock<Regex> = OnceLock::new();
    static LOCK: OnceLock<Regex> = OnceLock::new();
    branch_name
        .split('/')
        .map(|t| {
            let mut t = t.to_string();
            if t.starts_with('.') {
                t = re(&PERIOD, r"^\.*").replace(&t, o.replacement_token.as_str()).into_owned();
            }
            if t.to_lowercase().ends_with(".lock") {
                t = re(&LOCK, r"(?i)\.lock$").replace(&t, format!("{}lock", o.replacement_token)).into_owned();
            }
            t
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// No '..'.
pub fn rule03(branch_name: &str, o: &GitBranchNameOptions) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    re(&RE, r"\.{2,}").replace_all(branch_name, o.replacement_token.as_str()).into_owned()
}

/// No control characters, space, '~', '^', ':'.
pub fn rule04(branch_name: &str, o: &GitBranchNameOptions) -> String {
    let mut s = String::with_capacity(branch_name.len());
    for c in branch_name.chars() {
        if is_valid_path_char(c) || c.is_alphanumeric() {
            s.push(c);
        } else {
            s.push_str(&o.replacement_token);
        }
    }
    s
}

/// No '?', '*' or '['.
pub fn rule05(branch_name: &str, o: &GitBranchNameOptions) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    re(&RE, r"[?*\[]").replace_all(branch_name, o.replacement_token.as_str()).into_owned()
}

/// Cannot begin or end with '/' or contain multiple consecutive '/'.
pub fn rule06(branch_name: &str, o: &GitBranchNameOptions) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let mut b = re(&RE, r"/{2,}").replace_all(branch_name, "/").into_owned();
    if let Some(s) = b.strip_prefix('/') {
        b = s.to_string();
    }
    if !o.allow_trailing_slash {
        if let Some(s) = b.strip_suffix('/') {
            b = s.to_string();
        }
    }
    b
}

/// Cannot end with '.'.
pub fn rule07(branch_name: &str, o: &GitBranchNameOptions) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    re(&RE, r"\.+$").replace(branch_name, o.replacement_token.as_str()).into_owned()
}

/// Cannot contain '@{'.
pub fn rule08(branch_name: &str, o: &GitBranchNameOptions) -> String {
    branch_name.replace("@{", &o.replacement_token)
}

/// Cannot be '@'.
pub fn rule09(branch_name: &str, o: &GitBranchNameOptions) -> String {
    if branch_name == "@" { o.replacement_token.clone() } else { branch_name.to_string() }
}

/// Cannot contain '\'.
pub fn rule10(branch_name: &str, o: &GitBranchNameOptions) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    re(&RE, r"\\+").replace_all(branch_name, o.replacement_token.as_str()).into_owned()
}

/// Whether `name` is already a valid branch name.
pub fn is_valid_branch_name(name: &str) -> bool {
    !name.trim().is_empty() && normalise(name, &GitBranchNameOptions::new("_")) == name
}

#[cfg(test)]
mod tests {
    //! Ported from GitCommands.Tests/Git/GitBranchNameNormaliserTest.cs
    use super::*;

    fn o() -> GitBranchNameOptions {
        GitBranchNameOptions::new("_")
    }

    #[test]
    fn normalise_test() {
        let input = "[BUG-1234] some long description located [//test/*/dir?/.lock]{3,} and foo:bar\\can...";
        for (token, expected) in [
            ("_", "_BUG-1234]_some_long_description_located__/test/_/dir_/_lock]{3,}_and_foo_bar_can_"),
            ("-", "-BUG-1234]-some-long-description-located--/test/-/dir-/-lock]{3,}-and-foo-bar-can-"),
            ("", "BUG-1234]somelongdescriptionlocated/test/dir/lock]{3,}andfoobarcan"),
        ] {
            assert_eq!(normalise(input, &GitBranchNameOptions::new(token)), expected, "{token}");
        }
    }

    fn check(rule: fn(&str, &GitBranchNameOptions) -> String, cases: &[(&str, &str)]) {
        for (input, expected) in cases {
            assert_eq!(rule(input, &o()), *expected, "{input}");
        }
    }

    #[test]
    fn rule01_test() {
        check(rule01, &[
            (".test", "_test"),
            ("hierarchy/.test", "hierarchy/_test"),
            ("hierarchy/.test/foo", "hierarchy/_test/foo"),
            (".lock", "_lock"),
            ("pad.lock", "pad_lock"),
            ("pad.lock.lock", "pad.lock_lock"),
            ("hierarchy/sher.lock", "hierarchy/sher_lock"),
            ("hierarchy/sher.lok", "hierarchy/sher.lok"),
            ("hierarchy/sher.lock/foo", "hierarchy/sher_lock/foo"),
            ("hierarchy/sher.lok/foo", "hierarchy/sher.lok/foo"),
        ]);
    }

    #[test]
    fn rule03_test() {
        check(rule03, &[
            ("..test", "_test"),
            ("...test", "_test"),
            ("hierarchy/..test", "hierarchy/_test"),
            ("hierarchy/...test", "hierarchy/_test"),
            ("hierarchy/..test/foo", "hierarchy/_test/foo"),
            ("hierarchy/...test/foo", "hierarchy/_test/foo"),
            ("..lock", "_lock"),
            ("pad..lock", "pad_lock"),
            ("pad...lock", "pad_lock"),
            ("hierarchy/sher..lock", "hierarchy/sher_lock"),
            ("padlock...", "padlock_"),
            ("hierarchy/sher...lock", "hierarchy/sher_lock"),
            ("hierarchy/sher..lock/foo", "hierarchy/sher_lock/foo"),
            ("hierarchy/sher...lock/foo", "hierarchy/sher_lock/foo"),
            ("hierarchy/sher..lok/foo", "hierarchy/sher_lok/foo"),
            ("hier.....archy/sher...lok/fo..o", "hier_archy/sher_lok/fo_o"),
        ]);
    }

    #[test]
    fn rule04_test() {
        check(rule04, &[
            ("test:test", "test_test"),
            ("test test", "test_test"),
            ("test^test", "test_test"),
            ("test~test", "test_test"),
            ("hier archy:sher~lok/fo^o", "hier_archy_sher_lok/fo_o"),
            ("błąd", "błąd"),
            ("привет, ё-маё!", "привет,_ё-маё!"),
            ("Pokémon 195", "Pokémon_195"),
            ("Anhörung`!@#$%", "Anhörung`!@#$%"),
            ("test\"test", "test_test"),
            ("test<test>test", "test_test_test"),
            ("test|test", "test_test"),
        ]);
    }

    #[test]
    fn rule05_test() {
        check(rule05, &[
            ("test?", "test_"),
            ("?test", "_test"),
            ("test???test", "test___test"),
            ("test*", "test_"),
            ("*test", "_test"),
            ("test***test", "test___test"),
            ("test[foo]", "test_foo]"),
            ("[test]", "_test]"),
            ("testing?[*]*test", "testing___]_test"),
        ]);
    }

    #[test]
    fn rule06_test() {
        for (input, allow, expected) in [
            ("test/", false, "test"),
            ("test/", true, "test/"),
            ("/test", false, "test"),
            ("/test/", false, "test"),
            ("/test/", true, "test/"),
            ("/test/test/", false, "test/test"),
            ("/test/test/", true, "test/test/"),
            ("///test///test///", false, "test/test"),
            ("///test///test///", true, "test/test/"),
        ] {
            let opts = GitBranchNameOptions { replacement_token: "_".into(), allow_trailing_slash: allow };
            assert_eq!(rule06(input, &opts), expected, "{input}");
        }
    }

    #[test]
    fn rules_07_to_10() {
        check(rule07, &[("test.", "test_"), ("test..", "test_"), ("test...", "test_")]);
        check(rule08, &[("@{", "_"), ("@{test}", "_test}"), ("test@foo", "test@foo"), ("test@{bla}", "test_bla}")]);
        check(rule09, &[("@", "_")]);
        check(rule10, &[(r"test\foo\\bar\", "test_foo_bar_")]);
    }

    #[test]
    fn valid_names() {
        assert!(is_valid_branch_name("feature/abc-1"));
        assert!(!is_valid_branch_name("bad name"));
        assert!(!is_valid_branch_name(""));
    }
}
