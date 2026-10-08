//! WSL support (port of the WSL parts of `PathUtil` and `GitExecutor`).
//!
//! On Windows, repositories under `\\wsl$\<distro>\` or `\\wsl.localhost\<distro>\` are run with
//! the git of the distro: `wsl -d <distro> --cd <dir> --exec git …`. Windows git on these paths is
//! slow and usually refuses to work ("detected dubious ownership"). Paths passed to git are
//! converted to the distro's paths, and paths printed by git are converted back.

const WSL_PREFIX: &str = r"\\wsl$\";
const WSL_LOCALHOST_PREFIX: &str = r"\\wsl.localhost\";

fn starts_with_ignore_case(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s.is_char_boundary(prefix.len()) && s[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// The length of the `\\wsl$\` or `\\wsl.localhost\` prefix (also with `/` separators, as
/// produced by [`crate::args::to_posix_path`]), if any.
fn prefix_len(path: &str) -> Option<usize> {
    for prefix in [WSL_PREFIX, WSL_LOCALHOST_PREFIX] {
        if starts_with_ignore_case(path, prefix) || starts_with_ignore_case(path, &prefix.replace('\\', "/")) {
            return Some(prefix.len());
        }
    }
    None
}

/// Whether the path is a WSL path (`\\wsl$\…` or `\\wsl.localhost\…`). Port of `IsWslPath`.
pub fn is_wsl_path(path: &str) -> bool {
    starts_with_ignore_case(path, WSL_PREFIX) || starts_with_ignore_case(path, WSL_LOCALHOST_PREFIX)
}

/// Replaces `\\wsl.localhost\` with `\\wsl$\` and normalizes the case of the prefix.
/// Port of `NormalizeWslPath`.
pub fn normalize_wsl_path(path: &str) -> String {
    if starts_with_ignore_case(path, WSL_LOCALHOST_PREFIX) {
        return format!("{WSL_PREFIX}{}", &path[WSL_LOCALHOST_PREFIX.len()..]);
    }
    if starts_with_ignore_case(path, WSL_PREFIX) {
        return format!("{WSL_PREFIX}{}", &path[WSL_PREFIX.len()..]);
    }
    path.to_string()
}

/// The distro name (like `Ubuntu-22.04`) of a WSL path, empty otherwise. Port of `GetWslDistro`
/// (which also accepts `\\wsl.localhost\`).
pub fn distro(path: &str) -> String {
    if !is_wsl_path(path) {
        return String::new();
    }
    let path = normalize_wsl_path(path);
    let rest = &path[WSL_PREFIX.len()..];
    match rest.find(['\\', '/']) {
        Some(n) if n > 0 => rest[..n].to_string(),
        _ => String::new(),
    }
}

/// Converts a path of the app to the path for the git executable. Port of
/// `GetPathForGitExecution`: with a distro, `\\wsl$\<distro>\x` becomes `/x` and `C:\x` becomes
/// `/mnt/c/x`; otherwise only the separators change.
pub fn path_for_git(path: &str, wsl_distro: &str) -> String {
    let posix = path.replace('\\', "/");
    if path.is_empty() || wsl_distro.is_empty() {
        return posix;
    }
    if let Some(len) = prefix_len(path) {
        let rest = &posix[len..];
        let path_distro = rest.split('/').next().unwrap_or_default();
        if !path_distro.eq_ignore_ascii_case(wsl_distro) {
            return posix;
        }
        return rest[path_distro.len()..].to_string();
    }
    let b = path.as_bytes();
    if b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return format!("/mnt/{}{}", (b[0] as char).to_ascii_lowercase(), &posix[2..]);
    }
    posix
}

/// Converts a path printed by git to a path of the app (Windows path with `\` separators for
/// a distro). Port of `GetWindowsPath`.
pub fn windows_path(path: &str, wsl_distro: &str) -> String {
    if path.is_empty() || wsl_distro.is_empty() || prefix_len(path).is_some() {
        return path.to_string();
    }
    let native = |p: &str| p.replace('/', "\\");
    if path.starts_with("/mnt/") && path.len() > 7 {
        let drive = path[5..6].to_ascii_uppercase();
        return native(&format!("{drive}:{}", &path[6..]));
    }
    if path.starts_with(['\\', '/']) {
        return native(&format!("{WSL_PREFIX}{wsl_distro}{path}"));
    }
    native(&format!("{WSL_PREFIX}{wsl_distro}\\{path}"))
}

/// Converts the absolute paths in a git argument for a distro: the whole argument
/// (`C:/temp/patch`), or the value of an option (`--output=C:/temp/x`, `-Fc:/msg`).
pub fn convert_arg(arg: &str, wsl_distro: &str) -> String {
    let is_absolute = |s: &str| {
        let b = s.as_bytes();
        prefix_len(s).is_some() || (b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'/' || b[2] == b'\\'))
    };
    if is_absolute(arg) {
        return path_for_git(arg, wsl_distro);
    }
    if arg.starts_with("--") {
        if let Some((name, value)) = arg.split_once('=') {
            if is_absolute(value) {
                return format!("{name}={}", path_for_git(value, wsl_distro));
            }
        }
    }
    arg.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test cases from PathUtilTest.cs

    #[test]
    fn normalize() {
        assert_eq!(normalize_wsl_path(""), "");
        assert_eq!(normalize_wsl_path(" "), " ");
        assert_eq!(normalize_wsl_path("c:"), "c:");
        assert_eq!(normalize_wsl_path(r"C:\"), r"C:\");
        assert_eq!(normalize_wsl_path(r"\\wsl$\Ubuntu\home\jack\work\"), r"\\wsl$\Ubuntu\home\jack\work\");
        assert_eq!(normalize_wsl_path(r"\\Wsl.LoCALhosT\Ubuntu\home\jack\work\"), r"\\wsl$\Ubuntu\home\jack\work\");
        assert_eq!(normalize_wsl_path(r"\\wsl.localhost\Ubuntu\home\jack\work\"), r"\\wsl$\Ubuntu\home\jack\work\");
    }

    #[test]
    fn wsl_paths() {
        assert!(is_wsl_path(r"\\Wsl$\Ubuntu\work\..\GitExtensions\"));
        assert!(is_wsl_path(r"\\wsl$\Ubuntu\work\..\GitExtensions\"));
        assert!(!is_wsl_path(r"C:\work\..\GitExtensions\"));
        assert!(!is_wsl_path(r"\\Wsl$/Ubuntu\work\..\GitExtensions\"));
        assert!(is_wsl_path(r"\\Wsl.localhost\GitExtensions\"));
    }

    #[test]
    fn get_distro() {
        assert_eq!(distro(r"\\Wsl$\Ubuntu\work\..\GitExtensions\"), "Ubuntu");
        assert_eq!(distro(r"\\wsl$\Ubuntu/work/../GitExtensions"), "Ubuntu");
        assert_eq!(distro(r"\\wsl$\Ubuntu-20.04\work\..\GitExtensions\"), "Ubuntu-20.04");
        assert_eq!(distro(r"C:\work\..\GitExtensions\"), "");
        assert_eq!(distro(r"\\wsl$/Ubuntu/work/../GitExtensions"), "");
        // the C# version requires \\wsl$\ here; \\wsl.localhost\ is accepted too
        assert_eq!(distro(r"\\wsl.localhost\Ubuntu-22.04\home\jack"), "Ubuntu-22.04");
    }

    #[test]
    fn path_for_git_without_distro() {
        assert_eq!(path_for_git(r"\\wsl$\Ubuntu\work\..\GitExtensions\", ""), "//wsl$/Ubuntu/work/../GitExtensions/");
        assert_eq!(path_for_git(r"C:\work\..\GitExtensions\", ""), "C:/work/../GitExtensions/");
        assert_eq!(path_for_git(r"work\..\GitExtensions\", ""), "work/../GitExtensions/");
        // and back
        assert_eq!(windows_path("C:/work/../GitExtensions/", ""), "C:/work/../GitExtensions/");
    }

    #[test]
    fn path_for_git_wsl() {
        assert_eq!(path_for_git(r"\\Wsl$\Ubuntu\work\..\GitExtensions\", "Ubuntu"), "/work/../GitExtensions/");
        assert_eq!(path_for_git(r"\\wsl$\Ubuntu\work/../GitExtensions", "Ubuntu"), "/work/../GitExtensions");
        assert_eq!(path_for_git(r"\\wsl$\Ubuntu-20.04\work\..\GitExtensions\", "Ubuntu-20.04"), "/work/../GitExtensions/");
        assert_eq!(path_for_git(r"C:\work\..\GitExtensions\", "Ubuntu"), "/mnt/c/work/../GitExtensions/");
        assert_eq!(path_for_git(r"work\..\GitExtensions\", "Ubuntu"), "work/../GitExtensions/");
        assert_eq!(path_for_git(r"\\wsl.localhost\Ubuntu\home\jack", "Ubuntu"), "/home/jack");
        // already converted to posix separators
        assert_eq!(path_for_git("//wsl$/Ubuntu/home/jack/.git/COMMITMESSAGE", "Ubuntu"), "/home/jack/.git/COMMITMESSAGE");
    }

    #[test]
    fn path_for_git_unexpected_usage() {
        assert_eq!(path_for_git(r"\\wsl$\Ubuntu-20.04\work\..\GitExtensions\", "Ubuntu"), "//wsl$/Ubuntu-20.04/work/../GitExtensions/");
    }

    #[test]
    fn windows_path_wsl() {
        assert_eq!(windows_path("/work/../GitExtensions/", "Ubuntu"), r"\\wsl$\Ubuntu\work\..\GitExtensions\");
        assert_eq!(windows_path("/work/../GitExtensions", "Ubuntu"), r"\\wsl$\Ubuntu\work\..\GitExtensions");
        assert_eq!(windows_path("/work/../GitExtensions/", "Ubuntu-20.04"), r"\\wsl$\Ubuntu-20.04\work\..\GitExtensions\");
        assert_eq!(windows_path("/mnt/c/work/../GitExtensions/", "Ubuntu"), r"C:\work\..\GitExtensions\");
        assert_eq!(windows_path("work/../GitExtensions/", "Ubuntu"), r"\\wsl$\Ubuntu\work\..\GitExtensions\");
    }

    #[test]
    fn convert_arguments() {
        assert_eq!(convert_arg("C:/Users/jack/AppData/Local/Temp/x.patch", "Ubuntu"), "/mnt/c/Users/jack/AppData/Local/Temp/x.patch");
        assert_eq!(convert_arg("--output=D:/out.zip", "Ubuntu"), "--output=/mnt/d/out.zip");
        assert_eq!(convert_arg("//wsl$/Ubuntu/home/jack/repo/.git/COMMITMESSAGE", "Ubuntu"), "/home/jack/repo/.git/COMMITMESSAGE");
        assert_eq!(convert_arg("--format=%H", "Ubuntu"), "--format=%H");
        assert_eq!(convert_arg("src/main.rs", "Ubuntu"), "src/main.rs");
        assert_eq!(convert_arg("HEAD:src/a.txt", "Ubuntu"), "HEAD:src/a.txt");
    }
}
