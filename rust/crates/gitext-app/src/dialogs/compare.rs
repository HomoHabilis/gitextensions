//! Port of `FormDiff` / `FormCompareToBranch`: compare two branches or revisions.

use egui::{Ui, Vec2};
use gitext_core::ObjectId;

use super::{branch_combo, Cx, Dialog, DialogKind};
use crate::repo::RepoData;
use crate::views::revision_diff::RevisionDiffView;

pub struct CompareDialog {
    base: String,
    head: String,
    items: Vec<String>,
    merge_base: bool,
    resolved: Option<(ObjectId, ObjectId)>,
    diff: RevisionDiffView,
}

impl CompareDialog {
    pub fn new(data: &RepoData) -> Self {
        let current = data.current_branch.clone().unwrap_or_else(|| "HEAD".into());
        let base = data.current_ref().filter(|r| !r.merge_with.is_empty()).map(|r| format!("{}/{}", r.tracking_remote, r.merge_with)).unwrap_or_default();
        CompareDialog { base, head: current, items: data.branch_names(true), merge_base: true, resolved: None, diff: RevisionDiffView::default() }
    }
}

impl Dialog for CompareDialog {
    fn title(&self) -> String {
        "Compare".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(1100.0, 720.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let Some(m) = cx.module.cloned() else { return false };
        ui.horizontal(|ui| {
            ui.label("Base");
            branch_combo(ui, "cmp_base", &mut self.base, &self.items, 260.0);
            ui.label("Compare");
            branch_combo(ui, "cmp_head", &mut self.head, &self.items, 260.0);
            ui.checkbox(&mut self.merge_base, "From merge base (three dots)");
            if ui.button("Compare").clicked() {
                let mut a = m.rev_parse(self.base.trim());
                let b = m.rev_parse(self.head.trim());
                if self.merge_base {
                    if let Some(mb) = m.merge_base(self.base.trim(), self.head.trim()) {
                        a = mb;
                    }
                }
                self.resolved = (!a.is_zero() && !b.is_zero()).then_some((a, b));
                self.diff.list.clear();
                if self.resolved.is_none() {
                    cx.error("Compare", "Revision not found");
                }
            }
        });
        ui.separator();
        if let Some((a, b)) = self.resolved {
            let count = m.commit_count(&format!("{a}..{b}"));
            ui.label(format!("{count} commit(s) in {} not in base", self.head.trim()));
            let _ = self.diff.ui(ui, &m, Some(a), Some(b), &[], cx.settings, "compare_diff");
        } else {
            ui.label("Choose two branches or revisions and press Compare.");
        }
        !ui.input(|i| i.key_pressed(egui::Key::Escape))
    }
}
