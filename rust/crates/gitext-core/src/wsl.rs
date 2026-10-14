//! WSL support (port of the WSL parts of `PathUtil` and `GitExecutor`).
//!
//! On Windows, repositories under `\\wsl$\<distro>\` or `\\wsl.localhost\<distro>\` are run with
//! the git of the distro: `wsl -d <distro> --cd <dir> --exec git …`. Windows git on these paths is
//! slow and usually refuses to work ("detected dubious ownership"). Paths passed to git are
//! converted to the distro's paths, and paths printed by git are converted back.

const WSL_PREFIX: &str = r"\\wsl$\";
const WSL_LOCALHOST_PREFIX: &str = r"\\wsl.localhost\";
const PREFIXES: [&str; 2] = [WSL_PREFIX, WSL_LOCALHOST_PREFIX];

/// The length of the `\\wsl$\` or `\\wsl.localhost\` prefix of `path` (any case), if any. With
/// `posix`, `/` separators are accepted too (as produced by [`crate::args::to_posix_path`]).
fn prefix_len(path: &str, posix: bool) -> Option<usize> {
    let b = path.as_bytes();
    // the prefixes are ASCII: the length is a char boundary of `path`
    PREFIXES
        .into_iter()
        .find(|p| b.len() >= p.len() && b.iter().zip(p.bytes()).all(|(&c, p)| c.eq_ignore_ascii_case(&p) || (posix && p == b'\\' && c == b'/')))
        .map(str::len)
}

/// Whether the path starts with a drive like `C:`.
fn has_drive(b: &[u8]) -> bool {
    b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// Whether the path is a WSL path (`\\wsl$\…` or `\\wsl.localhost\…`). Port of `IsWslPath`.
pub fn is_wsl_path(path: &str) -> bool {
    prefix_len(path, false).is_some()
}

/// Replaces `\\wsl.localhost\` with `\\wsl$\` and normalizes the case of the prefix.
/// Port of `NormalizeWslPath`.
pub fn normalize_wsl_path(path: &str) -> String {
    match prefix_len(path, false) {
        Some(n) => format!("{WSL_PREFIX}{}", &path[n..]),
        None => path.to_string(),
    }
}

/// The distro name (like `Ubuntu-22.04`) of a WSL path, empty otherwise. Port of `GetWslDistro`
/// (which also accepts `\\wsl.localhost\`).
pub fn distro(path: &str) -> String {
    let Some(n) = prefix_len(path, false) else {
        return String::new();
    };
    let rest = &path[n..];
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
    if let Some(len) = prefix_len(path, true) {
        let rest = &posix[len..];
        let path_distro = rest.split('/').next().unwrap_or_default();
        if !path_distro.eq_ignore_ascii_case(wsl_distro) {
            return posix;
        }
        return rest[path_distro.len()..].to_string();
    }
    if has_drive(path.as_bytes()) {
        return format!("/mnt/{}{}", (path.as_bytes()[0] as char).to_ascii_lowercase(), &posix[2..]);
    }
    posix
}

/// Converts a path printed by git to a path of the app (Windows path with `\` separators for
/// a distro). Port of `GetWindowsPath`.
pub fn windows_path(path: &str, wsl_distro: &str) -> String {
    if path.is_empty() || wsl_distro.is_empty() || prefix_len(path, true).is_some() {
        return path.to_string();
    }
    let native = |p: &str| p.replace('/', "\\");
    // only `/mnt/<drive letter>` is a Windows drive: `/mnt/wsl`, `/mnt/wslg` or `/mnt/data` are not
    if let Some(rest) = path.strip_prefix("/mnt/") {
        let b = rest.as_bytes();
        if !b.is_empty() && b[0].is_ascii_alphabetic() && (b.len() == 1 || b[1] == b'/') {
            let tail = if b.len() > 1 { &rest[1..] } else { "/" };
            return native(&format!("{}:{tail}", (b[0] as char).to_ascii_uppercase()));
        }
    }
    if path.starts_with(['\\', '/']) {
        return native(&format!("{WSL_PREFIX}{wsl_distro}{path}"));
    }
    native(&format!("{WSL_PREFIX}{wsl_distro}\\{path}"))
}

/// Converts the absolute paths of a git command line for a distro ([`convert_arg`]), except
/// in messages (`-m <message>`, `--message=<message>`), which can start like a path (`C:\`).
pub fn convert_args(args: &[String], wsl_distro: &str) -> Vec<String> {
    let mut is_message = false;
    args.iter()
        .map(|arg| {
            let keep = is_message || arg.starts_with("--message=");
            is_message = arg == "-m" || arg == "--message";
            if keep { arg.clone() } else { convert_arg(arg, wsl_distro) }
        })
        .collect()
}

/// Converts the absolute paths in a git argument for a distro: the whole argument
/// (`C:/temp/patch`), or the value of an option (`--output=C:/temp/x`).
pub fn convert_arg(arg: &str, wsl_distro: &str) -> String {
    let is_absolute = |s: &str| prefix_len(s, true).is_some() || (has_drive(s.as_bytes()) && matches!(s.as_bytes()[2], b'/' | b'\\'));
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

    #[test]
    fn windows_path_only_converts_drive_mounts() {
        // `/mnt/<not a drive letter>` stays in the distro (it was turned into `D:ata\repo`)
        assert_eq!(windows_path("/mnt/data/repo", "Ubuntu"), r"\\wsl$\Ubuntu\mnt\data\repo");
        assert_eq!(windows_path("/mnt/wslg/runtime-dir", "Ubuntu"), r"\\wsl$\Ubuntu\mnt\wslg\runtime-dir");
        // the root of a drive
        assert_eq!(windows_path("/mnt/c", "Ubuntu"), r"C:\");
        assert_eq!(windows_path("/mnt/d/", "Ubuntu"), r"D:\");
        assert_eq!(windows_path("/mnt/c/x", "Ubuntu"), r"C:\x");
    }

    #[test]
    fn windows_path_non_ascii_does_not_panic() {
        // panicked: byte index 6 is not a char boundary
        assert_eq!(windows_path("/mnt/équipe/repo", "Ubuntu"), r"\\wsl$\Ubuntu\mnt\équipe\repo");
        assert_eq!(windows_path("/mnt/c/Users/Jérémie/repo", "Ubuntu"), r"C:\Users\Jérémie\repo");
        assert_eq!(windows_path("/home/日本/repo", "Ubuntu"), r"\\wsl$\Ubuntu\home\日本\repo");
    }

    #[test]
    fn messages_are_not_converted() {
        let args: Vec<String> = ["commit", "-m", r"C:\temp is no longer used", "--", "C:/x.txt"].iter().map(|s| s.to_string()).collect();
        assert_eq!(convert_args(&args, "Ubuntu"), ["commit", "-m", r"C:\temp is no longer used", "--", "/mnt/c/x.txt"]);
        let args: Vec<String> = ["stash", "push", "--message=D:/notes", "--message", "C:/a"].iter().map(|s| s.to_string()).collect();
        assert_eq!(convert_args(&args, "Ubuntu"), ["stash", "push", "--message=D:/notes", "--message", "C:/a"]);
        // `-m` only protects the next argument
        let args: Vec<String> = ["merge", "-m", "msg", "C:/x"].iter().map(|s| s.to_string()).collect();
        assert_eq!(convert_args(&args, "Ubuntu"), ["merge", "-m", "msg", "/mnt/c/x"]);
    }

    /// Paths printed by git or typed by the user can be anything: no conversion may panic.
    #[test]
    fn conversions_never_panic() {
        let pieces = ["", "/", "\\", "//", r"\\", "mnt", "/mnt/", "/mnt/c", "c:", "C:\\", "é", "日本", "wsl$", r"\\wsl$\", r"\\wsl.localhost\", "//wsl$/", "Ubuntu", "-", "--x=", "-m", "a b", "€", "\u{1F600}"];
        let mut n = 0;
        for a in pieces {
            for b in pieces {
                for c in pieces {
                    let s = format!("{a}{b}{c}");
                    for d in ["", "Ubuntu", "é"] {
                        let _ = (is_wsl_path(&s), normalize_wsl_path(&s), distro(&s), path_for_git(&s, d), windows_path(&s, d), convert_arg(&s, d));
                        n += 1;
                    }
                }
            }
        }
        assert_eq!(n, pieces.len().pow(3) * 3);
    }

    /// Every git command of a WSL repository goes through these conversions, and every path git
    /// prints comes back through them: they must stay cheap.
    #[test]
    fn conversion_throughput() {
        let args: Vec<String> = ["log", "-z", "--format=%H", "-m", r"C:\message", "--output=D:/out.txt", r"\\wsl$\Ubuntu\home\jack\repo\src\main.rs", "src/lib.rs"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let paths = ["/home/jack/repo/.git", "/mnt/c/Users/jack/repo", "/mnt/wslg/x", "/home/jérémie/dépôt"];
        let rounds = 20_000;
        let started = std::time::Instant::now();
        let mut total = 0;
        for _ in 0..rounds {
            total += convert_args(&args, "Ubuntu-22.04").len();
            for p in paths {
                total += windows_path(p, "Ubuntu-22.04").len();
            }
        }
        let per_round = started.elapsed() / rounds;
        eprintln!("wsl conversion: {per_round:?} per command line (8 arguments) and 4 printed paths");
        assert!(total > 0);
        // a few µs in debug builds; the budget only catches an accidental quadratic or allocation storm
        assert!(per_round < std::time::Duration::from_micros(200), "{per_round:?}");
    }
}
