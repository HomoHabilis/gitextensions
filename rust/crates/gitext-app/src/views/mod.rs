//! Views of the browse window (ports of the `GitUI.UserControls`).

pub mod commit_info;
pub mod dashboard;
pub mod diff_viewer;
pub mod file_list;
pub mod file_tree_view;
pub mod left_panel;
pub mod revision_diff;
pub mod revision_grid;

/// Updates the keyboard focus flag of a list that egui does not track focus for: a pointer press
/// inside `rect` on the list's layer gives it the focus, a press elsewhere takes it away. Presses
/// on menus and popups (e.g. the list's own context menu) leave it unchanged.
pub fn track_focus(ui: &egui::Ui, rect: egui::Rect, has_focus: &mut bool) {
    let Some(pos) = ui.input(|i| if i.pointer.any_pressed() { i.pointer.interact_pos() } else { None }) else { return };
    // panels are not areas, so a press on them reports no layer
    let layer = ui.ctx().layer_id_at(pos).unwrap_or_else(egui::LayerId::background);
    if layer.order >= egui::Order::Foreground {
        return;
    }
    *has_focus = layer == ui.layer_id() && rect.contains(pos);
}

/// Consumes a key press without modifiers.
pub fn plain_key(ctx: &egui::Context, key: egui::Key) -> bool {
    shortcut(ctx, egui::Modifiers::NONE, key)
}

/// Consumes a key press with exactly `modifiers` (egui alone ignores extra Shift and Alt).
pub fn shortcut(ctx: &egui::Context, modifiers: egui::Modifiers, key: egui::Key) -> bool {
    ctx.input_mut(|i| i.modifiers.alt == modifiers.alt && i.modifiers.shift == modifiers.shift && i.consume_key(modifiers, key))
}
