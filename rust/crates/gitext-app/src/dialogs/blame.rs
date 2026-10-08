//! Port of `FormBlame` / `BlameControl`.

use std::sync::Arc;

use egui::{FontId, Pos2, RichText, Sense, Ui, Vec2};
use gitext_core::blame::{GitBlame, GitBlameCommit};
use gitext_core::ObjectId;

use super::{Action, Cx, Dialog, DialogKind};
use crate::tasks::Loader;
use crate::theme::Palette;
use crate::util::format_date;

pub struct BlameDialog {
    file: String,
    rev: ObjectId,
    blame: Loader<(ObjectId, String), Result<GitBlame, String>>,
    selected_line: Option<usize>,
    scroll_to: Option<usize>,
    history: Vec<(ObjectId, Option<usize>)>,
}

impl BlameDialog {
    pub fn new(file: String, rev: ObjectId) -> Self {
        Self::with_line(file, rev, None)
    }

    pub fn with_line(file: String, rev: ObjectId, line: Option<usize>) -> Self {
        BlameDialog { file, rev, blame: Loader::default(), selected_line: line.map(|l| l.saturating_sub(1)), scroll_to: line.map(|l| l.saturating_sub(1)), history: Vec::new() }
    }
}

impl Dialog for BlameDialog {
    fn title(&self) -> String {
        format!("Blame {}", self.file)
    }

    fn id(&self) -> String {
        format!("blame:{}:{}", self.file, self.rev)
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(1100.0, 720.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let palette = Palette::for_ui(ui);
        let Some(m) = cx.module.cloned() else { return false };
        let (detect_moves, detect_copies, ignore_ws) = (cx.settings.detect_copy_in_file_on_blame, cx.settings.detect_copy_in_all_on_blame, cx.settings.ignore_whitespace_on_blame);
        let rev = self.rev;
        let file = self.file.clone();
        let mm = m.clone();
        let blame = self
            .blame
            .request(cx.ctx, (rev, file.clone()), move || mm.blame(rev, &file, detect_moves, detect_copies, ignore_ws).map_err(|e| e.to_string()))
            .cloned();
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.history.is_empty(), egui::Button::new("◀ Back")).clicked() {
                if let Some((r, l)) = self.history.pop() {
                    self.rev = r;
                    self.selected_line = l;
                    self.scroll_to = l;
                }
            }
            ui.label(RichText::new(&self.file).strong());
            ui.label(RichText::new(if self.rev.is_zero() { "working directory".to_string() } else { format!("at {}", self.rev.to_short_string()) }).color(palette.muted));
            if ui.button("File history").clicked() {
                cx.open(super::file_history::FileHistoryDialog::new(self.file.clone()));
            }
        });
        let blame = match blame {
            None => {
                ui.spinner();
                return true;
            }
            Some(Err(e)) => {
                ui.colored_label(palette.error, e);
                return !ui.button("Close").clicked();
            }
            Some(Ok(b)) => b,
        };

        // Commit details of the selected line
        let selected_commit: Option<Arc<GitBlameCommit>> = self.selected_line.and_then(|l| blame.lines.get(l)).map(|l| Arc::clone(&l.commit));
        egui::TopBottomPanel::bottom("blame_info").resizable(true).default_height(110.0).show_inside(ui, |ui| {
            ui.set_min_height(ui.available_height());
            match &selected_commit {
                Some(c) => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(c.object_id.to_short_string()).monospace().strong());
                        ui.label(&c.summary);
                    });
                    ui.label(RichText::new(format!("{} <{}> — {}", c.author, c.author_mail, format_date(c.author_time))).color(palette.muted));
                    ui.horizontal(|ui| {
                        if ui.button("Show commit").clicked() {
                            cx.push(Action::SelectRevision(c.object_id));
                        }
                        if let Some(prev) = c.previous {
                            if ui.button("Blame previous revision").clicked() {
                                let line = self.selected_line.unwrap_or(0) as i64 + 1;
                                let revision = gitext_core::GitRevision::new(c.object_id).with_parents(vec![prev]);
                                let target = m.original_line_in_previous_commit(&revision, &c.file_name, line);
                                self.history.push((self.rev, self.selected_line));
                                self.rev = prev;
                                self.file = c.file_name.clone();
                                self.selected_line = Some((target - 1).max(0) as usize);
                                self.scroll_to = self.selected_line;
                            }
                        }
                        if ui.button("Copy hash").clicked() {
                            cx.push(Action::Copy(c.object_id.to_string()));
                        }
                    });
                }
                None => {
                    ui.label(RichText::new("Click a line to see its commit.").color(palette.muted));
                }
            }
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            let font = FontId::monospace(ui.text_style_height(&egui::TextStyle::Monospace));
            let row_h = ui.fonts(|f| f.row_height(&font)) + 2.0;
            let char_w = ui.fonts(|f| f.glyph_width(&font, 'M'));
            let gutter = char_w * 44.0;
            let mut area = egui::ScrollArea::both().auto_shrink([false, false]);
            if let Some(l) = self.scroll_to.take() {
                area = area.vertical_scroll_offset((l as f32 * row_h - 150.0).max(0.0));
            }
            let max_len = blame.lines.iter().map(|l| l.text.chars().count()).max().unwrap_or(0);
            area.show_rows(ui, row_h, blame.lines.len(), |ui, range| {
                let width = (gutter + 60.0 + max_len as f32 * char_w).max(ui.available_width());
                ui.set_min_width(width);
                for i in range {
                    let line = &blame.lines[i];
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, row_h), Sense::click());
                    let painter = ui.painter_at(rect);
                    let first_of_block = i == 0 || blame.lines[i - 1].commit.object_id != line.commit.object_id;
                    let color = palette.lane(line.commit.object_id.hash_code());
                    painter.rect_filled(egui::Rect::from_min_size(rect.min, Vec2::new(3.0, row_h)), 0.0, color);
                    let same_selected = selected_commit.as_ref().is_some_and(|c| c.object_id == line.commit.object_id);
                    if same_selected {
                        painter.rect_filled(rect, 0.0, palette.diff_selected_bg.gamma_multiply(0.5));
                    }
                    if self.selected_line == Some(i) {
                        painter.rect_filled(rect, 0.0, palette.diff_selected_bg);
                    }
                    if first_of_block {
                        let info = format!(
                            "{} {} {}",
                            line.commit.object_id.to_short_string(),
                            crate::util::format_date(line.commit.author_time).get(..10).unwrap_or_default(),
                            line.commit.author
                        );
                        let info: String = info.chars().take(40).collect();
                        painter.text(Pos2::new(rect.left() + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, info, font.clone(), palette.muted);
                        painter.line_segment([rect.left_top(), rect.right_top()], egui::Stroke::new(0.5_f32, palette.line_number.gamma_multiply(0.4)));
                    }
                    painter.text(
                        Pos2::new(rect.left() + gutter, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        format!("{:>5}", line.final_line_number),
                        font.clone(),
                        palette.line_number,
                    );
                    painter.text(Pos2::new(rect.left() + gutter + char_w * 7.0, rect.center().y), egui::Align2::LEFT_CENTER, line.text.replace('\t', "    "), font.clone(), ui.visuals().text_color());
                    if resp.clicked() {
                        self.selected_line = Some(i);
                    }
                    resp.on_hover_text(line.commit.to_display_string(|s| s.to_string()));
                }
            });
        });
        !ui.input(|i| i.key_pressed(egui::Key::Escape))
    }
}
