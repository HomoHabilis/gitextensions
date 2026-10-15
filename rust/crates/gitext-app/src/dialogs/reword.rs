//! "Reword commit" and "Edit commit" of the revision grid (`RevisionGridControl.LaunchRebase`):
//! an interactive rebase from the parent of the commit, whose todo list is edited by a
//! `GIT_SEQUENCE_EDITOR` that marks the first commit `reword` or `edit`.

use egui::{RichText, Ui};
use gitext_core::commands::{self, RebaseOptions};
use gitext_core::{GitRevision, ObjectId};

use super::{ok_cancel, Cx, Dialog, DialogKind, GitRun};
use crate::i18n::{tr, trf};
use crate::tasks::Task;
use crate::theme::Palette;

/// File (in the git directory) the new message is written to for "Reword commit".
const MESSAGE_FILE: &str = "GITEXT_REWORD_MSG";

/// The rebase that marks the first commit of the todo list with `command` (`edit`, `reword`):
/// `git rebase -i --autostash <parent>` (or `--root`). As upstream, the todo list is edited
/// with sed: `-i.bak` is accepted by both GNU sed (Git for Windows, Linux) and BSD sed (macOS).
pub fn rebase_run(title: &str, command: &str, parent: Option<ObjectId>) -> GitRun {
    let options = RebaseOptions {
        branch_name: parent.map(|p| p.to_string()).unwrap_or_else(|| "--root".into()),
        interactive: true,
        auto_stash: true,
        support_rebase_merges: true,
        ..Default::default()
    };
    let args = commands::rebase(&options).expect("no from/onto");
    GitRun::new(title, args).conflicts().env("GIT_SEQUENCE_EDITOR", format!("sed -i.bak -e '1s/^pick /{command} /'"))
}

/// The rebase that rewords the commit after `parent` with `message`: the message is written to
/// a file of the git directory that the `GIT_EDITOR` of the `reword` step moves over the
/// message git asks for.
pub fn reword_run(git_dir: &std::path::Path, message: &str, parent: Option<ObjectId>) -> std::io::Result<GitRun> {
    std::fs::write(git_dir.join(MESSAGE_FILE), format!("{}\n", message.trim_end()))?;
    // git runs the editor in the top of the work tree, where `git rev-parse` finds the file
    // whatever the platform's path form (Windows, WSL); `mv` consumes it
    let editor = format!("mv -f \"$(git rev-parse --git-path {MESSAGE_FILE})\"");
    Ok(rebase_run(tr("Reword commit"), "reword", parent).env("GIT_EDITOR", editor))
}

/// Port of "Reword commit": asks the new message here (upstream opens git's editor) and
/// gives it to git as the editor of the `reword` step.
pub struct RewordCommitDialog {
    id: ObjectId,
    parent: Option<ObjectId>,
    subject: String,
    message: Option<String>,
    load: Option<Task<Option<String>>>,
}

impl RewordCommitDialog {
    pub fn new(rev: &GitRevision) -> Self {
        let message = rev.body().map(|b| b.trim_end().to_string());
        RewordCommitDialog { id: rev.object_id, parent: rev.parents().first().copied(), subject: rev.subject.clone(), message, load: None }
    }
}

impl Dialog for RewordCommitDialog {
    fn title(&self) -> String {
        tr("Reword commit").into()
    }

    fn kind(&self) -> DialogKind {
        DialogKind::Modal(620.0)
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let palette = Palette::for_ui(ui);
        let Some(m) = cx.module.cloned() else { return false };
        // the grid drops the body of old commits: read the message in the background
        if self.message.is_none() {
            let task = self.load.get_or_insert_with(|| {
                let id = self.id;
                Task::spawn(cx.ctx, move || m.get_revision(&id.to_string(), false).ok().flatten().map(|r| r.body().unwrap_or(&r.subject).trim_end().to_string()))
            });
            if let Some(msg) = task.try_take() {
                self.message = Some(msg.unwrap_or_else(|| self.subject.clone()));
            }
        }
        ui.label(trf("New message of commit {0}:", &[&self.id.to_short_string()]));
        let mut ok_enabled = false;
        match &mut self.message {
            None => {
                ui.spinner();
            }
            Some(msg) => {
                ui.add(egui::TextEdit::multiline(msg).desired_rows(10).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
                ok_enabled = !msg.trim().is_empty();
            }
        }
        ui.label(RichText::new(tr("The commits after it are rebased (git rebase -i): their hashes change.")).small().color(palette.muted));
        let (ok, cancel) = ok_cancel(ui, tr("Reword"), ok_enabled);
        if ok {
            let Some(msg) = &self.message else { return true };
            let Some(module) = cx.module else { return false };
            match reword_run(module.git_dir(), msg, self.parent) {
                Ok(run) => cx.run(run),
                Err(e) => cx.error(tr("Reword commit"), e.to_string()),
            }
            return false;
        }
        !cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitext_core::exec::Executable;

    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A repository with commits "one", "two", "three"; returns the ids (oldest first).
    fn repo() -> (tempfile::TempDir, Vec<ObjectId>) {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        git(d, &["init", "-q", "-b", "main"]);
        git(d, &["config", "user.name", "Test"]);
        git(d, &["config", "user.email", "test@example.com"]);
        let mut ids = Vec::new();
        for name in ["one", "two", "three"] {
            std::fs::write(d.join(name), name).unwrap();
            git(d, &["add", name]);
            git(d, &["commit", "-q", "-m", name]);
            ids.push(ObjectId::parse(&git(d, &["rev-parse", "HEAD"])).unwrap());
        }
        (t, ids)
    }

    fn run(dir: &std::path::Path, run: &GitRun) -> bool {
        let mut exe = Executable::git(dir.to_path_buf());
        exe.env.extend(run.env.iter().cloned());
        run.commands.iter().all(|args| exe.run(args).map(|r| r.success()).unwrap_or(false))
    }

    #[test]
    fn reword_changes_the_message_of_an_older_commit() {
        let (t, ids) = repo();
        let d = t.path();
        let r = reword_run(&d.join(".git"), "second commit\n\nwith a body", Some(ids[0])).unwrap();
        assert!(run(d, &r));
        assert_eq!(git(d, &["log", "--format=%s", "main"]), "three\nsecond commit\none");
        assert_eq!(git(d, &["log", "-1", "--format=%b", "main~1"]), "with a body");
        assert!(!d.join(".git").join(MESSAGE_FILE).exists(), "the message file is consumed");
    }

    #[test]
    fn reword_the_root_commit() {
        let (t, _) = repo();
        let d = t.path();
        assert!(run(d, &reword_run(&d.join(".git"), "first", None).unwrap()));
        assert_eq!(git(d, &["log", "--format=%s", "main"]), "three\ntwo\nfirst");
    }

    #[test]
    fn edit_stops_at_the_commit() {
        let (t, ids) = repo();
        let d = t.path();
        assert!(run(d, &rebase_run("Edit commit", "edit", Some(ids[0]))));
        assert!(d.join(".git/rebase-merge").exists(), "the rebase stopped");
        assert_eq!(git(d, &["rev-parse", "HEAD"]), ids[1].to_string(), "at the edited commit");
        git(d, &["rebase", "--continue"]);
        assert_eq!(git(d, &["rev-parse", "HEAD"]), ids[2].to_string(), "nothing changed");
    }
}
