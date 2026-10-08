//! Port of `FormRemotes`: add, edit, rename and delete remotes; default pull/push behaviour.

use egui::{RichText, Ui, Vec2};
use gitext_core::module::Remote;
use gitext_core::GitArgs;

use super::{Confirm, Cx, Dialog, DialogKind, GitRun};
use crate::tasks::Task;
use crate::theme::Palette;

#[derive(Default)]
pub struct RemotesDialog {
    remotes: Option<Vec<Remote>>,
    selected: Option<usize>,
    name: String,
    url: String,
    push_url: String,
    separate_push: bool,
    status: Option<String>,
    load: Option<Task<Vec<Remote>>>,
}

impl RemotesDialog {
    fn reload(&mut self, cx: &Cx) {
        if let Some(m) = cx.module {
            let m = m.clone();
            self.load = Some(Task::spawn(cx.ctx, move || m.get_remotes().unwrap_or_default()));
        }
    }

    fn select(&mut self, i: Option<usize>) {
        self.selected = i;
        match i.and_then(|i| self.remotes.as_ref()?.get(i).cloned()) {
            Some(r) => {
                self.name = r.name.clone();
                self.url = r.fetch_url.clone();
                self.push_url = r.push_urls.first().cloned().unwrap_or_default();
                self.separate_push = !self.push_url.is_empty() && self.push_url != self.url;
            }
            None => {
                self.name.clear();
                self.url.clear();
                self.push_url.clear();
                self.separate_push = false;
            }
        }
    }
}

impl Dialog for RemotesDialog {
    fn title(&self) -> String {
        "Remote repositories".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(760.0, 420.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let palette = Palette::for_ui(ui);
        if self.remotes.is_none() && self.load.is_none() {
            self.reload(cx);
        }
        if let Some(t) = &mut self.load {
            if let Some(r) = t.try_take() {
                self.remotes = Some(r);
                self.load = None;
                let sel = self.selected;
                self.select(sel);
            }
        }
        let Some(m) = cx.module.cloned() else { return false };
        let remotes = self.remotes.clone().unwrap_or_default();
        let mut keep = true;
        egui::SidePanel::left("remotes_list").resizable(false).exact_width(220.0).show_inside(ui, |ui| {
            for (i, r) in remotes.iter().enumerate() {
                if ui.selectable_label(self.selected == Some(i), &r.name).clicked() {
                    self.select(Some(i));
                }
            }
            ui.add_space(8.0);
            if ui.button("✚ New remote").clicked() {
                self.select(None);
            }
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.label(RichText::new(if self.selected.is_some() { "Edit remote" } else { "New remote" }).strong());
            egui::Grid::new("remote_edit").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(360.0));
                ui.end_row();
                ui.label("Url");
                ui.add(egui::TextEdit::singleline(&mut self.url).desired_width(360.0));
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut self.separate_push, "Separate push url");
                ui.end_row();
                if self.separate_push {
                    ui.label("Push url");
                    ui.add(egui::TextEdit::singleline(&mut self.push_url).desired_width(360.0));
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button(if self.selected.is_some() { "Save changes" } else { "Add" }).clicked() {
                    let result = match self.selected.and_then(|i| remotes.get(i)) {
                        None => m.add_remote(&self.name, &self.url).map(|_| ()),
                        Some(old) => {
                            let mut r = Ok(());
                            if old.name != self.name.trim() {
                                r = m.rename_remote(&old.name, self.name.trim()).map(|_| ());
                            }
                            if r.is_ok() && old.fetch_url != self.url.trim() {
                                r = m.set_remote_url(self.name.trim(), &self.url, false).map(|_| ());
                            }
                            r
                        }
                    };
                    let result = result.and_then(|_| {
                        if self.separate_push {
                            m.set_remote_url(self.name.trim(), &self.push_url, true).map(|_| ())
                        } else {
                            m.unset_config(&format!("remote.{}.pushurl", self.name.trim()), false).map(|_| ())
                        }
                    });
                    match result {
                        Ok(()) => {
                            self.status = Some("Saved.".into());
                            if self.selected.is_none() {
                                let name = self.name.trim().to_string();
                                cx.open(Confirm::new("Fetch", format!("Fetch branches from the new remote '{name}'?"), "Fetch", move |cx| {
                                    cx.run(GitRun::new(format!("Fetch {name}"), GitArgs::new("fetch").arg("--progress").arg(name)));
                                }));
                            }
                        }
                        Err(e) => self.status = Some(e.to_string()),
                    }
                    self.reload(cx);
                    cx.push(super::Action::RefreshStatus);
                }
                if let Some(r) = self.selected.and_then(|i| remotes.get(i)) {
                    if ui.button("Delete…").clicked() {
                        let name = r.name.clone();
                        cx.open(Confirm::new("Delete remote", format!("Delete remote '{name}' and its remote tracking branches?"), "Delete", move |cx| {
                            cx.run(GitRun::new("Delete remote", GitArgs::new("remote").arg("rm").arg(name)));
                        }));
                        self.remotes = None;
                        self.selected = None;
                    }
                    if ui.button("Prune").clicked() {
                        cx.run(GitRun::new("Prune", GitArgs::new("remote").arg("prune").arg(&r.name)).keep_open());
                    }
                    if ui.button("Fetch").clicked() {
                        cx.run(GitRun::new("Fetch", GitArgs::new("fetch").arg("--progress").arg(&r.name)));
                    }
                }
            });
            if let Some(s) = &self.status {
                ui.label(RichText::new(s).color(palette.muted));
            }
            ui.add_space(12.0);
            ui.separator();
            ui.label(RichText::new("Default pull behaviour of local branches").strong());
            egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                if let Some(d) = cx.data {
                    egui::Grid::new("default_pull").striped(true).show(ui, |ui| {
                        for b in d.local_branches() {
                            ui.label(&b.name);
                            ui.label(RichText::new(if b.merge_with.is_empty() { "-".into() } else { format!("{}/{}", b.tracking_remote, b.merge_with) }).color(palette.muted));
                            ui.end_row();
                        }
                    });
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                keep = false;
            }
        });
        keep
    }
}
