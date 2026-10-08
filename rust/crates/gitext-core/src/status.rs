//! Port of `GitItemStatus`, `GitItemStatusConverter` and `GetAllChangedFilesOutputParser`.

use crate::object_id::ObjectId;

/// Whether a change is in the work tree, the index or a commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StagedStatus {
    #[default]
    Unset = 0,
    None = 1,
    WorkTree = 2,
    Index = 3,
    Unknown = 4,
}

pub const ADDED_STATUS: char = 'A';
pub const COPIED_STATUS: char = 'C';
pub const DELETED_STATUS: char = 'D';
pub const MODIFIED_STATUS: char = 'M';
pub const RENAMED_STATUS: char = 'R';
pub const TYPE_CHANGED_STATUS: char = 'T';
pub const UNMERGED_STATUS: char = 'U';
pub const UNMODIFIED_STATUS_V1: char = ' ';
pub const UNMODIFIED_STATUS_V2: char = '.';
pub const IGNORED_STATUS: char = '!';
pub const UNTRACKED_STATUS: char = '?';
pub const UNUSED_CHARACTER: char = '&';

/// A changed file (work tree, index, or between revisions).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitItemStatus {
    pub name: String,
    pub old_name: Option<String>,
    pub tree_id: ObjectId,
    pub rename_copy_percentage: Option<String>,
    pub staged: StagedStatus,
    pub error_message: Option<String>,
    pub is_tracked: bool,
    pub is_deleted: bool,
    pub is_changed: bool,
    pub is_new: bool,
    pub is_ignored: bool,
    pub is_renamed: bool,
    pub is_copied: bool,
    pub is_unmerged: bool,
    pub is_assume_unchanged: bool,
    pub is_skip_worktree: bool,
    pub is_submodule: bool,
    pub is_dirty: bool,
    pub is_status_only: bool,
    pub is_range_diff: bool,
}

impl GitItemStatus {
    pub fn new(name: impl Into<String>) -> Self {
        GitItemStatus { name: name.into(), ..Default::default() }
    }

    /// Port of `GitItemStatusConverter.FromStatusCharacter`.
    pub fn from_status_character(staged: StagedStatus, file_name: impl Into<String>, x: char) -> Self {
        let is_new = matches!(x, ADDED_STATUS | UNTRACKED_STATUS | IGNORED_STATUS);
        GitItemStatus {
            name: file_name.into(),
            is_new,
            is_changed: matches!(x, MODIFIED_STATUS | TYPE_CHANGED_STATUS),
            is_deleted: x == DELETED_STATUS,
            is_renamed: x == RENAMED_STATUS,
            is_copied: x == COPIED_STATUS,
            is_tracked: !matches!(x, UNTRACKED_STATUS | IGNORED_STATUS | UNMODIFIED_STATUS_V1) || !is_new,
            is_ignored: x == IGNORED_STATUS,
            is_unmerged: x == UNMERGED_STATUS,
            staged,
            ..Default::default()
        }
    }

    pub fn default_status(name: &str) -> Self {
        Self::from_status_character(StagedStatus::WorkTree, name, UNUSED_CHARACTER)
    }

    /// Directory part of [`Self::name`] (port of `Path`).
    pub fn path(&self) -> &str {
        let name = &self.name;
        if name.is_empty() {
            return "";
        }
        let search = if name.ends_with('/') { &name[..name.len() - 1] } else { name.as_str() };
        match search.rfind('/') {
            Some(i) if i >= 1 => &name[..i],
            _ => "",
        }
    }

    /// File name without directory.
    pub fn file_name(&self) -> &str {
        let trimmed = self.name.trim_end_matches('/');
        trimmed.rsplit('/').next().unwrap_or(trimmed)
    }

    pub fn is_added(&self) -> bool {
        self.is_new || self.is_copied
    }

    pub fn is_uncommitted(&self) -> bool {
        matches!(self.staged, StagedStatus::WorkTree | StagedStatus::Index)
    }

    pub fn is_uncommitted_added(&self) -> bool {
        self.is_uncommitted() && self.is_added()
    }

    /// Port of `InvertStatus`.
    pub fn invert_status(&self) -> Self {
        let mut s = self.clone();
        if self.is_renamed {
            s.name = self.old_name.clone().unwrap_or_default();
            s.old_name = Some(self.name.clone());
        }
        s.is_new = self.is_deleted;
        s.is_deleted = self.is_new;
        s
    }

    /// A one letter status for lists (like `git status --short`).
    pub fn status_char(&self) -> char {
        if self.is_unmerged {
            'U'
        } else if self.is_renamed {
            'R'
        } else if self.is_copied {
            'C'
        } else if self.is_deleted {
            'D'
        } else if self.is_ignored {
            '!'
        } else if self.is_new && !self.is_tracked {
            '?'
        } else if self.is_new {
            'A'
        } else if self.is_submodule && self.is_dirty {
            'S'
        } else {
            'M'
        }
    }

    /// Port of `ToString`.
    pub fn description(&self) -> String {
        let mut s = String::new();
        if let Some(e) = self.error_message.as_deref().filter(|e| !e.trim().is_empty()) {
            s.push_str(e);
        }
        if self.is_renamed {
            s.push_str(&format!("Renamed\n   {}\n to\n   {}", self.old_name.as_deref().unwrap_or_default(), self.name));
        } else if self.is_copied {
            s.push_str(&format!("Copied\n   {}\n to\n   {}", self.old_name.as_deref().unwrap_or_default(), self.name));
        } else {
            s.push_str(&self.name);
        }
        if self.is_unmerged {
            s.push_str(" (Unmerged)");
        }
        if !matches!(self.staged, StagedStatus::None | StagedStatus::Unset) {
            s.push_str(&format!(" {:?}", self.staged));
        }
        if let Some(p) = self.rename_copy_percentage.as_deref().filter(|p| !p.is_empty()) {
            s.push_str(&format!("\nSimilarity {p}%"));
        }
        s
    }
}

/// Parses `git status --porcelain=2 -z` output.
pub fn parse_status_v2(output: &str) -> Vec<GitItemStatus> {
    let mut diff_files = Vec::new();
    let files: Vec<&str> = output.split('\0').filter(|f| !f.is_empty()).collect();
    let mut n = 0;
    while n < files.len() {
        let line = files[n];
        let chars: Vec<char> = line.chars().collect();
        n += 1;
        if chars.len() <= 2 || chars[1] != ' ' || chars[0] == '#' {
            continue;
        }
        let entry_type = chars[0];
        let mut update = |x: char, is_index: bool, subm: &[char], file_name: &str, old: Option<&str>, percent: Option<&str>| {
            if x == UNMODIFIED_STATUS_V2 {
                return;
            }
            let staged = if is_index { StagedStatus::Index } else { StagedStatus::WorkTree };
            let mut status = GitItemStatus::from_status_character(staged, file_name, x);
            if let Some(old) = old {
                status.old_name = Some(old.to_string());
            }
            if let Some(p) = percent {
                status.rename_copy_percentage = Some(p.to_string());
            }
            if subm[0] == 'S' {
                status.is_submodule = true;
                if !is_index {
                    status.is_changed = subm[1] == COPIED_STATUS;
                    status.is_dirty = subm[2] == MODIFIED_STATUS || subm[3] == UNMERGED_STATUS;
                }
            }
            diff_files.push(status);
        };

        if entry_type == UNTRACKED_STATUS || entry_type == IGNORED_STATUS {
            let name: String = chars[2..].iter().collect();
            update(entry_type, false, &['N', '.', '.', '.'], &name, None, None);
            continue;
        }
        if !matches!(entry_type, '1' | '2' | 'u') || chars.len() <= 3 {
            continue;
        }
        let x = chars[2];
        let y = chars[3];
        if chars.len() < 9 {
            continue;
        }
        let subm: Vec<char> = chars[5..9].to_vec();
        let (file_name, old_file_name, rename_percent): (String, Option<String>, Option<String>) = match entry_type {
            '1' => {
                if chars.len() <= 113 {
                    continue;
                }
                (chars[113..].iter().collect(), None, None)
            }
            '2' => {
                if chars.len() <= 114 || n >= files.len() {
                    continue;
                }
                let rest = &chars[114..];
                let Some(pos) = rest.iter().position(|c| !c.is_ascii_digit()) else {
                    continue;
                };
                let percent: String = rest[..pos].iter().collect();
                let name: String = rest[pos + 1..].iter().collect();
                let old = files[n].to_string();
                n += 1;
                (name, Some(old), Some(percent))
            }
            _ => {
                if chars.len() <= 161 {
                    continue;
                }
                (chars[161..].iter().collect(), None, None)
            }
        };
        if entry_type != 'u' || x != UNMERGED_STATUS || y != UNMERGED_STATUS {
            update(x, true, &subm, &file_name, old_file_name.as_deref(), rename_percent.as_deref());
        }
        update(y, false, &subm, &file_name, old_file_name.as_deref(), rename_percent.as_deref());
    }
    diff_files
}

/// Parses `git diff -z --raw` output (port of `GetDiffChangedFilesFromString`).
pub fn parse_diff_raw(output: &str, staged: StagedStatus) -> Vec<GitItemStatus> {
    let mut diff_files = Vec::new();
    let files: Vec<&str> = output.split('\0').filter(|f| !f.is_empty()).collect();
    let mut n = 0;
    while n < files.len() {
        if n >= files.len() - 1 {
            break;
        }
        let status = files[n];
        n += 1;
        let file_name = files[n];
        n += 1;
        if !status.starts_with(':') || status.len() < 15 {
            continue;
        }
        let Some(status_index) = status.rfind(|c: char| !c.is_ascii_digit()) else {
            continue;
        };
        let x = status[status_index..].chars().next().unwrap();
        if staged == StagedStatus::WorkTree && x == UNMERGED_STATUS {
            continue;
        }
        let mut item = GitItemStatus::from_status_character(staged, file_name, x);
        if x == RENAMED_STATUS || x == COPIED_STATUS {
            item.rename_copy_percentage = Some(status[status_index + 1..].to_string());
            item.old_name = Some(item.name.clone());
            if n < files.len() {
                item.name = files[n].to_string();
                n += 1;
            }
        }
        const SUB_MODE: &str = "160000";
        if status.get(1..7) == Some(SUB_MODE) || status.get(8..14) == Some(SUB_MODE) {
            item.is_submodule = true;
        }
        diff_files.push(item);
    }
    diff_files
}

#[cfg(test)]
mod tests {
    //! Ported from GitCommands.Tests/Git/GetAllChangedFilesOutputParserTest.cs (verified JSON).
    use super::*;
    use crate::testing::test_data_dir;

    fn to_json(s: &GitItemStatus) -> serde_json::Value {
        serde_json::json!({
            "Name": s.name,
            "OldName": s.old_name,
            "Path": { "Value": s.path() },
            "RenameCopyPercentage": s.rename_copy_percentage,
            "Staged": s.staged as i32,
            "IsTracked": s.is_tracked,
            "IsDeleted": s.is_deleted,
            "IsChanged": s.is_changed,
            "IsNew": s.is_new,
            "IsIgnored": s.is_ignored,
            "IsRenamed": s.is_renamed,
            "IsCopied": s.is_copied,
            "IsUnmerged": s.is_unmerged,
            "IsSubmodule": s.is_submodule,
            "IsDirty": s.is_dirty,
            "IsAdded": s.is_added(),
            "IsUncommitted": s.is_uncommitted(),
            "IsUncommittedAdded": s.is_uncommitted_added(),
        })
    }

    fn verify_json(file: &str, actual: &[GitItemStatus]) {
        let text = std::fs::read_to_string(test_data_dir().join("core/status").join(file)).unwrap();
        let expected: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap();
        let expected: Vec<serde_json::Value> = match expected {
            serde_json::Value::Array(a) => a,
            v => vec![v],
        };
        assert_eq!(expected.len(), actual.len(), "{file}");
        for (e, a) in expected.iter().zip(actual) {
            let a = to_json(a);
            for (k, v) in a.as_object().unwrap() {
                if k == "Path" {
                    assert_eq!(&e["Path"]["Value"], &v["Value"], "{file} {k}");
                } else {
                    assert_eq!(&e[k], v, "{file} {k} of {}", a["Name"]);
                }
            }
        }
    }

    const STATUS_CASES: [(&str, &str); 12] = [
        ("status_modified_files", "#Header\u{0}3 unknown info\u{0}1 .M S..U 160000 160000 160000 cbca134e29be13b35f21ca4553ba04f796324b1c cbca134e29be13b35f21ca4553ba04f796324b1c subm1\u{0}1 .M SCM. 160000 160000 160000 6bd3b036fc5718a51a0d27cde134c7019798c3ce 6bd3b036fc5718a51a0d27cde134c7019798c3ce subm2\u{0}\r\nwarning: LF will be replaced by CRLF in adfs.h.\nThe file will have its original line endings in your working directory.\nwarning: LF will be replaced by CRLF in dir.c.\nThe file will have its original line endings in your working directory."),
        ("status_ignored_files", "1 .M N... 160000 160000 160000 cbca134e29be13b35f21ca4553ba04f796324b1c cbca134e29be13b35f21ca4553ba04f796324b1c adfs.h\u{0}? untracked_file\u{0}"),
        ("status_staged_files", "1 M. N... 160000 160000 160000 cbca134e29be13b35f21ca4553ba04f796324b1c cbca134e29be13b35f21ca4553ba04f796324b1c adfs.h\u{0}1 MM N... 160000 160000 160000 cbca134e29be13b35f21ca4553ba04f796324b1c cbca134e29be13b35f21ca4553ba04f796324b1c adfs.h\u{0}"),
        ("status_untracked_files", "1 .M S... 160000 160000 160000 cbca134e29be13b35f21ca4553ba04f796324b1c cbca134e29be13b35f21ca4553ba04f796324b1c subm1\u{0}! ignored_file\u{0}"),
        ("status_with_spaces", "#Header\u{0}3 unknown info\u{0}1 .M N... 160000 160000 160000 cbca134e29be13b35f21ca4553ba04f796324b1c cbca134e29be13b35f21ca4553ba04f796324b1c  no trim space0 \u{0}1 .M SCM. 160000 160000 160000 6bd3b036fc5718a51a0d27cde134c7019798c3ce 6bd3b036fc5718a51a0d27cde134c7019798c3ce  no trim space1 \u{0}\r\nwarning: LF will be replaced by CRLF in adfs.h.\nThe file will have its original line endings in your working directory.\nwarning: LF will be replaced by CRLF in dir.c.\nThe file will have its original line endings in your working directory."),
        ("both_modified", "u UU N... 100644 100644 100644 100644 628b3ed138239cbf0cfd993deab272ef606ac34a bf42fd75847fdc76d4f4a2b89d92a1b289b2e4ea f7f9ab7c975ab68e9bd8bda93b8bb717987947ea GitUI/CommandsDialogs/FormFormatPatch.Designer.cs"),
        ("both_added", "u AA N... 000000 100644 100644 100644 0000000000000000000000000000000000000000 61780798228d17af2d34fce4cfbdf35556832472 78981922613b2afb6025042ff6bd878ac1994e85 t.t"),
        ("both_deleted", "u DD N... 100644 000000 000000 000000 3c70853f1ed9b82635f0763dd1373c3101e79d5d 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 GitUI/CommandsDialogs/FormFormatPatch.cs"),
        ("added_by_us", "u AU N... 000000 100644 000000 100644 0000000000000000000000000000000000000000 3c70853f1ed9b82635f0763dd1373c3101e79d5d 0000000000000000000000000000000000000000 a2"),
        ("added_by_them", "u UA N... 000000 000000 100644 100644 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 3c70853f1ed9b82635f0763dd1373c3101e79d5d b2"),
        ("deleted_by_them", "u UD N... 100644 100644 000000 100644 f7f9ab7c975ab68e9bd8bda93b8bb717987947ea bf42fd75847fdc76d4f4a2b89d92a1b289b2e4ea 0000000000000000000000000000000000000000 GitUI/CommandsDialogs/FormFormatPatch.Designer.cs"),
        ("deleted_by_us", "u DU N... 100644 000000 100644 100644 dc7a0a8364df7cb022b3e29a48ecd84992af4613 0000000000000000000000000000000000000000 bf42fd75847fdc76d4f4a2b89d92a1b289b2e4ea GitUI/CommandsDialogs/FormFormatPatch.Designer.cs"),
    ];

    #[test]
    fn test_get_status_changed_files_from_string() {
        for (name, input) in STATUS_CASES {
            verify_json(&format!("GetAllChangedFilesOutputParserTest.TestGetStatusChangedFilesFromString_testName={name}.verified.json"), &parse_status_v2(input));
        }
    }

    #[test]
    fn test_get_default_status() {
        verify_json("GetAllChangedFilesOutputParserTest.TestGetDefaultStatus.verified.json", &[GitItemStatus::default_status("filename.txt")]);
    }

    #[test]
    fn get_diff_changed_files_from_string() {
        const R: &str = ":100644 100644 96b438fc ffe29e27 ";
        let cases = [
            ("Ignore_unmerged_in_conflict_if_revision_is_work_tree", StagedStatus::WorkTree, format!("{R}M\0testfile.txt\0{R}U\0testfile.txt\0")),
            ("Include_unmerged_in_conflict_if_revision_is_index", StagedStatus::Index, format!("{R}M\0testfile.txt\0{R}U\0testfile2.txt\0")),
            ("Check_that_the_staged_status_is_None_if_not_IndexWorkTree1", StagedStatus::None, format!("{R}M\0testfile.txt\0{R}U\0testfile2.txt\0")),
            ("Check_that_the_staged_status_is_None_if_not_IndexWorkTree2", StagedStatus::None, format!("{R}M\0testfile.txt\0{R}U\0testfile2.txt\0")),
            (
                "Check_that_spaces_are_not_trimmed_in_file_names",
                StagedStatus::None,
                format!("{R}M\0 no trim space0 \0{R}U\0 no trim space1 \0{R}A\0 no trim space2 \0"),
            ),
            (
                "Rename_with_spaces",
                StagedStatus::None,
                format!("{R}R100\0CONTRIBUTING.md\0 CONTRIBUTI NG.md\0{R}C70\0apa.md\0 apa.md\0{R}A\0 co decov.yml\0{R}D\0CODE_OF_CONDUCT.md\0"),
            ),
        ];
        for (name, staged, input) in cases {
            verify_json(&format!("GetAllChangedFilesOutputParserTest.GetDiffChangedFilesFromString_testName={name}.verified.json"), &parse_diff_raw(&input, staged));
        }
    }

    #[test]
    fn path_and_invert() {
        let s = GitItemStatus::new("a/b/c.txt");
        assert_eq!(s.path(), "a/b");
        assert_eq!(s.file_name(), "c.txt");
        assert_eq!(GitItemStatus::new("dir/").path(), "");
        let mut r = GitItemStatus::new("new");
        r.is_renamed = true;
        r.old_name = Some("old".into());
        r.is_new = true;
        let i = r.invert_status();
        assert_eq!(i.name, "old");
        assert_eq!(i.old_name.as_deref(), Some("new"));
        assert!(i.is_deleted && !i.is_new);
    }
}
