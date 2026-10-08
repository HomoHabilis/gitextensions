//! Port of `GitBlame`, `GitBlameCommit`, `GitBlameLine`, `GitModule.ParseGitBlame` and
//! `GitBlameParser.GetOriginalLineInPreviousCommit`.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use regex::Regex;

use crate::object_id::ObjectId;

/// The commit information of a blamed line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitBlameCommit {
    pub object_id: ObjectId,
    pub author: String,
    pub author_mail: String,
    pub author_time: i64,
    pub author_time_zone: String,
    pub committer: String,
    pub committer_mail: String,
    pub committer_time: i64,
    pub committer_time_zone: String,
    pub summary: String,
    pub file_name: String,
    pub previous: Option<ObjectId>,
}

impl GitBlameCommit {
    /// Tooltip text (port of `GitBlameCommit.ToString`).
    pub fn to_display_string(&self, summary_builder: impl Fn(&str) -> String) -> String {
        let fmt = |t: i64| {
            chrono::DateTime::from_timestamp(t, 0)
                .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_default()
        };
        format!(
            "Author: {}\nAuthor date: {}\nCommitter: {}\nCommit date: {}\nCommit hash: {}\nSummary: {}\n\nFileName: {}",
            self.author,
            fmt(self.author_time),
            self.committer,
            fmt(self.committer_time),
            self.object_id.to_short_string(),
            summary_builder(&self.summary),
            self.file_name
        )
    }
}

/// One line of a blamed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitBlameLine {
    pub commit: Arc<GitBlameCommit>,
    pub final_line_number: u32,
    pub origin_line_number: u32,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitBlame {
    pub lines: Vec<GitBlameLine>,
}

fn header_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?<objectid>[0-9a-f]{40}) (?<origlinenum>\d+) (?<finallinenum>\d+)").unwrap())
}

/// Parses `git blame --porcelain` output.
pub fn parse_git_blame(output: &str) -> GitBlame {
    let mut commits: HashMap<ObjectId, Arc<GitBlameCommit>> = HashMap::new();
    let mut lines = Vec::new();
    let mut object_id = ObjectId::ZERO;
    let mut final_line = 0;
    let mut origin_line = 0;
    let mut has_header = false;
    let mut c = GitBlameCommit {
        object_id: ObjectId::ZERO,
        author: String::new(),
        author_mail: String::new(),
        author_time: 0,
        author_time_zone: String::new(),
        committer: String::new(),
        committer_mail: String::new(),
        committer_time: 0,
        committer_time_zone: String::new(),
        summary: String::new(),
        file_name: String::new(),
        previous: None,
    };
    for line in output.split('\n').map(|l| l.trim_end_matches('\r')) {
        if let Some(m) = header_regex().captures(line) {
            object_id = ObjectId::parse(&m["objectid"]).unwrap_or_default();
            final_line = m["finallinenum"].parse().unwrap_or(0);
            origin_line = m["origlinenum"].parse().unwrap_or(0);
        } else if let Some(text) = line.strip_prefix('\t') {
            if object_id.is_zero() {
                continue;
            }
            let commit = if has_header {
                let entry = commits.entry(object_id).or_insert_with(|| {
                    let mut commit = c.clone();
                    commit.object_id = object_id;
                    Arc::new(commit)
                });
                has_header = false;
                Arc::clone(entry)
            } else {
                match commits.get(&object_id) {
                    Some(c) => Arc::clone(c),
                    None => continue,
                }
            };
            lines.push(GitBlameLine { commit, final_line_number: final_line, origin_line_number: origin_line, text: text.to_string() });
        } else {
            has_header = true;
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "author" => c.author = value.to_string(),
                "author-mail" => c.author_mail = value.trim_matches(['<', '>']).to_string(),
                "author-time" => c.author_time = value.parse().unwrap_or(0),
                "author-tz" => c.author_time_zone = value.to_string(),
                "committer" => c.committer = value.to_string(),
                "committer-mail" => c.committer_mail = value.trim_matches(['<', '>']).to_string(),
                "committer-time" => c.committer_time = value.parse().unwrap_or(0),
                "committer-tz" => c.committer_time_zone = value.to_string(),
                "summary" => c.summary = value.to_string(),
                "filename" => c.file_name = value.to_string(),
                "previous" => c.previous = value.split(' ').next().and_then(ObjectId::try_parse),
                _ => {}
            }
        }
    }
    GitBlame { lines }
}

fn chunk_header_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"@@ -(?<prev>\d+)(?:,(?<removed>\d+))? \+(?<cur>\d+)(?:,(?<added>\d+))? @@").unwrap())
}

/// Given the `-U0` diff introduced by a blamed commit, computes the line in the parent commit
/// that corresponds to `selected_line` (port of `GitBlameParser`).
pub fn original_line_in_previous_commit(diff_output: &str, selected_line: i64) -> i64 {
    let chunks: Vec<&str> = diff_output.split("\n@@").skip(1).collect();
    for chunk in chunks.iter().rev() {
        let text = format!("@@{chunk}");
        let Some(m) = chunk_header_regex().captures(&text) else {
            continue;
        };
        let current: i64 = m["cur"].parse().unwrap_or(0);
        if current <= selected_line {
            let previous: i64 = m["prev"].parse().unwrap_or(0);
            let removed: i64 = m.name("removed").map(|g| g.as_str().parse().unwrap_or(1)).unwrap_or(1);
            let added: i64 = m.name("added").map(|g| g.as_str().parse().unwrap_or(1)).unwrap_or(1);
            return previous.max(selected_line - current + previous - added + removed);
        }
    }
    selected_line
}

#[cfg(test)]
mod tests {
    //! Ported from GitCommands.Tests/GitModuleTests.ParseGitBlame and GitBlameCommitTests.cs
    use super::*;
    use crate::testing::read_test_data;

    #[test]
    fn parse_git_blame_readme() {
        let result = parse_git_blame(&read_test_data("core/README.blame"));
        assert_eq!(result.lines.len(), 80);
        assert_eq!(result.lines[0].commit.object_id, ObjectId::parse("957ff3ce9193fec3bd2578378e71676841804935").unwrap());
        assert_eq!(result.lines[0].text, "# Git Extensions");
        assert_eq!(result.lines[0].origin_line_number, 1);
        assert_eq!(result.lines[0].final_line_number, 1);
        assert!(Arc::ptr_eq(&result.lines[1].commit, &result.lines[0].commit));
        assert!(Arc::ptr_eq(&result.lines[6].commit, &result.lines[0].commit));
        let last = result.lines.last().unwrap();
        assert_eq!(last.commit.object_id, ObjectId::parse("e3268019c66da7534414e9562ececdee5d455b1b").unwrap());
        assert_eq!(last.text, "");
        assert_eq!(result.lines[0].commit.author, "Henk Westhuis");
        assert_eq!(result.lines[0].commit.author_mail, "henk_westhuis@hotmail.com");
        assert_eq!(result.lines[0].commit.summary, "Updated readme.txt");
    }

    #[test]
    fn to_display_string() {
        let commit = GitBlameCommit {
            object_id: ObjectId::random(),
            author: "Author".into(),
            author_mail: "author@authormail.com".into(),
            author_time: 0,
            author_time_zone: "authorTimeZone".into(),
            committer: "committer".into(),
            committer_mail: "committer@authormail.com".into(),
            committer_time: 0,
            committer_time_zone: "committerTimeZone".into(),
            summary: "test summary".into(),
            file_name: "fileName.txt".into(),
            previous: None,
        };
        let s = commit.to_display_string(|s| format!("SOME BUILDER TEXT: {s}"));
        assert!(s.starts_with("Author: Author\nAuthor date: "));
        assert!(s.contains(&format!("Commit hash: {}\n", commit.object_id.to_short_string())));
        assert!(s.contains("Summary: SOME BUILDER TEXT: test summary\n\nFileName: fileName.txt"));
    }

    #[test]
    fn original_line_in_previous_commit_uses_chunk_offsets() {
        let diff = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -3 +3,2 @@\n-x\n+y\n+z\n@@ -10,0 +12,3 @@\n+a\n+b\n+c\n";
        assert_eq!(original_line_in_previous_commit(diff, 1), 1);
        assert_eq!(original_line_in_previous_commit(diff, 4), 3);
        assert_eq!(original_line_in_previous_commit(diff, 20), 15);
    }
}
