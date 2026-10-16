//! Port of `FormDiff` / `FormCompareToBranch`: compare two branches or revisions.

use egui::{Ui, Vec2};
use gitext_core::ObjectId;

use super::{branch_combo, Cx, Dialog, DialogKind};
use crate::i18n::{tr, trf};
use crate::repo::RepoData;
use crate::tasks::Loader;
use crate::views::revision_diff::RevisionDiffView;

pub struct CompareDialog {
    base: String,
    head: String,
    items: Vec<String>,
    merge_base: bool,
    resolved: Option<(ObjectId, ObjectId)>,
    diff: RevisionDiffView,
    /// Commits in head not in base (counted in the background, once per pair).
    count: Loader<(ObjectId, ObjectId), usize>,
}

impl CompareDialog {
    pub fn new(data: &RepoData) -> Self {
        let current = data.current_branch.clone().unwrap_or_else(|| "HEAD".into());
        let base = data.current_ref().filter(|r| !r.merge_with.is_empty()).map(|r| format!("{}/{}", r.tracking_remote, r.merge_with)).unwrap_or_default();
        CompareDialog { base, head: current, items: data.branch_names(true), merge_base: true, resolved: None, diff: RevisionDiffView::default(), count: Loader::default() }
    }

    /// The diff of `head` to `base`, shown at once (`RevisionGridControl.ShowFormDiff`).
    pub fn revisions(data: &RepoData, base: (String, ObjectId), head: (String, ObjectId)) -> Self {
        let resolved = Some((base.1, head.1));
        CompareDialog { base: base.0, head: head.0, items: data.branch_names(true), merge_base: false, resolved, diff: RevisionDiffView::default(), count: Loader::default() }
    }

    /// "Compare to branch" (`FormCompareToBranch`): the branch is chosen as the base of `head`.
    pub fn to_branch(data: &RepoData, head: ObjectId) -> Self {
        CompareDialog { base: String::new(), head: head.to_string(), items: data.branch_names(true), merge_base: false, resolved: None, diff: RevisionDiffView::default(), count: Loader::default() }
    }
}

impl Dialog for CompareDialog {
    fn title(&self) -> String {
        tr("Compare").into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(1100.0, 720.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let Some(m) = cx.module.cloned() else { return false };
        ui.horizontal(|ui| {
            ui.label(tr("Base"));
            branch_combo(ui, "cmp_base", &mut self.base, &self.items, 260.0);
            ui.label(tr("Compare"));
            branch_combo(ui, "cmp_head", &mut self.head, &self.items, 260.0);
            ui.checkbox(&mut self.merge_base, tr("From merge base (three dots)"));
            if ui.button(tr("Compare")).clicked() {
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
                    cx.error(tr("Compare"), tr("Revision not found"));
                }
            }
        });
        ui.separator();
        if let Some((a, b)) = self.resolved {
            let m2 = m.clone();
            let count = self.count.request(ui.ctx(), (a, b), move || m2.commit_count(&format!("{a}..{b}"))).map_or("…".to_string(), |c| c.to_string());
            ui.label(trf("{0} commit(s) in {1} not in {2}", &[&count, &self.head.trim(), &if self.base.trim().is_empty() { tr("base") } else { self.base.trim() }]));
            let _ = self.diff.ui(ui, &m, Some(a), Some(b), &[], cx.settings, "compare_diff");
        } else {
            ui.label(tr("Choose two branches or revisions and press Compare."));
        }
        !ui.input(|i| i.key_pressed(egui::Key::Escape))
    }
}
