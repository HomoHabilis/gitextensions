//! Port of `FileStatusList`: list of changed files, flat or as a folder tree.

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
        let names: Vec<String> = items.iter().map(|i| i.name.clone()).collect();
        if names == self.last_names {
            return false;
        }
        let old: Vec<String> = self.selected.iter().filter_map(|&i| self.last_names.get(i).cloned()).collect();
        self.selected = old.iter().filter_map(|n| names.iter().position(|m| m == n)).collect();
        self.last_names = names;
        self.anchor = self.selected.first().copied();
        true
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

        let filter = self.filter.to_lowercase();
        let visible: Vec<usize> = (0..items.len()).filter(|&i| filter.is_empty() || items[i].name.to_lowercase().contains(&filter)).collect();
        let filtered: Vec<GitItemStatus> = visible.iter().map(|&i| items[i].clone()).collect();
        let tree = create_tree_sorted_by_path(&filtered, !self.tree_mode, true);
        // map back to the original indexes
        let order: Vec<usize> = tree.items().iter().map(|&i| visible[i]).collect();

        // keyboard navigation
        let list_id = ui.make_persistent_id(id);
        let has_focus = ui.memory(|m| m.has_focus(list_id));
        if has_focus && !order.is_empty() {
            let (up, down) = ui.input(|i| (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown)));
            if up || down {
                let cur = self.selected.last().and_then(|s| order.iter().position(|x| x == s));
                let next = match cur {
                    Some(p) if up => p.saturating_sub(1),
                    Some(p) => (p + 1).min(order.len() - 1),
                    None => 0,
                };
                self.selected = vec![order[next]];
                self.anchor = Some(order[next]);
                resp.selection_changed = true;
            }
        }
        resp.has_focus = has_focus;

        egui::ScrollArea::both().id_salt(id).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            if items.is_empty() {
                ui.label(RichText::new("No changes").italics().color(palette.muted));
            }
            self.show_nodes(ui, &tree.nodes, &visible, items, &order, &palette, &mut resp, &mut menu, list_id, 0);
        });
        resp
    }

    #[allow(clippy::too_many_arguments)]
    fn show_nodes(
        &mut self,
        ui: &mut Ui,
        nodes: &[TreeNode],
        visible: &[usize],
        items: &[GitItemStatus],
        order: &[usize],
        palette: &Palette,
        resp: &mut FileListResponse,
        menu: &mut impl FnMut(&mut Ui, &[usize]),
        list_id: egui::Id,
        depth: usize,
    ) {
        for node in nodes {
            match &node.tag {
                NodeTag::Folder(path) => {
                    let header = egui::CollapsingHeader::new(RichText::new(format!("🗀 {}", node.text)).color(palette.muted))
                        .id_salt((list_id, path))
                        .default_open(true)
                        .show(ui, |ui| {
                            self.show_nodes(ui, &node.nodes, visible, items, order, palette, resp, menu, list_id, depth + 1);
                        });
                    header.header_response.context_menu(|ui| {
                        let sub: Vec<usize> = node.items().iter().map(|&i| visible[i]).collect();
                        menu(ui, &sub);
                    });
                }
                NodeTag::Item(i) => {
                    let index = visible[*i];
                    let item = &items[index];
                    let selected = self.selected.contains(&index);
                    let r = ui
                        .horizontal(|ui| {
                            status_badge(ui, item, palette);
                            let mut text = node.text.clone();
                            if let Some(old) = &item.old_name {
                                if item.is_renamed || item.is_copied {
                                    text = format!("{text}  (from {old})");
                                }
                            }
                            if item.is_submodule {
                                text = format!("{text} (submodule{})", if item.is_dirty { ", dirty" } else { "" });
                            }
                            let label = ui.add(egui::SelectableLabel::new(selected, text));
                            label
                        })
                        .inner;
                    let r = r.on_hover_text(item.description());
                    if r.clicked() {
                        ui.memory_mut(|m| m.request_focus(list_id));
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
            }
        }
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
