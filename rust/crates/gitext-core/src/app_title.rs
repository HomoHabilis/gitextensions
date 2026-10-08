//! Port of `AppTitleGenerator` and `RepositoryDescriptionProvider`.

use std::path::Path;

pub const APPLICATION_NAME: &str = "Git Extensions";

/// Short description of a repository: its directory name, with the chain of super
/// projects for submodules (`sub < parent`). Uninformative names are skipped for submodules.
pub fn repository_description(working_dir: &Path, is_valid_working_dir: impl Fn(&Path) -> bool) -> String {
    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut description = name(working_dir);
    let mut current = working_dir.to_path_buf();
    let mut first = true;
    while let Some(parent) = current.parent().map(Path::to_path_buf) {
        let mut probe = parent.clone();
        let mut found = None;
        loop {
            if is_valid_working_dir(&probe) {
                found = Some(probe.clone());
                break;
            }
            match probe.parent() {
                Some(p) => probe = p.to_path_buf(),
                None => break,
            }
        }
        let Some(super_dir) = found else { break };
        let is_uninformative = matches!(description.to_lowercase().as_str(), "app" | "repo" | "repository");
        if first && is_uninformative && working_dir.parent() != Some(super_dir.as_path()) {
            description = name(working_dir.parent().unwrap());
        }
        description = format!("{description} < {}", name(&super_dir));
        current = super_dir;
        first = false;
    }
    description
}

/// Port of `AppTitleGenerator.Generate`.
pub fn generate_title(description: Option<&str>, branch_name: Option<&str>, default_branch_name: &str, path_name: Option<&str>) -> String {
    let Some(description) = description.filter(|d| !d.trim().is_empty()) else {
        return APPLICATION_NAME.to_string();
    };
    let branch = branch_name.filter(|b| !b.trim().is_empty()).unwrap_or(default_branch_name);
    let path = path_name.filter(|p| !p.trim().is_empty()).map(|p| {
        let trimmed = p.trim_matches('"');
        let file = trimmed.rsplit('/').next().unwrap_or_default();
        let invalid = file.is_empty();
        if invalid {
            if p.starts_with('"') && p.ends_with('"') { format!("{p} ") } else { format!("\"{p}\" ") }
        } else {
            format!("\"{file}\" ")
        }
    });
    format!("{}{description} ({branch}) - {APPLICATION_NAME}", path.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    //! Ported from GitCommands.Tests/AppTitleGeneratorTests.cs and RepositoryDescriptionProviderTests.cs
    use super::*;

    const SHORT: &str = "repo";

    #[test]
    fn generate_should_return_default_title_if_invalid() {
        for d in [None, Some(""), Some(" "), Some("\t")] {
            assert_eq!(generate_title(d, None, "", None), APPLICATION_NAME);
        }
    }

    #[test]
    fn generate_branch_and_path() {
        assert!(generate_title(Some(SHORT), None, "(no branch)", None).starts_with(&format!("{SHORT} ((no branch)) - {APPLICATION_NAME}")));
        let b = "feature/my_(test)_branch";
        assert!(generate_title(Some(SHORT), Some(b), "", None).starts_with(&format!("{SHORT} ({b}) - {APPLICATION_NAME}")));
        assert!(generate_title(Some(SHORT), Some(b), "", Some("folder/folder/file")).starts_with(&format!("\"file\" {SHORT} ({b}) - ")));
        assert!(generate_title(Some(SHORT), None, "x", Some("\"folder/folder/file\"")).starts_with(&format!("\"file\" {SHORT} (x) - ")));
        for (p, e) in [("\"folder/folder/\"", "folder/folder/"), ("folder/folder/", "folder/folder/"), ("\"folder/folder/f*ile extra\"", "f*ile extra")] {
            assert!(generate_title(Some(SHORT), None, "x", Some(p)).starts_with(&format!("\"{e}\" {SHORT} (x) - ")), "{p}");
        }
    }

    #[test]
    fn repository_description_handles_subrepos() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test_repo");
        let sub = repo.join("submodule");
        let nested = sub.join("nested");
        let leaf = nested.join("leafsubmodule");
        std::fs::create_dir_all(&leaf).unwrap();
        let valid = |p: &Path| p == repo || p == sub || p == leaf;
        assert_eq!(repository_description(&repo, valid), "test_repo");
        assert_eq!(repository_description(&sub, valid), "submodule < test_repo");
        assert_eq!(repository_description(&leaf, valid), "leafsubmodule < submodule < test_repo");
    }

    #[test]
    fn repository_description_skips_uninformative_submodule_name() {
        for u in ["app", "repo", "repository"] {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("rootrepo");
            let repo = root.join("parent").join(u);
            std::fs::create_dir_all(&repo).unwrap();
            let valid = |p: &Path| p == root || p == repo;
            assert_eq!(repository_description(&repo, valid), "parent < rootrepo");
            // not skipped when the parent is the super repo
            let repo2 = root.join(u);
            let valid2 = |p: &Path| p == root || p == repo2;
            assert_eq!(repository_description(&repo2, valid2), format!("{u} < rootrepo"));
            // not skipped for a root repo
            assert_eq!(repository_description(&repo2, |p: &Path| p == repo2), u);
        }
    }
}
