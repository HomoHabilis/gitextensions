//! Ports of `FormCreateTag` and `FormDeleteTag`.

use egui::Ui;
use gitext_core::commands::{self, TagOperation};
use gitext_core::{GitArgs, ObjectId};

use super::{ok_cancel, Cx, Dialog, GitRun};
use crate::i18n::{tr, trf};
use crate::repo::RepoData;

pub struct CreateTagDialog {
    revision: String,
    name: String,
    operation: TagOperation,
    message: String,
    key: String,
    force: bool,
    push: bool,
}

impl CreateTagDialog {
    pub fn new(id: ObjectId) -> Self {
        CreateTagDialog {
            revision: if id.is_zero() { "HEAD".into() } else { id.to_string() },
            name: String::new(),
            operation: TagOperation::Lightweight,
            message: String::new(),
            key: String::new(),
            force: false,
            push: false,
        }
    }
}

impl Dialog for CreateTagDialog {
    fn title(&self) -> String {
        tr("Create tag").into()
    }

    fn kind(&self) -> super::DialogKind {
        super::DialogKind::Modal(480.0)
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        egui::Grid::new("create_tag").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label(tr("Tag name"));
            ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(320.0)).request_focus();
            ui.end_row();
            ui.label(tr("Create tag at"));
            ui.add(egui::TextEdit::singleline(&mut self.revision).desired_width(320.0));
            ui.end_row();
            ui.label(tr("Type"));
            egui::ComboBox::from_id_salt("tag_op")
                .selected_text(match self.operation {
                    TagOperation::Lightweight => tr("Lightweight tag"),
                    TagOperation::Annotate => tr("Annotated tag"),
                    TagOperation::SignWithDefaultKey => tr("Sign with default GPG key"),
                    TagOperation::SignWithSpecificKey => tr("Sign with specific GPG key"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.operation, TagOperation::Lightweight, tr("Lightweight tag"));
                    ui.selectable_value(&mut self.operation, TagOperation::Annotate, tr("Annotated tag"));
                    ui.selectable_value(&mut self.operation, TagOperation::SignWithDefaultKey, tr("Sign with default GPG key"));
                    ui.selectable_value(&mut self.operation, TagOperation::SignWithSpecificKey, tr("Sign with specific GPG key"));
                });
            ui.end_row();
            if self.operation == TagOperation::SignWithSpecificKey {
                ui.label(tr("Key id"));
                ui.text_edit_singleline(&mut self.key);
                ui.end_row();
            }
        });
        if self.operation.can_provide_message() {
            ui.label(tr("Message"));
            ui.add(egui::TextEdit::multiline(&mut self.message).desired_rows(4).desired_width(f32::INFINITY));
        }
        ui.checkbox(&mut self.force, tr("Force (replace an existing tag)"));
        ui.checkbox(&mut self.push, tr("Push tag to remote"));
        let (ok, cancel) = ok_cancel(ui, tr("Create tag"), !self.name.trim().is_empty());
        if ok {
            let Some(m) = cx.module else { return false };
            let id = m.rev_parse(&self.revision);
            let msg_file = m.git_dir().join("TAGMESSAGE");
            let msg_path = if self.operation.can_provide_message() {
                let _ = std::fs::write(&msg_file, &self.message);
                Some(msg_file.display().to_string())
            } else {
                None
            };
            match commands::create_tag(&self.name, id, self.operation, &self.key, msg_path.as_deref(), self.force) {
                Ok(args) => {
                    let mut cmds = vec![args];
                    if self.push {
                        if let Some(remote) = cx.data.and_then(|d| d.current_remote()) {
                            cmds.push(commands::push_tag(&remote, &self.name, false, Default::default()));
                        }
                    }
                    cx.run(GitRun::many(trf("Create tag {0}", &[&self.name]), cmds));
                    return false;
                }
                Err(e) => cx.error(tr("Create tag"), e),
            }
        }
        !cancel
    }
}

pub struct DeleteTagDialog {
    tag: String,
    tags: Vec<String>,
    remote: bool,
}

impl DeleteTagDialog {
    pub fn new(data: &RepoData, tag: Option<String>) -> Self {
        DeleteTagDialog { tag: tag.unwrap_or_default(), tags: data.tags().map(|t| t.name.clone()).collect(), remote: false }
    }
}

impl Dialog for DeleteTagDialog {
    fn title(&self) -> String {
        tr("Delete tag").into()
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        ui.label(tr("Tag to delete:"));
        super::branch_combo(ui, "delete_tag", &mut self.tag, &self.tags, 360.0);
        let remote = cx.data.and_then(|d| d.current_remote());
        if let Some(r) = &remote {
            ui.checkbox(&mut self.remote, trf("Also delete the tag on remote '{0}'", &[r]));
        }
        let (ok, cancel) = ok_cancel(ui, tr("Delete"), !self.tag.trim().is_empty());
        if ok {
            let mut cmds = vec![commands::delete_tag(self.tag.trim())];
            if self.remote {
                if let Some(r) = remote {
                    cmds.push(GitArgs::new("push").arg(r).arg(format!(":refs/tags/{}", self.tag.trim())));
                }
            }
            cx.run(GitRun::many(tr("Delete tag"), cmds));
            return false;
        }
        !cancel
    }
}
