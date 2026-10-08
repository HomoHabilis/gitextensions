//! Port of `FileViewer`: shows diffs (with line numbers and colors) or file content, and
//! supports selecting lines for staging, unstaging and resetting.

use egui::{Color32, FontId, Pos2, Sense, Ui, Vec2};
use gitext_core::patch::{parse_diff_lines, strip_ansi, DiffLine, DiffLineKind};

use crate::theme::Palette;

/// What is displayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewerContent {
    Empty(String),
    Diff(String),
    Text(String),
    Binary(String),
}

#[derive(Default)]
pub struct DiffViewer {
    content_key: String,
    lines: Vec<DiffLine>,
    text_lines: Vec<String>,
    is_diff: bool,
    pub selected: Vec<usize>,
    anchor: Option<usize>,
    pub find: String,
    find_open: bool,
    find_match: Option<usize>,
    scroll_to: Option<usize>,
    drag_start: Option<usize>,
}

/// Commands from the diff viewer context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerCommand {
    StageSelectedLines,
    UnstageSelectedLines,
    ResetSelectedLines,
    CopyPatch,
}

impl DiffViewer {
    fn set_content(&mut self, c: &ViewerContent) {
        let key = match c {
            ViewerContent::Empty(s) => format!("e{s}"),
            ViewerContent::Diff(s) => format!("d{s}"),
            ViewerContent::Text(s) => format!("t{s}"),
            ViewerContent::Binary(s) => format!("b{s}"),
        };
        if key == self.content_key {
            return;
        }
        self.content_key = key;
        self.selected.clear();
        self.anchor = None;
        self.find_match = None;
        match c {
            ViewerContent::Diff(d) => {
                self.is_diff = true;
                self.lines = parse_diff_lines(&strip_ansi(d));
                self.text_lines.clear();
            }
            ViewerContent::Text(t) => {
                self.is_diff = false;
                self.lines.clear();
                self.text_lines = t.lines().map(|l| l.replace('\t', "    ")).collect();
            }
            _ => {
                self.is_diff = false;
                self.lines.clear();
                self.text_lines.clear();
            }
        }
    }

    /// The selected line indexes (for creating partial patches).
    pub fn selected_lines(&self) -> Vec<usize> {
        let mut s = self.selected.clone();
        s.sort_unstable();
        s
    }

    pub fn has_change_selection(&self) -> bool {
        self.selected.iter().any(|&i| self.lines.get(i).is_some_and(|l| matches!(l.kind, DiffLineKind::Added | DiffLineKind::Removed)))
    }

    fn line_count(&self) -> usize {
        if self.is_diff { self.lines.len() } else { self.text_lines.len() }
    }

    fn line_text(&self, i: usize) -> &str {
        if self.is_diff { &self.lines[i].text } else { &self.text_lines[i] }
    }

    fn find_next(&mut self) {
        let needle = self.find.to_lowercase();
        if needle.is_empty() {
            return;
        }
        let n = self.line_count();
        let start = self.find_match.map(|m| m + 1).unwrap_or(0);
        for k in 0..n {
            let i = (start + k) % n;
            if self.line_text(i).to_lowercase().contains(&needle) {
                self.find_match = Some(i);
                self.scroll_to = Some(i);
                return;
            }
        }
    }

    /// Shows `content`. `can_stage` etc. enable the context menu entries.
    pub fn ui(&mut self, ui: &mut Ui, content: &ViewerContent, show_line_numbers: bool, menu: &[ViewerCommand]) -> Option<ViewerCommand> {
        self.set_content(content);
        let palette = Palette::for_ui(ui);
        let mut command = None;
        match content {
            ViewerContent::Empty(msg) | ViewerContent::Binary(msg) => {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new(msg).italics().color(palette.muted));
                });
                return None;
            }
            _ => {}
        }

        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) && ui.ui_contains_pointer() {
            self.find_open = true;
        }
        if self.find_open {
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.find).hint_text("Find…").desired_width(220.0));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.find_next();
                    r.request_focus();
                }
                if ui.button("Next").clicked() {
                    self.find_next();
                }
                if ui.button("✖").clicked() {
                    self.find_open = false;
                }
            });
        }

        let font = FontId::monospace(ui.text_style_height(&egui::TextStyle::Monospace));
        let row_h = ui.fonts(|f| f.row_height(&font)) + 2.0;
        let char_w = ui.fonts(|f| f.glyph_width(&font, 'M'));
        let count = self.line_count();
        let max_len = (0..count).map(|i| self.line_text(i).chars().count()).max().unwrap_or(0);
        let gutter = if show_line_numbers {
            if self.is_diff { char_w * 11.0 } else { char_w * (count.max(1).ilog10() as f32 + 2.0) }
        } else {
            0.0
        };
        let content_w = gutter + 8.0 + max_len as f32 * char_w + 20.0;

        let mut area = egui::ScrollArea::both().auto_shrink([false, false]).id_salt("diffviewer");
        if let Some(line) = self.scroll_to.take() {
            area = area.vertical_scroll_offset((line as f32 * row_h - 100.0).max(0.0));
        }
        let mut clicked_line: Option<(usize, bool, bool)> = None;
        let mut hovered_line = None;
        let mut secondary = None;
        let mut drag_line = None;
        area.show_rows(ui, row_h, count, |ui, range| {
            let width = content_w.max(ui.available_width());
            ui.set_min_width(width);
            for i in range {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, row_h), Sense::click_and_drag());
                let painter = ui.painter_at(rect);
                let (bg, fg) = self.colors(i, &palette, ui.visuals().text_color());
                if let Some(bg) = bg {
                    painter.rect_filled(rect, 0.0, bg);
                }
                if self.selected.contains(&i) {
                    painter.rect_filled(rect, 0.0, palette.diff_selected_bg);
                }
                if self.find_match == Some(i) {
                    painter.rect_stroke(rect.shrink(0.5), 0.0, egui::Stroke::new(1.0_f32, palette.warning));
                }
                let mut x = rect.left() + 4.0;
                if show_line_numbers {
                    let nums = if self.is_diff {
                        let l = &self.lines[i];
                        format!(
                            "{:>4} {:>4}",
                            l.old_line.map(|n| n.to_string()).unwrap_or_default(),
                            l.new_line.map(|n| n.to_string()).unwrap_or_default()
                        )
                    } else {
                        format!("{:>w$}", i + 1, w = (count.max(1).ilog10() + 1) as usize)
                    };
                    painter.text(Pos2::new(x, rect.center().y), egui::Align2::LEFT_CENTER, nums, font.clone(), palette.line_number);
                    x += gutter;
                    painter.line_segment(
                        [Pos2::new(x - 4.0, rect.top()), Pos2::new(x - 4.0, rect.bottom())],
                        egui::Stroke::new(1.0_f32, palette.line_number.gamma_multiply(0.3)),
                    );
                }
                painter.text(Pos2::new(x + 4.0, rect.center().y), egui::Align2::LEFT_CENTER, self.line_text(i), font.clone(), fg);
                if resp.clicked() {
                    let m = ui.input(|inp| inp.modifiers);
                    clicked_line = Some((i, m.shift, m.command || m.ctrl));
                }
                if resp.drag_started() {
                    self.drag_start = Some(i);
                }
                if resp.hovered() {
                    hovered_line = Some(i);
                }
                if resp.secondary_clicked() {
                    secondary = Some(i);
                }
                if ui.input(|inp| inp.pointer.primary_down()) && self.drag_start.is_some() && rect.contains(ui.input(|inp| inp.pointer.interact_pos().unwrap_or_default())) {
                    drag_line = Some(i);
                }
                resp.context_menu(|ui| {
                    for c in menu {
                        let label = match c {
                            ViewerCommand::StageSelectedLines => "Stage selected lines",
                            ViewerCommand::UnstageSelectedLines => "Unstage selected lines",
                            ViewerCommand::ResetSelectedLines => "Reset selected lines…",
                            ViewerCommand::CopyPatch => "Copy",
                        };
                        let enabled = *c == ViewerCommand::CopyPatch || self.has_change_selection();
                        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                            command = Some(*c);
                            ui.close_menu();
                        }
                    }
                    if ui.button("Find…").clicked() {
                        self.find_open = true;
                        ui.close_menu();
                    }
                });
            }
        });
        let _ = hovered_line;
        if let Some(l) = drag_line {
            if let Some(s) = self.drag_start {
                let (a, b) = if s <= l { (s, l) } else { (l, s) };
                self.selected = (a..=b).collect();
            }
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            self.drag_start = None;
        }
        if let Some((i, shift, ctrl)) = clicked_line {
            if shift {
                if let Some(a) = self.anchor {
                    let (lo, hi) = if a <= i { (a, i) } else { (i, a) };
                    self.selected = (lo..=hi).collect();
                }
            } else if ctrl {
                if let Some(p) = self.selected.iter().position(|&x| x == i) {
                    self.selected.remove(p);
                } else {
                    self.selected.push(i);
                }
                self.anchor = Some(i);
            } else {
                self.selected = vec![i];
                self.anchor = Some(i);
            }
        }
        if let Some(i) = secondary {
            if !self.selected.contains(&i) {
                self.selected = vec![i];
                self.anchor = Some(i);
            }
        }
        if command == Some(ViewerCommand::CopyPatch) {
            let text: Vec<&str> = self.selected_lines().iter().map(|&i| self.line_text(i)).collect();
            crate::util::copy_to_clipboard(ui.ctx(), text.join("\n"));
            command = None;
        }
        command
    }

    fn colors(&self, i: usize, p: &Palette, text: Color32) -> (Option<Color32>, Color32) {
        if !self.is_diff {
            return (None, text);
        }
        match self.lines[i].kind {
            DiffLineKind::Added => (Some(p.diff_added_bg), p.diff_added_fg),
            DiffLineKind::Removed => (Some(p.diff_removed_bg), p.diff_removed_fg),
            DiffLineKind::HunkHeader => (Some(p.diff_hunk_bg), p.diff_hunk_fg),
            DiffLineKind::Header => (None, p.diff_header_fg),
            DiffLineKind::NoNewline => (None, p.muted),
            _ => (None, text),
        }
    }
}

/// Decides how to show file bytes: text or binary notice.
pub fn content_from_bytes(bytes: &[u8]) -> ViewerContent {
    let probe = &bytes[..bytes.len().min(8000)];
    if probe.contains(&0) {
        return ViewerContent::Binary(format!("Binary file ({} bytes)", bytes.len()));
    }
    ViewerContent::Text(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_detection() {
        assert!(matches!(content_from_bytes(b"abc\0def"), ViewerContent::Binary(_)));
        assert_eq!(content_from_bytes(b"hello"), ViewerContent::Text("hello".into()));
    }
}
