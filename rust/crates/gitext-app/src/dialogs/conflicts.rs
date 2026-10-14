//! Port of `FormResolveConflicts`.

use egui::{Key, RichText, Ui, Vec2};
use gitext_core::exec::ExecResult;
use gitext_core::module::{ConflictData, ConflictSide};
use gitext_core::{commands, GitArgs, GitModule};

use super::{Action, Cx, Dialog, DialogKind, GitRun};
use crate::tasks::{Loader, Task};
use crate::theme::Palette;
use crate::views::plain_key;

/// What the selected file can be resolved with (the buttons of the dialog and the
/// `FormMergeConflicts` hotkeys: M merge, L local, R remote, B base, plus D delete).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Merge,
    Local,
    Remote,
    Base,
    Delete,
    Editor,
    MarkSolved,
}

/// Extensions of the backup files merge tools write next to the merged file.
const BACKUP_EXTENSIONS: [&str; 2] = ["orig", "bak"];

/// The merge tool runs while the dialog stays usable; the list is rescanned once it closes.
struct MergeToolRun {
    file: String,
    task: Task<Result<ExecResult, String>>,
}

/// The merge tool closed but the file is still in conflict: ask whether it is solved
/// (`_askMergeConflictSolved`), with the tool's output when it failed.
struct AskSolved {
    file: String,
    output: String,
}

#[derive(Default)]
pub struct ConflictsDialog {
    generation: u64,
    files: Loader<u64, Vec<ConflictData>>,
    selected: Option<String>,
    /// Row of the selection, so that the next file is selected once the selected one is resolved.
    selected_row: usize,
    merge_tool: Option<MergeToolRun>,
    /// Checked against the list rescanned after the merge tool closed.
    after_merge_tool: Option<AskSolved>,
    ask_solved: Option<AskSolved>,
}

impl ConflictsDialog {
    fn rescan(&mut self) {
        self.generation += 1;
    }

    /// The choices for a conflict, with their labels, as `FormResolveConflicts` offers them
    /// for each kind of conflict.
    fn choices(c: &ConflictData, rebasing: bool) -> Vec<(Choice, String, Option<Key>)> {
        let (ours, theirs) = if rebasing { ("theirs", "ours") } else { ("ours", "theirs") };
        let mut v = Vec::new();
        match (c.local.is_some(), c.remote.is_some()) {
            (true, true) => {
                if !c.is_special() {
                    v.push((Choice::Merge, "Merge in merge tool".to_string(), Some(Key::M)));
                }
                v.push((Choice::Local, format!("Choose local ({ours})"), Some(Key::L)));
                v.push((Choice::Remote, format!("Choose remote ({theirs})"), Some(Key::R)));
                if c.base.is_some() {
                    v.push((Choice::Base, "Choose base".to_string(), Some(Key::B)));
                } else {
                    v.push((Choice::Delete, "Delete file".to_string(), Some(Key::D)));
                }
            }
            (false, true) => {
                v.push((Choice::Delete, "Delete file (local)".to_string(), Some(Key::D)));
                v.push((Choice::Remote, "Keep modified (remote)".to_string(), Some(Key::R)));
            }
            (true, false) => {
                v.push((Choice::Local, "Keep modified (local)".to_string(), Some(Key::L)));
                v.push((Choice::Delete, "Delete file (remote)".to_string(), Some(Key::D)));
            }
            (false, false) => {
                v.push((Choice::Delete, "Delete file".to_string(), Some(Key::D)));
            }
        }
        if c.base.is_some() && !(c.local.is_some() && c.remote.is_some()) {
            v.push((Choice::Base, "Keep base file".to_string(), Some(Key::B)));
        }
        if c.local.is_some() || c.remote.is_some() {
            v.push((Choice::Editor, "Open in editor".to_string(), None));
        }
        v.push((Choice::MarkSolved, "Mark as solved".to_string(), None));
        v
    }

    fn description(c: &ConflictData) -> &'static str {
        match (c.base.is_some(), c.local.is_some(), c.remote.is_some()) {
            (true, true, true) => "The file has been changed both locally and remotely. Merge the changes.",
            (false, true, true) => "A file with the same name has been created locally and remotely. Choose the file you want to keep or merge the files.",
            (_, false, true) => "The file has been deleted locally and modified remotely. Choose to delete the file or keep the modified version.",
            (_, true, false) => "The file has been modified locally and deleted remotely. Choose to delete the file or keep the modified version.",
            (_, false, false) => "The file has been deleted both locally and remotely.",
        }
    }

    fn short_state(c: &ConflictData) -> &'static str {
        match (c.base.is_some(), c.local.is_some(), c.remote.is_some()) {
            (true, true, true) => "both modified",
            (false, true, true) => "both added",
            (_, false, true) => "deleted locally",
            (_, true, false) => "deleted remotely",
            (_, false, false) => "deleted by both",
        }
    }

    fn apply(&mut self, choice: Choice, c: &ConflictData, m: &GitModule, cx: &mut Cx) {
        let f = c.file_name.as_str();
        let result = match choice {
            Choice::Merge => {
                self.start_merge_tool(f, m, cx);
                return;
            }
            Choice::Editor => {
                crate::util::open_in_editor(&m.work_dir().join(f), &cx.settings.editor);
                return;
            }
            Choice::Local => m.choose_conflict_side(f, ConflictSide::Local),
            Choice::Remote => m.choose_conflict_side(f, ConflictSide::Remote),
            Choice::Base => m.choose_conflict_side(f, ConflictSide::Base),
            Choice::Delete => m.delete_conflicted_file(f),
            // -A also stages the deletion of a file missing from the working directory
            Choice::MarkSolved => m.run_checked(&GitArgs::new("add").arg("-A").arg("--").arg(f)).map(|_| ()),
        };
        if let Err(e) = result {
            cx.error("Resolve", e.to_string());
        }
        self.ask_solved = None;
        self.rescan();
    }

    fn start_merge_tool(&mut self, f: &str, m: &GitModule, cx: &mut Cx) {
        use gitext_core::diff_tools::{self, ToolConfigStore, ToolLaunch, ToolType};
        // the git of a WSL distro uses the tools configured in the distro
        let exe = m.git();
        let launch = if exe.wsl_distro.is_empty() {
            diff_tools::resolve_launch(&ToolConfigStore::new(exe), ToolType::Merge)
        } else {
            Some(ToolLaunch::Configured)
        };
        let Some(launch) = launch else {
            cx.open(super::tools::NoToolDialog { tool_type: ToolType::Merge });
            return;
        };
        // git leaves no `.orig` backup of the conflicted file
        let mut args = GitArgs::empty().args(["-c", "mergetool.keepBackup=false"]);
        args.add_all(diff_tools::launch_args(&launch, ToolType::Merge, &GitArgs::new("mergetool").arg("--").arg(f)).as_slice());
        // backups the merge tool itself writes next to the file (KDiff3 `.orig`, WinMerge `.bak`)
        let backups: Vec<std::path::PathBuf> = BACKUP_EXTENSIONS.iter().map(|ext| m.work_dir().join(format!("{f}.{ext}"))).filter(|p| !p.exists()).collect();
        let git = m.git();
        self.ask_solved = None;
        self.merge_tool = Some(MergeToolRun {
            file: f.to_string(),
            task: Task::spawn(cx.ctx, move || {
                let r = git.run(&args).map_err(|e| e.to_string());
                // only the ones created while the tool ran
                for b in backups.iter().filter(|b| b.is_file()) {
                    let _ = std::fs::remove_file(b);
                }
                r
            }),
        });
    }
}

impl Dialog for ConflictsDialog {
    fn title(&self) -> String {
        "Resolve merge conflicts".into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Window(Vec2::new(820.0, 480.0))
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let palette = Palette::for_ui(ui);
        let Some(m) = cx.module.cloned() else { return false };

        // the merge tool closed: rescan, then ask about the file if it is still in conflict
        if let Some(run) = self.merge_tool.as_mut() {
            if let Some(r) = run.task.try_take() {
                let output = match r {
                    Ok(r) if r.success() => String::new(),
                    Ok(r) => r.all_output(),
                    Err(e) => e,
                };
                self.after_merge_tool = Some(AskSolved { file: run.file.clone(), output });
                self.merge_tool = None;
                cx.push(Action::RefreshStatus);
                self.rescan();
            }
        }

        let mm = m.clone();
        let loaded = self.files.request(cx.ctx, self.generation, move || mm.get_conflicts()).is_some();
        // the previous list stays shown while rescanning
        let files = self.files.value.clone().unwrap_or_default();
        if loaded {
            if let Some(a) = self.after_merge_tool.take() {
                if files.iter().any(|c| c.file_name == a.file) {
                    self.ask_solved = Some(a);
                }
            }
        }

        // keep a selection: the next file once the selected one is resolved
        if !files.iter().any(|c| Some(&c.file_name) == self.selected.as_ref()) {
            self.selected = files.get(self.selected_row.min(files.len().saturating_sub(1))).map(|c| c.file_name.clone());
        }
        let row = files.iter().position(|c| Some(&c.file_name) == self.selected.as_ref());
        if let Some(r) = row {
            self.selected_row = r;
        }
        let current = row.map(|r| files[r].clone());

        let state = m.state();
        let busy = self.merge_tool.is_some();
        let choices = current.as_ref().map(|c| Self::choices(c, state.rebasing)).unwrap_or_default();

        // keys (FormMergeConflicts hotkeys)
        let mut chosen = None;
        if plain_key(ui.ctx(), Key::F5) {
            self.rescan();
        }
        if !files.is_empty() {
            if plain_key(ui.ctx(), Key::ArrowDown) {
                self.selected = files.get((self.selected_row + 1).min(files.len() - 1)).map(|c| c.file_name.clone());
            }
            if plain_key(ui.ctx(), Key::ArrowUp) {
                self.selected = files.get(self.selected_row.saturating_sub(1)).map(|c| c.file_name.clone());
            }
        }
        if !busy {
            if let Some(c) = &current {
                if plain_key(ui.ctx(), Key::Enter) && choices.iter().any(|(ch, ..)| *ch == Choice::Merge) {
                    chosen = Some(Choice::Merge);
                }
                for (ch, _, key) in &choices {
                    if key.is_some_and(|k| plain_key(ui.ctx(), k)) {
                        chosen = Some(*ch);
                    }
                }
                // choosing the deleted side deletes the file
                if c.local.is_none() && plain_key(ui.ctx(), Key::L) || c.remote.is_none() && plain_key(ui.ctx(), Key::R) {
                    chosen = Some(Choice::Delete);
                }
            }
        }

        if let Some(d) = state.description() {
            ui.label(RichText::new(d).strong().color(palette.warning));
        }
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
            // one file per row; the list takes the width the choices leave
            let list_width = (ui.available_width() - 420.0).max(300.0);
            ui.vertical(|ui| {
                ui.set_width(list_width);
                egui::ScrollArea::vertical()
                    .id_salt("conflict_files")
                    .auto_shrink([false, true])
                    .min_scrolled_height(320.0)
                    .show(ui, |ui| {
                        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                            for c in &files {
                                let is_sel = self.selected.as_deref() == Some(c.file_name.as_str());
                                let text = format!("{}  ({})", c.file_name, Self::short_state(c));
                                let r = ui.add(egui::SelectableLabel::new(is_sel, text).truncate());
                                if r.clicked() {
                                    self.selected = Some(c.file_name.clone());
                                }
                                if r.double_clicked() && !busy && !c.is_special() && c.local.is_some() && c.remote.is_some() {
                                    self.selected = Some(c.file_name.clone());
                                    chosen = Some(Choice::Merge);
                                }
                            }
                            if files.is_empty() && loaded {
                                ui.label(RichText::new("All conflicts are resolved.").color(palette.success));
                            }
                        });
                    });
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_max_width(400.0);
                if let Some(c) = &current {
                    ui.label(RichText::new(&c.file_name).strong());
                    ui.label(Self::description(c));
                    let (local, remote) = if state.rebasing {
                        ("the branch you are rebasing onto", "the commit you are rebasing")
                    } else {
                        ("the current branch", "the branch you are merging")
                    };
                    ui.label(RichText::new(format!("Local: {local}. Remote: {remote}.")).small().weak());
                    ui.add_space(6.0);
                }
                ui.add_enabled_ui(current.is_some() && !busy, |ui| {
                    for (ch, label, key) in &choices {
                        let hint = key.map(|k| k.name().to_string()).unwrap_or_default();
                        if ui.add_sized(Vec2::new(240.0, 26.0), egui::Button::new(label.as_str()).shortcut_text(hint)).clicked() {
                            chosen = Some(*ch);
                        }
                    }
                });
                if let Some(run) = &self.merge_tool {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(format!("Waiting for the merge tool to close ({})…", run.file));
                    });
                }
                if let Some(a) = self.ask_solved.as_ref().map(|a| AskSolved {
                    file: a.file.clone(),
                    output: a.output.clone(),
                }) {
                    ui.add_space(6.0);
                    ui.label(RichText::new(format!("The merge tool closed but '{}' is still in conflict. Is the merge conflict solved?", a.file)).color(palette.warning));
                    if !a.output.trim().is_empty() {
                        egui::ScrollArea::vertical().id_salt("merge_tool_output").max_height(80.0).show(ui, |ui| {
                            ui.label(RichText::new(a.output.trim()).monospace().small());
                        });
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Yes, mark as solved").clicked() {
                            let file = a.file.clone();
                            if let Err(e) = m.run_checked(&GitArgs::new("add").arg("-A").arg("--").arg(&file)) {
                                cx.error("Resolve", e.to_string());
                            }
                            self.ask_solved = None;
                            self.rescan();
                        } else if ui.button("No").clicked() {
                            self.ask_solved = None;
                        }
                    });
                }
                ui.add_space(12.0);
                if ui.add(egui::Button::new("⟳ Rescan").shortcut_text("F5")).clicked() {
                    self.rescan();
                }
            });
        });
        if let (Some(ch), Some(c)) = (chosen, &current) {
            // not on the list from before a rescan: the file may be resolved already
            if !busy && loaded {
                self.apply(ch, c, &m, cx);
            }
        }

        ui.separator();
        let mut keep = true;
        ui.horizontal(|ui| {
            if files.is_empty() && loaded {
                if state.rebasing && ui.button("Continue rebase").clicked() {
                    cx.run(GitRun::new("Continue rebase", commands::continue_rebase()).conflicts());
                    keep = false;
                }
                if state.merging && ui.button("Commit merge…").clicked() {
                    cx.push(Action::OpenCommit);
                    keep = false;
                }
                if state.cherry_picking && ui.button("Continue cherry-pick").clicked() {
                    cx.run(GitRun::new("Continue", GitArgs::new("cherry-pick").arg("--continue")).conflicts());
                    keep = false;
                }
                if state.reverting && ui.button("Continue revert").clicked() {
                    cx.run(GitRun::new("Continue", GitArgs::new("revert").arg("--continue")).conflicts());
                    keep = false;
                }
                if state.applying_patch && ui.button("Continue applying patches").clicked() {
                    cx.run(GitRun::new("Continue", commands::resolved_mailbox()).conflicts());
                    keep = false;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    keep = false;
                }
                if state.merging && ui.button("Abort merge").clicked() {
                    cx.run(GitRun::new("Abort merge", commands::abort_merge()));
                    keep = false;
                }
                if state.rebasing && ui.button("Abort rebase").clicked() {
                    cx.run(GitRun::new("Abort rebase", commands::abort_rebase()));
                    keep = false;
                }
            });
        });
        if !keep {
            cx.push(Action::RefreshStatus);
        }
        keep
    }
}
