//! Changed words inside modified lines: port of `DiffHighlightService.AddInlineDifferenceMarkers`
//! and `LinesMatcher`. A block of removed lines followed by added lines is paired line by line,
//! then the identical parts of each pair are found; the rest is what changed.
//!
//! Indexes are in chars (upstream works on UTF-16 chars).

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::patch::{DiffLine, DiffLineKind};

/// The changed parts of a removed or added line, in char indexes of [`DiffLine::text`]
/// (the `-`/`+` prefix included).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineMarks {
    pub changed: Vec<Range<usize>>,
    /// Where text was inserted in the paired line, when nothing changed here
    /// (upstream's inter-char marker).
    pub anchor: Option<usize>,
}

/// Upstream ignores the text after this many chars.
const MAX_LENGTH: usize = 2000;
/// Context lines allowed between removed and added lines of one block.
const MAX_GAP_LINES: usize = 5;
/// Larger blocks are paired by index: matching costs O(n²).
const MAX_COMBINATIONS: usize = 100 * 100;
const INSIGNIFICANT_WORD_MATCH_SCORE: f32 = 0.1;

/// The marks of each line of `lines` (`None` for lines without a paired line).
pub fn inline_differences(lines: &[DiffLine]) -> Vec<Option<LineMarks>> {
    let mut marks = vec![None; lines.len()];
    let texts: Vec<Vec<char>> =
        lines.iter().map(|l| if matches!(l.kind, DiffLineKind::Added | DiffLineKind::Removed) { l.text.chars().skip(1).take(MAX_LENGTH).collect() } else { Vec::new() }).collect();
    let mut index = 0;
    while index < lines.len() {
        let removed = block_of_lines(lines, DiffLineKind::Removed, &mut index, false);
        if removed.is_empty() {
            continue;
        }
        let added = block_of_lines(lines, DiffLineKind::Added, &mut index, true);
        if added.is_empty() {
            continue;
        }
        for (r, a) in find_line_pairs(&texts, &removed, &added) {
            let (mr, ma) = line_marks(&texts[r], &texts[a]);
            marks[r] = Some(mr);
            marks[a] = Some(ma);
        }
    }
    marks
}

/// The next block of `kind` lines from `index` (`GetBlockOfLines`): a few context lines
/// may interrupt it. With `found`, the block must start at `index`.
fn block_of_lines(lines: &[DiffLine], kind: DiffLineKind, index: &mut usize, mut found: bool) -> Vec<usize> {
    let mut result = Vec::new();
    let mut gap = 0;
    while *index < lines.len() {
        let line = &lines[*index];
        if line.kind != kind {
            if !found {
                *index += 1;
                continue;
            }
            if line.kind == DiffLineKind::Context && gap < MAX_GAP_LINES {
                gap += 1;
                *index += 1;
                continue;
            }
            break;
        }
        gap = 0;
        found = true;
        result.push(*index);
        *index += 1;
    }
    result
}

/// The marks of a removed and an added line (`AddDifferenceMarkers`): everything but the
/// identical parts is changed.
fn line_marks(removed: &[char], added: &[char]) -> (LineMarks, LineMarks) {
    let mut m = Markers::default();
    let (start, end) = difference_markers(&mut m, removed, added, 0, 0);
    if start > 0 {
        m.same_removed.push(0..start);
        m.same_added.push(0..start);
    }
    if end > 0 {
        m.same_removed.push(removed.len() - end..removed.len());
        m.same_added.push(added.len() - end..added.len());
    }
    // + 1: the prefix
    let shift = |r: Range<usize>| r.start + 1..r.end + 1;
    let marks = |same: Vec<Range<usize>>, len: usize, anchor: Option<usize>| LineMarks {
        changed: complement(same, len).into_iter().map(shift).collect(),
        anchor: anchor.map(|a| a + 1),
    };
    (marks(m.same_removed, removed.len(), m.anchor_removed), marks(m.same_added, added.len(), m.anchor_added))
}

#[derive(Default)]
struct Markers {
    same_removed: Vec<Range<usize>>,
    same_added: Vec<Range<usize>>,
    anchor_removed: Option<usize>,
    anchor_added: Option<usize>,
}

/// The parts of `0..len` not covered by `same`.
fn complement(mut same: Vec<Range<usize>>, len: usize) -> Vec<Range<usize>> {
    same.sort_by_key(|r| r.start);
    let mut out = Vec::new();
    let mut pos = 0;
    for r in same {
        if r.start > pos {
            out.push(pos..r.start);
        }
        pos = pos.max(r.end);
    }
    if pos < len {
        out.push(pos..len);
    }
    out
}

/// Marks the identical parts inside the texts, splitting them at their longest common word.
/// Returns the lengths of the identical parts at the start and at the end, which the caller
/// may join with its own identical parts.
fn difference_markers(m: &mut Markers, removed: &[char], added: &[char], offset_removed: usize, offset_added: usize) -> (usize, usize) {
    let mut start = 0;
    let mut end = 0;
    if removed == added {
        return (removed.len(), 0);
    }
    if let Some((len, mut at_removed, mut at_added)) = find_best_match(removed, added) {
        let mut identical = len;
        // the right part: its identical start joins the common word, its identical end is ours
        let (right_start, right_end) = difference_markers(m, &removed[at_removed + len..], &added[at_added + len..], offset_removed + at_removed + len, offset_added + at_added + len);
        identical += right_start;
        end = right_end;
        // the left part: its identical start is ours, its identical end joins the common word
        let (left_start, left_end) = difference_markers(m, &removed[..at_removed], &added[..at_added], offset_removed, offset_added);
        start = left_start;
        identical += left_end;
        at_removed -= left_end;
        at_added -= left_end;
        if at_removed == start && at_added == start {
            start += identical;
        } else if at_removed + identical + end == removed.len() && at_added + identical + end == added.len() {
            end += identical;
        } else {
            m.same_removed.push(offset_removed + at_removed..offset_removed + at_removed + identical);
            m.same_added.push(offset_added + at_added..offset_added + at_added + identical);
        }
    } else {
        let min = removed.len().min(added.len());
        while start < min && removed[start] == added[start] {
            start += 1;
        }
        let (mut end_removed, mut end_added) = (removed.len(), added.len());
        while end_removed > start && end_added > start && removed[end_removed - 1] == added[end_added - 1] {
            end_removed -= 1;
            end_added -= 1;
            end += 1;
        }
        let (diff_removed, diff_added) = (end_removed - start, end_added - start);
        if diff_removed == 0 && diff_added > 0 {
            m.anchor_removed = Some(offset_removed + start);
        } else if diff_removed > 0 && diff_added == 0 {
            m.anchor_added = Some(offset_added + start);
        }
    }
    (start, end)
}

/// The longest word (else subword) of both texts: its length and its first start in each.
fn find_best_match(removed: &[char], added: &[char]) -> Option<(usize, usize, usize)> {
    // the first longest word of `added` that `removed` has too
    let longest_common = |wr: &[Range<usize>], wa: &[Range<usize>]| -> Option<(usize, usize, usize)> {
        let removed_words: HashSet<&[char]> = wr.iter().map(|r| &removed[r.clone()]).collect();
        let mut best: Option<&Range<usize>> = None;
        for a in wa.iter().filter(|a| removed_words.contains(&added[(*a).clone()])) {
            if best.is_none_or(|b| a.len() > b.len()) {
                best = Some(a);
            }
        }
        let best = best?;
        let word = &added[best.clone()];
        let at_removed = wr.iter().find(|r| &removed[(*r).clone()] == word)?.start;
        Some((word.len(), at_removed, best.start))
    };
    let (wr, wa) = (words(removed), words(added));
    if let Some(found) = longest_common(&wr, &wa) {
        return Some(found);
    }
    let sub = |text: &[char], words: &[Range<usize>]| -> Vec<Range<usize>> {
        words.iter().flat_map(|w| subwords(&text[w.clone()]).into_iter().map(move |s| s.start + w.start..s.end + w.start)).collect()
    };
    longest_common(&sub(removed, &wr), &sub(added, &wa))
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The words (letters, digits and underscores) of `text`.
fn words(text: &[char]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if !is_word_char(text[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < text.len() && is_word_char(text[i]) {
            i += 1;
        }
        out.push(start..i);
    }
    out
}

/// The camelCase and snake_case parts of a word (`GetSubwords`).
fn subwords(word: &[char]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let end = word.len();
    if end == 0 {
        return out;
    }
    let mut start = 0;
    let mut previous_upper = word[0].is_uppercase();
    for index in 0..end {
        let current_upper = word[index].is_uppercase();
        if previous_upper != current_upper {
            previous_upper = current_upper;
            if current_upper {
                // emit the previous word, but no single '_'
                if !(index == 1 && !word[0].is_alphanumeric()) {
                    out.push(start..index);
                }
                start = index;
            }
        }
        // end the word at '_', but join a leading '_' to the first word
        if index > 0 && !word[index].is_alphanumeric() {
            if start < index && word[index - 1].is_alphanumeric() {
                out.push(start..index);
            }
            start = index + 1;
            previous_upper = true;
        }
    }
    if start < end && !(end == 1 && !word[0].is_alphanumeric()) {
        out.push(start..end);
    }
    out
}

/// A block of removed and added lines with what matching compares, computed once: the
/// pairing looks at the same line pairs again and again.
struct Block<'a> {
    removed: &'a [usize],
    added: &'a [usize],
    /// The length of each removed line trimmed.
    trimmed_len: Vec<usize>,
    /// Per removed × added pair: the trimmed lines are equal.
    same_trimmed: Vec<bool>,
    /// Per removed × added pair: the share of the words they have in common.
    score: Vec<f32>,
}

impl<'a> Block<'a> {
    fn new(texts: &[Vec<char>], removed: &'a [usize], added: &'a [usize]) -> Self {
        // words and trimmed lines as numbers, so comparing them is cheap
        let mut ids: HashMap<String, u32> = HashMap::new();
        let mut word_len: Vec<usize> = Vec::new();
        let mut trimmed_ids: HashMap<String, u32> = HashMap::new();
        let mut line = |i: usize| {
            let full: String = texts[i].iter().collect();
            let trimmed: Vec<char> = full.trim().chars().collect();
            let next = trimmed_ids.len() as u32;
            let trimmed_id = *trimmed_ids.entry(trimmed.iter().collect()).or_insert(next);
            let mut line_words: Vec<u32> = words(&trimmed)
                .into_iter()
                .map(|r| {
                    let next = ids.len() as u32;
                    *ids.entry(trimmed[r.clone()].iter().collect()).or_insert_with(|| {
                        word_len.push(r.len());
                        next
                    })
                })
                .collect();
            line_words.sort_unstable();
            line_words.dedup();
            (trimmed_id, trimmed.len(), line_words)
        };
        let r: Vec<_> = removed.iter().map(|&i| line(i)).collect();
        let a: Vec<_> = added.iter().map(|&i| line(i)).collect();
        let total = |w: &[u32]| w.iter().map(|&id| word_len[id as usize]).sum::<usize>();
        let (r_total, a_total): (Vec<usize>, Vec<usize>) = (r.iter().map(|l| total(&l.2)).collect(), a.iter().map(|l| total(&l.2)).collect());
        let mut same_trimmed = Vec::with_capacity(r.len() * a.len());
        let mut score = Vec::with_capacity(r.len() * a.len());
        for (ri, rl) in r.iter().enumerate() {
            for (ai, al) in a.iter().enumerate() {
                same_trimmed.push(rl.0 == al.0);
                score.push(if rl.2.is_empty() || al.2.is_empty() {
                    -1.0
                } else {
                    common_length(&rl.2, &al.2, &word_len) as f32 / r_total[ri].max(a_total[ai]) as f32
                });
            }
        }
        Block { removed, added, trimmed_len: r.iter().map(|l| l.1).collect(), same_trimmed, score }
    }

    fn at(&self, r: usize, a: usize) -> usize {
        r * self.added.len() + a
    }
}

/// The summed-up length of the words two sorted word lists have in common.
fn common_length(a: &[u32], b: &[u32], word_len: &[usize]) -> usize {
    let (mut i, mut j, mut sum) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                sum += word_len[a[i] as usize];
                i += 1;
                j += 1;
            }
        }
    }
    sum
}

/// Pairs related removed and added lines (`LinesMatcher.FindLinePairs`).
fn find_line_pairs(texts: &[Vec<char>], removed: &[usize], added: &[usize]) -> Vec<(usize, usize)> {
    let combinations = removed.len() * added.len();
    if combinations == 0 {
        return Vec::new();
    }
    if combinations == 1 || combinations > MAX_COMBINATIONS {
        return removed.iter().copied().zip(added.iter().copied()).collect();
    }
    let block = Block::new(texts, removed, added);
    let mut out = Vec::new();
    pair_lines(&block, 0..removed.len(), 0..added.len(), &mut out);
    out
}

/// Pairs the best matching lines of the ranges, then the lines before and after them.
fn pair_lines(b: &Block, removed: Range<usize>, added: Range<usize>, out: &mut Vec<(usize, usize)>) {
    let (r, a) = best_line_match(b, removed.clone(), added.clone());
    if r > removed.start && a > added.start {
        pair_lines(b, removed.start..r, added.start..a, out);
    }
    out.push((b.removed[r], b.added[a]));
    if r + 1 < removed.end && a + 1 < added.end {
        pair_lines(b, r + 1..removed.end, a + 1..added.end, out);
    }
}

fn best_line_match(b: &Block, removed: Range<usize>, added: Range<usize>) -> (usize, usize) {
    // first the longest line that is only indented differently
    let mut best: Option<(usize, usize, usize)> = None;
    for r in removed.clone() {
        if let Some(a) = added.clone().find(|&a| b.same_trimmed[b.at(r, a)]) {
            let len = b.trimmed_len[r];
            if best.is_none_or(|(_, _, l)| len > l) {
                best = Some((r, a, len));
            }
        }
    }
    if let Some((r, a, _)) = best {
        return (r, a);
    }
    // then the lines whose common words are the longest
    let mut best = (removed.start, added.start);
    let mut max_score = -1.0_f32;
    for (ri, ai) in all_combinations(removed.len(), added.len()) {
        let (r, a) = (removed.start + ri, added.start + ai);
        let score = b.score[b.at(r, a)];
        if max_score < score {
            max_score = score;
            best = (r, a);
            if max_score == 1.0 {
                return best;
            }
        }
    }
    if max_score <= INSIGNIFICANT_WORD_MATCH_SCORE { (removed.start, added.start) } else { best }
}

/// All index pairs, diagonal by diagonal: (0,0), (1,0), (0,1), (2,0), (1,1), …
fn all_combinations(first_end: usize, second_end: usize) -> impl Iterator<Item = (usize, usize)> {
    let upper = (0..first_end).flat_map(move |diagonal| (0..(diagonal + 1).min(second_end)).map(move |second| (diagonal - second, second)));
    let lower = (1..second_end).flat_map(move |diagonal| (diagonal..(first_end + diagonal).min(second_end)).map(move |second| (first_end - 1 + diagonal - second, second)));
    upper.chain(lower)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::parse_diff_lines;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn texts(s: &str, ranges: &[Range<usize>]) -> Vec<String> {
        let c = chars(s);
        ranges.iter().map(|r| c[r.clone()].iter().collect()).collect()
    }

    #[test]
    fn combinations_visit_every_pair_once() {
        for f in 1..=4 {
            for s in 1..=4 {
                let all: Vec<_> = all_combinations(f, s).collect();
                assert_eq!(all.len(), f * s);
                assert_eq!(all.iter().collect::<HashSet<_>>().len(), f * s);
                assert!(all.iter().all(|&(a, b)| a < f && b < s));
            }
        }
        assert_eq!(all_combinations(2, 2).collect::<Vec<_>>(), vec![(0, 0), (1, 0), (0, 1), (1, 1)]);
    }

    #[test]
    fn words_and_subwords() {
        let t = "---abc---123---def_7---";
        assert_eq!(texts(t, &words(&chars(t))), ["abc", "123", "def_7"]);
        let cases: &[(&str, &[&str], &[usize])] = &[
            ("", &[], &[]),
            ("_", &[], &[]),
            ("camelCase", &["camel", "Case"], &[0, 5]),
            ("_precedingUnderScore", &["_preceding", "Under", "Score"], &[0, 10, 15]),
            ("python_style", &["python", "style"], &[0, 7]),
            ("multiple__under___scores_", &["multiple", "under", "scores"], &[0, 10, 18]),
            ("CAPITALSwithsuffixNext_Upper_lower", &["CAPITALSwithsuffix", "Next", "Upper", "lower"], &[0, 18, 23, 29]),
            ("    sum += x;", &["sum", "x"], &[4, 11]),
        ];
        for (word, expected, starts) in cases {
            let s = subwords(&chars(word));
            assert_eq!(texts(word, &s), *expected, "{word}");
            assert_eq!(s.iter().map(|r| r.start).collect::<Vec<_>>(), *starts, "{word}");
        }
    }

    fn pairs(removed: &[&str], added: &[&str]) -> Vec<(usize, usize)> {
        let all: Vec<Vec<char>> = removed.iter().chain(added).map(|s| chars(s)).collect();
        let r: Vec<usize> = (0..removed.len()).collect();
        let a: Vec<usize> = (removed.len()..all.len()).collect();
        find_line_pairs(&all, &r, &a).into_iter().map(|(r, a)| (r, a - removed.len())).collect()
    }

    #[test]
    fn pairs_indented_lines_first() {
        assert_eq!(pairs(&["r0", " trimmed line\t", "r2"], &["a0", "a1", "a2", "trimmed line", "a4"]), [(0, 0), (1, 3), (2, 4)]);
    }

    #[test]
    fn pairs_lines_with_the_longest_common_words() {
        assert_eq!(
            pairs(
                &["line 0 had some words", "line 1 had some more common words", " trimmed line\t", "r2"],
                &["some words", "line 1 has some words", "line 2 has some common words", "trimmed line", "a4"]
            ),
            [(0, 1), (1, 2), (2, 3), (3, 4)]
        );
    }

    #[test]
    fn large_blocks_are_paired_by_index() {
        let removed: Vec<String> = (0..10).map(|i| format!("line{i}")).collect();
        let mut added = vec!["other line".to_string(); 10];
        added.extend(removed.iter().cloned());
        added.push("one more".into());
        let r: Vec<&str> = removed.iter().map(String::as_str).collect();
        let a: Vec<&str> = added.iter().map(String::as_str).collect();
        // 10 * 21 is small enough to match
        assert_eq!(pairs(&r, &a)[0], (0, 10));
        let mut many = a.clone();
        many.extend(std::iter::repeat_n("x", 1000));
        assert_eq!(pairs(&r, &many)[..2], [(0, 0), (1, 1)]);
    }

    fn changed(removed: &str, added: &str) -> (Vec<String>, Vec<String>, Option<usize>, Option<usize>) {
        let (r, a) = line_marks(&chars(removed), &chars(added));
        // the ranges include the prefix
        let (pr, pa) = (format!("-{removed}"), format!("+{added}"));
        (texts(&pr, &r.changed), texts(&pa, &a.changed), r.anchor, a.anchor)
    }

    #[test]
    fn marks_the_changed_words() {
        assert_eq!(changed("let x = old_value + 1;", "let x = new_value + 1;"), (vec!["old".into()], vec!["new".into()], None, None));
        assert_eq!(changed("call(a, b)", "call(a, b, c)"), (vec![], vec![", c".into()], Some(10), None));
        assert_eq!(changed("call(a, b, c)", "call(a, c)"), (vec!["b, ".into()], vec![], None, Some(9)));
        assert_eq!(changed("same", "same"), (vec![], vec![], None, None));
        assert_eq!(changed("    foo();", "\tfoo();"), (vec!["    ".into()], vec!["\t".into()], None, None));
    }

    #[test]
    fn marks_paired_lines_of_a_diff() {
        let diff = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n-int count = 1;\n context\n+int count = 2;\n+added line\n";
        let lines = parse_diff_lines(diff);
        let marks = inline_differences(&lines);
        let at = |i: usize| marks[i].as_ref().map(|m| texts(&lines[i].text, &m.changed));
        assert_eq!(at(4), Some(vec!["1".to_string()]));
        assert_eq!(at(6), Some(vec!["2".to_string()]));
        assert_eq!(at(5), None);
        assert_eq!(at(7), None);
    }
}

