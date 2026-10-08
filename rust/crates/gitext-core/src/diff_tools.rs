//! Diff and merge tools: port of `GitCommands.DiffMergeTools` (`RegisteredDiffMergeTools`,
//! `DiffMergeToolConfigurationManager`, `PathUtil.FindInFolders`).
//!
//! The tools are configured in the global git config like Git Extensions does:
//! `diff.guitool` / `merge.guitool` name the tool, `difftool.<name>.path|cmd` and
//! `mergetool.<name>.path|cmd` tell git how to start it. The app then runs
//! `git difftool --gui` / `git mergetool --gui`.

use std::path::{Path, PathBuf};

use crate::args::GitArgs;
use crate::exec::Executable;

/// `SettingKeyString.DiffToolKey`.
pub const DIFF_TOOL_KEY: &str = "diff.guitool";
/// `SettingKeyString.MergeToolKey`.
pub const MERGE_TOOL_KEY: &str = "merge.guitool";
/// `SettingKeyString.MergeToolNoGuiKey` (also the fallback for older configurations).
pub const MERGE_TOOL_NO_GUI_KEY: &str = "merge.tool";
/// Fallback for the diff tool of configurations not made by Git Extensions.
pub const DIFF_TOOL_NO_GUI_KEY: &str = "diff.tool";

const DEFAULT_DIFF_COMMAND: &str = r#""$LOCAL" "$REMOTE""#;
const DEFAULT_MERGE_COMMAND: &str = r#""$LOCAL" "$REMOTE" "$BASE" "$MERGED""#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolType {
    Diff,
    Merge,
}

impl ToolType {
    /// The config key naming the tool and the prefix of its settings (`GetInfo`).
    pub fn keys(self) -> (&'static str, &'static str) {
        match self {
            ToolType::Diff => (DIFF_TOOL_KEY, "difftool"),
            ToolType::Merge => (MERGE_TOOL_KEY, "mergetool"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ToolType::Diff => "diff",
            ToolType::Merge => "merge",
        }
    }
}

/// A known diff/merge tool (port of the `DiffMergeTool` subclasses).
#[derive(Debug, Clone, Copy)]
pub struct DiffMergeTool {
    /// The git tool name (`--tool=<name>`).
    pub name: &'static str,
    /// Name shown to the user.
    pub display_name: &'static str,
    exe_windows: &'static str,
    /// Executable on Linux/macOS, `None` when the tool only exists on Windows.
    exe_unix: Option<&'static str>,
    /// `None`: not a diff tool. `Some("")`: the default command.
    diff_command: Option<&'static str>,
    merge_command: Option<&'static str>,
    /// Folders below `Program Files` / `%LOCALAPPDATA%\Programs` (Windows).
    windows_folders: &'static [&'static str],
    /// Absolute locations on Linux/macOS besides `PATH` (app bundles, flatpak exports).
    unix_paths: &'static [&'static str],
}

/// The registered tools, in the order they are offered (and auto detected).
pub const REGISTERED_TOOLS: &[DiffMergeTool] = &[
    DiffMergeTool {
        name: "winmerge",
        display_name: "WinMerge",
        exe_windows: "WinMergeU.exe",
        exe_unix: None,
        diff_command: Some(r#"-e -u "$LOCAL" "$REMOTE""#),
        merge_command: Some(r#"-e -u  -wl -wr -fm -dl "Mine: $LOCAL" -dm "Merged: $BASE" -dr "Theirs: $REMOTE" "$LOCAL" "$BASE" "$REMOTE" -o "$MERGED""#),
        windows_folders: &[r"WinMerge\"],
        unix_paths: &[],
    },
    DiffMergeTool {
        name: "meld",
        display_name: "Meld",
        exe_windows: "Meld.exe",
        exe_unix: Some("meld"),
        diff_command: Some(""),
        merge_command: Some(r#""$LOCAL" "$BASE" "$REMOTE" --output "$MERGED""#),
        windows_folders: &[r"Meld\", r"Meld (x86)\"],
        unix_paths: &[
            "/var/lib/flatpak/exports/bin/org.gnome.meld",
            "~/.local/share/flatpak/exports/bin/org.gnome.meld",
            "/Applications/Meld.app/Contents/MacOS/Meld",
        ],
    },
    DiffMergeTool {
        name: "kdiff3",
        display_name: "KDiff3",
        exe_windows: "kdiff3.exe",
        exe_unix: Some("kdiff3"),
        diff_command: Some(""),
        merge_command: Some(r#""$BASE" "$LOCAL" "$REMOTE" -o "$MERGED""#),
        windows_folders: &["KDiff3"],
        unix_paths: &["/var/lib/flatpak/exports/bin/org.kde.kdiff3", "/Applications/kdiff3.app/Contents/MacOS/kdiff3"],
    },
    DiffMergeTool {
        name: "bc",
        display_name: "Beyond Compare",
        exe_windows: "bcomp.exe",
        exe_unix: Some("bcompare"),
        diff_command: Some(""),
        merge_command: Some(""),
        windows_folders: &[r"Beyond Compare 5\", r"Beyond Compare 4 (x86)\", r"Beyond Compare 4\"],
        unix_paths: &["/Applications/Beyond Compare.app/Contents/MacOS/bcomp"],
    },
    DiffMergeTool {
        name: "bc3",
        display_name: "Beyond Compare 3",
        exe_windows: "bcomp.exe",
        exe_unix: None,
        diff_command: Some(""),
        merge_command: Some(""),
        windows_folders: &[r"Beyond Compare 3 (x86)\", r"Beyond Compare 3\"],
        unix_paths: &[],
    },
    DiffMergeTool {
        name: "p4merge",
        display_name: "P4Merge",
        exe_windows: "p4merge.exe",
        exe_unix: Some("p4merge"),
        diff_command: Some(""),
        merge_command: Some(r#""$BASE" "$LOCAL" "$REMOTE" "$MERGED""#),
        windows_folders: &[r"Perforce\"],
        unix_paths: &["/Applications/p4merge.app/Contents/MacOS/p4merge"],
    },
    DiffMergeTool {
        name: "vscode",
        display_name: "Visual Studio Code",
        exe_windows: "Code.exe",
        exe_unix: Some("code"),
        diff_command: Some(r#"--new-window --wait --diff "$LOCAL" "$REMOTE""#),
        merge_command: Some(r#"--new-window --wait --merge "$REMOTE" "$LOCAL" "$BASE" "$MERGED""#),
        windows_folders: &[r"Microsoft VS Code\"],
        unix_paths: &["/snap/bin/code", "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code"],
    },
    DiffMergeTool {
        name: "tortoisemerge",
        display_name: "TortoiseGitMerge",
        exe_windows: "TortoiseGitMerge.exe",
        exe_unix: None,
        diff_command: None,
        merge_command: Some(r#"-base:"$BASE" -mine:"$LOCAL" -theirs:"$REMOTE" -merged:"$MERGED""#),
        windows_folders: &[r"TortoiseGit\bin\"],
        unix_paths: &[],
    },
    DiffMergeTool {
        name: "tortoisediff",
        display_name: "TortoiseGitMerge (diff)",
        exe_windows: "TortoiseGitMerge.exe",
        exe_unix: None,
        diff_command: Some(""),
        merge_command: Some(r#"-base:"$BASE" -mine:"$LOCAL" -theirs:"$REMOTE" -merged:"$MERGED""#),
        windows_folders: &[r"TortoiseGit\bin\"],
        unix_paths: &[],
    },
    DiffMergeTool {
        name: "TortoiseGitIDiff",
        display_name: "TortoiseGitIDiff (images)",
        exe_windows: "TortoiseGitIDiff.exe",
        exe_unix: None,
        diff_command: Some(r#"/left:"$LOCAL" /right:"$REMOTE" /fit /overlay"#),
        merge_command: None,
        windows_folders: &[r"TortoiseGit\bin\"],
        unix_paths: &[],
    },
    DiffMergeTool {
        name: "smerge",
        display_name: "Sublime Merge",
        exe_windows: "smerge.exe",
        exe_unix: Some("smerge"),
        diff_command: Some(r#"mergetool "$LOCAL" "$REMOTE" -o="$MERGED""#),
        merge_command: Some(r#"mergetool "$BASE" "$LOCAL" "$REMOTE" -o="$MERGED""#),
        windows_folders: &[r"Sublime Merge\"],
        unix_paths: &["/opt/sublime_merge/sublime_merge"],
    },
    DiffMergeTool {
        name: "diffmerge",
        display_name: "DiffMerge",
        exe_windows: "sgdm.exe",
        exe_unix: Some("diffmerge"),
        diff_command: Some(""),
        merge_command: Some(r#"-merge -result="$MERGED" "$LOCAL" "$BASE" "$REMOTE""#),
        windows_folders: &[r"SourceGear\Common\DiffMerge\", r"SourceGear\DiffMerge\"],
        unix_paths: &[],
    },
    DiffMergeTool {
        name: "araxis",
        display_name: "Araxis Merge",
        exe_windows: "Compare.exe",
        // not "compare" on Linux: that is usually ImageMagick
        exe_unix: None,
        diff_command: Some(""),
        merge_command: Some(r#"/merge /wait /a2 /3 "$LOCAL" "$BASE" "$REMOTE" "$MERGED""#),
        windows_folders: &[r"Araxis\", r"Araxis\Araxis Merge\"],
        unix_paths: &[],
    },
    DiffMergeTool {
        name: "semanticmerge",
        display_name: "SemanticMerge",
        exe_windows: "semanticmergetool.exe",
        exe_unix: Some("semanticmergetool"),
        diff_command: Some(r#"-s "$LOCAL" -d "$REMOTE""#),
        merge_command: Some(r#"-s "$REMOTE" -d "$LOCAL" -b "$BASE" -r "$MERGED""#),
        windows_folders: &[r"semanticmerge\", r"PlasticSCM4\semanticmerge\"],
        unix_paths: &[],
    },
    DiffMergeTool {
        name: "vsdiffmerge",
        display_name: "Visual Studio (vsdiffmerge)",
        exe_windows: "vsdiffmerge.exe",
        exe_unix: None,
        diff_command: Some(""),
        merge_command: Some(r#"/m "$REMOTE" "$LOCAL" "$BASE" "$MERGED""#),
        windows_folders: &[
            r"Microsoft Visual Studio\2022\Community\Common7\IDE\CommonExtensions\Microsoft\TeamFoundation\Team Explorer\",
            r"Microsoft Visual Studio\2022\Professional\Common7\IDE\CommonExtensions\Microsoft\TeamFoundation\Team Explorer\",
            r"Microsoft Visual Studio\2022\Enterprise\Common7\IDE\CommonExtensions\Microsoft\TeamFoundation\Team Explorer\",
        ],
        unix_paths: &[],
    },
];

impl DiffMergeTool {
    /// The executable file name on this platform (`ExeFileName`).
    pub fn exe_file_name(&self) -> &'static str {
        if cfg!(windows) {
            self.exe_windows
        } else {
            self.exe_unix.unwrap_or(self.exe_windows)
        }
    }

    /// Whether the tool exists on this platform (WinMerge and the Tortoise tools are Windows only).
    pub fn is_available_on_this_platform(&self) -> bool {
        cfg!(windows) || self.exe_unix.is_some()
    }

    pub fn supports(&self, t: ToolType) -> bool {
        match t {
            ToolType::Diff => self.diff_command.is_some(),
            ToolType::Merge => self.merge_command.is_some(),
        }
    }

    /// The arguments of the tool (without the executable).
    pub fn command(&self, t: ToolType) -> &'static str {
        match t {
            ToolType::Diff => match self.diff_command {
                Some("") => DEFAULT_DIFF_COMMAND,
                Some(c) => c,
                None => "",
            },
            ToolType::Merge => match self.merge_command {
                Some("") => DEFAULT_MERGE_COMMAND,
                Some(c) => c,
                None => "",
            },
        }
    }

    /// Looks for the executable in the usual locations (`PathUtil.FindInFolders`), then in `PATH`.
    pub fn find(&self) -> Option<PathBuf> {
        if !self.is_available_on_this_platform() {
            return None;
        }
        let exe = self.exe_file_name();
        if cfg!(windows) {
            if let Some(p) = find_in_program_folders(exe, self.windows_folders, &|k| std::env::var_os(k).map(PathBuf::from)) {
                return Some(p);
            }
        } else {
            for p in self.unix_paths {
                let p = expand_home(p);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
        find_in_path(exe)
    }
}

/// The registered tool named `name` (case insensitive, `RegisteredDiffMergeTools.Get`).
pub fn get(name: &str) -> Option<&'static DiffMergeTool> {
    let name = name.trim();
    REGISTERED_TOOLS.iter().find(|t| t.name.eq_ignore_ascii_case(name))
}

/// The tools of a type that exist on this platform.
pub fn tools_for(t: ToolType) -> impl Iterator<Item = &'static DiffMergeTool> {
    REGISTERED_TOOLS.iter().filter(move |tool| tool.supports(t) && tool.is_available_on_this_platform())
}

fn expand_home(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().map(|h| h.join(rest)).unwrap_or_else(|| PathBuf::from(p)),
        None => PathBuf::from(p),
    }
}

/// Port of `PathUtil.FindInFolders`: relative folders are looked up below
/// `%LOCALAPPDATA%\Programs`, `%ProgramFiles%`, `%ProgramW6432%` and `%ProgramFiles(x86)%`.
pub fn find_in_program_folders(exe: &str, folders: &[&str], env: &dyn Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    for folder in folders {
        let folder = folder.trim_end_matches(['\\', '/']);
        let folder_path = Path::new(folder);
        if folder_path.is_absolute() {
            let p = folder_path.join(exe);
            if p.is_file() {
                return Some(p);
            }
            continue;
        }
        let candidates = [
            env("LOCALAPPDATA").map(|d| d.join("Programs").join(folder)),
            env("ProgramFiles").map(|d| d.join(folder)),
            env("ProgramW6432").map(|d| d.join(folder)),
            env("ProgramFiles(x86)").map(|d| d.join(folder)),
        ];
        for dir in candidates.into_iter().flatten() {
            let p = dir.join(exe);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Looks for an executable in `PATH` (`PathUtil.TryFindFullPath`).
pub fn find_in_path(exe: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let p = dir.join(exe);
        if p.is_file() {
            return Some(p);
        }
        if cfg!(windows) && Path::new(exe).extension().is_none() {
            let p = dir.join(format!("{exe}.exe"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// The installed tools of a type, in order of preference.
pub fn detect(t: ToolType) -> Vec<(&'static DiffMergeTool, PathBuf)> {
    tools_for(t).filter_map(|tool| tool.find().map(|p| (tool, p))).collect()
}

/// Port of `DiffMergeToolConfiguration`: the tool with its executable and full commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolConfiguration {
    pub exe_file_name: String,
    /// The executable, with `/` separators (git config style).
    pub path: String,
    pub diff_command: String,
    pub merge_command: String,
    pub full_diff_command: String,
    pub full_merge_command: String,
}

impl ToolConfiguration {
    pub fn new(exe_file_name: &str, path: &str, diff_command: &str, merge_command: &str) -> Self {
        let path = path.replace('\\', "/");
        let full = |cmd: &str| if cmd.trim().is_empty() { String::new() } else { format!("\"{path}\" {cmd}") };
        ToolConfiguration {
            exe_file_name: exe_file_name.to_string(),
            full_diff_command: full(diff_command),
            full_merge_command: full(merge_command),
            path,
            diff_command: diff_command.to_string(),
            merge_command: merge_command.to_string(),
        }
    }

    pub fn full_command(&self, t: ToolType) -> &str {
        match t {
            ToolType::Diff => &self.full_diff_command,
            ToolType::Merge => &self.full_merge_command,
        }
    }
}

/// Port of `LoadDiffMergeToolConfig`: the configuration of `tool_name`, with the executable
/// from `user_supplied_path`, else the configured path (`configured_path`), else found by
/// `find`, else the bare executable name.
pub fn load_tool_config(
    tool_name: &str,
    user_supplied_path: Option<&str>,
    configured_path: Option<&str>,
    find: &dyn Fn(&DiffMergeTool) -> Option<PathBuf>,
) -> Option<ToolConfiguration> {
    let tool_name = tool_name.trim();
    if tool_name.is_empty() {
        return None;
    }
    let user = user_supplied_path.map(str::trim).filter(|p| !p.is_empty());
    let Some(tool) = get(tool_name) else {
        let exe = format!("{tool_name}.exe");
        let path = user.map(str::to_string).or_else(|| find_in_path(&exe).map(|p| p.display().to_string())).unwrap_or_default();
        return Some(ToolConfiguration::new(&exe, &path, "", ""));
    };
    let path = user
        .map(str::to_string)
        .or_else(|| configured_path.map(|p| p.trim().trim_matches('"').to_string()).filter(|p| !p.is_empty()))
        .or_else(|| find(tool).map(|p| p.display().to_string()))
        .unwrap_or_else(|| tool.exe_file_name().to_string());
    Some(ToolConfiguration::new(tool.exe_file_name(), &path, tool.command(ToolType::Diff), tool.command(ToolType::Merge)))
}

/// Reads and writes the tool settings in the global git config
/// (port of `DiffMergeToolConfigurationManager`).
pub struct ToolConfigStore {
    git: Executable,
}

impl Default for ToolConfigStore {
    fn default() -> Self {
        ToolConfigStore { git: Executable::git(std::env::temp_dir()) }
    }
}

impl ToolConfigStore {
    /// A store for the config seen from `dir` (global and system config, plus the repository's).
    pub fn new(git: Executable) -> Self {
        ToolConfigStore { git }
    }

    pub fn get(&self, key: &str) -> String {
        self.git
            .run(&GitArgs::new("config").arg("--get").arg(key))
            .map(|r| r.stdout_str().trim().to_string())
            .unwrap_or_default()
    }

    fn set_global(&self, key: &str, value: &str) -> crate::exec::GitResult<()> {
        if value.trim().is_empty() {
            // unsetting a missing key fails with exit code 5: not an error here
            let _ = self.git.run(&GitArgs::new("config").arg("--global").arg("--unset").arg(key));
            return Ok(());
        }
        self.git.run_checked(&GitArgs::new("config").arg("--global").arg(key).arg(value))?;
        Ok(())
    }

    /// The configured tool name (`ConfiguredDiffTool` / `ConfiguredMergeTool`), falling back to
    /// the non-gui key.
    pub fn configured_tool(&self, t: ToolType) -> String {
        let (key, _) = t.keys();
        let name = self.get(key);
        if !name.is_empty() {
            return name;
        }
        self.get(match t {
            ToolType::Diff => DIFF_TOOL_NO_GUI_KEY,
            ToolType::Merge => MERGE_TOOL_NO_GUI_KEY,
        })
    }

    /// `<prefix>.<tool>.<suffix>` (`GetToolSetting`).
    pub fn tool_setting(&self, tool: &str, t: ToolType, suffix: &str) -> String {
        if tool.trim().is_empty() {
            return String::new();
        }
        self.get(&setting_key(tool, t, suffix))
    }

    /// Port of `ConfigureDiffMergeTool`.
    pub fn configure(&self, tool: &str, t: ToolType, path: &str, command: &str) -> crate::exec::GitResult<()> {
        let tool = tool.trim();
        if tool.is_empty() {
            return Ok(());
        }
        let (key, _) = t.keys();
        self.set_global(key, tool)?;
        self.set_global(&setting_key(tool, t, "path"), &path.trim().replace('\\', "/"))?;
        self.set_global(&setting_key(tool, t, "cmd"), &command.trim().replace('\\', "/"))?;
        Ok(())
    }

    /// Port of `UnsetCurrentTool`.
    pub fn unset(&self, t: ToolType) -> crate::exec::GitResult<()> {
        let (key, _) = t.keys();
        self.set_global(key, "")
    }
}

/// `<difftool|mergetool>.<tool>.<suffix>`.
pub fn setting_key(tool: &str, t: ToolType, suffix: &str) -> String {
    let (_, prefix) = t.keys();
    format!("{prefix}.{}.{suffix}", tool.trim())
}

/// The executable of a tool command line: the first (possibly quoted) word.
pub fn command_executable(command: &str) -> String {
    let c = command.trim();
    if let Some(rest) = c.strip_prefix('"') {
        return rest.split('"').next().unwrap_or_default().to_string();
    }
    c.split_whitespace().next().unwrap_or_default().to_string()
}

/// Whether an executable (absolute path, or a name looked up in `PATH`) exists.
pub fn executable_exists(exe: &str) -> bool {
    let exe = exe.trim().trim_matches('"');
    if exe.is_empty() {
        return false;
    }
    let p = expand_home(exe);
    if p.is_absolute() || exe.contains(['/', '\\']) {
        return p.is_file();
    }
    find_in_path(exe).is_some()
}

/// How the app starts a tool for `git difftool` / `git mergetool`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolLaunch {
    /// The configured tool works: `--gui`.
    Configured,
    /// Use another (detected) tool: `-c <prefix>.<name>.cmd=<cmd> … --tool=<name>`.
    Override { name: String, command: String, reason: String },
}

/// Decides which tool to start (`git difftool --gui` with the configured tool, or a detected
/// one when nothing usable is configured, e.g. a WinMerge configuration on Linux). `None`:
/// no tool is configured nor installed.
pub fn resolve_launch(store: &ToolConfigStore, t: ToolType) -> Option<ToolLaunch> {
    let configured = store.configured_tool(t);
    if !configured.is_empty() && configured_tool_is_usable(store, &configured, t) {
        return Some(ToolLaunch::Configured);
    }
    let (tool, path) = detect(t).into_iter().next()?;
    let config = ToolConfiguration::new(tool.exe_file_name(), &path.display().to_string(), tool.command(ToolType::Diff), tool.command(ToolType::Merge));
    let reason = if configured.is_empty() {
        format!("No {} tool is configured: using {} ({}).", t.label(), tool.display_name, config.path)
    } else {
        format!("The configured {} tool '{configured}' is not installed: using {} ({}).", t.label(), tool.display_name, config.path)
    };
    Some(ToolLaunch::Override { name: tool.name.to_string(), command: config.full_command(t).to_string(), reason })
}

fn configured_tool_is_usable(store: &ToolConfigStore, name: &str, t: ToolType) -> bool {
    let cmd = store.tool_setting(name, t, "cmd");
    if !cmd.is_empty() {
        return executable_exists(&command_executable(&cmd));
    }
    let path = store.tool_setting(name, t, "path");
    if !path.is_empty() {
        return executable_exists(&path);
    }
    match get(name) {
        Some(tool) => tool.find().is_some(),
        // a tool git knows (or one we cannot check): let git try
        None => true,
    }
}

/// Adds the tool options to a `difftool` / `mergetool` command (`--gui`, or the override).
pub fn launch_args(launch: &ToolLaunch, t: ToolType, rest: &GitArgs) -> GitArgs {
    let command = match t {
        ToolType::Diff => "difftool",
        ToolType::Merge => "mergetool",
    };
    let mut a = match launch {
        ToolLaunch::Configured => GitArgs::new(command).arg("--gui"),
        ToolLaunch::Override { name, command: cmd, .. } => {
            let key = setting_key(name, t, "cmd");
            GitArgs::with_config(&[(key.as_str(), cmd.as_str())], command).arg(format!("--tool={name}"))
        }
    };
    a.add("--no-prompt");
    // `rest` is the command line without the git command itself
    for arg in rest.as_slice().iter().skip(1) {
        a.add(arg.clone());
    }
    a
}

/// Hint shown when no tool is installed.
pub fn install_hint() -> &'static str {
    if cfg!(windows) {
        "Install WinMerge (https://winmerge.org) or Meld (https://meld.app), then select it in Settings > Git > Diff and merge tools."
    } else if cfg!(target_os = "macos") {
        "Install Meld (brew install --cask meld), P4Merge or Beyond Compare, then select it in Settings > Git > Diff and merge tools."
    } else {
        "Install Meld (Ubuntu/Debian: sudo apt install meld, Fedora: sudo dnf install meld), then select it in Settings > Git > Diff and merge tools."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // From DiffMergeToolConfigurationManagerTests / RegisteredDiffMergeToolsTests

    #[test]
    fn keys_match_git_extensions() {
        assert_eq!(DIFF_TOOL_KEY, "diff.guitool");
        assert_eq!(MERGE_TOOL_KEY, "merge.guitool");
        assert_eq!(MERGE_TOOL_NO_GUI_KEY, "merge.tool");
        assert_eq!(setting_key("bla", ToolType::Diff, "path"), "difftool.bla.path");
        assert_eq!(setting_key("bla", ToolType::Merge, "cmd"), "mergetool.bla.cmd");
    }

    #[test]
    fn load_tool_config_requires_a_name() {
        for name in ["", "\t"] {
            assert!(load_tool_config(name, Some(""), None, &|_| None).is_none());
        }
    }

    #[test]
    fn load_tool_config_unregistered_with_user_path() {
        let c = load_tool_config("bla", Some(r"c:\some\path\to the tool\bla.exe"), None, &|_| None).unwrap();
        assert_eq!(c.exe_file_name, "bla.exe");
        assert_eq!(c.path, "c:/some/path/to the tool/bla.exe");
        assert!(c.diff_command.is_empty());
        assert!(c.merge_command.is_empty());
    }

    #[test]
    fn load_tool_config_unregistered_without_path() {
        for p in [None, Some(""), Some("\t")] {
            let c = load_tool_config("bla", p, None, &|_| None).unwrap();
            assert_eq!(c.exe_file_name, "bla.exe");
            assert_eq!(c.path, "");
            assert!(c.full_diff_command.is_empty());
        }
    }

    #[test]
    fn load_tool_config_registered() {
        let winmerge = get("WinMerge").unwrap();
        let path = r"C:\Program Files\WinMerge\WinMergeU.exe";
        let c = load_tool_config("winmerge", Some(path), None, &|_| None).unwrap();
        assert_eq!(c.path, "C:/Program Files/WinMerge/WinMergeU.exe");
        assert_eq!(c.diff_command, winmerge.command(ToolType::Diff));
        assert_eq!(c.full_diff_command, r#""C:/Program Files/WinMerge/WinMergeU.exe" -e -u "$LOCAL" "$REMOTE""#);
        assert_eq!(
            c.full_merge_command,
            r#""C:/Program Files/WinMerge/WinMergeU.exe" -e -u  -wl -wr -fm -dl "Mine: $LOCAL" -dm "Merged: $BASE" -dr "Theirs: $REMOTE" "$LOCAL" "$BASE" "$REMOTE" -o "$MERGED""#
        );
        // configured path, then found path, then the bare executable name
        let c = load_tool_config("meld", None, Some("\"/opt/meld/bin/meld\""), &|_| None).unwrap();
        assert_eq!(c.path, "/opt/meld/bin/meld");
        let c = load_tool_config("meld", None, None, &|_| Some(PathBuf::from("/usr/bin/meld"))).unwrap();
        assert_eq!(c.full_merge_command, r#""/usr/bin/meld" "$LOCAL" "$BASE" "$REMOTE" --output "$MERGED""#);
        let c = load_tool_config("kdiff3", None, None, &|_| None).unwrap();
        assert_eq!(c.path, get("kdiff3").unwrap().exe_file_name());
    }

    #[test]
    fn registered_tools() {
        assert!(get("meld").is_some_and(|t| t.supports(ToolType::Diff) && t.supports(ToolType::Merge)));
        assert!(get("tortoisemerge").is_some_and(|t| !t.supports(ToolType::Diff) && t.supports(ToolType::Merge)));
        assert!(get("TortoiseGitIDiff").is_some_and(|t| t.supports(ToolType::Diff) && !t.supports(ToolType::Merge)));
        assert!(get("nope").is_none());
        // WinMerge only exists on Windows; Meld everywhere
        assert_eq!(tools_for(ToolType::Diff).any(|t| t.name == "winmerge"), cfg!(windows));
        assert!(tools_for(ToolType::Merge).any(|t| t.name == "meld"));
        // unique names
        let mut names: Vec<_> = REGISTERED_TOOLS.iter().map(|t| t.name.to_lowercase()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), REGISTERED_TOOLS.len());
    }

    #[test]
    fn program_folders_search() {
        let dir = tempfile::tempdir().unwrap();
        let pf = dir.path().join("pf");
        let local = dir.path().join("local");
        std::fs::create_dir_all(pf.join("WinMerge")).unwrap();
        std::fs::write(pf.join("WinMerge").join("WinMergeU.exe"), "").unwrap();
        std::fs::create_dir_all(local.join("Programs").join("Microsoft VS Code")).unwrap();
        std::fs::write(local.join("Programs").join("Microsoft VS Code").join("Code.exe"), "").unwrap();
        let env = |k: &str| match k {
            "ProgramFiles" => Some(pf.clone()),
            "LOCALAPPDATA" => Some(local.clone()),
            _ => None,
        };
        assert_eq!(find_in_program_folders("WinMergeU.exe", &[r"WinMerge\"], &env), Some(pf.join("WinMerge").join("WinMergeU.exe")));
        assert_eq!(
            find_in_program_folders("Code.exe", &[r"Microsoft VS Code\"], &env),
            Some(local.join("Programs").join("Microsoft VS Code").join("Code.exe"))
        );
        assert_eq!(find_in_program_folders("Meld.exe", &[r"Meld\"], &env), None);
    }

    #[test]
    fn command_executables() {
        assert_eq!(command_executable(r#""C:/Program Files/WinMerge/WinMergeU.exe" -e -u "$LOCAL" "$REMOTE""#), "C:/Program Files/WinMerge/WinMergeU.exe");
        assert_eq!(command_executable(r#"meld "$LOCAL" "$REMOTE""#), "meld");
        assert_eq!(command_executable("  "), "");
        assert!(!executable_exists(""));
        assert!(!executable_exists("/no/such/tool"));
        assert!(executable_exists(if cfg!(windows) { "cmd" } else { "sh" }));
    }

    #[test]
    fn launch_arguments() {
        let rest = GitArgs::new("difftool").arg("--cached").arg("--").arg("a.txt");
        assert_eq!(launch_args(&ToolLaunch::Configured, ToolType::Diff, &rest).as_slice(), ["difftool", "--gui", "--no-prompt", "--cached", "--", "a.txt"]);
        let o = ToolLaunch::Override { name: "meld".into(), command: r#""/usr/bin/meld" "$LOCAL" "$REMOTE""#.into(), reason: String::new() };
        assert_eq!(
            launch_args(&o, ToolType::Diff, &rest).as_slice(),
            ["-c", r#"difftool.meld.cmd="/usr/bin/meld" "$LOCAL" "$REMOTE""#, "difftool", "--tool=meld", "--no-prompt", "--cached", "--", "a.txt"]
        );
    }

    #[test]
    fn configure_and_resolve_in_isolated_config() {
        // a private global config: HOME / XDG_CONFIG_HOME of the git process
        let home = tempfile::tempdir().unwrap();
        let mut exe = Executable::git(home.path());
        exe.env.push(("HOME".into(), home.path().display().to_string()));
        exe.env.push(("XDG_CONFIG_HOME".into(), home.path().join(".config").display().to_string()));
        exe.env.push(("GIT_CONFIG_NOSYSTEM".into(), "1".into()));
        let store = ToolConfigStore::new(exe);
        assert_eq!(store.configured_tool(ToolType::Diff), "");

        let tool_cmd = if cfg!(windows) { "cmd" } else { "sh" };
        store.configure("mytool", ToolType::Diff, r"C:\tools\x.exe", &format!("{tool_cmd} \"$LOCAL\" \"$REMOTE\"")).unwrap();
        assert_eq!(store.configured_tool(ToolType::Diff), "mytool");
        assert_eq!(store.tool_setting("mytool", ToolType::Diff, "path"), "C:/tools/x.exe");
        assert_eq!(resolve_launch(&store, ToolType::Diff), Some(ToolLaunch::Configured));

        // a configured tool that does not exist is replaced by a detected one (or none)
        store.configure("mytool", ToolType::Diff, "", "/no/such/tool \"$LOCAL\"").unwrap();
        match resolve_launch(&store, ToolType::Diff) {
            None => assert!(detect(ToolType::Diff).is_empty()),
            Some(ToolLaunch::Override { reason, .. }) => assert!(reason.contains("'mytool' is not installed")),
            Some(ToolLaunch::Configured) => panic!("the missing tool must not be used"),
        }

        store.unset(ToolType::Diff).unwrap();
        assert_eq!(store.get(DIFF_TOOL_KEY), "");
    }
}
