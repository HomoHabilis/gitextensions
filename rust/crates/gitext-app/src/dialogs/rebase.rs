//! Port of `FormRebase`.

use egui::Ui;
use gitext_core::commands::{self, RebaseOptions};
use gitext_core::GitArgs;

use super::{branch_combo, ok_cancel, Cx, Dialog, GitRun};
use crate::i18n::{tr, trf};
use crate::repo::RepoData;

pub struct RebaseDialog {
    onto: String,
    items: Vec<String>,
    pub interactive: bool,
    auto_squash: bool,
    preserve_merges: bool,
    auto_stash: bool,
    ignore_date: bool,
    committer_date: bool,
    update_refs: bool,
    specific_range: bool,
    from: String,
}

impl RebaseDialog {
    pub fn new(data: &RepoData, onto: &str) -> Self {
        RebaseDialog {
            onto: onto.to_string(),
            items: data.branch_names(true),
            interactive: false,
            auto_squash: true,
            preserve_merges: false,
            auto_stash: false,
            ignore_date: false,
            committer_date: false,
            update_refs: false,
            specific_range: false,
            from: String::new(),
        }
    }
}

impl Dialog for RebaseDialog {
    fn title(&self) -> String {
        tr("Rebase").into()
    }

    fn kind(&self) -> super::DialogKind {
        super::DialogKind::Modal(480.0)
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let current = cx.current_branch().unwrap_or_else(|| "HEAD".into());
        ui.label(trf("Rebase current branch {0} on top of:", &[&current]));
        branch_combo(ui, "rebase_onto", &mut self.onto, &self.items, 440.0);
        ui.checkbox(&mut self.interactive, tr("Interactive rebase"));
        if self.interactive {
            ui.indent("ia", |ui| {
                ui.checkbox(&mut self.auto_squash, tr("Autosquash"));
                ui.label(egui::RichText::new(tr("The todo list opens in the editor configured in git (core.editor / GIT_SEQUENCE_EDITOR).")).small());
            });
        }
        ui.checkbox(&mut self.preserve_merges, tr("Rebase merges (--rebase-merges)"));
        ui.checkbox(&mut self.auto_stash, tr("Auto stash local changes"));
        ui.checkbox(&mut self.update_refs, tr("Update dependent branches (--update-refs)"));
        ui.checkbox(&mut self.ignore_date, tr("Ignore date"));
        ui.checkbox(&mut self.committer_date, tr("Committer date is author date"));
        ui.checkbox(&mut self.specific_range, tr("Specific range (--onto): rebase commits after"));
        if self.specific_range {
            ui.add(egui::TextEdit::singleline(&mut self.from).hint_text(tr("upstream / from revision")).desired_width(440.0));
        }
        let (ok, cancel) = ok_cancel(ui, tr("Rebase"), !self.onto.trim().is_empty());
        if ok {
            let o = RebaseOptions {
                branch_name: if self.specific_range { current.clone() } else { self.onto.trim().to_string() },
                interactive: self.interactive,
                preserve_merges: self.preserve_merges,
                auto_squash: self.auto_squash,
                auto_stash: self.auto_stash,
                ignore_date: self.ignore_date,
                committer_date_is_author_date: self.committer_date,
                support_rebase_merges: true,
                update_refs: self.update_refs.then_some(true),
                from: self.specific_range.then(|| self.from.trim().to_string()),
                on_to: self.specific_range.then(|| self.onto.trim().to_string()),
            };
            match commands::rebase(&o) {
                Ok(mut args) => {
                    if !self.specific_range {
                        // `git rebase <upstream>` rebases the current branch
                        let v: Vec<String> = args.as_slice().to_vec();
                        args = GitArgs::empty().args(v);
                    }
                    cx.run(GitRun::new(tr("Rebase"), args).conflicts().keep_open());
                }
                Err(e) => cx.error(tr("Rebase"), e),
            }
            return false;
        }
        !cancel
    }
}
