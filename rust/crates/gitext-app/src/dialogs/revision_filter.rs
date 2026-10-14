//! Port of `FormRevisionFilter` (Ctrl+I): the advanced filter of the revision grid.

use egui::{RichText, Ui};
use gitext_core::settings::BranchFilterMode;

use super::{ok_cancel, Action, Cx, Dialog, DialogKind};
use crate::theme::Palette;
use crate::views::revision_grid::RevisionFilter;

pub struct RevisionFilterDialog {
    filter: RevisionFilter,
    /// Each criterion applies when checked, as upstream (the text is kept when unchecked).
    by_since: bool,
    by_until: bool,
    by_author: bool,
    by_committer: bool,
    by_message: bool,
    by_diff: bool,
    by_limit: bool,
    by_path: bool,
    limit: usize,
    branches: bool,
    branch_filter: String,
    current_branch_only: bool,
    reflog: bool,
    first_parent: bool,
    hide_merges: bool,
}

impl RevisionFilterDialog {
    pub fn new(filter: &RevisionFilter, settings: &gitext_core::settings::AppSettings) -> Self {
        let f = filter.clone();
        let has = |s: &str| !s.trim().is_empty();
        RevisionFilterDialog {
            by_since: has(&f.since),
            by_until: has(&f.until),
            by_author: has(&f.author),
            by_committer: has(&f.committer),
            by_message: has(&f.message),
            by_diff: has(&f.diff_contains),
            by_limit: f.limit.is_some(),
            by_path: has(&f.path),
            limit: f.limit.unwrap_or(settings.max_revision_graph_commits.clamp(1, 100_000)),
            branches: settings.branch_filter_mode == BranchFilterMode::Specific,
            branch_filter: settings.branch_filter.clone(),
            current_branch_only: settings.branch_filter_mode == BranchFilterMode::Current,
            reflog: settings.show_reflog_references,
            first_parent: settings.show_first_parent,
            hide_merges: !settings.show_merge_commits,
            filter: f,
        }
    }

    /// The filter as set in the dialog: unchecked criteria are cleared.
    fn result(&self) -> RevisionFilter {
        let mut f = self.filter.clone();
        let keep = |on: bool, s: &str| if on { s.trim().to_string() } else { String::new() };
        f.since = keep(self.by_since, &f.since);
        f.until = keep(self.by_until, &f.until);
        f.author = keep(self.by_author, &f.author);
        f.committer = keep(self.by_committer, &f.committer);
        f.message = keep(self.by_message, &f.message);
        f.diff_contains = keep(self.by_diff, &f.diff_contains);
        f.path = keep(self.by_path, &f.path);
        f.limit = self.by_limit.then_some(self.limit.max(1));
        if f.path.is_empty() {
            f.follow = false;
        }
        f
    }
}

impl Dialog for RevisionFilterDialog {
    fn title(&self) -> String {
        "Filter".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Modal(560.0)
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let palette = Palette::for_ui(ui);
        let text_row = |ui: &mut Ui, on: &mut bool, label: &str, value: &mut String, hint: &str| {
            ui.checkbox(on, label);
            ui.add_enabled(*on, egui::TextEdit::singleline(value).hint_text(hint).desired_width(360.0));
            ui.end_row();
        };
        egui::Grid::new("revision_filter").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            text_row(ui, &mut self.by_since, "Since", &mut self.filter.since, "2024-01-31, 2 weeks ago…");
            text_row(ui, &mut self.by_until, "Until", &mut self.filter.until, "2024-12-31, yesterday…");
            text_row(ui, &mut self.by_author, "Author", &mut self.filter.author, "regular expression");
            text_row(ui, &mut self.by_committer, "Committer", &mut self.filter.committer, "regular expression");
            text_row(ui, &mut self.by_message, "Message", &mut self.filter.message, "regular expression");
            text_row(ui, &mut self.by_diff, "Diff contains", &mut self.filter.diff_contains, "regular expression (slow)");
            ui.label("");
            let any_text = self.by_author || self.by_committer || self.by_message || self.by_diff;
            let mut ignore_case = !self.filter.match_case;
            if ui.add_enabled(any_text, egui::Checkbox::new(&mut ignore_case, "Ignore case")).changed() {
                self.filter.match_case = !ignore_case;
            }
            ui.end_row();
            ui.checkbox(&mut self.by_limit, "Limit");
            ui.add_enabled(self.by_limit, egui::DragValue::new(&mut self.limit).range(1..=10_000_000).speed(10));
            ui.end_row();
            text_row(ui, &mut self.by_path, "Path filter", &mut self.filter.path, "path or pathspec");
            ui.checkbox(&mut self.branches, "Branches");
            ui.add_enabled(self.branches, egui::TextEdit::singleline(&mut self.branch_filter).hint_text("branch names or patterns (feature/*)").desired_width(360.0));
            ui.end_row();
        });
        ui.add_space(6.0);
        if ui.checkbox(&mut self.current_branch_only, "Show current branch only").changed() && self.current_branch_only {
            self.branches = false;
        }
        if self.branches {
            self.current_branch_only = false;
        }
        ui.checkbox(&mut self.reflog, "Show reflog");
        ui.checkbox(&mut self.first_parent, "Show only first parent");
        ui.checkbox(&mut self.hide_merges, "Hide merge commits");
        ui.checkbox(&mut self.filter.simplify_by_decoration, "Simplify by decoration");
        ui.add_enabled_ui(self.by_path, |ui| {
            ui.checkbox(&mut self.filter.full_history, "Full history");
            ui.add_enabled(self.filter.full_history, egui::Checkbox::new(&mut self.filter.simplify_merges, "Simplify merges"));
        });
        ui.label(RichText::new("Full history and simplify merges apply with a path filter.").small().color(palette.muted));
        let (ok, cancel) = ok_cancel(ui, "OK", true);
        if ok {
            let s = &mut *cx.settings;
            s.branch_filter_mode = if self.branches && !self.branch_filter.trim().is_empty() {
                BranchFilterMode::Specific
            } else if self.current_branch_only {
                BranchFilterMode::Current
            } else {
                BranchFilterMode::All
            };
            s.branch_filter = self.branch_filter.trim().to_string();
            s.show_reflog_references = self.reflog;
            s.show_first_parent = self.first_parent;
            s.show_merge_commits = !self.hide_merges;
            cx.push(Action::SaveSettings);
            cx.push(Action::SetRevisionFilter(self.result()));
            return false;
        }
        !cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchecked_criteria_are_cleared() {
        let f = RevisionFilter { author: "me".into(), message: "fix".into(), path: "src".into(), follow: true, ..Default::default() };
        let mut d = RevisionFilterDialog::new(&f, &Default::default());
        assert!(d.by_author && d.by_message && d.by_path && !d.by_limit);
        d.by_message = false;
        d.by_path = false;
        d.by_limit = true;
        d.limit = 50;
        let r = d.result();
        assert_eq!(r.author, "me");
        assert_eq!(r.message, "");
        assert_eq!(r.path, "");
        assert!(!r.follow);
        assert_eq!(r.limit, Some(50));
    }
}
