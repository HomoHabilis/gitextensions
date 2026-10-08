//! URL helpers: port of `RepoNameExtractor` and the clone dialog's destination name logic.

fn file_name_without_extension(path: &str) -> String {
    let name = path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or_default();
    let name = if path.ends_with(['/', '\\']) { "" } else { name };
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].to_string(),
        _ => name.to_string(),
    }
}

/// (project, repository name) from a remote url (port of `RepoNameExtractor.Get`).
pub fn repo_project_and_name(remote_url: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(url) = remote_url else {
        return (None, None);
    };
    let repo = file_name_without_extension(url);
    let dir = match url.rfind(['/', '\\']) {
        Some(i) => &url[..i],
        None => "",
    };
    let project = file_name_without_extension(dir);
    (Some(project), Some(repo))
}

/// The directory name `git clone` would create for `url` (used by the clone dialog).
pub fn clone_directory_name(url: &str) -> String {
    let u = url.trim().trim_end_matches(['/', '\\']);
    let u = u.strip_suffix(".git").unwrap_or(u);
    let u = u.trim_end_matches(['/', '\\']);
    let last = u.rsplit(['/', '\\', ':']).next().unwrap_or_default();
    last.to_string()
}

#[cfg(test)]
mod tests {
    //! Ported from GitCommands.Tests/RepoNameExtractorTest.cs
    use super::*;

    #[test]
    fn repo_name_extractor_cases() {
        for (url, project, repo) in [
            (Some("https://github.com/project/repo.git"), Some("project"), Some("repo")),
            (Some("file://github/project/repo.git"), Some("project"), Some("repo")),
            (Some("https://github.com/extra/extra/project/repo.git"), Some("project"), Some("repo")),
            (None, None, None),
            (Some("https://github.com/project/"), Some("project"), Some("")),
            (Some("git@github.com/project/repo.git"), Some("project"), Some("repo")),
        ] {
            let (p, r) = repo_project_and_name(url);
            assert_eq!(p.as_deref(), project, "{url:?}");
            assert_eq!(r.as_deref(), repo, "{url:?}");
        }
    }

    #[test]
    fn clone_directory_names() {
        assert_eq!(clone_directory_name("https://github.com/gitextensions/gitextensions.git"), "gitextensions");
        assert_eq!(clone_directory_name("git@github.com:user/repo.git"), "repo");
        assert_eq!(clone_directory_name("/srv/repos/proj/"), "proj");
        assert_eq!(clone_directory_name("C:\\repos\\proj.git"), "proj");
    }
}
