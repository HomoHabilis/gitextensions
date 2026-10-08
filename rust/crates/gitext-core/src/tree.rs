//! Port of `GitTreeParser` and `GitItem`.

use std::sync::OnceLock;

use regex::Regex;

use crate::object_id::ObjectId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GitObjectType {
    None,
    Commit,
    Tree,
    Blob,
}

/// An entry of `git ls-tree`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitItem {
    pub mode: u32,
    pub object_type: GitObjectType,
    pub object_id: ObjectId,
    pub name: String,
}

impl GitItem {
    pub fn guid(&self) -> String {
        self.object_id.to_string()
    }
}

fn tree_line() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?<mode>\d{6}) (?<type>(?:blob|tree|commit)+) (?<objectid>[0-9a-f]{40})\t(?<name>.+)$").unwrap())
}

fn ls_files_line() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?<mode>\d{6}) (?<objectid>[0-9a-f]{40}) (?:[0-9])\t(?<name>.+)$").unwrap())
}

/// Parses one `ls-tree` line.
pub fn parse_single(raw: &str) -> Option<GitItem> {
    let m = tree_line().captures(raw)?;
    let object_type = match &m["type"] {
        "blob" => GitObjectType::Blob,
        "tree" => GitObjectType::Tree,
        "commit" => GitObjectType::Commit,
        _ => GitObjectType::None,
    };
    Some(GitItem {
        mode: m["mode"].parse().ok()?,
        object_type,
        object_id: ObjectId::parse(&m["objectid"]).ok()?,
        name: m["name"].to_string(),
    })
}

/// Parses `git ls-tree -z` output.
pub fn parse(tree: &str) -> Vec<GitItem> {
    if tree.trim().is_empty() {
        return Vec::new();
    }
    tree.split('\0').filter_map(parse_single).collect()
}

/// Parses `git ls-files --stage -z` output.
pub fn parse_ls_files(tree: &str) -> Vec<GitItem> {
    tree.split('\0')
        .filter_map(|raw| {
            let m = ls_files_line().captures(raw)?;
            let mode: u32 = m["mode"].parse().ok()?;
            let object_type = match mode {
                160000 => GitObjectType::Commit,
                40000 => GitObjectType::Tree,
                _ => GitObjectType::Blob,
            };
            Some(GitItem { mode, object_type, object_id: ObjectId::parse(&m["objectid"]).ok()?, name: m["name"].to_string() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! Ported from GitCommands.Tests/Git/GitTreeParserTests.cs
    use super::*;

    fn ls_tree_output() -> String {
        [
            "100644 blob 25d7b5d771e84982a3dfd8bd537531d8fb45d491\t.editorconfig",
            "100644 blob bf29d31ff93be092ce746849e8db0984d4a83231\t.gitattributes",
            "040000 tree 93185d6bd18327f5a23bc34e7eb75e66ec0ef2d1\t.github",
            "100644 blob 46cccae116d2e5a1a2f818b0b31adde4ab3800a9\t.gitignore",
            "100644 blob e55070b6c781e278bc68fc1b2525f56318d18244\t.gitmodules",
            "100644 blob 1a569e3aa555e8cdf14dcc29f9bf4edf9aa465eb\t.mailmap",
            "040000 tree 5c1f6ae123f16e2bee1c5a064cf293c11250d98f\t.nuget",
            "100644 blob 1e53ed8f6759a92d4596af6a99ef04f1554bfd57\t.travis.yml",
            "100644 blob 960498876e233a6119e20fc73171bca2e26f57c0\t space first",
            "100644 blob 38f33cf556b4aae690c640e48375cf1ff659b7a6\t space /in / path .txt",
            "040000 tree 58d57013ed2ef925fc1b3f6fe72ead258c522e75\tBin",
            "160000 commit ec097fc11ec61f502d5fced60e27d54b5fe326c0\tsubm",
            "040000 tree 0c7cce8981b980d03431f65b9b54c680a467fa2e\tBuild",
        ]
        .join("\0")
    }

    #[test]
    fn parse_should_return_empty_if_null() {
        assert!(parse("").is_empty());
    }

    #[test]
    fn parse_should_return_the_list() {
        let items = parse(&ls_tree_output());
        assert_eq!(items.len(), 13);
        assert_eq!(items[3].guid(), "46cccae116d2e5a1a2f818b0b31adde4ab3800a9");
        assert_eq!(items[3].mode, 100644);
        assert_eq!(items[3].name, ".gitignore");
        assert_eq!(items[3].object_type, GitObjectType::Blob);
        assert_eq!(items[9].name, " space /in / path .txt");
        assert_eq!(items[10].mode, 40000);
        assert_eq!(items[10].object_type, GitObjectType::Tree);
        assert_eq!(items[11].mode, 160000);
        assert_eq!(items[11].object_type, GitObjectType::Commit);
    }

    #[test]
    fn parse_single_should_return_null_if_input_invalid() {
        for s in ["", "Hello World", "ZZZZZZ blob 0000000000000000000000000000000000000000\tREADME.md"] {
            assert!(parse_single(s).is_none());
        }
    }

    #[test]
    fn parse_single_should_return_git_item() {
        let item = parse_single("100644 blob 25d7b5d771e84982a3dfd8bd537531d8fb45d491\t.editorconfig").unwrap();
        assert_eq!(item.guid(), "25d7b5d771e84982a3dfd8bd537531d8fb45d491");
        assert_eq!(item.mode, 100644);
        assert_eq!(item.name, ".editorconfig");
        assert_eq!(item.object_type, GitObjectType::Blob);
    }

    #[test]
    fn parse_ls_files_types() {
        let items = parse_ls_files("100644 07c4d877fa885b9ef1ea2c343fe237beaf7a087c 0\texternals/Directory.Build.props\0160000 1b0386aea1acdd2ba258977bd79e40a0a7b95665 0\texternals/Git.hub");
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].object_type, GitObjectType::Commit);
    }
}
