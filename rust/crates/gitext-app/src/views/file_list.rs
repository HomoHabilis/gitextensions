//! Port of `FileStatusList`: list of changed files, flat or as a folder tree.

use std::collections::HashSet;

use egui::{RichText, Sense, Ui, Vec2};
use gitext_core::file_tree::{create_tree_sorted_by_path, NodeTag, TreeNode};
use gitext_core::status::GitItemStatus;

use crate::theme::Palette;

#[derive(Default)]
pub struct FileList {
    /// Selected item indexes (into the items slice).
    pub selected: Vec<usize>,
    pub tree_mode: bool,
    pub filter: String,
    anchor: Option<usize>,
    /// Item names of the last frame, to keep the selection when the list changes.
    last_names: Vec<String>,
    /// The sorted tree of the last frame, rebuilt when the items, filter or mode change.
    layout: Option<Layout>,
    /// Paths of the folders collapsed in the tree mode.
    collapsed: HashSet<String>,
    /// Whether the arrow keys move the selection (the list was clicked last).
    has_focus: bool,
    /// Scroll the selected item into view on the next frame.
    scroll_to_selected: bool,
}

struct Layout {
    filter: String,
    tree_mode: bool,
    /// The items in display order.
    order: Vec<usize>,
    /// The rows shown: the items, and the folders in the tree mode except inside collapsed ones.
    rows: Vec<Row>,
}

struct Row {
    depth: usize,
    text: String,
    kind: RowKind,
}

enum RowKind {
    Item(usize),
    /// A folder, with its path and the items below it.
    Folder(String, Vec<usize>),
}

/// Appends the rows of `nodes` (`visible` maps the tree's item indexes to the items).
fn add_rows(rows: &mut Vec<Row>, nodes: &[TreeNode], visible: &[usize], collapsed: &HashSet<String>, depth: usize) {
    for node in nodes {
        match &node.tag {
            NodeTag::Folder(path) => {
                let items = node.items().iter().map(|&i| visible[i]).collect();
                rows.push(Row { depth, text: node.text.clone(), kind: RowKind::Folder(path.clone(), items) });
                if !collapsed.contains(path) {
                    add_rows(rows, &node.nodes, visible, collapsed, depth + 1);
                }
            }
            NodeTag::Item(i) => rows.push(Row { depth, text: node.text.clone(), kind: RowKind::Item(visible[*i]) }),
        }
    }
}

#[derive(Default)]
pub struct FileListResponse {
    pub selection_changed: bool,
    pub double_clicked: Option<usize>,
    pub has_focus: bool,
}

pub fn status_badge(ui: &mut Ui, s: &GitItemStatus, palette: &Palette) {
    let c = s.status_char();
    let color = match c {
        'A' => palette.status_added,
        'D' => palette.status_deleted,
        'R' | 'C' => palette.status_renamed,
        'U' => palette.status_conflict,
        '?' | '!' => palette.status_untracked,
        _ => palette.status_modified,
    };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(16.0, 16.0), Sense::hover());
    ui.painter().rect_filled(rect.shrink(1.0), 3.0, color.gamma_multiply(0.22));
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, c, egui::FontId::monospace(11.0), color);
}

impl FileList {
    pub fn selected_items<'a>(&self, items: &'a [GitItemStatus]) -> Vec<&'a GitItemStatus> {
        self.selected.iter().filter_map(|&i| items.get(i)).collect()
    }

    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
    }

    pub fn select_first(&mut self, items: &[GitItemStatus]) {
        if !items.is_empty() {
            self.selected = vec![0];
            self.anchor = Some(0);
        }
    }

    /// Keeps the selection by name when the items change.
    fn sync(&mut self, items: &[GitItemStatus]) -> bool {
        if items.len() == self.last_names.len() && items.iter().zip(&self.last_names).all(|(i, n)| i.name == *n) {
            return false;
        }
        let names: Vec<String> = items.iter().map(|i| i.name.clone()).collect();
        let old: Vec<String> = self.selected.iter().filter_map(|&i| self.last_names.get(i).cloned()).collect();
        self.selected = old.iter().filter_map(|n| names.iter().position(|m| m == n)).collect();
        self.last_names = names;
        self.anchor = self.selected.first().copied();
        self.layout = None;
        true
    }

    /// Sorts and filters the items when they, the filter or the mode changed.
    fn update_layout(&mut self, items: &[GitItemStatus]) {
        if self.layout.as_ref().is_some_and(|l| l.filter == self.filter && l.tree_mode == self.tree_mode) {
            return;
        }
        let filter = self.filter.to_lowercase();
        let visible: Vec<usize> = (0..items.len()).filter(|&i| filter.is_empty() || items[i].name.to_lowercase().contains(&filter)).collect();
        let tree = if visible.len() == items.len() {
            create_tree_sorted_by_path(items, !self.tree_mode, true)
        } else {
            let filtered: Vec<GitItemStatus> = visible.iter().map(|&i| items[i].clone()).collect();
            create_tree_sorted_by_path(&filtered, !self.tree_mode, true)
        };
        // map back to the original indexes
        let order: Vec<usize> = tree.items().iter().map(|&i| visible[i]).collect();
        let mut rows = Vec::new();
        add_rows(&mut rows, &tree.nodes, &visible, &self.collapsed, 0);
        self.layout = Some(Layout { filter: self.filter.clone(), tree_mode: self.tree_mode, order, rows });
    }

    fn click(&mut self, index: usize, order: &[usize], ui: &Ui) {
        let m = ui.input(|i| i.modifiers);
        if m.shift {
            if let Some(a) = self.anchor {
                let pa = order.iter().position(|&x| x == a).unwrap_or(0);
                let pb = order.iter().position(|&x| x == index).unwrap_or(0);
                let (lo, hi) = if pa <= pb { (pa, pb) } else { (pb, pa) };
                self.selected = order[lo..=hi].to_vec();
                return;
            }
        }
        if m.command || m.ctrl {
            if let Some(p) = self.selected.iter().position(|&s| s == index) {
                self.selected.remove(p);
            } else {
                self.selected.push(index);
            }
        } else {
            self.selected = vec![index];
        }
        self.anchor = Some(index);
    }

    /// Shows the list. `menu` draws the context menu for the current selection.
    pub fn ui(&mut self, ui: &mut Ui, id: &str, items: &[GitItemStatus], mut menu: impl FnMut(&mut Ui, &[usize])) -> FileListResponse {
        let mut resp = FileListResponse::default();
        let palette = Palette::for_ui(ui);
        let changed = self.sync(items);
        if changed && self.selected.is_empty() && !items.is_empty() {
            resp.selection_changed = true;
        }

        self.update_layout(items);
        // taken for the frame so that the rows can update the selection
        let layout = self.layout.take().expect("layout");
        let order = &layout.order;

        // keyboard navigation (egui only keeps focus on widgets, so the list tracks its own like the revision grid)
        let has_focus = self.has_focus && !ui.ctx().wants_keyboard_input();
        if has_focus && !order.is_empty() {
            let (up, down, home, end) = ui.input(|i| {
                (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::Home), i.key_pressed(egui::Key::End))
            });
            if up || down || home || end {
                // the items shown (the tree mode hides the items of collapsed folders)
                let shown: Vec<usize> = layout.rows.iter().filter_map(|r| if let RowKind::Item(i) = r.kind { Some(i) } else { None }).collect();
                if !shown.is_empty() {
                    let cur = self.selected.last().and_then(|s| shown.iter().position(|x| x == s));
                    let next = match cur {
                        _ if home => 0,
                        _ if end => shown.len() - 1,
                        Some(p) if up => p.saturating_sub(1),
                        Some(p) => (p + 1).min(shown.len() - 1),
                        None => 0,
                    };
                    if self.selected != [shown[next]] {
                        self.selected = vec![shown[next]];
                        self.anchor = Some(shown[next]);
                        self.scroll_to_selected = true;
                        resp.selection_changed = true;
                    }
                }
            }
        }
        resp.has_focus = has_focus;

        let area = egui::ScrollArea::both().id_salt(id).auto_shrink([false, false]);
        let inner_rect = if items.is_empty() {
            area.show(ui, |ui| {
                ui.label(RichText::new("No changes").italics().color(palette.muted));
            })
            .inner_rect
        } else {
            // only the rows in view are laid out
            ui.spacing_mut().item_spacing.y = 1.0;
            let row_h = ui.spacing().interact_size.y;
            let indent = ui.spacing().indent;
            let mut toggled = None;
            // the row to bring into view, which may be outside the rows laid out
            let scroll_row = self
                .scroll_to_selected
                .then(|| self.selected.last().and_then(|s| layout.rows.iter().position(|r| matches!(r.kind, RowKind::Item(i) if i == *s))))
                .flatten();
            self.scroll_to_selected = false;
            let out = area.show_rows(ui, row_h, layout.rows.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 1.0;
                if let Some(row) = scroll_row {
                    let row_step = row_h + ui.spacing().item_spacing.y;
                    let top = ui.cursor().top() + (row as f32 - range.start as f32) * row_step;
                    let rect = egui::Rect::from_min_size(egui::pos2(ui.cursor().left(), top), Vec2::new(1.0, row_h));
                    ui.scroll_to_rect(rect, None);
                }
                for row in &layout.rows[range] {
                    match &row.kind {
                        RowKind::Item(index) => self.show_item(ui, *index, row.depth as f32 * indent, &row.text, items, order, &palette, &mut resp, &mut menu),
                        RowKind::Folder(path, sub) => {
                            let open = !self.collapsed.contains(path);
                            let r = ui
                                .horizontal(|ui| {
                                    ui.add_space(row.depth as f32 * indent);
                                    let text = RichText::new(format!("{} 🗀 {}", if open { "⏷" } else { "⏵" }, row.text)).color(palette.muted);
                                    ui.add(egui::Button::new(text).frame(false))
                                })
                                .inner;
                            if r.clicked() {
                                toggled = Some(path.clone());
                            }
                            r.context_menu(|ui| menu(ui, sub));
                        }
                    }
                }
            });
            if let Some(path) = toggled {
                if !self.collapsed.remove(&path) {
                    self.collapsed.insert(path);
                }
                self.layout = None;
                self.has_focus = true;
                return resp;
            }
            out.inner_rect
        };
        if ui.input(|i| i.pointer.any_pressed()) {
            self.has_focus = ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| inner_rect.contains(p));
        }
        if self.layout.is_none() {
            self.layout = Some(layout);
        }
        resp
    }

    #[allow(clippy::too_many_arguments)]
    fn show_item(
        &mut self,
        ui: &mut Ui,
        index: usize,
        indent: f32,
        name: &str,
        items: &[GitItemStatus],
        order: &[usize],
        palette: &Palette,
        resp: &mut FileListResponse,
        menu: &mut impl FnMut(&mut Ui, &[usize]),
    ) {
        let item = &items[index];
        let selected = self.selected.contains(&index);
        let r = ui
            .horizontal(|ui| {
                ui.add_space(indent);
                status_badge(ui, item, palette);
                let mut text = name.to_string();
                if let Some(old) = &item.old_name {
                    if item.is_renamed || item.is_copied {
                        text = format!("{text}  (from {old})");
                    }
                }
                if item.is_submodule {
                    text = format!("{text} (submodule{})", if item.is_dirty { ", dirty" } else { "" });
                }
                ui.add(egui::Button::selectable(selected, text))
            })
            .inner;
        let r = r.on_hover_ui(|ui| {
            ui.label(item.description());
        });
        if r.clicked() {
            self.click(index, order, ui);
            resp.selection_changed = true;
        }
        if r.double_clicked() {
            resp.double_clicked = Some(index);
        }
        if r.secondary_clicked() && !selected {
            self.selected = vec![index];
            self.anchor = Some(index);
            resp.selection_changed = true;
        }
        let sel = self.selected.clone();
        r.context_menu(|ui| menu(ui, &sel));
    }

    /// Filter box and view mode toggle.
    pub fn toolbar(&mut self, ui: &mut Ui) {
        // right-to-left so the text box takes exactly the remaining width (a fixed estimate
        // makes a resizable parent panel grow every frame)
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.selectable_label(self.tree_mode, "🌲").on_hover_text("Show as tree").clicked() {
                    self.tree_mode = !self.tree_mode;
                }
                ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Filter files…").desired_width(ui.available_width()));
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitext_core::status::StagedStatus;

    fn key(k: egui::Key) -> egui::Event {
        egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }
    }

    /// The arrow keys move over the rows shown, also those not laid out (only the rows in view
    /// are), and the selected row is scrolled into view.
    #[test]
    fn arrow_keys_move_over_all_rows() {
        let items: Vec<GitItemStatus> = (0..300).map(|i| GitItemStatus::from_status_character(StagedStatus::None, &format!("f{i:03}.txt"), 'M')).collect();
        let ctx = egui::Context::default();
        let mut list = FileList { has_focus: true, ..Default::default() };
        let area_id = std::cell::Cell::new(None);
        let time = std::cell::Cell::new(0.0);
        let frame = |list: &mut FileList, events: Vec<egui::Event>| {
            // a second per frame, so that scroll animations end
            time.set(time.get() + 1.0);
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))), events, time: Some(time.get()), ..Default::default() };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    area_id.set(Some(ui.make_persistent_id(egui::Id::new("test"))));
                    list.ui(ui, "test", &items, |_, _| {});
                });
            });
        };
        frame(&mut list, vec![]);
        frame(&mut list, vec![key(egui::Key::ArrowDown)]);
        assert_eq!(list.selected, [0]);
        frame(&mut list, vec![key(egui::Key::ArrowDown)]);
        assert_eq!(list.selected, [1]);
        frame(&mut list, vec![key(egui::Key::End)]);
        assert_eq!(list.selected, [299]);
        for _ in 0..3 {
            frame(&mut list, vec![]);
        }
        // scrolled to the bottom: the last row is in view
        let offset = egui::scroll_area::State::load(&ctx, area_id.get().unwrap()).unwrap().offset.y;
        let row_step = ctx.style().spacing.interact_size.y + 1.0;
        assert!(offset > 250.0 * row_step, "offset {offset}");
        frame(&mut list, vec![key(egui::Key::ArrowUp)]);
        assert_eq!(list.selected, [298]);
        frame(&mut list, vec![key(egui::Key::Home)]);
        assert_eq!(list.selected, [0]);
    }
}
