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
    /// The content shown, compared each frame to detect changes.
    content: Option<ViewerContent>,
    lines: Vec<DiffLine>,
    text_lines: Vec<String>,
    is_diff: bool,
    /// The length in characters of the longest line.
    max_len: usize,
    pub selected: Vec<usize>,
    anchor: Option<usize>,
    pub find: String,
    find_open: bool,
    find_match: Option<usize>,
    scroll_to: Option<usize>,
    drag_start: Option<usize>,
    /// Whether the keys act on the viewer (it was clicked last), like the file lists.
    has_focus: bool,
}

/// Commands from the diff viewer context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerCommand {
    StageSelectedLines,
    UnstageSelectedLines,
    ResetSelectedLines,
    CopyPatch,
    /// Insert the selected lines into the commit message (commit dialog).
    AddToCommitMessage,
}

impl ViewerCommand {
    fn label(self) -> &'static str {
        match self {
            ViewerCommand::StageSelectedLines => "Stage selected lines",
            ViewerCommand::UnstageSelectedLines => "Unstage selected lines",
            ViewerCommand::ResetSelectedLines => "Reset selected lines…",
            ViewerCommand::CopyPatch => "Copy",
            ViewerCommand::AddToCommitMessage => "Add selection to commit message",
        }
    }

    /// The key of the command (`FileViewer` / `FormCommit` hotkeys).
    fn key(self) -> (egui::Modifiers, egui::Key) {
        match self {
            ViewerCommand::StageSelectedLines => (egui::Modifiers::NONE, egui::Key::S),
            ViewerCommand::UnstageSelectedLines => (egui::Modifiers::NONE, egui::Key::U),
            ViewerCommand::ResetSelectedLines => (egui::Modifiers::NONE, egui::Key::R),
            ViewerCommand::CopyPatch => (egui::Modifiers::COMMAND, egui::Key::C),
            ViewerCommand::AddToCommitMessage => (egui::Modifiers::NONE, egui::Key::C),
        }
    }
}

impl DiffViewer {
    fn set_content(&mut self, c: &ViewerContent) {
        if self.content.as_ref() == Some(c) {
            return;
        }
        self.content = Some(c.clone());
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
        self.max_len = (0..self.line_count()).map(|i| self.line_text(i).chars().count()).max().unwrap_or(0);
    }

    /// The selected line indexes (for creating partial patches).
    pub fn selected_lines(&self) -> Vec<usize> {
        let mut s = self.selected.clone();
        s.sort_unstable();
        s
    }

    /// Whether the keys act on the viewer (it was clicked last and no text box has the focus).
    pub fn has_focus(&self, ctx: &egui::Context) -> bool {
        self.has_focus && !ctx.wants_keyboard_input()
    }

    /// Gives the viewer the keyboard focus (or takes it away).
    pub fn set_focus(&mut self, focus: bool) {
        self.has_focus = focus;
    }

    /// The text of the selected lines, without the diff markers.
    pub fn selected_text(&self) -> String {
        let lines: Vec<&str> = self
            .selected_lines()
            .into_iter()
            .map(|i| {
                let t = self.line_text(i);
                match self.lines.get(i).map(|l| l.kind) {
                    Some(DiffLineKind::Added | DiffLineKind::Removed | DiffLineKind::Context) if self.is_diff => t.get(1..).unwrap_or_default(),
                    _ => t,
                }
            })
            .collect();
        lines.join("\n")
    }

    fn is_change(&self, i: usize) -> bool {
        self.is_diff && self.lines.get(i).is_some_and(|l| matches!(l.kind, DiffLineKind::Added | DiffLineKind::Removed))
    }

    /// Selects the next (or previous) block of changed lines (`NextChange` / `PreviousChange`).
    fn select_change(&mut self, backwards: bool) {
        let starts: Vec<usize> = (0..self.lines.len()).filter(|&i| self.is_change(i) && (i == 0 || !self.is_change(i - 1))).collect();
        let current = self.selected.iter().min().copied();
        let start = if backwards {
            starts.iter().rev().find(|&&s| current.is_some_and(|c| s < c)).or(starts.first())
        } else {
            starts.iter().find(|&&s| current.is_none_or(|c| s > c)).or(starts.last())
        };
        if let Some(&start) = start {
            let end = (start..self.lines.len()).take_while(|&i| self.is_change(i)).last().unwrap_or(start);
            self.selected = (start..=end).collect();
            self.anchor = Some(start);
            self.scroll_to = Some(start);
        }
    }

    pub fn has_change_selection(&self) -> bool {
        self.selected.iter().any(|&i| self.lines.get(i).is_some_and(|l| matches!(l.kind, DiffLineKind::Added | DiffLineKind::Removed)))
    }

    pub(crate) fn line_count(&self) -> usize {
        if self.is_diff { self.lines.len() } else { self.text_lines.len() }
    }

    fn line_text(&self, i: usize) -> &str {
        if self.is_diff { &self.lines[i].text } else { &self.text_lines[i] }
    }

    fn find_next(&mut self, backwards: bool) {
        let needle = self.find.to_lowercase();
        if needle.is_empty() {
            return;
        }
        let n = self.line_count();
        let start = match self.find_match {
            Some(m) if backwards => m + n - 1,
            Some(m) => m + 1,
            None if backwards => n.saturating_sub(1),
            None => 0,
        };
        for k in 0..n {
            let i = if backwards { (start + n - k) % n } else { (start + k) % n };
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

        if ui.ui_contains_pointer() || self.has_focus(ui.ctx()) {
            if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
                self.find_open = true;
            }
        }
        if self.find_open {
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.find).hint_text("Find…").desired_width(220.0));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.find_next(false);
                    r.request_focus();
                }
                if ui.button("Next").on_hover_text("F3 (Shift+F3: previous)").clicked() {
                    self.find_next(false);
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
        let gutter = if show_line_numbers {
            if self.is_diff { char_w * 11.0 } else { char_w * (count.max(1).ilog10() as f32 + 2.0) }
        } else {
            0.0
        };
        let content_w = gutter + 8.0 + self.max_len as f32 * char_w + 20.0;

        let mut area = egui::ScrollArea::both().auto_shrink([false, false]).id_salt("diffviewer");
        if let Some(line) = self.scroll_to.take() {
            area = area.vertical_scroll_offset((line as f32 * row_h - 100.0).max(0.0));
        }
        let mut clicked_line: Option<(usize, bool, bool)> = None;
        let mut hovered_line = None;
        let mut secondary = None;
        let mut drag_line = None;
        // No vertical gap between rows: the backgrounds of adjacent changed lines must touch.
        // show_rows also adds item_spacing.y to its row stride, so zero it before the call.
        let item_spacing = ui.spacing().item_spacing;
        ui.spacing_mut().item_spacing.y = 0.0;
        let area_rect = area.show_rows(ui, row_h, count, |ui, range| {
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
                    painter.rect_stroke(rect.shrink(0.5), 0.0, egui::Stroke::new(1.0_f32, palette.warning), egui::StrokeKind::Middle);
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
                    for &c in menu {
                        let (m, k) = c.key();
                        let enabled = self.can_run(c);
                        if ui.add_enabled(enabled, egui::Button::new(c.label()).shortcut_text(ui.ctx().format_shortcut(&egui::KeyboardShortcut::new(m, k)))).clicked() {
                            command = Some(c);
                            ui.close_kind(egui::UiKind::Menu);
                        }
                    }
                    if ui.add(egui::Button::new("Find…").shortcut_text(ui.ctx().format_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::F)))).clicked() {
                        self.find_open = true;
                        ui.close_kind(egui::UiKind::Menu);
                    }
                });
            }
        })
        .inner_rect;
        ui.spacing_mut().item_spacing = item_spacing;
        let _ = hovered_line;
        crate::views::track_focus(ui, area_rect, &mut self.has_focus);
        if self.has_focus(ui.ctx()) {
            for &c in menu {
                let (m, k) = c.key();
                if c != ViewerCommand::CopyPatch && self.can_run(c) && crate::views::shortcut(ui.ctx(), m, k) {
                    command = Some(c);
                }
            }
            // Ctrl+C arrives as a copy event, not as a key
            let copy = ui.input_mut(|i| {
                let n = i.events.len();
                i.events.retain(|e| !matches!(e, egui::Event::Copy));
                i.events.len() != n
            });
            if copy && !self.selected.is_empty() {
                command = Some(ViewerCommand::CopyPatch);
            }
            if crate::views::shortcut(ui.ctx(), egui::Modifiers::ALT, egui::Key::ArrowDown) {
                self.select_change(false);
            }
            if crate::views::shortcut(ui.ctx(), egui::Modifiers::ALT, egui::Key::ArrowUp) {
                self.select_change(true);
            }
            if crate::views::shortcut(ui.ctx(), egui::Modifiers::SHIFT, egui::Key::F3) {
                self.find_next(true);
            }
            if crate::views::plain_key(ui.ctx(), egui::Key::F3) {
                self.find_next(false);
            }
        }
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

    fn can_run(&self, c: ViewerCommand) -> bool {
        match c {
            ViewerCommand::CopyPatch | ViewerCommand::AddToCommitMessage => !self.selected.is_empty(),
            _ => self.has_change_selection(),
        }
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

    fn run_frame(ctx: &egui::Context, viewer: &mut DiffViewer, content: &ViewerContent, menu: &[ViewerCommand], events: Vec<egui::Event>) -> Option<ViewerCommand> {
        let modifiers = events.iter().find_map(|e| if let egui::Event::Key { modifiers, .. } = e { Some(*modifiers) } else { None }).unwrap_or_default();
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0))), events, modifiers, ..Default::default() };
        let mut out = None;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                out = viewer.ui(ui, content, true, menu);
            });
        });
        out
    }

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    /// Alt+Down / Alt+Up select the blocks of changed lines, S stages them (only when the viewer
    /// has the focus and the command is offered).
    #[test]
    fn change_navigation_and_line_keys() {
        let diff = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,5 +1,5 @@\n one\n-two\n+2\n three\n-four\n+4\n";
        let content = ViewerContent::Diff(diff.into());
        let menu = [ViewerCommand::StageSelectedLines, ViewerCommand::CopyPatch];
        let ctx = egui::Context::default();
        let mut viewer = DiffViewer::default();
        run_frame(&ctx, &mut viewer, &content, &menu, vec![]);
        let changed: Vec<usize> = (0..viewer.lines.len()).filter(|&i| viewer.is_change(i)).collect();
        assert_eq!(changed.len(), 4);
        // without the focus the keys do nothing
        run_frame(&ctx, &mut viewer, &content, &menu, vec![key(egui::Key::ArrowDown, egui::Modifiers::ALT)]);
        assert!(viewer.selected.is_empty());
        viewer.set_focus(true);
        run_frame(&ctx, &mut viewer, &content, &menu, vec![key(egui::Key::ArrowDown, egui::Modifiers::ALT)]);
        assert_eq!(viewer.selected_lines(), changed[..2]);
        run_frame(&ctx, &mut viewer, &content, &menu, vec![key(egui::Key::ArrowDown, egui::Modifiers::ALT)]);
        assert_eq!(viewer.selected_lines(), changed[2..]);
        run_frame(&ctx, &mut viewer, &content, &menu, vec![key(egui::Key::ArrowUp, egui::Modifiers::ALT)]);
        assert_eq!(viewer.selected_lines(), changed[..2]);
        assert_eq!(viewer.selected_text(), "two\n2");
        assert_eq!(run_frame(&ctx, &mut viewer, &content, &menu, vec![key(egui::Key::S, egui::Modifiers::NONE)]), Some(ViewerCommand::StageSelectedLines));
        // U is not offered here
        assert_eq!(run_frame(&ctx, &mut viewer, &content, &menu, vec![key(egui::Key::U, egui::Modifiers::NONE)]), None);
        // Shift+S is not S
        assert_eq!(run_frame(&ctx, &mut viewer, &content, &menu, vec![key(egui::Key::S, egui::Modifiers::SHIFT)]), None);
    }

    #[test]
    fn binary_detection() {
        assert!(matches!(content_from_bytes(b"abc\0def"), ViewerContent::Binary(_)));
        assert_eq!(content_from_bytes(b"hello"), ViewerContent::Text("hello".into()));
    }
}
