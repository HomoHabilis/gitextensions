//! Git Extensions — cross-platform Rust port.
//!
//! Command line (port of `GitUICommands.RunCommand`):
//! `gitext [command] [arguments] [path]`, e.g. `gitext browse ~/src/repo`, `gitext commit`,
//! `gitext blame src/main.rs 42`. Without a command the repository in the current directory
//! (or the dashboard) is shown.

mod app;
mod browse;
mod cli;
mod dialogs;
mod repo;
mod tasks;
mod theme;
mod util;
mod views;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    let parsed = match cli::parse(&args, &cwd) {
        Ok(p) => p,
        Err(message) => {
            println!("{message}");
            return Ok(());
        }
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Git Extensions")
            .with_app_id("gitextensions")
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([720.0, 460.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Git Extensions",
        options,
        Box::new(move |cc| Ok(Box::new(app::GitExtApp::new(cc, parsed.repo, parsed.command)))),
    )
}
