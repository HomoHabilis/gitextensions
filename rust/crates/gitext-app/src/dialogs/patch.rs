//! Ports of `FormFormatPatch`, `FormApplyPatch` and `FormViewPatch`.

use egui::{RichText, Ui, Vec2};
use gitext_core::patch::create_patches_from_string;
use gitext_core::{commands, GitArgs, ObjectId};

use super::{ok_cancel, Cx, Dialog, DialogKind, GitRun};
use crate::i18n::tr;
use crate::views::diff_viewer::{DiffViewer, ViewerContent};

pub struct FormatPatchDialog {
    from: String,
    to: String,
    output_dir: String,
}

impl FormatPatchDialog {
    pub fn new(selected: &[ObjectId], id: ObjectId) -> Self {
        let (from, to) = match selected {
            [a, .., b] => (a.to_string(), b.to_string()),
            _ => (format!("{id}~1"), id.to_string()),
        };
        FormatPatchDialog { from, to, output_dir: String::new() }
    }
}

impl Dialog for FormatPatchDialog {
    fn title(&self) -> String {
        tr("Format patch").into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        egui::Grid::new("fp").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label(tr("From (exclusive)"));
            ui.add(egui::TextEdit::singleline(&mut self.from).desired_width(320.0));
            ui.end_row();
            ui.label(tr("To (inclusive)"));
            ui.add(egui::TextEdit::singleline(&mut self.to).desired_width(320.0));
            ui.end_row();
            ui.label(tr("Output directory"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.output_dir).desired_width(260.0));
                if ui.button("…").clicked() {
                    if let Some(p) = crate::util::pick_folder(None) {
                        self.output_dir = p;
                    }
                }
            });
            ui.end_row();
        });
        let (ok, cancel) = ok_cancel(ui, tr("Create patches"), !self.output_dir.trim().is_empty());
        if ok {
            let args = GitArgs::new("format-patch").arg("-M").arg("-C").arg("-o").arg(self.output_dir.trim()).arg(format!("{}..{}", self.from.trim(), self.to.trim()));
            cx.run(GitRun::new(tr("Format patch"), args).no_refresh().keep_open());
            return false;
        }
        !cancel
    }
}

pub struct ApplyPatchDialog {
    path: String,
    is_dir: bool,
    sign_off: bool,
    ignore_whitespace: bool,
}

impl ApplyPatchDialog {
    pub fn new(path: Option<String>) -> Self {
        ApplyPatchDialog { path: path.unwrap_or_default(), is_dir: false, sign_off: false, ignore_whitespace: false }
    }
}

impl Dialog for ApplyPatchDialog {
    fn title(&self) -> String {
        tr("Apply patch").into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let applying = cx.data.is_some_and(|d| d.state.applying_patch);
        if applying {
            ui.label(RichText::new(tr("A patch is being applied (git am).")).strong());
            ui.horizontal(|ui| {
                if ui.button(tr("Continue (resolved)")).clicked() {
                    cx.run(GitRun::new(tr("Resolved"), commands::resolved_mailbox()).conflicts());
                }
                if ui.button(tr("Skip patch")).clicked() {
                    cx.run(GitRun::new(tr("Skip"), commands::skip_mailbox()).conflicts());
                }
                if ui.button(tr("Abort")).clicked() {
                    cx.run(GitRun::new(tr("Abort"), commands::abort_mailbox()));
                }
            });
            ui.separator();
        }
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.is_dir, false, tr("Patch file"));
            ui.radio_value(&mut self.is_dir, true, tr("Directory with patches"));
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(330.0));
            if ui.button("…").clicked() {
                let p = if self.is_dir { crate::util::pick_folder(None) } else { crate::util::pick_file(None, Some((tr("Patch"), &["patch", "diff", "eml"]))) };
                if let Some(p) = p {
                    self.path = p;
                }
            }
        });
        ui.checkbox(&mut self.sign_off, tr("Sign off"));
        ui.checkbox(&mut self.ignore_whitespace, tr("Ignore whitespace"));
        let (ok, cancel) = ok_cancel(ui, tr("Apply"), !self.path.trim().is_empty());
        if ok {
            let p = self.path.trim();
            let is_mailbox = self.is_dir || std::fs::read_to_string(p).map(|t| t.starts_with("From ")).unwrap_or(false);
            let args = if self.is_dir {
                let mut files: Vec<String> = std::fs::read_dir(p)
                    .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path().display().to_string()).filter(|f| f.ends_with(".patch")).collect())
                    .unwrap_or_default();
                files.sort();
                commands::apply_mailbox_patch(self.sign_off, self.ignore_whitespace, None).args(files)
            } else if is_mailbox {
                commands::apply_mailbox_patch(self.sign_off, self.ignore_whitespace, Some(p))
            } else {
                commands::apply_diff_patch(self.ignore_whitespace, p)
            };
            cx.run(GitRun::new(tr("Apply patch"), args).conflicts());
            return false;
        }
        !cancel
    }
}

#[derive(Default)]
pub struct ViewPatchDialog {
    path: String,
    patches: Vec<gitext_core::patch::Patch>,
    selected: usize,
    viewer: DiffViewer,
}

impl Dialog for ViewPatchDialog {
    fn title(&self) -> String {
        tr("View patch file").into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(980.0, 640.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(420.0));
            if ui.button(tr("Browse…")).clicked() {
                if let Some(p) = crate::util::pick_file(None, Some((tr("Patch"), &["patch", "diff"]))) {
                    self.path = p;
                }
            }
            if ui.button(tr("Load")).clicked() {
                match std::fs::read(&self.path) {
                    Ok(b) => {
                        self.patches = create_patches_from_string(&String::from_utf8_lossy(&b));
                        self.selected = 0;
                    }
                    Err(e) => cx.error(tr("View patch"), e.to_string()),
                }
            }
        });
        egui::SidePanel::left("vp_files").resizable(true).default_width(260.0).show_inside(ui, |ui| {
            for (i, p) in self.patches.iter().enumerate() {
                let name = p.file_name_b.clone().unwrap_or_else(|| p.file_name_a.clone());
                if ui.selectable_label(self.selected == i, format!("{:?}: {name}", p.change_type)).clicked() {
                    self.selected = i;
                }
            }
        });
        let content = self.patches.get(self.selected).map(|p| ViewerContent::Diff(p.text.clone())).unwrap_or(ViewerContent::Empty(tr("Load a patch file").into()));
        self.viewer.ui(ui, &content, true, &[]);
        !ui.input(|i| i.key_pressed(egui::Key::Escape))
    }
}
