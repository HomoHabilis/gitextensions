//! Ports of `FormGitIgnore` and `FormAddToGitIgnore`.

use egui::{RichText, Ui, Vec2};

use super::{ok_cancel, Action, Cx, Dialog, DialogKind};

const DEFAULT_PATTERNS: &str = "#ignore thumbnails created by windows\nThumbs.db\n#Ignore files built by Visual Studio\n*.obj\n*.exe\n*.pdb\n*.user\n*.aps\n*.pch\n*.vspscc\n*_i.c\n*_p.c\n*.ncb\n*.suo\n*.tlb\n*.tlh\n*.bak\n*.cache\n*.ilk\n*.log\n[Bb]in\n[Dd]ebug*/\n*.lib\n*.sbr\nobj/\n[Rr]elease*/\n_ReSharper*/\n[Tt]est[Rr]esult*\n.vs/\n#Nuget packages folder\npackages/\n#Rust\ntarget/\n#Node\nnode_modules/\n";

pub struct GitIgnoreDialog {
    local: bool,
    text: Option<String>,
}

impl GitIgnoreDialog {
    /// `local`: edit `.git/info/exclude` instead of `.gitignore`.
    pub fn new(local: bool) -> Self {
        GitIgnoreDialog { local, text: None }
    }
}

impl Dialog for GitIgnoreDialog {
    fn title(&self) -> String {
        if self.local { "Edit .git/info/exclude".into() } else { "Edit .gitignore".into() }
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(640.0, 560.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let Some(m) = cx.module else { return false };
        let path = if self.local { m.exclude_file_path() } else { m.work_dir().join(".gitignore") };
        let text = self.text.get_or_insert_with(|| std::fs::read_to_string(&path).unwrap_or_default());
        ui.horizontal(|ui| {
            ui.label(RichText::new(path.display().to_string()).small());
            if ui.button("Add default patterns").clicked() {
                text.push_str(DEFAULT_PATTERNS);
            }
        });
        let h = ui.available_height() - 50.0;
        egui::ScrollArea::vertical().max_height(h).show(ui, |ui| {
            ui.add_sized(Vec2::new(ui.available_width(), h), egui::TextEdit::multiline(text).font(egui::TextStyle::Monospace));
        });
        let (ok, cancel) = ok_cancel(ui, "Save", true);
        if ok {
            if let Some(p) = path.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            if let Err(e) = std::fs::write(&path, text.as_bytes()) {
                cx.error("Save", e.to_string());
                return true;
            }
            cx.push(Action::RefreshStatus);
            return false;
        }
        !cancel
    }
}

pub struct AddToGitIgnoreDialog {
    patterns: String,
    local: bool,
}

impl AddToGitIgnoreDialog {
    pub fn new(files: Vec<String>) -> Self {
        AddToGitIgnoreDialog { patterns: files.iter().map(|f| format!("/{f}")).collect::<Vec<_>>().join("\n"), local: false }
    }
}

impl Dialog for AddToGitIgnoreDialog {
    fn title(&self) -> String {
        "Add to .gitignore".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Modal(520.0)
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.label("Patterns to ignore:");
        ui.add(egui::TextEdit::multiline(&mut self.patterns).font(egui::TextStyle::Monospace).desired_rows(5).desired_width(f32::INFINITY));
        ui.checkbox(&mut self.local, "Add to .git/info/exclude (not shared)");
        let patterns: Vec<&str> = self.patterns.lines().filter(|l| !l.trim().is_empty()).collect();
        if let Some(m) = cx.module {
            let matching = m.files_matching_ignore(&patterns);
            ui.label(RichText::new(format!("{} untracked file(s) will be ignored", matching.len())).small());
        }
        let (ok, cancel) = ok_cancel(ui, "Add", !patterns.is_empty());
        if ok {
            if let Some(m) = cx.module {
                if let Err(e) = m.add_to_ignore(&patterns, self.local) {
                    cx.error("Add to .gitignore", e.to_string());
                }
            }
            cx.push(Action::RefreshStatus);
            return false;
        }
        !cancel
    }
}

/// Port of `FormAddFiles`: `git add` of a path or pattern.
pub struct AddFilesDialog {
    pattern: String,
    force: bool,
}

impl AddFilesDialog {
    pub fn new(pattern: String) -> Self {
        AddFilesDialog { pattern, force: false }
    }
}

impl Dialog for AddFilesDialog {
    fn title(&self) -> String {
        "Add files".into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.label("Files to add (a path or a pattern, e.g. *.txt):");
        ui.add(egui::TextEdit::singleline(&mut self.pattern).desired_width(400.0).font(egui::TextStyle::Monospace));
        ui.checkbox(&mut self.force, "Force (also add ignored files)");
        let (ok, cancel) = ok_cancel(ui, "Add files", !self.pattern.trim().is_empty());
        if ok {
            let mut args = gitext_core::GitArgs::new("add");
            if self.force {
                args.add("--force");
            }
            args.add("--");
            args.add(self.pattern.trim());
            cx.run(super::GitRun::new("Add files", args));
            return false;
        }
        !cancel
    }
}
