//! Port of `GitCommands.RevisionReader`: reads `git log` output into [`GitRevision`]s.

use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::args::GitArgs;
use crate::exec::{Executable, GitResult};
use crate::object_id::{ObjectId, SHA1_CHAR_COUNT};
use crate::revision::GitRevision;

const REFLOG_SELECTOR_FORMAT: &str = "%gD%n";
const NOTES_PREFIX: &str = "\u{039d}\u{043e}t\u{0435}\u{0282}:"; // Unicode look-alikes, "Νоtеʂ:"
const NOTES_FORMAT: &str = "%n\u{039d}\u{043e}t\u{0435}\u{0282}:%n%N";

/// Number of days for which full commit bodies are kept (about 6 months).
pub const OFFSET_DAYS_FOR_OLDEST_BODY: i64 = 6 * 30;

/// Revision sort order (`RevisionSortOrder` setting).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum RevisionSortOrder {
    #[default]
    GitDefault,
    AuthorDate,
    Topology,
}

/// Parses `git log -z --pretty=format:<log_format()>` output.
#[derive(Debug, Clone)]
pub struct RevisionParser {
    pub has_reflog_selector: bool,
    pub has_notes: bool,
    /// Bodies of commits older than this unix time are not kept (memory).
    pub oldest_body: i64,
    pub parse_errors: usize,
}

impl Default for RevisionParser {
    fn default() -> Self {
        RevisionParser { has_reflog_selector: false, has_notes: false, oldest_body: 0, parse_errors: 0 }
    }
}

/// The `--pretty=format:` string.
pub fn log_format(has_reflog_selector: bool, has_notes: bool) -> String {
    format!(
        "%H%T%P%n%at%n%ct%n%aN%n%aE%n%cN%n%cE%n{}%B{}",
        if has_reflog_selector { REFLOG_SELECTOR_FORMAT } else { "" },
        if has_notes { NOTES_FORMAT } else { "" }
    )
}

/// Unix time `days` ago.
pub fn unix_time_days_ago(days: i64) -> i64 {
    chrono::Utc::now().timestamp() - days * 24 * 3600
}

fn parse_u64_prefix(s: &[u8]) -> Option<(i64, usize)> {
    let n = s.iter().take_while(|b| b.is_ascii_digit()).count();
    if n == 0 {
        return None;
    }
    std::str::from_utf8(&s[..n]).ok()?.parse::<i64>().ok().map(|v| (v, n))
}

impl RevisionParser {
    pub fn new(has_reflog_selector: bool, has_notes: bool, oldest_body: i64) -> Self {
        RevisionParser { has_reflog_selector, has_notes, oldest_body, parse_errors: 0 }
    }

    fn error(&mut self) -> Option<GitRevision> {
        self.parse_errors += 1;
        None
    }

    /// Parses one NUL-separated chunk of the log output.
    pub fn try_parse_revision(&mut self, buffer: &[u8]) -> Option<GitRevision> {
        if buffer.len() < SHA1_CHAR_COUNT * 2 {
            return self.error();
        }
        let Some(object_id) = ObjectId::try_parse_bytes(&buffer[..SHA1_CHAR_COUNT]) else {
            return self.error();
        };
        let Some(tree_id) = ObjectId::try_parse_bytes(&buffer[SHA1_CHAR_COUNT..2 * SHA1_CHAR_COUNT]) else {
            return self.error();
        };
        let mut offset = SHA1_CHAR_COUNT * 2;

        // Zero or more parent ids separated by ' ' and terminated by '\n'.
        let mut no_parents: i32 = 0;
        {
            let mut o = offset;
            while o < buffer.len() && buffer[o] != b'\n' {
                if no_parents > 0 {
                    o += 1;
                }
                o += SHA1_CHAR_COUNT;
                no_parents += 1;
                if o >= buffer.len() || !matches!(buffer[o], b'\n' | b' ') {
                    no_parents = -1;
                    break;
                }
            }
        }
        let mut parent_ids = Vec::new();
        if no_parents <= 0 {
            offset += 1;
        } else {
            for _ in 0..no_parents {
                let Some(id) = ObjectId::try_parse_bytes(&buffer[offset..offset + SHA1_CHAR_COUNT]) else {
                    return self.error();
                };
                parent_ids.push(id);
                offset += SHA1_CHAR_COUNT + 1;
            }
        }

        let Some((author_unix_time, n)) = buffer.get(offset..).and_then(parse_u64_prefix) else {
            return self.error();
        };
        offset += n + 1;
        let Some((commit_unix_time, n)) = buffer.get(offset..).and_then(parse_u64_prefix) else {
            return self.error();
        };
        offset += n + 1;

        let next_line = |offset: &mut usize| -> Option<String> {
            if *offset >= buffer.len() {
                return None;
            }
            let len = buffer[*offset..].iter().position(|&b| b == b'\n')?;
            let s = String::from_utf8_lossy(&buffer[*offset..*offset + len]).into_owned();
            *offset += len + 1;
            Some(s)
        };
        let author = next_line(&mut offset);
        let author_email = next_line(&mut offset);
        let committer = next_line(&mut offset);
        let committer_email = next_line(&mut offset);
        let (Some(author), Some(author_email), Some(committer), Some(committer_email)) = (author, author_email, committer, committer_email)
        else {
            return self.error();
        };

        let mut revision = GitRevision::new(object_id).with_parents(parent_ids);
        revision.tree_id = tree_id;
        revision.author = author;
        revision.author_email = author_email;
        revision.author_unix_time = author_unix_time;
        revision.committer = committer;
        revision.committer_email = committer_email;
        revision.commit_unix_time = commit_unix_time;

        let rest = buffer.get(offset..).unwrap_or_default();
        let decoded_owned = String::from_utf8_lossy(rest);
        let mut decoded: &str = decoded_owned.trim_end();

        if self.has_reflog_selector {
            let Some(line_length) = decoded.find('\n') else {
                return self.error();
            };
            revision.reflog_selector = (line_length > 0).then(|| decoded[..line_length].to_string());
            decoded = &decoded[line_length + 1..];
        }

        let keep_body = commit_unix_time >= self.oldest_body;
        match decoded.find(['\n', '\u{b}']) {
            None => {
                revision.subject = decoded.to_string();
                revision.has_multi_line_message = false;
            }
            Some(first_line_end) => {
                revision.subject = decoded[..first_line_end].trim_end().to_string();
                let (body, notes) = self.split(decoded);
                revision.has_multi_line_message = revision.subject.chars().count() < body.chars().count();
                if keep_body {
                    // Handle '\v' (Shift-Enter) as '\n'
                    if revision.has_multi_line_message {
                        revision.set_body(Some(body.replace('\u{b}', "\n")));
                    }
                    if self.has_notes {
                        revision.notes = Some(notes.replace('\u{b}', "\n"));
                    }
                } else if self.has_notes && notes.is_empty() {
                    revision.notes = Some(String::new());
                }
            }
        }
        Some(revision)
    }

    fn split<'a>(&self, decoded: &'a str) -> (&'a str, &'a str) {
        if self.has_notes {
            let marker = format!("\n{NOTES_PREFIX}");
            if let Some(split_pos) = decoded.rfind(&marker) {
                let body = decoded[..split_pos].trim_end();
                let start = (split_pos + marker.len() + 1).min(decoded.len());
                return (body, &decoded[start..]);
            }
        }
        (decoded, "")
    }
}

/// Splits a byte stream on NUL bytes, calling `f` for every chunk.
pub fn split_log_output(mut reader: impl Read, mut f: impl FnMut(&[u8]) -> bool) {
    let mut buf = vec![0u8; 64 * 1024];
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let mut start = 0;
        for i in 0..n {
            if buf[i] == 0 {
                pending.extend_from_slice(&buf[start..i]);
                if !f(&pending) {
                    return;
                }
                pending.clear();
                start = i + 1;
            }
        }
        pending.extend_from_slice(&buf[start..n]);
    }
    if !pending.is_empty() {
        f(&pending);
    }
}

/// Options for loading the revision grid log.
#[derive(Debug, Clone, Default)]
pub struct LogOptions {
    /// Revision arguments (e.g. `--all`, branch names, `--since=`), already split.
    pub revision_filter: Vec<String>,
    pub path_filter: Vec<String>,
    pub has_notes: bool,
    pub sort_order: RevisionSortOrder,
    /// Keep bodies for commits newer than this.
    pub oldest_body: i64,
    pub max_count: Option<usize>,
}

/// Port of `BuildArguments`.
pub fn build_log_arguments(options: &LogOptions) -> GitArgs {
    let mut args = GitArgs::new("log");
    args.add("-z");
    args.add(format!("--pretty=format:{}", log_format(false, options.has_notes)));
    args.add_if(options.sort_order == RevisionSortOrder::AuthorDate, "--author-date-order");
    args.add_if(options.sort_order == RevisionSortOrder::Topology, "--topo-order");
    if let Some(max) = options.max_count {
        args.add(format!("--max-count={max}"));
    }
    args.add_all(options.revision_filter.iter().cloned());
    args.add("--");
    args.add_all(options.path_filter.iter().cloned());
    args
}

/// Streams the revisions of `git log` in batches to `on_batch` (on the calling thread).
/// Returns early if `cancel` is set.
pub fn read_log(
    working_dir: &Path,
    options: &LogOptions,
    cancel: Arc<AtomicBool>,
    mut on_batch: impl FnMut(Vec<GitRevision>),
) -> GitResult<usize> {
    let exe = Executable::git(working_dir);
    let args = build_log_arguments(options);
    let process = exe.spawn_raw(&args)?;
    let mut parser = RevisionParser::new(false, options.has_notes, options.oldest_body);
    let mut batch = Vec::with_capacity(100);
    let mut total = 0;
    let mut batch_size = 100;
    let mut last_flush = std::time::Instant::now();
    split_log_output(process.stdout, |chunk| {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        if let Some(rev) = parser.try_parse_revision(chunk) {
            batch.push(rev);
            total += 1;
            if batch.len() >= batch_size || last_flush.elapsed().as_millis() > 300 {
                on_batch(std::mem::take(&mut batch));
                batch_size = 5000;
                last_flush = std::time::Instant::now();
            }
        }
        true
    });
    if !batch.is_empty() {
        on_batch(batch);
    }
    let _ = process.child.lock().map(|mut c| {
        if cancel.load(Ordering::Relaxed) {
            let _ = c.kill();
        }
        let _ = c.wait();
    });
    Ok(total)
}

/// Runs a log-like command and parses all revisions.
pub fn read_revisions(exe: &Executable, args: &GitArgs, has_reflog_selector: bool) -> GitResult<Vec<GitRevision>> {
    let out = exe.run(args)?;
    let mut parser = RevisionParser::new(has_reflog_selector, false, 0);
    let mut revisions = Vec::new();
    for chunk in out.stdout.split(|&b| b == 0) {
        if chunk.is_empty() {
            continue;
        }
        if let Some(r) = parser.try_parse_revision(chunk) {
            revisions.push(r);
        }
    }
    Ok(revisions)
}

/// Port of `AddAutoStash`: the autostash revision during a rebase.
pub fn read_autostash(git_dir: &Path, label: &str) -> Option<GitRevision> {
    let file = git_dir.join("rebase-merge").join("autostash");
    let text = std::fs::read_to_string(&file).ok()?;
    let id = ObjectId::try_parse(text.lines().next()?.trim())?;
    let modified = std::fs::metadata(&file).ok()?.modified().ok()?;
    let now = modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    let mut rev = GitRevision::new(id);
    rev.author_unix_time = now;
    rev.commit_unix_time = now;
    rev.is_autostash = true;
    rev.subject = label.to_string();
    if let Ok(orig) = std::fs::read_to_string(git_dir.join("rebase-merge").join("orig-head")) {
        if let Some(orig) = orig.lines().next().and_then(|l| ObjectId::try_parse(l.trim())) {
            rev.parent_ids = Some(vec![orig]);
        }
    }
    Some(rev)
}

#[cfg(test)]
mod tests {
    //! Ported from GitCommands.Tests/RevisionReaderTests.cs, using the original binary test data
    //! and verified JSON snapshots.
    use super::*;
    use crate::testing::test_data_dir;

    fn six_months() -> i64 {
        // new DateTimeOffset(new DateTime(2021, 01, 01)).ToUnixTimeSeconds()
        1_609_459_200
    }

    #[test]
    fn build_arguments_should_be_nul_terminated() {
        let args = build_log_arguments(&LogOptions::default());
        assert!(args.to_string().starts_with("log -z "));
    }

    #[test]
    fn try_parse_revision_should_return_false_if_argument_is_invalid() {
        let mut p = RevisionParser::new(false, false, six_months());
        assert!(p.try_parse_revision(&[]).is_none());
    }

    fn check(name: &str, expected_return: bool, has_reflog_selector: bool, has_empty_reflog_selector: bool, has_notes: bool) {
        let dir = test_data_dir().join("core/RevisionReader");
        let chunk = std::fs::read(dir.join(format!("{name}.bin"))).unwrap();
        let mut p = RevisionParser::new(has_reflog_selector, has_notes, six_months());
        let rev = p.try_parse_revision(&chunk);
        assert_eq!(rev.is_some(), expected_return, "{name}");
        let Some(rev) = rev else { return };
        assert_eq!(rev.reflog_selector.is_none(), has_empty_reflog_selector, "{name}");

        let json_path = dir.join(format!("RevisionReaderTests.TryParseRevision_test_testName={name}.verified.json"));
        let text = std::fs::read_to_string(json_path).unwrap();
        let expected: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap();
        let s = |k: &str| expected[k].as_str().map(str::to_string);
        assert_eq!(Some(rev.guid()), s("Guid"), "{name}");
        assert_eq!(Some(rev.author.clone()), s("Author"), "{name}");
        assert_eq!(Some(rev.author_email.clone()), s("AuthorEmail"), "{name}");
        assert_eq!(Some(rev.committer.clone()), s("Committer"), "{name}");
        assert_eq!(Some(rev.committer_email.clone()), s("CommitterEmail"), "{name}");
        assert_eq!(rev.author_unix_time, expected["AuthorUnixTime"].as_i64().unwrap(), "{name}");
        assert_eq!(rev.commit_unix_time, expected["CommitUnixTime"].as_i64().unwrap(), "{name}");
        assert_eq!(Some(rev.subject.clone()), s("Subject"), "{name}");
        assert_eq!(rev.body().map(str::to_string), s("Body"), "{name}");
        assert_eq!(rev.has_multi_line_message, expected["HasMultiLineMessage"].as_bool().unwrap(), "{name}");
        assert_eq!(rev.notes, s("Notes"), "{name}");
        assert_eq!(rev.reflog_selector, s("ReflogSelector"), "{name}");
        assert_eq!(rev.has_parent(), expected["HasParent"].as_bool().unwrap(), "{name}");
        assert_eq!(rev.parents().len(), expected["ParentIds"].as_array().map(|a| a.len()).unwrap_or(0), "{name}");
    }

    #[test]
    fn try_parse_revision_test() {
        check("bad_parentid", false, false, true, false);
        check("bad_parentid_length", false, false, true, false);
        check("bad_sha", false, false, true, false);
        check("empty", false, false, true, false);
        check("illegal_timestamp", false, false, true, false);
        check("multi_pathfilter", true, false, true, false);
        check("no_subject", true, false, true, false);
        check("normal", true, false, true, false);
        check("short_sha", false, false, true, false);
        check("simple_pathfilter", true, false, true, false);
        check("subject_no_body", true, false, true, false);
        check("empty_commit", true, false, true, false);
        check("vertical_tab", true, false, true, false);
        check("reflogselector", true, true, false, false);
        check("reflogselector_empty", true, true, true, false);
        check("notes_data", true, false, true, true);
        check("notes_empty", true, false, true, true);
        check("subject_starts_with_newline", true, false, true, false);
    }

    #[test]
    fn split_log_output_on_nul() {
        let mut chunks = Vec::new();
        split_log_output(&b"abc\0def\0gh"[..], |c| {
            chunks.push(c.to_vec());
            true
        });
        assert_eq!(chunks, vec![b"abc".to_vec(), b"def".to_vec(), b"gh".to_vec()]);
    }
}
