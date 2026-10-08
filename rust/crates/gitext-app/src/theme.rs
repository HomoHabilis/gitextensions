//! Colors and visuals. Replaces the WinForms theming (`AppColor`, `Themes`) with palettes
//! designed to read well in both light and dark mode.

use egui::{Color32, Visuals};
use gitext_core::settings::Theme;
use gitext_graph::LANE_COLOR_COUNT;

/// Colors used by the custom drawn widgets.
#[derive(Debug, Clone)]
pub struct Palette {
    pub lanes: [Color32; LANE_COLOR_COUNT as usize],
    pub non_relative: Color32,
    pub node_outline: Color32,
    pub label_branch: Color32,
    pub label_current_branch: Color32,
    pub label_remote: Color32,
    pub label_tag: Color32,
    pub label_stash: Color32,
    pub label_other: Color32,
    pub label_text: Color32,
    pub diff_added_bg: Color32,
    pub diff_removed_bg: Color32,
    pub diff_added_fg: Color32,
    pub diff_removed_fg: Color32,
    pub diff_hunk_fg: Color32,
    pub diff_hunk_bg: Color32,
    pub diff_header_fg: Color32,
    pub diff_selected_bg: Color32,
    pub line_number: Color32,
    pub muted: Color32,
    pub author_highlight_bg: Color32,
    pub artificial_fg: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub success: Color32,
    pub status_added: Color32,
    pub status_modified: Color32,
    pub status_deleted: Color32,
    pub status_renamed: Color32,
    pub status_conflict: Color32,
    pub status_untracked: Color32,
}

impl Palette {
    pub fn new(dark: bool) -> Self {
        if dark {
            Palette {
                lanes: [
                    Color32::from_rgb(0x5a, 0xb0, 0xff),
                    Color32::from_rgb(0xff, 0x8a, 0x5c),
                    Color32::from_rgb(0x5f, 0xd3, 0x8d),
                    Color32::from_rgb(0xd3, 0x86, 0xff),
                    Color32::from_rgb(0xff, 0xcf, 0x4d),
                    Color32::from_rgb(0x4d, 0xd9, 0xd9),
                    Color32::from_rgb(0xff, 0x6b, 0x9a),
                    Color32::from_rgb(0xa8, 0xd8, 0x4e),
                ],
                non_relative: Color32::from_gray(0x5c),
                node_outline: Color32::from_gray(0xf0),
                label_branch: Color32::from_rgb(0x2e, 0x6b, 0x3e),
                label_current_branch: Color32::from_rgb(0x2f, 0x8a, 0x4c),
                label_remote: Color32::from_rgb(0x3a, 0x55, 0x8c),
                label_tag: Color32::from_rgb(0x7a, 0x62, 0x1c),
                label_stash: Color32::from_rgb(0x6a, 0x3f, 0x8a),
                label_other: Color32::from_rgb(0x55, 0x55, 0x5f),
                label_text: Color32::from_gray(0xf2),
                diff_added_bg: Color32::from_rgb(0x1c, 0x3a, 0x26),
                diff_removed_bg: Color32::from_rgb(0x46, 0x1f, 0x24),
                diff_added_fg: Color32::from_rgb(0xa6, 0xe3, 0xb2),
                diff_removed_fg: Color32::from_rgb(0xf2, 0xa6, 0xad),
                diff_hunk_fg: Color32::from_rgb(0x8f, 0xb4, 0xff),
                diff_hunk_bg: Color32::from_rgb(0x22, 0x2a, 0x3d),
                diff_header_fg: Color32::from_rgb(0xd0, 0xb0, 0x70),
                diff_selected_bg: Color32::from_rgba_unmultiplied(0x5a, 0x8c, 0xff, 0x55),
                line_number: Color32::from_gray(0x78),
                muted: Color32::from_gray(0x96),
                author_highlight_bg: Color32::from_rgb(0x23, 0x29, 0x33),
                artificial_fg: Color32::from_rgb(0x9a, 0xb8, 0xd8),
                warning: Color32::from_rgb(0xff, 0xc1, 0x4d),
                error: Color32::from_rgb(0xff, 0x6b, 0x6b),
                success: Color32::from_rgb(0x6b, 0xd6, 0x8a),
                status_added: Color32::from_rgb(0x6b, 0xd6, 0x8a),
                status_modified: Color32::from_rgb(0x5a, 0xb0, 0xff),
                status_deleted: Color32::from_rgb(0xff, 0x6b, 0x6b),
                status_renamed: Color32::from_rgb(0xd3, 0x86, 0xff),
                status_conflict: Color32::from_rgb(0xff, 0xa0, 0x40),
                status_untracked: Color32::from_gray(0xa0),
            }
        } else {
            Palette {
                lanes: [
                    Color32::from_rgb(0x1f, 0x6f, 0xd1),
                    Color32::from_rgb(0xd9, 0x5c, 0x1c),
                    Color32::from_rgb(0x1a, 0x93, 0x4b),
                    Color32::from_rgb(0x8e, 0x3f, 0xc7),
                    Color32::from_rgb(0xb8, 0x86, 0x00),
                    Color32::from_rgb(0x0e, 0x8f, 0x9a),
                    Color32::from_rgb(0xd0, 0x2a, 0x6a),
                    Color32::from_rgb(0x5c, 0x8a, 0x10),
                ],
                non_relative: Color32::from_gray(0xc4),
                node_outline: Color32::from_gray(0x20),
                label_branch: Color32::from_rgb(0xd6, 0xf2, 0xdc),
                label_current_branch: Color32::from_rgb(0xa8, 0xe6, 0xb6),
                label_remote: Color32::from_rgb(0xd8, 0xe4, 0xfa),
                label_tag: Color32::from_rgb(0xfa, 0xec, 0xc0),
                label_stash: Color32::from_rgb(0xec, 0xdc, 0xf8),
                label_other: Color32::from_rgb(0xe6, 0xe6, 0xea),
                label_text: Color32::from_gray(0x1e),
                diff_added_bg: Color32::from_rgb(0xe3, 0xf7, 0xe6),
                diff_removed_bg: Color32::from_rgb(0xfc, 0xe6, 0xe8),
                diff_added_fg: Color32::from_rgb(0x11, 0x63, 0x29),
                diff_removed_fg: Color32::from_rgb(0x9a, 0x1b, 0x27),
                diff_hunk_fg: Color32::from_rgb(0x2a, 0x4f, 0xa8),
                diff_hunk_bg: Color32::from_rgb(0xea, 0xf0, 0xfb),
                diff_header_fg: Color32::from_rgb(0x8a, 0x5a, 0x00),
                diff_selected_bg: Color32::from_rgba_unmultiplied(0x1f, 0x6f, 0xd1, 0x40),
                line_number: Color32::from_gray(0x9a),
                muted: Color32::from_gray(0x70),
                author_highlight_bg: Color32::from_rgb(0xec, 0xf3, 0xfd),
                artificial_fg: Color32::from_rgb(0x3a, 0x5a, 0x8a),
                warning: Color32::from_rgb(0xb3, 0x6b, 0x00),
                error: Color32::from_rgb(0xc4, 0x1e, 0x2a),
                success: Color32::from_rgb(0x1a, 0x7f, 0x37),
                status_added: Color32::from_rgb(0x1a, 0x7f, 0x37),
                status_modified: Color32::from_rgb(0x1f, 0x6f, 0xd1),
                status_deleted: Color32::from_rgb(0xc4, 0x1e, 0x2a),
                status_renamed: Color32::from_rgb(0x8e, 0x3f, 0xc7),
                status_conflict: Color32::from_rgb(0xc2, 0x5e, 0x00),
                status_untracked: Color32::from_gray(0x80),
            }
        }
    }

    pub fn lane(&self, index: i32) -> Color32 {
        self.lanes[(index.rem_euclid(LANE_COLOR_COUNT)) as usize]
    }

    pub fn for_ui(ui: &egui::Ui) -> Palette {
        Palette::new(ui.visuals().dark_mode)
    }
}

/// Applies the theme preference and refined visuals.
pub fn apply(ctx: &egui::Context, theme: Theme, ui_scale: f32, font_size: f32, mono_size: f32) {
    ctx.set_theme(match theme {
        Theme::System => egui::ThemePreference::System,
        Theme::Light => egui::ThemePreference::Light,
        Theme::Dark => egui::ThemePreference::Dark,
    });
    ctx.set_visuals_of(egui::Theme::Dark, dark_visuals());
    ctx.set_visuals_of(egui::Theme::Light, light_visuals());
    for t in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(t, |s| {
            use egui::{FontFamily, FontId, TextStyle};
            s.text_styles.insert(TextStyle::Body, FontId::new(font_size, FontFamily::Proportional));
            s.text_styles.insert(TextStyle::Button, FontId::new(font_size, FontFamily::Proportional));
            s.text_styles.insert(TextStyle::Monospace, FontId::new(mono_size, FontFamily::Monospace));
            s.text_styles.insert(TextStyle::Small, FontId::new(font_size * 0.82, FontFamily::Proportional));
            s.text_styles.insert(TextStyle::Heading, FontId::new(font_size * 1.45, FontFamily::Proportional));
            s.spacing.item_spacing = egui::vec2(8.0, 5.0);
            s.spacing.button_padding = egui::vec2(8.0, 3.0);
            s.spacing.interact_size.y = 22.0;
        });
    }
    if (ctx.zoom_factor() - ui_scale).abs() > f32::EPSILON && ui_scale > 0.3 {
        ctx.set_zoom_factor(ui_scale);
    }
}

fn dark_visuals() -> Visuals {
    let mut v = Visuals::dark();
    let bg = Color32::from_rgb(0x1e, 0x20, 0x24);
    v.panel_fill = bg;
    v.window_fill = Color32::from_rgb(0x25, 0x27, 0x2c);
    v.extreme_bg_color = Color32::from_rgb(0x17, 0x19, 0x1c);
    v.faint_bg_color = Color32::from_rgb(0x23, 0x25, 0x2a);
    v.selection.bg_fill = Color32::from_rgb(0x2b, 0x4f, 0x80);
    v.hyperlink_color = Color32::from_rgb(0x6c, 0xb6, 0xff);
    v.window_rounding = egui::Rounding::same(8.0);
    v.menu_rounding = egui::Rounding::same(6.0);
    v.widgets.noninteractive.bg_stroke.color = Color32::from_rgb(0x34, 0x37, 0x3e);
    v.widgets.noninteractive.fg_stroke.color = Color32::from_gray(0xd6);
    v.widgets.inactive.fg_stroke.color = Color32::from_gray(0xe2);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x2c, 0x2f, 0x35);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x36, 0x3a, 0x42);
    v.striped = true;
    v
}

fn light_visuals() -> Visuals {
    let mut v = Visuals::light();
    v.panel_fill = Color32::from_rgb(0xf7, 0xf8, 0xfa);
    v.window_fill = Color32::from_rgb(0xff, 0xff, 0xff);
    v.extreme_bg_color = Color32::WHITE;
    v.faint_bg_color = Color32::from_rgb(0xf0, 0xf2, 0xf5);
    v.selection.bg_fill = Color32::from_rgb(0xc5, 0xdc, 0xfa);
    v.selection.stroke.color = Color32::from_rgb(0x10, 0x30, 0x60);
    v.widgets.noninteractive.fg_stroke.color = Color32::from_gray(0x24);
    v.widgets.inactive.fg_stroke.color = Color32::from_gray(0x1c);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0xea, 0xec, 0xf0);
    v.window_rounding = egui::Rounding::same(8.0);
    v.menu_rounding = egui::Rounding::same(6.0);
    v.striped = true;
    v
}
