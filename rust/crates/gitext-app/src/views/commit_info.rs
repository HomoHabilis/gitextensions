//! Port of `CommitInfo`: header, message, parents/children, containing branches and tags.

use std::collections::VecDeque;
use std::sync::Arc;

use egui::{RichText, Ui};
use gitext_core::{GitModule, GitRevision, ObjectId};

use crate::tasks::{Loader, Task};
use crate::theme::Palette;
use crate::util::{format_date, short_date};

/// What git finds slowly about a commit (searching the history of all refs).
#[derive(Debug, Clone, Default)]
pub struct CommitRefs {
    pub branches: Vec<String>,
    pub tags: Vec<String>,
    pub describe: Option<String>,
    pub gpg: Option<String>,
}

impl CommitRefs {
    /// Runs the git commands at the same time: the wait is the slowest one, not their sum.
    fn load(m: &GitModule, id: ObjectId) -> Self {
        std::thread::scope(|s| {
            let branches = s.spawn(|| m.branches_containing(id, true, true));
            let tags = s.spawn(|| m.tags_containing(id));
            let describe = s.spawn(|| m.describe(id));
            let gpg = m.gpg_info(id);
            CommitRefs { branches: branches.join().unwrap_or_default(), tags: tags.join().unwrap_or_default(), describe: describe.join().unwrap_or_default(), gpg }
        })
    }
}

/// How many commits' refs are kept, so that going back to a commit shows them at once.
const REFS_CACHE: usize = 64;

#[derive(Default)]
pub struct CommitInfo {
    /// The full revision (the message body and notes), when the grid has not loaded them.
    revision: Loader<ObjectId, Option<GitRevision>>,
    /// Refs of recent commits, newest last. Cleared when the refs change ([`Self::invalidate`]).
    refs: VecDeque<(ObjectId, Arc<CommitRefs>)>,
    /// The refs being loaded: one commit at a time, so that moving through the commits does not
    /// pile up slow git processes; the selected commit is loaded next.
    refs_task: Option<(ObjectId, Task<CommitRefs>)>,
    pub show_branches: bool,
}

pub enum CommitInfoLink {
    Select(ObjectId),
}

impl CommitInfo {
    pub fn invalidate(&mut self) {
        self.revision.invalidate();
        self.refs.clear();
        // a load started before the refs changed would cache stale refs
        self.refs_task = None;
    }

    /// The refs of `id`, loading them when not known.
    fn refs(&mut self, ctx: &egui::Context, module: &GitModule, id: ObjectId) -> Option<Arc<CommitRefs>> {
        if let Some((task_id, task)) = &mut self.refs_task {
            if let Some(refs) = task.try_take() {
                if self.refs.len() >= REFS_CACHE {
                    self.refs.pop_front();
                }
                self.refs.push_back((*task_id, Arc::new(refs)));
                self.refs_task = None;
            }
        }
        if let Some((_, refs)) = self.refs.iter().find(|(i, _)| *i == id) {
            return Some(Arc::clone(refs));
        }
        if self.refs_task.is_none() {
            let m = module.clone();
            self.refs_task = Some((id, Task::spawn(ctx, move || CommitRefs::load(&m, id))));
        }
        None
    }

    pub fn ui(&mut self, ui: &mut Ui, module: &GitModule, rev: Option<&GitRevision>, children: Vec<ObjectId>) -> Option<CommitInfoLink> {
        let palette = Palette::for_ui(ui);
        let Some(rev) = rev else {
            ui.label(RichText::new("No commit selected").italics().color(palette.muted));
            return None;
        };
        let mut link = None;
        let id = rev.object_id;
        if rev.is_artificial() {
            ui.heading(&rev.subject);
            ui.label(RichText::new(if id == ObjectId::WORK_TREE { "Changes in the working directory that are not staged." } else { "Changes staged in the index, to be committed." }).color(palette.muted));
            return None;
        }
        // the message and the notes, unless the grid has them: one quick git process
        let full = if rev.body().is_some() && rev.notes.is_some() {
            None
        } else {
            let m = module.clone();
            self.revision.request(ui.ctx(), id, move || m.get_revision(&id.to_string(), true).ok().flatten()).cloned().flatten()
        };
        let refs = self.refs(ui.ctx(), module, id);

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let full = full.as_ref().unwrap_or(rev);
            egui::Grid::new("commit_header").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                ui.label(RichText::new("Author").color(palette.muted));
                ui.label(format!("{} <{}>", full.author, full.author_email));
                ui.end_row();
                ui.label(RichText::new("Date").color(palette.muted));
                ui.label(format!("{} ({})", short_date(full.author_unix_time), format_date(full.author_unix_time)));
                ui.end_row();
                if full.committer != full.author || full.committer_email != full.author_email || full.commit_unix_time != full.author_unix_time {
                    ui.label(RichText::new("Committer").color(palette.muted));
                    ui.label(format!("{} <{}>", full.committer, full.committer_email));
                    ui.end_row();
                    ui.label(RichText::new("Commit date").color(palette.muted));
                    ui.label(format!("{} ({})", short_date(full.commit_unix_time), format_date(full.commit_unix_time)));
                    ui.end_row();
                }
                ui.label(RichText::new("Commit hash").color(palette.muted));
                ui.horizontal(|ui| {
                    ui.label(RichText::new(id.to_string()).monospace());
                    if ui.small_button("📋").on_hover_text("Copy").clicked() {
                        crate::util::copy_to_clipboard(ui.ctx(), id.to_string());
                    }
                });
                ui.end_row();
                if full.has_parent() {
                    ui.label(RichText::new(if full.parents().len() > 1 { "Parents" } else { "Parent" }).color(palette.muted));
                    ui.horizontal_wrapped(|ui| {
                        for p in full.parents() {
                            if ui.link(RichText::new(p.to_short_string()).monospace()).clicked() {
                                link = Some(CommitInfoLink::Select(*p));
                            }
                        }
                    });
                    ui.end_row();
                }
                if !children.is_empty() {
                    ui.label(RichText::new(if children.len() > 1 { "Children" } else { "Child" }).color(palette.muted));
                    ui.horizontal_wrapped(|ui| {
                        for c in &children {
                            if ui.link(RichText::new(c.to_short_string()).monospace()).clicked() {
                                link = Some(CommitInfoLink::Select(*c));
                            }
                        }
                    });
                    ui.end_row();
                }
            });
            ui.add_space(8.0);
            let body = full.body().unwrap_or(&full.subject);
            ui.add(egui::Label::new(RichText::new(body).size(ui.text_style_height(&egui::TextStyle::Body) + 0.5)).wrap().selectable(true));
            if let Some(notes) = full.notes.as_deref().filter(|n| !n.trim().is_empty()) {
                ui.add_space(6.0);
                ui.label(RichText::new("Notes:").color(palette.muted));
                ui.label(notes);
            }
            ui.add_space(10.0);
            ui.separator();
            match refs.as_deref() {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Loading branches and tags…").color(palette.muted));
                    });
                }
                Some(d) => {
                    if let Some(desc) = &d.describe {
                        ui.label(RichText::new(format!("Describe: {desc}")).color(palette.muted));
                    }
                    if !d.branches.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("Contained in branches:").color(palette.muted));
                            let max = if self.show_branches { usize::MAX } else { 12 };
                            for b in d.branches.iter().take(max) {
                                ui.label(RichText::new(b).color(palette.lanes[2]));
                            }
                            if d.branches.len() > max && ui.link(format!("…and {} more", d.branches.len() - max)).clicked() {
                                self.show_branches = true;
                            }
                        });
                    } else {
                        ui.label(RichText::new("Contained in no branch").color(palette.muted));
                    }
                    if !d.tags.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("Contained in tags:").color(palette.muted));
                            for t in d.tags.iter().take(20) {
                                ui.label(RichText::new(t).color(palette.lanes[4]));
                            }
                            if d.tags.len() > 20 {
                                ui.label(format!("…and {} more", d.tags.len() - 20));
                            }
                        });
                    }
                    if let Some(g) = &d.gpg {
                        ui.add_space(6.0);
                        ui.label(RichText::new(format!("🔏 {g}")).color(palette.muted));
                    }
                }
            }
        });
        link
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn ms(d: Duration) -> f64 {
        d.as_secs_f64() * 1000.0
    }

    /// Time until the details of an old commit (message body not loaded by the grid) are shown,
    /// against loading them one after the other as before. Run in a repository with history:
    /// `GITEXT_BENCH_REPO=<path> cargo test --release -p gitext-app details_latency -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn details_latency() {
        let Some(repo) = std::env::var_os("GITEXT_BENCH_REPO") else { return };
        let module = GitModule::open(repo).unwrap();
        let ids = module.git().output(&gitext_core::GitArgs::new("rev-list").args(["--max-count=5", "--skip=3000", "HEAD"])).unwrap();
        for id in ids.lines().filter_map(|l| ObjectId::try_parse(l.trim())) {
            // before: one task, everything shown together at the end
            let t = Instant::now();
            let _ = module.get_revision(&id.to_string(), true);
            let _ = (module.branches_containing(id, true, true), module.tags_containing(id), module.describe(id), module.gpg_info(id));
            let before = t.elapsed();

            let mut rev = GitRevision::new(id);
            rev.has_multi_line_message = true;
            let ctx = egui::Context::default();
            let mut info = CommitInfo::default();
            let t = Instant::now();
            let (mut message_after, mut refs_after) = (None, None);
            while refs_after.is_none() || message_after.is_none() {
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        info.ui(ui, &module, Some(&rev), Vec::new());
                    });
                });
                if message_after.is_none() && info.revision.value.is_some() {
                    message_after = Some(t.elapsed());
                }
                if refs_after.is_none() && info.refs.iter().any(|(i, _)| *i == id) {
                    refs_after = Some(t.elapsed());
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            eprintln!(
                "{}: before: everything at {:.0} ms | after: message at {:.0} ms, branches and tags at {:.0} ms",
                id.to_short_string(),
                ms(before),
                ms(message_after.unwrap()),
                ms(refs_after.unwrap())
            );
        }
    }
}
