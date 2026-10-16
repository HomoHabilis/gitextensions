//! Port of `FileViewer`: shows diffs (with line numbers and colors) or file content, and
//! supports selecting lines for staging, unstaging and resetting.

use egui::{Color32, FontId, Pos2, RichText, Sense, Ui, Vec2};
use gitext_core::inline_diff::{inline_differences, LineMarks};
use gitext_core::patch::{parse_diff_lines, strip_ansi, DiffLine, DiffLineKind};
use gitext_core::settings::{AppSettings, DiffAppearance};

use super::highlight::{highlight_diff, highlight_text, parse_ansi, Span};
use crate::i18n::{tr, trf};
use crate::tasks::Task;
use crate::theme::Palette;

/// What is displayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewerContent {
    Empty(String),
    Diff(String),
    Text(String),
    Binary(String),
    /// git's coloured output (`--color=always`): word diff, difftastic, range-diff, grep.
    Colored(String),
}

#[derive(Default)]
pub struct DiffViewer {
    /// The content shown, compared each frame to detect changes.
    content: Option<ViewerContent>,
    lines: Vec<DiffLine>,
    /// The changed words of each modified line (`MarkInlineDifferences`).
    marks: Vec<Option<LineMarks>>,
    /// The marks of a large diff, found in the background.
    marks_task: Option<Task<Vec<Option<LineMarks>>>>,
    /// The colours of each line (syntax highlighting, or git's colours); none when empty.
    styles: Vec<Vec<Span>>,
    styles_task: Option<Task<Vec<Vec<Span>>>>,
    /// What `styles` were made for: dark mode, syntax highlighting on.
    styles_for: Option<(bool, bool)>,
    /// The file shown as text, for its syntax.
    file_name: Option<String>,
    /// Don't colour the code (the `syntax_highlighting` setting, applied by the toolbar).
    no_syntax: bool,
    /// The toolbar offers the word diff and difftastic views (the diff tab loads them).
    pub appearance_menu: bool,
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
    /// The "go to line" box is shown (`FormGoToLine`).
    goto_open: bool,
    goto_text: String,
    goto_focus: bool,
    /// Show the content even though it is larger than [`LARGE_CONTENT`].
    show_large: bool,
    /// Draw spaces and tabs as visible characters (`ShowNonPrintingChars`).
    show_whitespace: bool,
}

/// Content larger than this is shown only on request, like upstream's 5 MB preview guard:
/// laying out millions of lines makes the viewer slow.
const LARGE_CONTENT: usize = 5 * 1024 * 1024;

/// Diffs with more lines get their changed words in the background (about 1 s for 80,000
/// changed lines), smaller ones before they are first shown so the highlighting doesn't flicker.
const BACKGROUND_MARKS_LINES: usize = 2000;

/// Commands from the diff viewer context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerCommand {
    StageSelectedLines,
    UnstageSelectedLines,
    ResetSelectedLines,
    CopyPatch,
    /// Copy the selected lines as they were before the change (context and removed lines).
    CopyOldVersion,
    /// Copy the selected lines as they are after the change (context and added lines).
    CopyNewVersion,
    /// Insert the selected lines into the commit message (commit dialog).
    AddToCommitMessage,
}

impl ViewerCommand {
    fn label(self) -> &'static str {
        match self {
            ViewerCommand::StageSelectedLines => tr("Stage selected lines"),
            ViewerCommand::UnstageSelectedLines => tr("Unstage selected lines"),
            ViewerCommand::ResetSelectedLines => tr("Reset selected lines…"),
            ViewerCommand::CopyPatch => tr("Copy"),
            ViewerCommand::CopyOldVersion => tr("Copy old version"),
            ViewerCommand::CopyNewVersion => tr("Copy new version"),
            ViewerCommand::AddToCommitMessage => tr("Add selection to commit message"),
        }
    }

    /// The key of the command (`FileViewer` / `FormCommit` hotkeys).
    fn key(self) -> Option<(egui::Modifiers, egui::Key)> {
        match self {
            ViewerCommand::StageSelectedLines => Some((egui::Modifiers::NONE, egui::Key::S)),
            ViewerCommand::UnstageSelectedLines => Some((egui::Modifiers::NONE, egui::Key::U)),
            ViewerCommand::ResetSelectedLines => Some((egui::Modifiers::NONE, egui::Key::R)),
            ViewerCommand::CopyPatch => Some((egui::Modifiers::COMMAND, egui::Key::C)),
            ViewerCommand::CopyOldVersion | ViewerCommand::CopyNewVersion => None,
            ViewerCommand::AddToCommitMessage => Some((egui::Modifiers::NONE, egui::Key::C)),
        }
    }
}

impl DiffViewer {
    fn set_content(&mut self, ctx: &egui::Context, c: &ViewerContent) {
        if let Some(marks) = self.marks_task.as_mut().and_then(Task::try_take) {
            self.marks = marks;
            self.marks_task = None;
        }
        if self.content.as_ref() == Some(c) {
            return;
        }
        self.content = Some(c.clone());
        self.styles.clear();
        self.styles_task = None;
        self.styles_for = None;
        self.selected.clear();
        self.anchor = None;
        self.find_match = None;
        self.show_large = false;
        match c {
            ViewerContent::Diff(d) => {
                self.is_diff = true;
                self.lines = parse_diff_lines(&strip_ansi(d));
                self.marks.clear();
                self.marks_task = None;
                if self.lines.len() <= BACKGROUND_MARKS_LINES {
                    self.marks = inline_differences(&self.lines);
                } else {
                    let lines = self.lines.clone();
                    self.marks_task = Some(Task::spawn(ctx, move || inline_differences(&lines)));
                }
                self.text_lines.clear();
            }
            ViewerContent::Text(t) => {
                self.is_diff = false;
                self.lines.clear();
                self.marks.clear();
                self.marks_task = None;
                self.text_lines = t.lines().map(|l| l.replace('\t', "    ")).collect();
            }
            ViewerContent::Colored(t) => {
                self.is_diff = false;
                self.lines.clear();
                self.marks.clear();
                self.marks_task = None;
                // the colours depend on the palette: parsed again in `update_styles`
                self.text_lines = parse_ansi(t, &crate::theme::Palette::new(true)).0;
            }
            _ => {
                self.is_diff = false;
                self.lines.clear();
                self.marks.clear();
                self.marks_task = None;
                self.text_lines.clear();
            }
        }
        self.max_len = (0..self.line_count()).map(|i| self.line_text(i).chars().count()).max().unwrap_or(0);
    }

    /// Names the file shown as text, so that it is coloured by its language.
    pub fn set_file_name(&mut self, name: Option<&str>) {
        if self.file_name.as_deref() != name {
            self.file_name = name.map(str::to_string);
            self.styles_for = None;
        }
    }

    /// Finds the colours of the lines when the content, the theme or the setting changed:
    /// syntax highlighting runs in the background (syntect does about 300 KB/s).
    fn update_styles(&mut self, ui: &Ui) {
        if let Some(styles) = self.styles_task.as_mut().and_then(Task::try_take) {
            self.styles = styles;
            self.styles_task = None;
        }
        let key = (ui.visuals().dark_mode, !self.no_syntax);
        if self.styles_for == Some(key) {
            return;
        }
        self.styles_for = Some(key);
        self.styles.clear();
        self.styles_task = None;
        let (dark, syntax) = key;
        match &self.content {
            Some(ViewerContent::Colored(t)) => self.styles = parse_ansi(t, &Palette::for_ui(ui)).1,
            Some(ViewerContent::Diff(_)) if syntax => {
                let lines = self.lines.clone();
                self.styles_task = Some(Task::spawn(ui.ctx(), move || highlight_diff(&lines, dark)));
            }
            Some(ViewerContent::Text(_)) if syntax => {
                if let Some(name) = self.file_name.clone().filter(|n| super::highlight::has_syntax(n)) {
                    let lines = self.text_lines.clone();
                    self.styles_task = Some(Task::spawn(ui.ctx(), move || highlight_text(&lines, &name, dark)));
                }
            }
            _ => {}
        }
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

    /// The selected lines of one side of the diff, without the diff markers: the old version
    /// (context and removed lines) or the new one (context and added lines).
    pub fn selected_version(&self, new: bool) -> String {
        let lines: Vec<&str> = self
            .selected_lines()
            .into_iter()
            .filter_map(|i| {
                let l = self.lines.get(i)?;
                let keep = match l.kind {
                    DiffLineKind::Context => true,
                    DiffLineKind::Added => new,
                    DiffLineKind::Removed => !new,
                    _ => false,
                };
                keep.then(|| l.text.get(1..).unwrap_or_default())
            })
            .collect();
        lines.join("\n")
    }

    /// The line index of line number `n` (1-based): of the new version in a diff (the closest
    /// following line when `n` is not shown), of the text otherwise.
    fn line_index(&self, n: usize) -> Option<usize> {
        if n == 0 {
            return None;
        }
        if !self.is_diff {
            return (self.line_count() > 0).then(|| (n - 1).min(self.line_count() - 1));
        }
        self.lines.iter().position(|l| l.new_line.is_some_and(|x| x as usize >= n)).or_else(|| self.lines.iter().rposition(|l| l.new_line.is_some()))
    }

    /// Selects and shows line number `n` (`GoToLine`).
    pub fn go_to_line(&mut self, n: usize) {
        if let Some(i) = self.line_index(n) {
            self.selected = vec![i];
            self.anchor = Some(i);
            self.scroll_to = Some(i);
        }
    }

    /// The toolbar above the viewer (port of the `FileViewer` toolbar): previous / next change,
    /// whitespace options, context lines, entire file and go to line. The diff options are kept
    /// in `settings`, like upstream remembers them; returns true when it changed them (the
    /// caller saves the settings, and the diffs reload as their options are part of their key).
    pub fn toolbar(&mut self, ui: &mut Ui, content: &ViewerContent, settings: &mut AppSettings) -> bool {
        self.set_content(ui.ctx(), content);
        let before = settings.clone();
        let diff = self.is_diff;
        // the options of the git command producing a coloured view (word diff, difftastic, grep)
        let options = diff || matches!(content, ViewerContent::Colored(_));
        // wraps in a narrow pane rather than widening it (commit details right of the graph)
        ui.horizontal_wrapped(|ui| {
            ui.set_max_width(ui.available_width());
            ui.spacing_mut().item_spacing.x = 4.0;
            if ui.add_enabled(diff, egui::Button::new("⏶").small()).on_hover_text(format!("{} (Alt+Up)", tr("Previous change"))).clicked() {
                self.select_change(true);
            }
            if ui.add_enabled(diff, egui::Button::new("⏷").small()).on_hover_text(format!("{} (Alt+Down)", tr("Next change"))).clicked() {
                self.select_change(false);
            }
            ui.separator();
            ui.add_enabled_ui(options, |ui| {
                // the label says which option is on, so an ignored change is never a surprise
                let label = if settings.ignore_whitespace {
                    tr("Whitespace: ignore all")
                } else if settings.ignore_whitespace_changes {
                    tr("Whitespace: ignore amount")
                } else if settings.ignore_whitespace_at_eol {
                    tr("Whitespace: ignore at EOL")
                } else {
                    tr("Whitespace")
                };
                ui.menu_button(RichText::new(label).small(), |ui| {
                    ui.checkbox(&mut settings.ignore_whitespace, tr("Ignore all whitespace changes (-w)"));
                    ui.add_enabled(
                        !settings.ignore_whitespace,
                        egui::Checkbox::new(&mut settings.ignore_whitespace_changes, tr("Ignore changes in amount of whitespace (-b)")),
                    );
                    ui.add_enabled(
                        !settings.ignore_whitespace && !settings.ignore_whitespace_changes,
                        egui::Checkbox::new(&mut settings.ignore_whitespace_at_eol, tr("Ignore whitespace changes at end of line")),
                    );
                });
            });
            if ui
                .selectable_label(settings.show_whitespace, RichText::new(tr("Show whitespace")).small())
                .on_hover_text(tr("Show whitespace characters"))
                .clicked()
            {
                settings.show_whitespace = !settings.show_whitespace;
            }
            ui.separator();
            ui.add_enabled_ui(options && !settings.show_entire_file, |ui| {
                if ui.add_enabled(settings.context_lines > 0, egui::Button::new("−").small()).on_hover_text(tr("Fewer lines of context")).clicked() {
                    settings.context_lines -= 1;
                }
                ui.label(RichText::new(trf("{0} lines", &[&settings.context_lines])).small()).on_hover_text(tr("Lines of context around the changes"));
                if ui.add(egui::Button::new("+").small()).on_hover_text(tr("More lines of context")).clicked() {
                    settings.context_lines += 1;
                }
            });
            if ui
                .add_enabled(options, egui::Button::new(RichText::new(tr("Entire file")).small()).selected(settings.show_entire_file))
                .on_hover_text(tr("Show the entire file with the changes"))
                .clicked()
            {
                settings.show_entire_file = !settings.show_entire_file;
            }
            ui.separator();
            if ui.add(egui::Button::new(RichText::new(tr("Go to line…")).small()).selected(self.goto_open)).on_hover_text("Ctrl+G").clicked() {
                self.open_go_to_line();
            }
            if ui
                .selectable_label(settings.syntax_highlighting, RichText::new(tr("Syntax colours")).small())
                .on_hover_text(tr("Colour the code by its language"))
                .clicked()
            {
                settings.syntax_highlighting = !settings.syntax_highlighting;
            }
            if self.appearance_menu {
                let label = match settings.diff_appearance {
                    DiffAppearance::Patch => tr("View: patch"),
                    DiffAppearance::WordDiff => tr("View: word diff"),
                    DiffAppearance::Difftastic => tr("View: difftastic"),
                };
                ui.menu_button(RichText::new(label).small(), |ui| {
                    ui.radio_value(&mut settings.diff_appearance, DiffAppearance::Patch, tr("Patch"));
                    ui.radio_value(&mut settings.diff_appearance, DiffAppearance::WordDiff, tr("Git word diff"));
                    ui.radio_value(&mut settings.diff_appearance, DiffAppearance::Difftastic, tr("Difftastic"))
                        .on_hover_text(tr("Structural diff, needs difftastic set up as a git difftool (difftool.difftastic.cmd)"));
                });
            }
        });
        self.show_whitespace = settings.show_whitespace;
        self.no_syntax = !settings.syntax_highlighting;
        *settings != before
    }

    fn open_go_to_line(&mut self) {
        self.goto_open = true;
        self.goto_focus = true;
        self.goto_text.clear();
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
        if self.is_diff {
            self.lines.len()
        } else {
            self.text_lines.len()
        }
    }

    /// The content shown (tests).
    #[cfg(test)]
    pub(crate) fn content(&self) -> Option<&ViewerContent> {
        self.content.as_ref()
    }

    fn line_text(&self, i: usize) -> &str {
        if self.is_diff {
            &self.lines[i].text
        } else {
            &self.text_lines[i]
        }
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
        self.set_content(ui.ctx(), content);
        self.update_styles(ui);
        let palette = Palette::for_ui(ui);
        let mut command = None;
        match content {
            ViewerContent::Empty(msg) | ViewerContent::Binary(msg) => {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new(msg).italics().color(palette.muted));
                });
                return None;
            }
            ViewerContent::Diff(t) | ViewerContent::Text(t) if t.len() > LARGE_CONTENT && !self.show_large => {
                ui.centered_and_justified(|ui| {
                    let text = trf("This file is over {0} MB. Click to show it.", &[&(LARGE_CONTENT / (1024 * 1024))]);
                    if ui.link(RichText::new(text).italics()).clicked() {
                        self.show_large = true;
                    }
                });
                return None;
            }
            _ => {}
        }

        if (ui.ui_contains_pointer() || self.has_focus(ui.ctx())) && ui.input(|i| i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::F)) {
            self.find_open = true;
        }
        if self.has_focus(ui.ctx()) && crate::views::shortcut(ui.ctx(), egui::Modifiers::COMMAND, egui::Key::G) {
            self.open_go_to_line();
        }
        if self.goto_open {
            ui.horizontal(|ui| {
                ui.label(tr("Go to line:"));
                let r = ui.add(egui::TextEdit::singleline(&mut self.goto_text).desired_width(80.0));
                if std::mem::take(&mut self.goto_focus) {
                    r.request_focus();
                }
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if enter || ui.button(tr("Go")).clicked() {
                    if let Ok(n) = self.goto_text.trim().parse::<usize>() {
                        self.go_to_line(n);
                        self.goto_open = false;
                        self.has_focus = true;
                    }
                }
                if ui.button("✖").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape))) {
                    self.goto_open = false;
                }
            });
        }
        if self.find_open {
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.find).hint_text(tr("Find…")).desired_width(220.0));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.find_next(false);
                    r.request_focus();
                }
                if ui.button(tr("Next")).on_hover_text(tr("F3 (Shift+F3: previous)")).clicked() {
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
        // git's coloured views number their lines themselves (grep), or have none to number
        let show_line_numbers = show_line_numbers && !matches!(content, ViewerContent::Colored(_));
        let gutter = if show_line_numbers {
            if self.is_diff {
                char_w * 11.0
            } else {
                char_w * (count.max(1).ilog10() as f32 + 2.0)
            }
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
        let area_rect = area
            .show_rows(ui, row_h, count, |ui, range| {
                let width = content_w.max(ui.available_width());
                ui.set_min_width(width);
                for i in range {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, row_h), Sense::click_and_drag());
                    let painter = ui.painter_at(rect);
                    let (bg, fg) = self.colors(i, &palette, ui.visuals().text_color());
                    if let Some(bg) = bg {
                        painter.rect_filled(rect, 0.0, bg);
                    }
                    let x = rect.left() + 4.0;
                    let text_x = x + if show_line_numbers { gutter } else { 0.0 } + 4.0;
                    let text = self.line_text(i);
                    let kind = self.is_diff.then(|| self.lines[i].kind);
                    let galley = match self.styles.get(i).filter(|s| !s.is_empty()) {
                        Some(spans) => painter.layout_job(styled_line(text, kind, spans, self.show_whitespace, &font, fg)),
                        None if self.show_whitespace => painter.layout_no_wrap(visible_whitespace(text, kind), font.clone(), fg),
                        None => painter.layout_no_wrap(text.to_string(), font.clone(), fg),
                    };
                    let text_pos = Pos2::new(text_x, rect.center().y - galley.size().y / 2.0);
                    if let Some(marks) = self.marks.get(i).and_then(Option::as_ref) {
                        let added = kind == Some(DiffLineKind::Added);
                        let col = |c: usize| {
                            let c = if self.show_whitespace { whitespace_column(text, c) } else { c };
                            text_pos.x + galley.pos_from_cursor(egui::text::CCursor::new(c)).min.x
                        };
                        let word_bg = if added { palette.diff_added_word_bg } else { palette.diff_removed_word_bg };
                        for r in &marks.changed {
                            let span = egui::Rect::from_x_y_ranges(col(r.start)..=col(r.end), rect.y_range());
                            painter.rect_filled(span, 0.0, word_bg);
                        }
                        if let Some(a) = marks.anchor {
                            // text was inserted in the paired line here
                            let anchor = if added { palette.diff_removed_fg } else { palette.diff_added_fg };
                            let ax = col(a);
                            painter.line_segment([Pos2::new(ax, rect.top()), Pos2::new(ax, rect.bottom())], egui::Stroke::new(2.0_f32, anchor));
                        }
                    }
                    if self.selected.contains(&i) {
                        painter.rect_filled(rect, 0.0, palette.diff_selected_bg);
                    }
                    if self.find_match == Some(i) {
                        painter.rect_stroke(rect.shrink(0.5), 0.0, egui::Stroke::new(1.0_f32, palette.warning), egui::StrokeKind::Middle);
                    }
                    if show_line_numbers {
                        let nums = if self.is_diff {
                            let l = &self.lines[i];
                            format!("{:>4} {:>4}", l.old_line.map(|n| n.to_string()).unwrap_or_default(), l.new_line.map(|n| n.to_string()).unwrap_or_default())
                        } else {
                            format!("{:>w$}", i + 1, w = (count.max(1).ilog10() + 1) as usize)
                        };
                        painter.text(Pos2::new(x, rect.center().y), egui::Align2::LEFT_CENTER, nums, font.clone(), palette.line_number);
                        painter.line_segment(
                            [Pos2::new(x + gutter - 4.0, rect.top()), Pos2::new(x + gutter - 4.0, rect.bottom())],
                            egui::Stroke::new(1.0_f32, palette.line_number.gamma_multiply(0.3)),
                        );
                    }
                    painter.galley(text_pos, galley, fg);
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
                    if ui.input(|inp| inp.pointer.primary_down())
                        && self.drag_start.is_some()
                        && rect.contains(ui.input(|inp| inp.pointer.interact_pos().unwrap_or_default()))
                    {
                        drag_line = Some(i);
                    }
                    resp.context_menu(|ui| {
                        let copy_versions = if self.is_diff { &[ViewerCommand::CopyOldVersion, ViewerCommand::CopyNewVersion][..] } else { &[] };
                        for &c in menu.iter().chain(copy_versions) {
                            let enabled = self.can_run(c);
                            let mut button = egui::Button::new(c.label());
                            if let Some((m, k)) = c.key() {
                                button = button.shortcut_text(ui.ctx().format_shortcut(&egui::KeyboardShortcut::new(m, k)));
                            }
                            if ui.add_enabled(enabled, button).clicked() {
                                command = Some(c);
                                ui.close_kind(egui::UiKind::Menu);
                            }
                        }
                        if ui
                            .add(
                                egui::Button::new(tr("Find…"))
                                    .shortcut_text(ui.ctx().format_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::F))),
                            )
                            .clicked()
                        {
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
                let Some((m, k)) = c.key() else { continue };
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
        match command {
            Some(ViewerCommand::CopyPatch) => {
                let text: Vec<&str> = self.selected_lines().iter().map(|&i| self.line_text(i)).collect();
                crate::util::copy_to_clipboard(ui.ctx(), text.join("\n"));
                command = None;
            }
            Some(c @ (ViewerCommand::CopyOldVersion | ViewerCommand::CopyNewVersion)) => {
                crate::util::copy_to_clipboard(ui.ctx(), self.selected_version(c == ViewerCommand::CopyNewVersion));
                command = None;
            }
            _ => {}
        }
        command
    }

    fn can_run(&self, c: ViewerCommand) -> bool {
        match c {
            ViewerCommand::CopyPatch | ViewerCommand::AddToCommitMessage | ViewerCommand::CopyOldVersion | ViewerCommand::CopyNewVersion => {
                !self.selected.is_empty()
            }
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
        return ViewerContent::Binary(trf("Binary file ({0} bytes)", &[&bytes.len()]));
    }
    ViewerContent::Text(String::from_utf8_lossy(bytes).into_owned())
}

/// Shows spaces as `·` and tabs as `→` in file content. In a diff only the content of
/// context, added and removed lines changes: not their `+`/`-`/space prefix, nor the headers.
fn visible_whitespace(text: &str, kind: Option<DiffLineKind>) -> String {
    let show = |t: &str| t.replace(' ', "·").replace('\t', "→   ");
    match kind {
        None => show(text),
        Some(DiffLineKind::Context | DiffLineKind::Added | DiffLineKind::Removed) => {
            let prefix = text.chars().next().map_or(0, char::len_utf8);
            format!("{}{}", &text[..prefix], show(&text[prefix..]))
        }
        Some(_) => text.to_string(),
    }
}

/// Where whitespace is shown in a line of `kind` (see [`visible_whitespace`]): after the
/// prefix of diff content lines, nowhere in diff headers, everywhere in files.
fn whitespace_from(text: &str, kind: Option<DiffLineKind>) -> Option<usize> {
    match kind {
        None => Some(0),
        Some(DiffLineKind::Context | DiffLineKind::Added | DiffLineKind::Removed) => Some(text.chars().next().map_or(0, char::len_utf8)),
        Some(_) => None,
    }
}

/// A line with its colours (`spans`, in bytes; the rest has colour `fg`).
fn styled_line(text: &str, kind: Option<DiffLineKind>, spans: &[Span], show_whitespace: bool, font: &FontId, fg: Color32) -> egui::text::LayoutJob {
    let ws_from = whitespace_from(text, kind).filter(|_| show_whitespace);
    let mut cuts: Vec<usize> = spans
        .iter()
        .flat_map(|s| [s.range.start, s.range.end])
        .chain([0, text.len()])
        .chain(ws_from)
        .filter(|&c| c <= text.len() && text.is_char_boundary(c))
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    let mut job = egui::text::LayoutJob::default();
    for w in cuts.windows(2) {
        let (start, end) = (w[0], w[1]);
        let span = spans.iter().find(|s| s.range.start <= start && end <= s.range.end);
        let mut format = egui::TextFormat::simple(font.clone(), span.and_then(|s| s.fg).unwrap_or(fg));
        if let Some(bg) = span.and_then(|s| s.bg) {
            format.background = bg;
        }
        let piece = &text[start..end];
        if ws_from.is_some_and(|p| start >= p) {
            job.append(&piece.replace(' ', "·").replace('\t', "→   "), 0.0, format);
        } else {
            job.append(piece, 0.0, format);
        }
    }
    job
}

/// The column of char `c` of a diff line once [`visible_whitespace`] made tabs 4 chars wide.
fn whitespace_column(text: &str, c: usize) -> usize {
    c + 3 * text.chars().take(c).skip(1).filter(|&ch| ch == '\t').count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styled_lines_show_whitespace_like_plain_ones() {
        let font = FontId::monospace(12.0);
        let spans = [Span { range: 1..4, fg: Some(Color32::RED), bg: None }, Span { range: 5..9, fg: None, bg: Some(Color32::BLUE) }];
        for (text, kind) in [("+\tlet x = 1;", Some(DiffLineKind::Added)), ("@@ -1 +1 @@ x", Some(DiffLineKind::HunkHeader)), ("a b\tc d", None)] {
            for ws in [false, true] {
                let job = styled_line(text, kind, &spans, ws, &font, Color32::WHITE);
                let expected = if ws { visible_whitespace(text, kind) } else { text.to_string() };
                assert_eq!(job.text, expected, "{text:?} {ws}");
            }
        }
        let job = styled_line("+abcdefgh", Some(DiffLineKind::Added), &spans, false, &font, Color32::WHITE);
        let colours: Vec<(Color32, Color32)> = job.sections.iter().map(|s| (s.format.color, s.format.background)).collect();
        assert_eq!(colours[0].0, Color32::WHITE, "the prefix keeps the line colour");
        assert_eq!(colours[1].0, Color32::RED);
        assert_eq!(colours[3].1, Color32::BLUE);
    }

    #[test]
    fn whitespace_is_shown_in_content_only() {
        assert_eq!(visible_whitespace("a b\tc", None), "a·b→   c");
        assert_eq!(visible_whitespace(" x y", Some(DiffLineKind::Context)), " x·y");
        assert_eq!(visible_whitespace("+x ", Some(DiffLineKind::Added)), "+x·");
        assert_eq!(visible_whitespace("@@ -1 +1 @@", Some(DiffLineKind::HunkHeader)), "@@ -1 +1 @@");
    }

    fn run_frame(
        ctx: &egui::Context,
        viewer: &mut DiffViewer,
        content: &ViewerContent,
        menu: &[ViewerCommand],
        events: Vec<egui::Event>,
    ) -> Option<ViewerCommand> {
        let modifiers = events.iter().find_map(|e| if let egui::Event::Key { modifiers, .. } = e { Some(*modifiers) } else { None }).unwrap_or_default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0))),
            events,
            modifiers,
            ..Default::default()
        };
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
    fn copy_versions_and_go_to_line() {
        let diff = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n one\n-two\n+2\n three\n@@ -10,2 +10,2 @@\n ten\n-eleven\n+11\n";
        let content = ViewerContent::Diff(diff.into());
        let ctx = egui::Context::default();
        let mut viewer = DiffViewer::default();
        run_frame(&ctx, &mut viewer, &content, &[], vec![]);
        viewer.selected = (0..viewer.lines.len()).collect();
        assert_eq!(viewer.selected_version(false), "one\ntwo\nthree\nten\neleven");
        assert_eq!(viewer.selected_version(true), "one\n2\nthree\nten\n11");
        // line numbers of the new version; a hidden line goes to the next shown one
        viewer.go_to_line(2);
        assert_eq!(viewer.selected_text(), "2");
        viewer.go_to_line(5);
        assert_eq!(viewer.selected_text(), "ten");
        viewer.go_to_line(999);
        assert_eq!(viewer.selected_text(), "11");
        let text = ViewerContent::Text("a\nb\nc".into());
        run_frame(&ctx, &mut viewer, &text, &[], vec![]);
        viewer.go_to_line(2);
        assert_eq!(viewer.selected_text(), "b");
    }

    #[test]
    fn large_content_is_shown_on_request() {
        let content = ViewerContent::Text("x\n".repeat(LARGE_CONTENT / 2 + 1));
        let ctx = egui::Context::default();
        let mut viewer = DiffViewer::default();
        run_frame(&ctx, &mut viewer, &content, &[], vec![]);
        assert!(!viewer.show_large);
        viewer.show_large = true;
        run_frame(&ctx, &mut viewer, &content, &[], vec![]);
        // the same content keeps the choice, new content asks again
        assert!(viewer.show_large);
        run_frame(&ctx, &mut viewer, &ViewerContent::Text("y\n".repeat(LARGE_CONTENT / 2 + 1)), &[], vec![]);
        assert!(!viewer.show_large);
    }

    #[test]
    fn binary_detection() {
        assert!(matches!(content_from_bytes(b"abc\0def"), ViewerContent::Binary(_)));
        assert_eq!(content_from_bytes(b"hello"), ViewerContent::Text("hello".into()));
    }
}
