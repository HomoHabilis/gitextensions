//! Port of `Patch` and `PatchProcessor`, plus diff line classification and partial
//! patch creation used for staging / unstaging / resetting selected lines
//! (port of the functionality of `PatchManager`).

use std::sync::OnceLock;

use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchChangeType {
    NewFile,
    DeleteFile,
    ChangeFile,
    ChangeFileMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchFileType {
    Binary,
    Text,
}

/// The diff of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    pub header: String,
    pub index: Option<String>,
    pub file_type: PatchFileType,
    pub file_name_a: String,
    pub file_name_b: Option<String>,
    pub change_type: PatchChangeType,
    pub text: String,
}

const ESC: &str = r"\x1b\[[^m]*m";

fn strip_wrapping_escapes() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!(r"^(?:{ESC})?(?<line>.*?)(?:{ESC})?\s*$")).unwrap())
}

fn patch_header_file_name() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"^diff --(?<type>git|cc|combined)\s["]?(?:[^/\s]+/)?(?<filenamea>.*?)["]?(?: ["]?[^/\s]+/(?<filenameb>.*?)["]?)?\s*$"#).unwrap()
    })
}

fn patch_header() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!(r"^(?:{ESC})?diff --(?:git|cc|combined)\s")).unwrap())
}

fn file_name_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?:---|\+\+\+) ["]?[^/\s]+/(?<filename>.*)["]?"#).unwrap())
}

fn strip(line: &str) -> String {
    strip_wrapping_escapes().captures(line).and_then(|c| c.name("line")).map(|m| m.as_str().to_string()).unwrap_or_else(|| line.to_string())
}

/// Removes ANSI escape sequences from a line.
pub fn strip_ansi(line: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(ESC).unwrap()).replace_all(line, "").into_owned()
}

/// Unescapes git's octal escaped file names (`\303\244.txt` → `ä.txt`). A run of escapes with
/// a value above `\377` is kept unchanged (port of `GitModule.UnescapeOctalCodePoints`).
pub fn unescape_octal_code_points(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?:\\[0-7]{3})+").unwrap());
    re.replace_all(s, |c: &regex::Captures| {
        let run = &c[0];
        let mut bytes = Vec::with_capacity(run.len() / 4);
        for code in run.split('\\').filter(|p| !p.is_empty()) {
            match u32::from_str_radix(code, 8) {
                Ok(v) if v <= 255 => bytes.push(v as u8),
                _ => return run.to_string(),
            }
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
    .into_owned()
}

/// Port of `PatchProcessor.CreatePatchesFromString`.
pub fn create_patches_from_string(patch_text: &str) -> Vec<Patch> {
    let lines: Vec<&str> = patch_text.split('\n').collect();
    let mut i = 0;
    while i < lines.len() && !patch_header().is_match(lines[i]) {
        i += 1;
    }
    let mut patches = Vec::new();
    while i < lines.len() {
        if let Some(p) = create_patch(&lines, &mut i) {
            patches.push(p);
        }
        i += 1;
    }
    patches
}

fn create_patch(lines: &[&str], line_index: &mut usize) -> Option<Patch> {
    let header = strip(lines[*line_index]);
    let caps = patch_header_file_name().captures(&header)?;
    let is_combined = &caps["type"] != "git";
    if !is_combined && caps.name("filenameb").is_none() {
        return None;
    }
    let mut file_name_a = caps["filenamea"].trim().to_string();
    let mut file_name_b = if is_combined { None } else { Some(caps["filenameb"].trim().to_string()) };

    let mut text = String::new();
    text.push_str(&header);
    if *line_index < lines.len() - 1 {
        text.push('\n');
    }
    let mut change_type = PatchChangeType::ChangeFile;
    let mut file_type = PatchFileType::Text;
    let mut index = None;
    let mut done = false;
    let mut i = *line_index + 1;

    while i < lines.len() {
        let line = strip(lines[i]);
        if patch_header().is_match(&line) {
            done = true;
            break;
        }
        if line.starts_with("@@") {
            break;
        }
        if line.starts_with("index ") {
            index = Some(line.clone());
        } else if line.starts_with("new file mode ") {
            change_type = PatchChangeType::NewFile;
        } else if line.starts_with("deleted file mode ") {
            change_type = PatchChangeType::DeleteFile;
        } else if line.starts_with("old mode ") {
            change_type = PatchChangeType::ChangeFileMode;
        } else if (line.starts_with("Binary files a/") && line.ends_with(" and /dev/null differ"))
            || (line.starts_with("Binary files /dev/null and b/") && line.ends_with(" differ"))
            || line.starts_with("GIT binary patch")
            || (line.starts_with("Binary files ") && line.ends_with(" differ"))
        {
            file_type = PatchFileType::Binary;
            text.push_str(&line);
            if i < lines.len() - 1 {
                text.push('\n');
            }
            i += 1;
            break;
        }
        if line.starts_with("--- /dev/null") || line.starts_with("+++ /dev/null") {
            // new/deleted file
        } else if line.starts_with("--- ") {
            if let Some(c) = file_name_regex().captures(&unescape_octal_code_points(&line)) {
                file_name_a = c["filename"].trim().trim_end_matches('"').to_string();
            }
        } else if line.starts_with("+++ ") {
            if let Some(c) = file_name_regex().captures(&unescape_octal_code_points(&line)) {
                file_name_b = Some(c["filename"].trim().trim_end_matches('"').to_string());
            }
        }
        text.push_str(&line);
        if i < lines.len() - 1 {
            text.push('\n');
        }
        i += 1;
    }

    while !done && i < lines.len() {
        let line = lines[i];
        if patch_header().is_match(line) {
            break;
        }
        text.push_str(line);
        if i < lines.len() - 1 {
            text.push('\n');
        }
        i += 1;
    }
    *line_index = i - 1;
    Some(Patch { header, index, file_type, file_name_a, file_name_b, change_type, text })
}

/// Classification of a line in a diff view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Header,
    HunkHeader,
    Context,
    Added,
    Removed,
    /// `\ No newline at end of file`
    NoNewline,
    /// Combined diff lines (merge commits) or other
    Other,
}

/// A line of a diff with its line numbers in the old and new file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    /// Index of the hunk this line belongs to.
    pub hunk: Option<usize>,
}

fn parse_hunk_header(line: &str) -> Option<(u32, u32, u32, u32)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@").unwrap());
    let c = re.captures(line)?;
    let n = |i: usize, d: u32| c.get(i).map(|m| m.as_str().parse().unwrap_or(d)).unwrap_or(d);
    Some((n(1, 0), n(2, 1), n(3, 0), n(4, 1)))
}

/// Splits a unified diff (one file) into classified lines with line numbers.
pub fn parse_diff_lines(diff: &str) -> Vec<DiffLine> {
    let mut out = Vec::new();
    let mut old_no = 0u32;
    let mut new_no = 0u32;
    let mut hunk: Option<usize> = None;
    let mut in_body = false;
    for raw in diff.lines() {
        let line = raw.to_string();
        if line.starts_with("diff --") {
            in_body = false;
            out.push(DiffLine { kind: DiffLineKind::Header, text: line, old_line: None, new_line: None, hunk: None });
            continue;
        }
        if let Some((o, _, n, _)) = parse_hunk_header(&line) {
            old_no = o;
            new_no = n;
            hunk = Some(hunk.map_or(0, |h| h + 1));
            in_body = true;
            out.push(DiffLine { kind: DiffLineKind::HunkHeader, text: line, old_line: None, new_line: None, hunk });
            continue;
        }
        if line.starts_with("@@@") {
            in_body = true;
            hunk = Some(hunk.map_or(0, |h| h + 1));
            out.push(DiffLine { kind: DiffLineKind::HunkHeader, text: line, old_line: None, new_line: None, hunk });
            continue;
        }
        if !in_body {
            out.push(DiffLine { kind: DiffLineKind::Header, text: line, old_line: None, new_line: None, hunk: None });
            continue;
        }
        let (kind, ol, nl) = match line.chars().next() {
            Some('+') => {
                new_no += 1;
                (DiffLineKind::Added, None, Some(new_no - 1))
            }
            Some('-') => {
                old_no += 1;
                (DiffLineKind::Removed, Some(old_no - 1), None)
            }
            Some(' ') | None => {
                old_no += 1;
                new_no += 1;
                (DiffLineKind::Context, Some(old_no - 1), Some(new_no - 1))
            }
            Some('\\') => (DiffLineKind::NoNewline, None, None),
            _ => (DiffLineKind::Other, None, None),
        };
        out.push(DiffLine { kind, text: line, old_line: ol, new_line: nl, hunk });
    }
    out
}

/// Creates a patch containing only the selected lines of `diff` (a single file diff as output by
/// `git diff`), suitable for `git apply --cached` (stage, `reverse == false`) or
/// `git apply --cached --reverse` (unstage / reset, `reverse == true`).
///
/// `selected` contains indexes into the lines of `diff` (as returned by [`parse_diff_lines`]).
/// Returns `None` if no change is selected.
pub fn create_partial_patch(diff: &str, selected: &[usize], reverse: bool) -> Option<String> {
    let lines = parse_diff_lines(diff);
    let mut header = String::new();
    let mut body = String::new();
    let mut any = false;
    let mut i = 0;
    while i < lines.len() && lines[i].kind == DiffLineKind::Header {
        header.push_str(&lines[i].text);
        header.push('\n');
        i += 1;
    }
    let mut offset: i64 = 0;
    while i < lines.len() {
        if lines[i].kind != DiffLineKind::HunkHeader {
            i += 1;
            continue;
        }
        let (old_start, _, new_start, _) = parse_hunk_header(&lines[i].text).unwrap_or((1, 0, 1, 0));
        i += 1;
        let mut hunk_text = String::new();
        let mut old_count = 0i64;
        let mut new_count = 0i64;
        let mut hunk_has_change = false;
        let mut last_kept_was_dropped_add = false;
        while i < lines.len() && !matches!(lines[i].kind, DiffLineKind::HunkHeader | DiffLineKind::Header) {
            let l = &lines[i];
            let is_selected = selected.contains(&i);
            let content = l.text.get(1..).unwrap_or_default();
            match l.kind {
                DiffLineKind::Context => {
                    hunk_text.push_str(&l.text);
                    hunk_text.push('\n');
                    old_count += 1;
                    new_count += 1;
                    last_kept_was_dropped_add = false;
                }
                DiffLineKind::Added | DiffLineKind::Removed => {
                    // In "reverse" mode the patch is applied in reverse: the roles of + and - swap
                    // for unselected lines.
                    let is_add = l.kind == DiffLineKind::Added;
                    if is_selected {
                        hunk_text.push_str(&l.text);
                        hunk_text.push('\n');
                        if is_add {
                            new_count += 1;
                        } else {
                            old_count += 1;
                        }
                        hunk_has_change = true;
                        last_kept_was_dropped_add = false;
                    } else if is_add == reverse {
                        // keep as context
                        hunk_text.push(' ');
                        hunk_text.push_str(content);
                        hunk_text.push('\n');
                        old_count += 1;
                        new_count += 1;
                        last_kept_was_dropped_add = false;
                    } else {
                        // drop the line
                        last_kept_was_dropped_add = true;
                    }
                }
                DiffLineKind::NoNewline => {
                    if !last_kept_was_dropped_add {
                        hunk_text.push_str(&l.text);
                        hunk_text.push('\n');
                    }
                }
                _ => {}
            }
            i += 1;
        }
        if hunk_has_change {
            any = true;
            let (os, ns) = if reverse {
                (new_start as i64 - offset, new_start as i64)
            } else {
                (old_start as i64, old_start as i64 + offset)
            };
            body.push_str(&format!("@@ -{},{} +{},{} @@\n", os.max(0), old_count, ns.max(0), new_count));
            body.push_str(&hunk_text);
            offset += new_count - old_count;
        }
    }
    any.then(|| format!("{header}{body}"))
}

/// Creates the patch for a single hunk (stage/unstage hunk).
pub fn create_hunk_patch(diff: &str, hunk: usize, reverse: bool) -> Option<String> {
    let lines = parse_diff_lines(diff);
    let selected: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.hunk == Some(hunk) && matches!(l.kind, DiffLineKind::Added | DiffLineKind::Removed))
        .map(|(i, _)| i)
        .collect();
    create_partial_patch(diff, &selected, reverse)
}

#[cfg(test)]
mod tests {
    //! Ported from GitCommands.Tests/Patches/PatchProcessorTest.cs
    use super::*;
    use crate::testing::test_data_dir;

    fn load(name: &str) -> String {
        let bytes = std::fs::read(test_data_dir().join("core/patches").join(name)).unwrap();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn small_patch(reverse: bool) -> (String, Patch) {
        let header =
            if reverse { "diff --git a/thisisatestb.txt b/thisisatesta.txt" } else { "diff --git b/thisisatesta.txt a/thisisatestb.txt" };
        let index = "index 5e4dce2..5eb1e6f 100644";
        let mut text = format!("{header}\n{index}\n");
        if reverse {
            text.push_str("--- b/thisisatestb.txt\n+++ a/thisisatesta.txt\n");
        } else {
            text.push_str("--- a/thisisatesta.txt\n+++ b/thisisatestb.txt\n");
        }
        text.push_str("@@ -1,2 +1,2 @@\n iiiiii\n-ąśdkjaldskjlaksd\n+changed again€\n");
        let p = Patch {
            header: header.into(),
            index: Some(index.into()),
            file_type: PatchFileType::Text,
            file_name_a: "thisisatesta.txt".into(),
            file_name_b: Some("thisisatestb.txt".into()),
            change_type: PatchChangeType::ChangeFile,
            text: text.clone(),
        };
        (text, p)
    }

    #[test]
    fn test_correctly_load_patch() {
        let (output, expected) = small_patch(false);
        let p = &create_patches_from_string(&output)[0];
        assert_eq!(p.header, expected.header);
        assert_eq!(p.file_name_a, expected.file_name_a);
        assert_eq!(p.index, expected.index);
        assert_eq!(p.change_type, expected.change_type);
        assert_eq!(p.text, expected.text);
    }

    #[test]
    fn test_correctly_load_reverse_patch() {
        let (output, expected) = small_patch(true);
        let p = &create_patches_from_string(&output)[0];
        assert_eq!(p.header, expected.header);
        assert_eq!(Some(p.file_name_a.clone()), expected.file_name_b);
        assert_eq!(p.index, expected.index);
        assert_eq!(p.text, expected.text);
    }

    #[test]
    fn test_big_patch() {
        let patches = create_patches_from_string(&load("big.patch"));
        assert_eq!(patches.len(), 17);
        let a: std::collections::HashSet<_> = patches.iter().map(|p| p.file_name_a.clone()).collect();
        let b: std::collections::HashSet<_> = patches.iter().map(|p| p.file_name_b.clone()).collect();
        assert_eq!(a.len(), 17);
        assert_eq!(b.len(), 17);
        assert_eq!(patches.iter().filter(|p| p.change_type == PatchChangeType::NewFile).count(), 1);
        assert_eq!(patches.iter().filter(|p| p.change_type == PatchChangeType::DeleteFile).count(), 1);
        assert_eq!(patches.iter().filter(|p| p.change_type == PatchChangeType::ChangeFile).count(), 15);
        let small = create_patches_from_string(&small_patch(false).0);
        assert_eq!(small.iter().filter(|p| p.change_type == PatchChangeType::ChangeFile).count(), 1);
    }

    #[test]
    fn test_correctly_loads_binary_patch() {
        let patches = create_patches_from_string(&load("bigBin.patch"));
        assert_eq!(patches.iter().filter(|p| p.file_type == PatchFileType::Binary).count(), 248);
    }

    #[test]
    fn test_correctly_loads_rebase_diff() {
        assert_eq!(create_patches_from_string(&load("rebase.diff")).len(), 13);
    }

    #[test]
    fn test_combined_diff() {
        let diff = "diff --cc GitCommands/Patches/PatchProcessor.cs\nindex ec3da25f4,5acc3b45b..000000000\n--- a/GitCommands/Patches/PatchProcessor.cs\n+++ b/GitCommands/Patches/PatchProcessor.cs\ndiff --combined UnitTests/GitCommandsTests/Patches/PatchProcessorTest.cs\nindex cdf8bebba,55ff37bb9..000000000\n--- a/UnitTests/GitCommandsTests/Patches/PatchProcessorTest.cs\n+++ b/UnitTests/GitCommandsTests/Patches/PatchProcessorTest.cs\n";
        assert_eq!(create_patches_from_string(diff).len(), 2);
    }

    #[test]
    fn color_diff() {
        let patches = create_patches_from_string(&load("color.diff"));
        assert_eq!(patches.len(), 1);
        let p = &patches[0];
        assert_eq!(p.header, "diff --git a/GitCommands/Patches/PatchProcessor.cs b/GitCommands/Patches/PatchProcessor.cs");
        assert_eq!(p.file_name_a, "GitCommands/Patches/PatchProcessor.cs");
        assert_eq!(p.file_name_b.as_deref(), Some("GitCommands/Patches/PatchProcessor.cs"));
        assert_eq!(p.index.as_deref(), Some("index 70b40..c1e6c 100644"));
        assert_eq!(p.change_type, PatchChangeType::ChangeFile);
        assert_eq!(p.file_type, PatchFileType::Text);
    }

    #[test]
    fn color_prefix_diff() {
        for (src, dst) in [("before:/", "after:/"), ("a:./", "b:./"), ("./", "./")] {
            let diff = load("color-prefix.diff").replace("[PLACEHOLDER_PREFIX_SRC]", src).replace("[PLACEHOLDER_PREFIX_DST]", dst);
            let patches = create_patches_from_string(&diff);
            assert_eq!(patches.len(), 1);
            let p = &patches[0];
            assert_eq!(p.header, format!("diff --git {src}GitCommands/Patches/PatchProcessor.cs {dst}GitCommands/Patches/PatchProcessor.cs"));
            assert_eq!(p.file_name_a, "GitCommands/Patches/PatchProcessor.cs");
            assert_eq!(p.file_name_b.as_deref(), Some("GitCommands/Patches/PatchProcessor.cs"));
            assert_eq!(p.index.as_deref(), Some("index 70b40..c1e6c 100644"));
        }
    }

    #[test]
    fn color_bin_diff() {
        let patches = create_patches_from_string(&load("color-binary.diff"));
        assert_eq!(patches.len(), 1);
        let p = &patches[0];
        assert_eq!(p.header, "diff --git a/syscolor 3 gray.7z b/syscolor 3 gray.7z");
        assert_eq!(p.file_name_a, "syscolor 3 gray.7z");
        assert_eq!(p.file_name_b.as_deref(), Some("syscolor 3 gray.7z"));
        assert_eq!(p.index.as_deref(), Some("index 33b006c1..00000000"));
        assert_eq!(p.change_type, PatchChangeType::DeleteFile);
        assert_eq!(p.file_type, PatchFileType::Binary);
    }

    #[test]
    fn create_patch_from_string_with_spaces() {
        let text = "diff --git a/sub modules/test submodule b/sub modules/test submodule\n--- a/sub modules/test submodule    \n+++ b/sub modules/test submodule    \n@@ -1 +1 @@\n-Subproject commit 4c54fbefd8032acb59aa33ade3fb4bdff32bdde7\n+Subproject commit 4c54fbefd8032acb59aa33ade3fb4bdff32bdde7-dirty";
        let p = &create_patches_from_string(text)[0];
        assert_eq!(p.file_name_a, "sub modules/test submodule");
        assert_eq!(p.file_name_b.as_deref(), Some("sub modules/test submodule"));
        assert_eq!(p.change_type, PatchChangeType::ChangeFile);
        assert_eq!(
            p.text,
            "diff --git a/sub modules/test submodule b/sub modules/test submodule\n--- a/sub modules/test submodule\n+++ b/sub modules/test submodule\n@@ -1 +1 @@\n-Subproject commit 4c54fbefd8032acb59aa33ade3fb4bdff32bdde7\n+Subproject commit 4c54fbefd8032acb59aa33ade3fb4bdff32bdde7-dirty"
        );
    }

    #[test]
    fn create_patch_from_string_text_change_file() {
        let e = '\u{1b}';
        let text = format!("From 42a3043eafe08409c55b48c36661cf6cf3055c68 Mon Sep 17 00:00:00 2001\nFrom: Some One <else@mail.net>\nDate: Mon, 2 Sep 2024 17:42:00 +0200\nSubject: Patch for test\n\n---\n{e}[1mdiff --git a/old.txt b/new.txt{e}[m\n{e}[1mindex cb36533..5550a88 100644{e}[m\n{e}[1m--- a/old.txt{e}[m\n{e}[1m+++ b/new.txt{e}[m\n{e}[7;37m@@ -1 +1 @@{e}[m\n{e}[7;31m-И ПОКА{e}[m\n{e}[7;32m+{e}[m{e}[7;32mA ПОКА{e}[m");
        let patches = create_patches_from_string(&text);
        assert_eq!(patches.len(), 1);
        let p = &patches[0];
        assert_eq!(p.header, "diff --git a/old.txt b/new.txt");
        assert_eq!(p.index.as_deref(), Some("index cb36533..5550a88 100644"));
        assert_eq!(p.file_name_a, "old.txt");
        assert_eq!(p.file_name_b.as_deref(), Some("new.txt"));
        assert_eq!(
            p.text,
            format!("diff --git a/old.txt b/new.txt\nindex cb36533..5550a88 100644\n--- a/old.txt\n+++ b/new.txt\n{e}[7;37m@@ -1 +1 @@{e}[m\n{e}[7;31m-И ПОКА{e}[m\n{e}[7;32m+{e}[m{e}[7;32mA ПОКА{e}[m")
        );
    }

    const DIFF: &str = "diff --git a/f.txt b/f.txt\nindex 1..2 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -1,4 +1,4 @@\n a\n-b\n+B\n c\n-d\n+D\n";

    #[test]
    fn parse_diff_lines_numbers() {
        let lines = parse_diff_lines(DIFF);
        assert_eq!(lines[4].kind, DiffLineKind::HunkHeader);
        assert_eq!((lines[5].old_line, lines[5].new_line), (Some(1), Some(1)));
        assert_eq!((lines[6].kind, lines[6].old_line), (DiffLineKind::Removed, Some(2)));
        assert_eq!((lines[7].kind, lines[7].new_line), (DiffLineKind::Added, Some(2)));
        assert_eq!((lines[9].kind, lines[9].old_line), (DiffLineKind::Removed, Some(4)));
    }

    #[test]
    fn partial_patch_stage_selected_lines() {
        // stage only the b -> B change
        let p = create_partial_patch(DIFF, &[6, 7], false).unwrap();
        assert_eq!(p, "diff --git a/f.txt b/f.txt\nindex 1..2 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -1,4 +1,4 @@\n a\n-b\n+B\n c\n d\n");
        // unstage only the d -> D change (reverse): unselected '+' kept as context, '-' dropped
        let p = create_partial_patch(DIFF, &[9, 10], true).unwrap();
        assert_eq!(p, "diff --git a/f.txt b/f.txt\nindex 1..2 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -1,4 +1,4 @@\n a\n B\n c\n-d\n+D\n");
        assert!(create_partial_patch(DIFF, &[5], false).is_none());
        assert!(create_hunk_patch(DIFF, 0, false).is_some());
    }

    #[test]
    fn unescape_octal_code_points_handles_octal_codes() {
        // Ported from GitModuleTests.UnescapeOctalCodePoints_handles_octal_codes
        for (input, expected) in [
            ("", ""),
            (" ", " "),
            ("Hello, World!", "Hello, World!"),
            (r"\353\221\220\353\213\244.txt", "두다.txt"),
            (r"Invalid byte \777.txt", r"Invalid byte \777.txt"),
            (r"\353\221\220\353\213\244 \777.txt", r"두다 \777.txt"),
            (r"\353\221\220\353\213\244\777.txt", r"\353\221\220\353\213\244\777.txt"),
        ] {
            assert_eq!(unescape_octal_code_points(input), expected, "{input}");
        }
    }
}
