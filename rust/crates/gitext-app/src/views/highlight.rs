//! Colours of the text in the diff viewer: syntax highlighting by file name (upstream picks the
//! editor highlighting mode by extension) with syntect's default syntaxes and themes.

use std::ops::Range;
use std::sync::OnceLock;

use egui::Color32;
use gitext_core::patch::{DiffLine, DiffLineKind};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

/// A coloured part of a line, in bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub range: Range<usize>,
    pub fg: Option<Color32>,
    pub bg: Option<Color32>,
}

/// Longer lines are not highlighted (minified files): syntect is slow on them.
const MAX_LINE: usize = 2000;

fn syntaxes() -> &'static (SyntaxSet, ThemeSet) {
    static SETS: OnceLock<(SyntaxSet, ThemeSet)> = OnceLock::new();
    SETS.get_or_init(|| (SyntaxSet::load_defaults_newlines(), ThemeSet::load_defaults()))
}

fn theme(dark: bool) -> &'static Theme {
    &syntaxes().1.themes[if dark { "base16-eighties.dark" } else { "InspiredGitHub" }]
}

/// The syntax of a file, from its extension or its name (`Makefile`).
fn syntax_for(path: &str) -> Option<&'static SyntaxReference> {
    let ss = &syntaxes().0;
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let syntax = name.rsplit_once('.').and_then(|(_, ext)| ss.find_syntax_by_extension(&ext.to_lowercase())).or_else(|| ss.find_syntax_by_extension(name))?;
    // plain text has nothing to colour
    (syntax.name != "Plain Text").then_some(syntax)
}

/// Whether files named like `path` are highlighted.
pub fn has_syntax(path: &str) -> bool {
    syntax_for(path).is_some()
}

/// The file a header line of a diff is about (`diff --git a/x b/x`, `+++ b/x`, `diff --cc x`).
fn header_path(line: &str) -> Option<&str> {
    if let Some(rest) = line.strip_prefix("+++ ") {
        return (rest != "/dev/null").then(|| rest.strip_prefix("b/").unwrap_or(rest));
    }
    if let Some(rest) = line.strip_prefix("diff --git ") {
        return rest.rsplit_once(" b/").map(|(_, p)| p);
    }
    line.strip_prefix("diff --cc ").or_else(|| line.strip_prefix("diff --combined "))
}

fn color(c: syntect::highlighting::Color) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b)
}

/// The colours of `text` (one line, without its newline) shifted by `offset` bytes; advances
/// the highlighter's state.
fn highlight_line(h: &mut HighlightLines, text: &str, offset: usize) -> Vec<Span> {
    let mut line = String::with_capacity(text.len() + 1);
    line.push_str(text);
    line.push('\n');
    let Ok(parts) = h.highlight_line(&line, &syntaxes().0) else { return Vec::new() };
    let mut spans = Vec::with_capacity(parts.len());
    let mut pos = 0;
    for (style, part) in parts {
        let end = (pos + part.len()).min(text.len());
        if end > pos {
            spans.push(Span { range: offset + pos..offset + end, fg: Some(color(style.foreground)), bg: None });
        }
        pos += part.len();
    }
    spans
}

/// The colours of each line of a file named `path`.
pub fn highlight_text(lines: &[String], path: &str, dark: bool) -> Vec<Vec<Span>> {
    let Some(syntax) = syntax_for(path) else { return Vec::new() };
    let mut h = HighlightLines::new(syntax, theme(dark));
    lines.iter().map(|l| if l.len() > MAX_LINE { Vec::new() } else { highlight_line(&mut h, l, 0) }).collect()
}

/// The colours of each line of a diff: the old and the new version of each hunk are
/// highlighted separately, so that removed and added lines continue the right code. The
/// `-`/`+` prefix keeps the diff colour.
pub fn highlight_diff(lines: &[DiffLine], dark: bool) -> Vec<Vec<Span>> {
    let mut out = vec![Vec::new(); lines.len()];
    let mut syntax: Option<&SyntaxReference> = None;
    let mut versions: Option<(HighlightLines, HighlightLines)> = None;
    for (i, line) in lines.iter().enumerate() {
        match line.kind {
            DiffLineKind::Header => {
                if let Some(path) = header_path(&line.text) {
                    syntax = syntax_for(path);
                }
                versions = None;
            }
            DiffLineKind::HunkHeader => {
                // the code between hunks is not shown: start again
                versions = syntax.map(|s| (HighlightLines::new(s, theme(dark)), HighlightLines::new(s, theme(dark))));
            }
            DiffLineKind::Context | DiffLineKind::Added | DiffLineKind::Removed => {
                let Some((old, new)) = versions.as_mut() else { continue };
                let prefix = line.text.chars().next().map_or(0, char::len_utf8);
                let text = &line.text[prefix..];
                if text.len() > MAX_LINE {
                    continue;
                }
                out[i] = match line.kind {
                    DiffLineKind::Removed => highlight_line(old, text, prefix),
                    DiffLineKind::Added => highlight_line(new, text, prefix),
                    _ => {
                        highlight_line(old, text, prefix);
                        highlight_line(new, text, prefix)
                    }
                };
            }
            _ => {}
        }
    }
    out
}

/// The current SGR attributes of [`parse_ansi`].
#[derive(Default, Clone, Copy)]
struct Sgr {
    fg: Option<AnsiColor>,
    bg: Option<AnsiColor>,
    reverse: bool,
    dim: bool,
}

#[derive(Clone, Copy)]
enum AnsiColor {
    Basic(u8),
    Rgb(u8, u8, u8),
}

impl Sgr {
    fn apply(&mut self, params: &str) {
        let codes: Vec<u32> = params.split(';').map(|c| c.parse().unwrap_or(0)).collect();
        let mut i = 0;
        while i < codes.len() {
            match codes[i] {
                0 => *self = Sgr::default(),
                2 => self.dim = true,
                7 => self.reverse = true,
                22 => self.dim = false,
                27 => self.reverse = false,
                c @ 30..=37 => self.fg = Some(AnsiColor::Basic((c - 30) as u8)),
                c @ 90..=97 => self.fg = Some(AnsiColor::Basic((c - 90) as u8)),
                39 => self.fg = None,
                c @ 40..=47 => self.bg = Some(AnsiColor::Basic((c - 40) as u8)),
                c @ 100..=107 => self.bg = Some(AnsiColor::Basic((c - 100) as u8)),
                49 => self.bg = None,
                c @ (38 | 48) => {
                    // 38;5;n (256 colours, the first 16 are the basic ones) and 38;2;r;g;b
                    let color = match codes.get(i + 1) {
                        Some(5) => {
                            let n = codes.get(i + 2).copied().unwrap_or(0);
                            i += 2;
                            (n < 16).then_some(AnsiColor::Basic((n % 8) as u8))
                        }
                        Some(2) => {
                            let at = |k: usize| codes.get(i + k).copied().unwrap_or(0) as u8;
                            let rgb = AnsiColor::Rgb(at(2), at(3), at(4));
                            i += 4;
                            Some(rgb)
                        }
                        _ => None,
                    };
                    if c == 38 {
                        self.fg = color;
                    } else {
                        self.bg = color;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    /// The colours, in the palette's diff colours where they mean added and removed.
    fn colors(&self, p: &crate::theme::Palette) -> (Option<Color32>, Option<Color32>) {
        let fg_of = |c: AnsiColor| match c {
            AnsiColor::Basic(1) => Some(p.diff_removed_fg),
            AnsiColor::Basic(2) => Some(p.diff_added_fg),
            AnsiColor::Basic(3) => Some(p.warning),
            AnsiColor::Basic(4) => Some(p.lanes[0]),
            AnsiColor::Basic(5) => Some(p.status_renamed),
            AnsiColor::Basic(6) => Some(p.diff_hunk_fg),
            AnsiColor::Basic(_) => None,
            AnsiColor::Rgb(r, g, b) => Some(Color32::from_rgb(r, g, b)),
        };
        let bg_of = |c: AnsiColor| match c {
            AnsiColor::Basic(1) => Some(p.diff_removed_word_bg),
            AnsiColor::Basic(2) => Some(p.diff_added_word_bg),
            AnsiColor::Basic(0 | 7) => None,
            c => fg_of(c).map(|c| c.gamma_multiply(0.35)),
        };
        let (mut fg, bg) = if self.reverse { (self.bg.and_then(fg_of), self.fg.and_then(bg_of)) } else { (self.fg.and_then(fg_of), self.bg.and_then(bg_of)) };
        if self.dim {
            fg = Some(fg.unwrap_or(p.muted).gamma_multiply(0.7));
        }
        (fg, bg)
    }
}

/// The lines of git's coloured output (`--color=always`) without the escape codes, and their
/// colours.
pub fn parse_ansi(text: &str, p: &crate::theme::Palette) -> (Vec<String>, Vec<Vec<Span>>) {
    let mut lines = Vec::new();
    let mut styles = Vec::new();
    let mut sgr = Sgr::default();
    for raw in text.lines() {
        let mut line = String::with_capacity(raw.len());
        let mut spans: Vec<Span> = Vec::new();
        let mut rest = raw;
        loop {
            let (plain, after) = match rest.find('\x1b') {
                Some(at) => (&rest[..at], Some(&rest[at + 1..])),
                None => (rest, None),
            };
            if !plain.is_empty() {
                let plain = plain.replace('\t', "    ");
                let (fg, bg) = sgr.colors(p);
                let range = line.len()..line.len() + plain.len();
                line.push_str(&plain);
                if fg.is_some() || bg.is_some() {
                    match spans.last_mut() {
                        Some(last) if last.range.end == range.start && last.fg == fg && last.bg == bg => last.range.end = range.end,
                        _ => spans.push(Span { range, fg, bg }),
                    }
                }
            }
            let Some(after) = after else { break };
            // a CSI sequence: ESC [ params final-byte
            let Some(body) = after.strip_prefix('[') else {
                rest = after;
                continue;
            };
            let end = body.find(|c: char| ('@'..='~').contains(&c)).unwrap_or(body.len());
            if body[end..].starts_with('m') {
                sgr.apply(&body[..end]);
            }
            rest = body.get(end + 1..).unwrap_or_default();
        }
        lines.push(line);
        styles.push(spans);
    }
    (lines, styles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitext_core::patch::parse_diff_lines;

    #[test]
    fn ansi_colours_become_spans() {
        let p = crate::theme::Palette::new(true);
        let text = "\x1b[1mdiff --git a/f b/f\x1b[m\nkeep \x1b[7;31mold\x1b[m\x1b[32mnew\x1b[m end\x1b[K\n\tx";
        let (lines, styles) = parse_ansi(text, &p);
        assert_eq!(lines, ["diff --git a/f b/f", "keep oldnew end", "    x"]);
        assert!(styles[0].is_empty());
        assert_eq!(styles[1], [Span { range: 5..8, fg: None, bg: Some(p.diff_removed_word_bg) }, Span { range: 8..11, fg: Some(p.diff_added_fg), bg: None }]);
        assert!(styles[2].is_empty());
        let (_, rgb) = parse_ansi("\x1b[38;2;1;2;3mx\x1b[38;5;2my\x1b[0m", &p);
        assert_eq!(rgb[0][0].fg, Some(Color32::from_rgb(1, 2, 3)));
        assert_eq!(rgb[0][1].fg, Some(p.diff_added_fg));
    }

    #[test]
    fn syntax_from_the_file_name() {
        assert!(has_syntax("src/main.rs"));
        assert!(has_syntax("a/B.CS"));
        assert!(has_syntax("Makefile"));
        assert!(!has_syntax("notes.txt"));
        assert!(!has_syntax("README"));
        assert_eq!(header_path("diff --git a/x y.rs b/x y.rs"), Some("x y.rs"));
        assert_eq!(header_path("+++ b/src/a.py"), Some("src/a.py"));
        assert_eq!(header_path("+++ /dev/null"), None);
        assert_eq!(header_path("diff --cc m.c"), Some("m.c"));
    }

    #[test]
    fn diff_lines_are_coloured_after_their_prefix() {
        let diff = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,2 +1,2 @@\n fn main() {\n-    let x = 1;\n+    let x = \"two\";\n }\n";
        let lines = parse_diff_lines(diff);
        let spans = highlight_diff(&lines, true);
        assert_eq!(spans.len(), lines.len());
        // headers keep their colour
        assert!(spans[..4].iter().all(Vec::is_empty));
        for i in 4..8 {
            assert!(!spans[i].is_empty(), "line {i}");
            assert_eq!(spans[i][0].range.start, 1, "the prefix is not coloured");
            assert_eq!(spans[i].last().unwrap().range.end, lines[i].text.len());
        }
        // a keyword and a string get different colours
        let colours = |i: usize| spans[i].iter().map(|s| s.fg).collect::<std::collections::HashSet<_>>().len();
        assert!(colours(6) >= 3);
        // other files are not highlighted
        let txt = parse_diff_lines("diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1 +1 @@\n-a\n+b\n");
        assert!(highlight_diff(&txt, true).iter().all(Vec::is_empty));
    }

    #[test]
    fn text_lines_are_coloured() {
        let lines: Vec<String> = ["def f(x):", "    return \"x\""].iter().map(|s| s.to_string()).collect();
        let spans = highlight_text(&lines, "a.py", false);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[1].last().unwrap().range.end, lines[1].len());
        assert!(highlight_text(&lines, "a.unknown", false).is_empty());
    }
}
