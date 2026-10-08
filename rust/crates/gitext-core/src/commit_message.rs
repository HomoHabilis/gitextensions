//! Port of `CommitMessageManager`: persists the commit dialog message and amend state in
//! the repository's git directory.

use std::path::{Path, PathBuf};

/// Whether the message is a normal commit message or a merge message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitMessageType {
    Normal,
    Merge,
}

#[derive(Debug, Clone)]
pub struct CommitMessageManager {
    amend_save_state_path: PathBuf,
    pub commit_message_path: PathBuf,
    pub merge_message_path: PathBuf,
    overridden_commit_message: Option<String>,
    pub remember_amend_commit_state: bool,
}

impl CommitMessageManager {
    pub fn new(git_dir: &Path, overridden_commit_message: Option<String>) -> Self {
        CommitMessageManager {
            amend_save_state_path: git_dir.join("GitExtensions.amend"),
            commit_message_path: git_dir.join("COMMITMESSAGE"),
            merge_message_path: git_dir.join("MERGE_MSG"),
            overridden_commit_message,
            remember_amend_commit_state: true,
        }
    }

    pub fn amend_state(&self) -> bool {
        if !self.remember_amend_commit_state {
            return false;
        }
        std::fs::read_to_string(&self.amend_save_state_path).map(|s| s.trim_end().eq_ignore_ascii_case("true")).unwrap_or(false)
    }

    pub fn set_amend_state(&self, amend: bool) {
        if self.remember_amend_commit_state && amend {
            let _ = std::fs::write(&self.amend_save_state_path, "True");
        } else if self.amend_save_state_path.exists() {
            let _ = std::fs::remove_file(&self.amend_save_state_path);
        }
    }

    pub fn is_merge_commit(&self) -> bool {
        self.merge_message_path.exists()
    }

    fn merge_or_commit_message_path(&self) -> &Path {
        if self.is_merge_commit() { &self.merge_message_path } else { &self.commit_message_path }
    }

    pub fn merge_or_commit_message(&self) -> String {
        if let Some(m) = &self.overridden_commit_message {
            return m.clone();
        }
        std::fs::read_to_string(self.merge_or_commit_message_path()).unwrap_or_default()
    }

    pub fn reset_commit_message(&mut self) {
        self.overridden_commit_message = None;
        let _ = std::fs::remove_file(&self.commit_message_path);
        let _ = std::fs::remove_file(&self.amend_save_state_path);
    }

    pub fn set_merge_or_commit_message(&self, message: Option<&str>) {
        let content = message.unwrap_or_default();
        if Some(content) == self.overridden_commit_message.as_deref() {
            return;
        }
        let path = self.merge_or_commit_message_path();
        if path.parent().is_some_and(|p| p.is_dir()) {
            let _ = std::fs::write(path, content);
        }
    }

    pub fn write_commit_message_to_file(&self, message: &str, kind: CommitMessageType, using_commit_template: bool, ensure_second_line_empty: bool) -> std::io::Result<PathBuf> {
        let formatted = format_commit_message(message, using_commit_template, ensure_second_line_empty);
        let path = if kind == CommitMessageType::Normal { &self.commit_message_path } else { &self.merge_message_path };
        std::fs::write(path, formatted)?;
        Ok(path.clone())
    }
}

/// Port of `FormatCommitMessage`: removes template comments and ensures the second line is empty.
pub fn format_commit_message(message: &str, using_commit_template: bool, ensure_second_line_empty: bool) -> String {
    if message.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let mut line_number = 1;
    for line in message.split('\n') {
        if using_commit_template && line.starts_with('#') {
            continue;
        }
        if ensure_second_line_empty && line_number == 2 && !line.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
        out.push('\n');
        line_number += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    //! Ported from GitCommands.Tests/CommitMessageManagerTests.cs
    use super::*;

    fn setup() -> (tempfile::TempDir, CommitMessageManager) {
        let dir = tempfile::tempdir().unwrap();
        let m = CommitMessageManager::new(dir.path(), None);
        (dir, m)
    }

    #[test]
    fn amend_state() {
        let (dir, m) = setup();
        assert!(!m.amend_state());
        for content in ["", " ", "\n", "0", "1", "false", "yes", "on", "checked", "true.", "true\nx"] {
            std::fs::write(dir.path().join("GitExtensions.amend"), content).unwrap();
            assert!(!m.amend_state(), "{content:?}");
        }
        for content in ["true", "True", "TrUe", "true ", "true\n"] {
            std::fs::write(dir.path().join("GitExtensions.amend"), content).unwrap();
            assert!(m.amend_state(), "{content:?}");
        }
        m.set_amend_state(false);
        assert!(!dir.path().join("GitExtensions.amend").exists());
        m.set_amend_state(true);
        assert!(m.amend_state());
        let mut m2 = m.clone();
        m2.remember_amend_commit_state = false;
        assert!(!m2.amend_state());
    }

    #[test]
    fn merge_or_commit_message() {
        let (dir, m) = setup();
        assert_eq!(m.commit_message_path, dir.path().join("COMMITMESSAGE"));
        assert_eq!(m.merge_or_commit_message(), "");
        assert!(!m.is_merge_commit());
        std::fs::write(&m.commit_message_path, "commit message").unwrap();
        assert_eq!(m.merge_or_commit_message(), "commit message");
        std::fs::write(&m.merge_message_path, "merge message").unwrap();
        assert!(m.is_merge_commit());
        assert_eq!(m.merge_or_commit_message(), "merge message");
        let o = CommitMessageManager::new(dir.path(), Some("overridden".into()));
        assert_eq!(o.merge_or_commit_message(), "overridden");
        // the overridden message is not remembered
        o.set_merge_or_commit_message(Some("overridden"));
        assert_eq!(std::fs::read_to_string(&m.merge_message_path).unwrap(), "merge message");
        m.set_merge_or_commit_message(Some("new"));
        assert_eq!(std::fs::read_to_string(&m.merge_message_path).unwrap(), "new");
    }

    #[test]
    fn reset_commit_message_deletes_files() {
        let (_dir, mut m) = setup();
        std::fs::write(&m.commit_message_path, "a").unwrap();
        std::fs::write(&m.merge_message_path, "m").unwrap();
        m.set_amend_state(true);
        m.reset_commit_message();
        assert!(!m.commit_message_path.exists());
        assert!(!m.amend_state());
        assert!(m.merge_message_path.exists());
    }

    #[test]
    fn write_commit_message_to_file() {
        let (_dir, m) = setup();
        let p = m.write_commit_message_to_file("msg", CommitMessageType::Normal, false, false).unwrap();
        assert_eq!(std::fs::read_to_string(p).unwrap(), "msg\n");
        m.write_commit_message_to_file("msg", CommitMessageType::Merge, false, false).unwrap();
        assert!(m.is_merge_commit());
    }

    #[test]
    fn format_commit_message_cases() {
        const NL: &str = "\n";
        let cases: Vec<(&str, bool, bool, String)> = vec![
            ("", false, false, "".into()),
            ("", true, true, "".into()),
            ("\n", false, false, format!("{NL}{NL}")),
            ("\n", true, true, format!("{NL}{NL}")),
            ("1", true, false, format!("1{NL}")),
            ("#1", false, false, format!("#1{NL}")),
            ("#1", true, false, "".into()),
            ("1\n\n3", false, false, format!("1{NL}{NL}3{NL}")),
            ("1\n\n3", false, true, format!("1{NL}{NL}3{NL}")),
            ("1\n2\n3", false, false, format!("1{NL}2{NL}3{NL}")),
            ("1\n2\n3", false, true, format!("1{NL}{NL}2{NL}3{NL}")),
            ("#0\n1\n\n3", true, false, format!("1{NL}{NL}3{NL}")),
            ("#0\n1\n\n3", true, true, format!("1{NL}{NL}3{NL}")),
            ("#0\n1\n2\n3", true, false, format!("1{NL}2{NL}3{NL}")),
            ("#0\n1\n2\n3", true, true, format!("1{NL}{NL}2{NL}3{NL}")),
            ("#0\n1\n#0\n2\n3", true, true, format!("1{NL}{NL}2{NL}3{NL}")),
            ("1\n2\n3\n4\n5\n\n7\n\n\n10", true, true, format!("1{NL}{NL}2{NL}3{NL}4{NL}5{NL}{NL}7{NL}{NL}{NL}10{NL}")),
            ("1\n2\n3\n4\n5\n\n7\n\n\n10\n", false, true, format!("1{NL}{NL}2{NL}3{NL}4{NL}5{NL}{NL}7{NL}{NL}{NL}10{NL}{NL}")),
        ];
        for (msg, tpl, second, expected) in cases {
            assert_eq!(format_commit_message(msg, tpl, second), expected, "{msg:?} {tpl} {second}");
        }
    }
}
