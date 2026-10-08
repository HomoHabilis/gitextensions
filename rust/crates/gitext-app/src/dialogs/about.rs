//! Ports of `FormAbout` and `FormCommandlineHelp`.

use egui::{RichText, Ui};

use super::{Cx, Dialog};

pub struct AboutDialog;

impl Dialog for AboutDialog {
    fn title(&self) -> String {
        "About Git Extensions".into()
    }

    fn ui(&mut self, ui: &mut Ui, _cx: &mut Cx) -> bool {
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Git Extensions").size(24.0).strong());
            ui.label(format!("Version {} (Rust port)", env!("CARGO_PKG_VERSION")));
            ui.add_space(6.0);
            ui.label("A graphical user interface for git, ported to Rust and egui so that it runs on Windows, macOS and Linux.");
            let git = gitext_core::Executable::git(std::env::temp_dir()).output(&gitext_core::GitArgs::new("version")).unwrap_or_default();
            ui.label(RichText::new(git.trim()).small());
            ui.add_space(6.0);
            ui.hyperlink_to("gitextensions.github.io", "https://gitextensions.github.io/");
            ui.label(RichText::new("Licensed under the GNU GPL v3").small());
        });
        ui.add_space(8.0);
        !ui.vertical_centered(|ui| ui.button("Close").clicked()).inner
    }
}

pub struct CommandLineHelpDialog;

impl Dialog for CommandLineHelpDialog {
    fn title(&self) -> String {
        "Command line usage".into()
    }

    fn kind(&self) -> super::DialogKind {
        super::DialogKind::Modal(560.0)
    }

    fn ui(&mut self, ui: &mut Ui, _cx: &mut Cx) -> bool {
        ui.label(RichText::new(crate::cli::HELP).monospace());
        !ui.button("Close").clicked()
    }
}
