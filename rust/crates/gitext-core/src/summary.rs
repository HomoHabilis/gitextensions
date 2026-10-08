//! Port of `GitCommands.GitRevisionSummaryBuilder`: shortens commit messages for tooltips.

const COMMIT_SUMMARY_MAX_LINE_LENGTH: usize = 150;
const COMMIT_SUMMARY_MAX_NUMBER_OF_LINES: usize = 30;

/// Builds a summary of a commit body, limiting line length and number of lines.
/// Returns `None` for an empty or whitespace body.
pub fn build_summary(body: Option<&str>) -> Option<String> {
    let body = body?;
    if body.trim().is_empty() {
        return None;
    }
    let chars: Vec<char> = body.chars().collect();
    let mut s = String::with_capacity(body.len().min(5000));
    let mut line_count = 0;
    let mut line_start = 0usize;
    let append_line = |s: &mut String, start: usize, length: usize, ellipsis: bool| {
        s.extend(&chars[start..start + length]);
        if ellipsis {
            s.push_str(" [...]");
        }
        s.push('\n');
    };
    for pos in 0..chars.len() {
        if chars[pos] == '\n' {
            if pos - line_start > COMMIT_SUMMARY_MAX_LINE_LENGTH {
                append_line(&mut s, line_start, COMMIT_SUMMARY_MAX_LINE_LENGTH, true);
            } else {
                append_line(&mut s, line_start, pos - line_start, false);
            }
            line_start = pos + 1;
            line_count += 1;
            if line_count == COMMIT_SUMMARY_MAX_NUMBER_OF_LINES {
                s.push_str("[...]");
                return Some(s);
            }
        }
        if pos == chars.len() - 1 {
            if pos + 1 - line_start > COMMIT_SUMMARY_MAX_LINE_LENGTH + 1 {
                append_line(&mut s, line_start, COMMIT_SUMMARY_MAX_LINE_LENGTH, true);
            } else if pos >= line_start {
                append_line(&mut s, line_start, pos - line_start + 1, false);
            }
        }
    }
    Some(s.trim_end_matches('\n').to_string())
}

#[cfg(test)]
mod tests {
    //! Ported from GitCommands.Tests/GitRevisionSummaryBuilderTests.cs
    use super::*;
    use crate::testing::{read_test_data, verify};

    #[test]
    fn should_return_null_when_body_is_null_or_whitespace() {
        assert_eq!(build_summary(None), None);
        assert_eq!(build_summary(Some("")), None);
        assert_eq!(build_summary(Some("  ")), None);
    }

    #[test]
    fn should_have_same_content_when_no_ellipsis() {
        for body in ["toto", "toto\ntata\ntiti"] {
            assert_eq!(build_summary(Some(body)).as_deref(), Some(body));
        }
    }

    #[test]
    fn should_do_ellipsis() {
        for name in ["Too_many_lines", "Too_long_lines"] {
            let content = read_test_data(&format!("core/{name}.txt"));
            verify(
                &format!("core/GitRevisionSummaryBuilderTests.Should_do_ellipsis_testName={name}.verified.txt"),
                &build_summary(Some(&content)).unwrap(),
            );
        }
    }
}
