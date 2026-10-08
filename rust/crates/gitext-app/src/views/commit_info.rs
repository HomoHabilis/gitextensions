//! Port of `CommitInfo`: header, message, parents/children, containing branches and tags.

use std::collections::HashMap;

use egui::{RichText, Ui};
use gitext_core::{GitModule, GitRevision, ObjectId};

use crate::tasks::Loader;
use crate::theme::Palette;
use crate::util::{format_date, short_date};

/// What is loaded of the details of a commit (`None` while loading).
struct Loaded {
    revision: Option<GitRevision>,
    describe: Option<String>,
    branches: Option<Vec<String>>,
    tags: Option<Vec<String>>,
}

/// Results kept for recently shown commits, so that going back to one shows it at once.
const CACHE_SIZE: usize = 256;

#[derive(Default)]
pub struct CommitInfo {
    /// The full message, when the grid did not load it.
    revision: Loader<ObjectId, GitRevision>,
    describe: Loader<ObjectId, Option<String>>,
    /// The branches and tags containing the commit: `--contains` walks the history, which can
    /// take a while in a large repository without a commit-graph. Each shows when it is ready
    /// (local branches are usually found much faster than tags).
    branches: Loader<ObjectId, Vec<String>>,
    tags: Loader<ObjectId, Vec<String>>,
    revision_cache: HashMap<ObjectId, GitRevision>,
    describe_cache: HashMap<ObjectId, Option<String>>,
    branches_cache: HashMap<ObjectId, Vec<String>>,
    tags_cache: HashMap<ObjectId, Vec<String>>,
    /// The signature is checked only on request: `%G?` runs gpg, which is slow on Windows
    /// (the C# app shows it in a separate tab).
    gpg: Loader<ObjectId, Option<String>>,
    gpg_requested: Option<ObjectId>,
    pub show_branches: bool,
}

pub enum CommitInfoLink {
    Select(ObjectId),
}

impl CommitInfo {
    pub fn invalidate(&mut self) {
        self.revision.invalidate();
        self.describe.invalidate();
        self.branches.invalidate();
        self.tags.invalidate();
        self.gpg.invalidate();
        self.revision_cache.clear();
        self.describe_cache.clear();
        self.branches_cache.clear();
        self.tags_cache.clear();
    }

    /// Loads the details of `id` in the background, each git command on its own so that each
    /// part shows as soon as it is ready. Returns what is ready: the full revision, the closest
    /// tag, the containing branches and tags.
    ///
    /// As in the C# app, each git process counts (starting one is slow on Windows): the message
    /// is read again only when the grid did not load it, and only local branches are searched
    /// (`branch -a --contains` also walks every remote branch).
    fn load(&mut self, ctx: &egui::Context, module: &GitModule, rev: &GitRevision) -> Loaded {
        let id = rev.object_id;
        // the message comes from the grid, unless it dropped the body of an old commit
        let revision = if rev.body().is_some() {
            Some(rev.clone())
        } else {
            let m = module.clone();
            let rev = rev.clone();
            // the grid loads notes when they are shown
            cached(&mut self.revision, &mut self.revision_cache, ctx, id, move || m.get_revision(&id.to_string(), rev.notes.is_some()).ok().flatten().unwrap_or(rev))
        };
        let m = module.clone();
        let describe = cached(&mut self.describe, &mut self.describe_cache, ctx, id, move || m.describe(id));
        let m = module.clone();
        let branches = cached(&mut self.branches, &mut self.branches_cache, ctx, id, move || m.branches_containing(id, true, false));
        let m = module.clone();
        let tags = cached(&mut self.tags, &mut self.tags_cache, ctx, id, move || m.tags_containing(id));
        Loaded { revision, describe: describe.flatten(), branches, tags }
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
        let Loaded { revision: full, describe, branches, tags } = self.load(ui.ctx(), module, rev);

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
            if let Some(desc) = &describe {
                ui.label(RichText::new(format!("Describe: {desc}")).color(palette.muted));
            }
            match &branches {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Loading branches…").color(palette.muted));
                    });
                }
                Some(branches) if branches.is_empty() => {
                    ui.label(RichText::new("Contained in no branch").color(palette.muted));
                }
                Some(branches) => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Contained in branches:").color(palette.muted));
                        let max = if self.show_branches { usize::MAX } else { 12 };
                        for b in branches.iter().take(max) {
                            ui.label(RichText::new(b).color(palette.lanes[2]));
                        }
                        if branches.len() > max && ui.link(format!("…and {} more", branches.len() - max)).clicked() {
                            self.show_branches = true;
                        }
                    });
                }
            }
            match &tags {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Loading tags…").color(palette.muted));
                    });
                }
                Some(tags) if tags.is_empty() => {}
                Some(tags) => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Contained in tags:").color(palette.muted));
                        for t in tags.iter().take(20) {
                            ui.label(RichText::new(t).color(palette.lanes[4]));
                        }
                        if tags.len() > 20 {
                            ui.label(format!("…and {} more", tags.len() - 20));
                        }
                    });
                }
            }
            ui.add_space(6.0);
            if self.gpg_requested == Some(id) {
                let m = module.clone();
                match self.gpg.request(ui.ctx(), id, move || m.gpg_info(id)) {
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("Checking the signature…").color(palette.muted));
                        });
                    }
                    Some(g) => {
                        ui.label(RichText::new(format!("🔏 {}", g.as_deref().unwrap_or("No signature"))).color(palette.muted));
                    }
                }
            } else if ui.link(RichText::new("🔏 Check signature").color(palette.muted)).clicked() {
                self.gpg_requested = Some(id);
            }
        });
        link
    }
}

/// The value for `id` from `cache`, or loads it in the background with `loader` and caches it.
fn cached<T: Clone + Send + 'static>(loader: &mut Loader<ObjectId, T>, cache: &mut HashMap<ObjectId, T>, ctx: &egui::Context, id: ObjectId, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    if let Some(v) = cache.get(&id) {
        return Some(v.clone());
    }
    let v = loader.request_latest(ctx, id, f).cloned();
    if let Some(v) = &v {
        cache_insert(cache, id, v.clone());
    }
    v
}

fn cache_insert<T>(cache: &mut HashMap<ObjectId, T>, id: ObjectId, value: T) {
    if cache.len() >= CACHE_SIZE && !cache.contains_key(&id) {
        cache.clear();
    }
    cache.insert(id, value);
}
