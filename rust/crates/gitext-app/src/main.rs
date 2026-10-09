//! Git Extensions — cross-platform Rust port.
//!
//! Command line (port of `GitUICommands.RunCommand`):
//! `gitext [command] [arguments] [path]`, e.g. `gitext browse ~/src/repo`, `gitext commit`,
//! `gitext blame src/main.rs 42`. Without a command the repository in the current directory
//! (or the dashboard) is shown.

// No console window behind the GUI on Windows (release builds; debug builds keep it for logs).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod browse;
mod cli;
mod dialogs;
mod prof;
mod repo;
mod shell_ext;
mod tasks;
mod theme;
mod util;
mod views;
mod wsl;

use std::path::{Path, PathBuf};

use app::StartCommand;
use cli::CliExit;

fn main() -> eframe::Result<()> {
    wsl::prefer_x11_for_gtk_tools();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    let (repo, command) = match cli::parse(&args, &cwd) {
        Ok(p) => (p.repo, p.command),
        Err(CliExit::Info(text)) => {
            println!("{text}");
            return Ok(());
        }
        Err(CliExit::Error(message)) => {
            report_error(&message);
            std::process::exit(2);
        }
        Err(CliExit::NotARepository(dir)) => match offer_init(&dir) {
            Some(dir) => (None, StartCommand::Init(Some(dir.display().to_string()))),
            None => std::process::exit(1),
        },
    };
    // commands that do not open a window
    match &command {
        StartCommand::ShellExt(install) => {
            let hidden = load_settings().shell_menu_hidden_items;
            let result = if *install { shell_ext::register(&hidden) } else { shell_ext::unregister() };
            if let Err(e) = result {
                eprintln!("{e}");
                std::process::exit(1);
            }
            return Ok(());
        }
        StartCommand::DiffTool(file) => {
            if let Err(e) = run_difftool(repo.as_deref().unwrap_or(&cwd), file) {
                report_error(&e);
                std::process::exit(1);
            }
            return Ok(());
        }
        _ => {}
    }
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
        Box::new(move |cc| Ok(Box::new(app::GitExtApp::new(cc, repo, command)))),
    )
}

fn load_settings() -> gitext_core::settings::AppSettings {
    let settings = gitext_core::settings::AppSettings::default_path().map(|p| gitext_core::settings::AppSettings::load(&p)).unwrap_or_default();
    gitext_core::exec::set_git_command(&settings.git_command);
    gitext_core::exec::set_wsl_git_enabled(settings.wsl_git_enabled);
    settings
}

/// Shows an error: in a message box on Windows, where the application has no console (and
/// is often started from the Explorer menu), on stderr elsewhere.
fn report_error(message: &str) {
    eprintln!("{message}");
    #[cfg(windows)]
    {
        rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title("Git Extensions").set_description(message).set_buttons(rfd::MessageButtons::Ok).show();
    }
}

/// A repository command was run outside of a repository (e.g. from the Explorer menu):
/// offers to create a repository in `dir` (Windows). Returns the directory to initialize.
fn offer_init(dir: &Path) -> Option<PathBuf> {
    let message = format!("'{}' is not inside a git repository.", dir.display());
    #[cfg(windows)]
    {
        let answer = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Info)
            .set_title("Git Extensions")
            .set_description(format!("{message}\n\nCreate a new repository here?"))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        (answer == rfd::MessageDialogResult::Yes).then(|| dir.to_path_buf())
    }
    #[cfg(not(windows))]
    {
        eprintln!("{message}");
        None
    }
}

/// `gitext difftool <file>` (port of `GitModule.OpenWithDifftool`): starts the diff tool for
/// the changes of a file, without a window.
fn run_difftool(repo: &Path, file: &str) -> Result<(), String> {
    use gitext_core::diff_tools::{self, ToolConfigStore, ToolLaunch, ToolType};
    use gitext_core::GitArgs;
    load_settings();
    let module = gitext_core::GitModule::open(repo).map_err(|e| e.to_string())?;
    let exe = module.git();
    let status = exe.run(&GitArgs::new("status").arg("--porcelain").arg("--").arg(file)).map_err(|e| e.to_string())?;
    if status.success() && status.stdout_str().trim().is_empty() {
        return Err(format!("'{file}' has no changes."));
    }
    // the git of a WSL distro uses the tools configured in the distro
    let launch = if exe.wsl_distro.is_empty() { diff_tools::resolve_launch(&ToolConfigStore::new(exe.clone()), ToolType::Diff) } else { Some(ToolLaunch::Configured) };
    let launch = launch.ok_or_else(|| format!("No diff tool is installed.\n\n{}", diff_tools::install_hint()))?;
    let args = diff_tools::launch_args(&launch, ToolType::Diff, &GitArgs::new("difftool").arg("--").arg(file));
    let result = exe.run(&args).map_err(|e| e.to_string())?;
    if !result.success() {
        return Err(format!("The diff tool exited with code {}.\n\n{}", result.exit_code, result.all_output().trim()));
    }
    Ok(())
}
