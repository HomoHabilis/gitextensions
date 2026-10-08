//! Port of `CommitInfo`: header, message, parents/children, containing branches and tags.

use std::collections::HashMap;

use egui::{RichText, Ui};
use gitext_core::{GitModule, GitRevision, ObjectId};

use crate::tasks::Loader;
use crate::theme::Palette;
use crate::util::{format_date, short_date};

/// The parts of the details that git finds quickly.
#[derive(Debug, Clone, Default)]
pub struct CommitDetails {
    pub revision: Option<GitRevision>,
    pub describe: Option<String>,
    pub gpg: Option<String>,
}

/// The branches and tags containing the commit: `--contains` walks the history, which can
/// take a while in a large repository without a commit-graph.
#[derive(Debug, Clone, Default)]
pub struct CommitRefs {
    pub branches: Vec<String>,
    pub tags: Vec<String>,
}

/// Results kept for recently shown commits, so that going back to one shows it at once.
const CACHE_SIZE: usize = 256;

#[derive(Default)]
pub struct CommitInfo {
    details: Loader<ObjectId, CommitDetails>,
    refs: Loader<ObjectId, CommitRefs>,
    details_cache: HashMap<ObjectId, CommitDetails>,
    refs_cache: HashMap<ObjectId, CommitRefs>,
    pub show_branches: bool,
}

pub enum CommitInfoLink {
    Select(ObjectId),
}

impl CommitInfo {
    pub fn invalidate(&mut self) {
        self.details.invalidate();
        self.refs.invalidate();
        self.details_cache.clear();
        self.refs_cache.clear();
    }

    /// Loads the details and the containing refs of `id` in the background, both in parallel and
    /// each with its git commands in parallel. Returns what is ready.
    fn load(&mut self, ctx: &egui::Context, module: &GitModule, rev: &GitRevision) -> (Option<CommitDetails>, Option<CommitRefs>) {
        let id = rev.object_id;
        let details = match self.details_cache.get(&id) {
            Some(d) => Some(d.clone()),
            None => {
                let m = module.clone();
                let rev = rev.clone();
                let d = self
                    .details
                    .request_latest(ctx, id, move || {
                        std::thread::scope(|s| {
                            let describe = s.spawn(|| m.describe(id));
                            let gpg = s.spawn(|| m.gpg_info(id));
                            let revision = m.get_revision(&id.to_string(), true).ok().flatten().or(Some(rev));
                            CommitDetails { revision, describe: describe.join().unwrap_or_default(), gpg: gpg.join().unwrap_or_default() }
                        })
                    })
                    .cloned();
                if let Some(d) = &d {
                    cache_insert(&mut self.details_cache, id, d.clone());
                }
                d
            }
        };
        let refs = match self.refs_cache.get(&id) {
            Some(r) => Some(r.clone()),
            None => {
                let m = module.clone();
                let r = self
                    .refs
                    .request_latest(ctx, id, move || {
                        std::thread::scope(|s| {
                            let tags = s.spawn(|| m.tags_containing(id));
                            let branches = m.branches_containing(id, true, true);
                            CommitRefs { branches, tags: tags.join().unwrap_or_default() }
                        })
                    })
                    .cloned();
                if let Some(r) = &r {
                    cache_insert(&mut self.refs_cache, id, r.clone());
                }
                r
            }
        };
        (details, refs)
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
        let (details, refs) = self.load(ui.ctx(), module, rev);
        let details = details.as_ref();

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let full = details.and_then(|d| d.revision.as_ref()).unwrap_or(rev);
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
            if let Some(desc) = details.and_then(|d| d.describe.as_ref()) {
                ui.label(RichText::new(format!("Describe: {desc}")).color(palette.muted));
            }
            match &refs {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Loading branches and tags…").color(palette.muted));
                    });
                }
                Some(d) => {
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
                }
            }
            if let Some(g) = details.and_then(|d| d.gpg.as_ref()) {
                ui.add_space(6.0);
                ui.label(RichText::new(format!("🔏 {g}")).color(palette.muted));
            }
        });
        link
    }
}

fn cache_insert<T>(cache: &mut HashMap<ObjectId, T>, id: ObjectId, value: T) {
    if cache.len() >= CACHE_SIZE && !cache.contains_key(&id) {
        cache.clear();
    }
    cache.insert(id, value);
}
