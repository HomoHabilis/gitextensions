//! Windows Explorer context menu (port of `GitExtensionsShellEx` and `ShellExtensionManager`).
//!
//! The C++ shell extension of Git Extensions is a COM context menu handler. This port
//! registers static, cascading Explorer verbs instead (`HKCU\Software\Classes\…\shell`):
//! nothing is loaded into Explorer, so it cannot slow it down or crash it, and no
//! administrator rights are needed. The commands run `gitext -C <path> <command>`; a
//! command run outside of a repository offers to create one there.
//!
//! There are three menus, like the original: one for files, one for folders and one for the
//! background of a folder window. Items can be hidden in *Settings → General*.

// The registry code is only used on Windows (the menu building is tested everywhere).
#![cfg_attr(not(windows), allow(dead_code))]

/// Where an item is shown.
const FILE: u8 = 1;
const FOLDER: u8 = 2;
const BACKGROUND: u8 = 4;
const DIR: u8 = FOLDER | BACKGROUND;

/// Icon resource ids of `gitext.exe` (see `res/gitext.rc`, same ids as the C++ extension).
pub mod icon {
    pub const APP: u16 = 1;
    pub const ADDED: u16 = 202;
    pub const BROWSE: u16 = 203;
    pub const BRANCH_CREATE: u16 = 204;
    pub const BRANCH_CHECKOUT: u16 = 205;
    pub const REVISION_CHECKOUT: u16 = 206;
    pub const CLONE: u16 = 209;
    pub const COMMIT: u16 = 210;
    pub const FILE_HISTORY: u16 = 211;
    pub const PULL: u16 = 212;
    pub const PUSH: u16 = 213;
    pub const RESET_FILE: u16 = 214;
    pub const SETTINGS: u16 = 215;
    pub const VIEW_CHANGES: u16 = 216;
    pub const STASH: u16 = 217;
    pub const CREATE_REPOSITORY: u16 = 218;
    pub const PATCH_APPLY: u16 = 219;
}

/// A context menu item.
#[derive(Debug, Clone, Copy)]
pub struct MenuItem {
    /// The `gitext` command, also the id used to hide the item.
    pub command: &'static str,
    pub text: &'static str,
    pub icon: u16,
    shown_on: u8,
    separator_before: bool,
}

const fn item(command: &'static str, text: &'static str, icon: u16, shown_on: u8, separator_before: bool) -> MenuItem {
    MenuItem { command, text, icon, shown_on, separator_before }
}

/// All items, in menu order.
pub const ITEMS: &[MenuItem] = &[
    item("browse", "Open repository", icon::BROWSE, FILE | DIR, false),
    item("commit", "Commit...", icon::COMMIT, FILE | DIR, false),
    item("pull", "Pull...", icon::PULL, DIR, false),
    item("push", "Push...", icon::PUSH, DIR, false),
    item("stash", "View stash", icon::STASH, DIR, false),
    item("viewdiff", "View changes", icon::VIEW_CHANGES, DIR, false),
    item("checkoutbranch", "Checkout branch...", icon::BRANCH_CHECKOUT, DIR, true),
    item("checkoutrevision", "Checkout revision...", icon::REVISION_CHECKOUT, DIR, false),
    item("branch", "Create branch...", icon::BRANCH_CREATE, DIR, false),
    item("difftool", "Open with difftool", icon::VIEW_CHANGES, FILE, true),
    item("filehistory", "File history", icon::FILE_HISTORY, FILE | FOLDER, false),
    item("blame", "Blame", icon::FILE_HISTORY, FILE, false),
    item("reset", "Reset file changes...", icon::RESET_FILE, FILE | FOLDER, false),
    item("addfiles", "Add files...", icon::ADDED, FILE | FOLDER, false),
    item("applypatch", "Apply patch...", icon::PATCH_APPLY, FILE, false),
    item("clone", "Clone...", icon::CLONE, DIR, true),
    item("init", "Create new repository...", icon::CREATE_REPOSITORY, DIR, false),
    item("settings", "Settings", icon::SETTINGS, FILE | DIR, true),
];

/// Name of the cascading menu key.
const MENU_KEY: &str = "GitExtensions";

/// The registry classes the menus are added to, the items shown there and the placeholder
/// for the selected path.
const MENUS: [(&str, u8, &str); 3] = [("*", FILE, "%1"), ("Directory", FOLDER, "%1"), ("Directory\\Background", BACKGROUND, "%V")];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegValue {
    Str(String),
    Dword(u32),
}

/// A registry key (relative to `Software\Classes`) and its values (`""` is the default value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegKey {
    pub path: String,
    pub values: Vec<(&'static str, RegValue)>,
}

/// `ECF_SEPARATORBEFORE` of the `CommandFlags` value.
const ECF_SEPARATORBEFORE: u32 = 0x40;

/// The keys of the menus, for `exe` and without the `hidden` commands.
pub fn registry_keys(exe: &str, hidden: &[String]) -> Vec<RegKey> {
    let icon = |id: u16| RegValue::Str(format!("{exe},-{id}"));
    let mut keys = Vec::new();
    for (class, on, placeholder) in MENUS {
        let items: Vec<&MenuItem> = ITEMS.iter().filter(|i| i.shown_on & on != 0 && !hidden.iter().any(|h| h == i.command)).collect();
        if items.is_empty() {
            continue;
        }
        let menu = format!("{class}\\shell\\{MENU_KEY}");
        keys.push(RegKey {
            path: menu.clone(),
            values: vec![
                ("MUIVerb", RegValue::Str("Git Extensions".into())),
                ("Icon", icon(icon::APP)),
                // an empty SubCommands value: the items are the "shell" subkeys
                ("SubCommands", RegValue::Str(String::new())),
                ("MultiSelectModel", RegValue::Str("Single".into())),
            ],
        });
        let mut previous_section_shown = false;
        for (n, item) in items.iter().enumerate() {
            let key = format!("{menu}\\shell\\{:02}{}", n + 1, item.command);
            let mut values = vec![("MUIVerb", RegValue::Str(item.text.into())), ("Icon", icon(item.icon))];
            if item.separator_before && previous_section_shown {
                values.push(("CommandFlags", RegValue::Dword(ECF_SEPARATORBEFORE)));
            }
            previous_section_shown = true;
            keys.push(RegKey { path: key.clone(), values });
            keys.push(RegKey { path: format!("{key}\\command"), values: vec![("", RegValue::Str(command_line(exe, placeholder, item.command)))] });
        }
    }
    keys
}

/// `"gitext.exe" -C "%1" <command>`.
fn command_line(exe: &str, placeholder: &str, command: &str) -> String {
    format!("\"{exe}\" -C \"{placeholder}\" {command}")
}

/// The keys that [`registry_keys`] creates and [`unregister`] deletes (with their subkeys).
pub fn menu_keys() -> Vec<String> {
    MENUS.iter().map(|(class, ..)| format!("{class}\\shell\\{MENU_KEY}")).collect()
}

/// Cleans a `-C` path from Explorer: `"%V"` of a drive root is `"C:\"`, which the command
/// line parser reads as `C:"` (the backslash escapes the quote).
pub fn clean_shell_path(path: &str) -> String {
    let p = path.trim().trim_end_matches('"');
    if p.len() == 2 && p.ends_with(':') {
        format!("{p}\\")
    } else {
        p.to_string()
    }
}

#[cfg(windows)]
mod platform {
    use winreg::enums::HKEY_CURRENT_USER;

    use super::{RegKey, RegValue};

    const CLASSES: &str = "Software\\Classes";

    fn classes() -> std::io::Result<winreg::RegKey> {
        winreg::RegKey::predef(HKEY_CURRENT_USER).create_subkey(CLASSES).map(|(k, _)| k)
    }

    pub fn write(keys: &[RegKey]) -> std::io::Result<()> {
        let root = classes()?;
        for k in keys {
            let (key, _) = root.create_subkey(&k.path)?;
            for (name, value) in &k.values {
                match value {
                    RegValue::Str(s) => key.set_value(name, s)?,
                    RegValue::Dword(d) => key.set_value(name, d)?,
                }
            }
        }
        Ok(())
    }

    pub fn delete(paths: &[String]) -> std::io::Result<()> {
        let root = classes()?;
        for p in paths {
            match root.delete_subkey_all(p) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        Ok(())
    }

    pub fn exists(path: &str) -> bool {
        classes().and_then(|r| r.open_subkey(path)).is_ok()
    }
}

/// Whether the menus are registered (for the current user).
pub fn is_registered() -> bool {
    #[cfg(windows)]
    {
        menu_keys().iter().any(|k| platform::exists(k))
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Adds the menus for the running executable (replacing a previous registration).
pub fn register(hidden: &[String]) -> Result<(), String> {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        platform::delete(&menu_keys()).map_err(|e| e.to_string())?;
        platform::write(&registry_keys(&exe.display().to_string(), hidden)).map_err(|e| format!("Could not write the registry: {e}"))
    }
    #[cfg(not(windows))]
    {
        let _ = hidden;
        Err("The Explorer integration is only available on Windows.".into())
    }
}

/// Removes the menus.
pub fn unregister() -> Result<(), String> {
    #[cfg(windows)]
    {
        platform::delete(&menu_keys()).map_err(|e| format!("Could not write the registry: {e}"))
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value<'a>(keys: &'a [RegKey], path: &str, name: &str) -> Option<&'a RegValue> {
        keys.iter().find(|k| k.path == path)?.values.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }

    #[test]
    fn builds_cascading_menus() {
        let exe = r"C:\Program Files\GitExtensions\gitext.exe";
        let keys = registry_keys(exe, &[]);
        assert_eq!(value(&keys, r"*\shell\GitExtensions", "SubCommands"), Some(&RegValue::Str(String::new())));
        assert_eq!(value(&keys, r"*\shell\GitExtensions", "Icon"), Some(&RegValue::Str(format!("{exe},-1"))));
        assert_eq!(
            value(&keys, r"*\shell\GitExtensions\shell\01browse\command", ""),
            Some(&RegValue::Str(format!("\"{exe}\" -C \"%1\" browse")))
        );
        assert_eq!(
            value(&keys, r"Directory\Background\shell\GitExtensions\shell\01browse\command", ""),
            Some(&RegValue::Str(format!("\"{exe}\" -C \"%V\" browse")))
        );
        // file items only in the file menu, repository commands not in it
        assert!(keys.iter().any(|k| k.path.starts_with(r"*\shell") && k.path.ends_with("difftool")));
        assert!(!keys.iter().any(|k| k.path.starts_with(r"Directory\") && k.path.ends_with("difftool")));
        assert!(!keys.iter().any(|k| k.path.starts_with(r"*\shell") && k.path.ends_with("push")));
        // background menu: no file history of the folder itself
        assert!(!keys.iter().any(|k| k.path.starts_with(r"Directory\Background") && k.path.ends_with("filehistory")));
    }

    #[test]
    fn items_are_ordered_and_separated() {
        let keys = registry_keys("gitext.exe", &[]);
        let items: Vec<&RegKey> = keys.iter().filter(|k| k.path.starts_with(r"Directory\shell\GitExtensions\shell\") && !k.path.ends_with("command")).collect();
        let names: Vec<&str> = items.iter().map(|k| k.path.rsplit('\\').next().unwrap()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "Explorer sorts the subkeys by name");
        let first_with_separator = items.iter().position(|k| k.values.iter().any(|(n, _)| *n == "CommandFlags"));
        assert_eq!(names[first_with_separator.unwrap()], "07checkoutbranch");
        assert!(!items[0].values.iter().any(|(n, _)| *n == "CommandFlags"));
    }

    #[test]
    fn hides_items() {
        let hidden = vec!["browse".to_string(), "settings".to_string()];
        let keys = registry_keys("gitext.exe", &hidden);
        assert!(!keys.iter().any(|k| k.path.ends_with("browse") || k.path.ends_with("settings")));
        assert!(keys.iter().any(|k| k.path == r"*\shell\GitExtensions\shell\01commit"));
        // the first shown item never has a separator
        let first = keys.iter().find(|k| k.path == r"*\shell\GitExtensions\shell\01commit").unwrap();
        assert!(!first.values.iter().any(|(n, _)| *n == "CommandFlags"));
        // no menu at all when every item is hidden
        let all: Vec<String> = ITEMS.iter().map(|i| i.command.to_string()).collect();
        assert!(registry_keys("gitext.exe", &all).is_empty());
    }

    #[test]
    fn cleans_explorer_paths() {
        assert_eq!(clean_shell_path("C:\""), "C:\\");
        assert_eq!(clean_shell_path(r"C:\src\repo"), r"C:\src\repo");
        assert_eq!(clean_shell_path(r"\\wsl$\Ubuntu\home"), r"\\wsl$\Ubuntu\home");
    }
}
