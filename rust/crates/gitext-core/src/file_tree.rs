//! Port of `FileStatusList.StatusSorter`: builds the folder tree shown in file lists.

use std::cmp::Ordering;

use crate::status::GitItemStatus;

/// What a tree node represents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeTag {
    /// A folder with its relative path.
    Folder(String),
    /// A file: index into the input slice.
    Item(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    pub text: String,
    pub tag: NodeTag,
    pub nodes: Vec<TreeNode>,
}

impl TreeNode {
    fn folder(path: &str) -> Self {
        TreeNode { text: path.to_string(), tag: NodeTag::Folder(path.to_string()), nodes: Vec::new() }
    }

    /// All item indexes below this node, in display order.
    pub fn items(&self) -> Vec<usize> {
        let mut v = Vec::new();
        self.collect(&mut v);
        v
    }

    fn collect(&self, v: &mut Vec<usize>) {
        if let NodeTag::Item(i) = self.tag {
            v.push(i);
        }
        for n in &self.nodes {
            n.collect(v);
        }
    }
}

fn culture_cmp(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| b.cmp(a))
}

fn compare_path(l: &str, r: &str) -> Ordering {
    if l.is_empty() || r.is_empty() {
        return match (l.is_empty(), r.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Less,
            _ => Ordering::Greater,
        };
    }
    let (tl, sl) = l.split_once('/').unwrap_or((l, ""));
    let (tr, sr) = r.split_once('/').unwrap_or((r, ""));
    match culture_cmp(tl, tr) {
        Ordering::Equal => compare_path(sl, sr),
        o => o,
    }
}

fn starts_with_dir(long: &str, short: &str) -> bool {
    long.starts_with(short) && long.as_bytes().get(short.len()) == Some(&b'/')
}

/// Port of `PathFirstComparer`: folders before files, then by name.
pub fn path_first_compare(l: &GitItemStatus, r: &GitItemStatus) -> Ordering {
    let (lp, rp) = (l.path(), r.path());
    let path_cmp = match (lp.is_empty(), rp.is_empty()) {
        (true, true) => Ordering::Equal,
        (false, true) => Ordering::Less,
        (true, false) => Ordering::Greater,
        _ => compare_path(lp, rp),
    };
    match path_cmp {
        Ordering::Less => {
            if starts_with_dir(rp, lp) {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        }
        Ordering::Greater => {
            if starts_with_dir(lp, rp) {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        Ordering::Equal => culture_cmp(&l.name, &r.name),
    }
}

/// Port of `GetCommonPath`.
pub fn get_common_path(a: &str, b: &str) -> String {
    let a = format!("{a}/");
    let b = format!("{b}/");
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let mut common_end = 0;
    loop {
        if common_end >= ab.len() || common_end >= bb.len() || ab[common_end] != bb[common_end] {
            while common_end > 0 {
                common_end -= 1;
                if ab[common_end] == b'/' {
                    break;
                }
            }
            return a[..common_end].to_string();
        }
        common_end += 1;
    }
}

/// Builds the sorted tree of `statuses` (port of `CreateTreeSortedByPath`).
/// `text_of` gives the display text of an item (the C# test uses `ToString()`, i.e. the name).
pub fn create_tree_sorted_by_path(statuses: &[GitItemStatus], flat: bool, merge_single_items_with_folder: bool) -> TreeNode {
    let mut order: Vec<usize> = (0..statuses.len()).collect();
    order.sort_by(|&a, &b| path_first_compare(&statuses[a], &statuses[b]));

    let mut root = TreeNode::folder("");
    // Path (as indexes) to the current parent node from the root.
    let mut parent: Vec<usize> = Vec::new();
    for i in order {
        let status = &statuses[i];
        if !flat {
            parent = get_or_create_parent(&mut root, &parent, status.path());
        }
        let leaf = TreeNode { text: status.name.clone(), tag: NodeTag::Item(i), nodes: Vec::new() };
        node_at_mut(&mut root, &parent).nodes.push(leaf);
    }
    if !flat {
        for n in &mut root.nodes {
            remove_parent_path(n, "", merge_single_items_with_folder);
        }
    }
    root
}

fn node_at<'a>(root: &'a TreeNode, path: &[usize]) -> &'a TreeNode {
    path.iter().fold(root, |n, &i| &n.nodes[i])
}

fn node_at_mut<'a>(root: &'a mut TreeNode, path: &[usize]) -> &'a mut TreeNode {
    path.iter().fold(root, |n, &i| &mut n.nodes[i])
}

fn folder_path(n: &TreeNode) -> &str {
    match &n.tag {
        NodeTag::Folder(p) => p,
        NodeTag::Item(_) => "",
    }
}

fn get_or_create_parent(root: &mut TreeNode, previous: &[usize], current_path: &str) -> Vec<usize> {
    let previous_path = folder_path(node_at(root, previous)).to_string();
    if previous_path == current_path {
        return previous.to_vec();
    }
    let common_path = get_common_path(&previous_path, current_path);
    let common_parent: Vec<usize> = if common_path.is_empty() {
        Vec::new()
    } else {
        let mut split_candidate = previous.to_vec();
        let mut split_candidate_path = previous_path.clone();
        while !split_candidate.is_empty() {
            let parent = &split_candidate[..split_candidate.len() - 1];
            let parent_path = folder_path(node_at(root, parent));
            if !parent.is_empty() && parent_path.starts_with(&common_path) {
                split_candidate_path = parent_path.to_string();
                split_candidate.pop();
            } else {
                break;
            }
        }
        if split_candidate_path == common_path {
            split_candidate
        } else {
            // Split: insert a common folder node in place of split_candidate
            let (idx, parent_path) = split_candidate.split_last().unwrap();
            let parent_node = node_at_mut(root, parent_path);
            let sub = parent_node.nodes.remove(*idx);
            let mut common = TreeNode::folder(&common_path);
            common.nodes.push(sub);
            parent_node.nodes.insert(*idx, common);
            split_candidate
        }
    };
    if current_path == common_path {
        return common_parent;
    }
    let node = node_at_mut(root, &common_parent);
    node.nodes.push(TreeNode::folder(current_path));
    let mut p = common_parent;
    p.push(node.nodes.len() - 1);
    p
}

fn remove_parent_path(node: &mut TreeNode, parent_path: &str, merge_single: bool) {
    if merge_single && node.nodes.len() == 1 && node.nodes[0].nodes.is_empty() {
        let single = node.nodes.remove(0);
        node.tag = single.tag;
        node.text = single.text;
    }
    if !parent_path.is_empty() && node.text.starts_with(parent_path) && node.text.len() > parent_path.len() {
        node.text = node.text[parent_path.len() + 1..].to_string();
    }
    let my_path = folder_path(node).to_string();
    for n in &mut node.nodes {
        remove_parent_path(n, &my_path, merge_single);
    }
}

#[cfg(test)]
mod tests {
    //! Ported from GitUI.Tests/UserControls/FileStatusListSorterTests.cs (verified snapshots).
    use super::*;
    use crate::testing::test_data_dir;

    fn to_json(node: &TreeNode, statuses: &[GitItemStatus]) -> serde_json::Value {
        let tag = match &node.tag {
            NodeTag::Folder(p) => format!("RelativePath: {p}"),
            NodeTag::Item(i) => format!("FileStatusItem: {}", statuses[*i].name),
        };
        serde_json::json!({
            "Text": node.text,
            "Tag": tag,
            "Nodes": node.nodes.iter().map(|n| to_json(n, statuses)).collect::<Vec<_>>(),
        })
    }

    fn verify(name: &str, root: &TreeNode, statuses: &[GitItemStatus]) {
        let text = std::fs::read_to_string(test_data_dir().join("core/file_tree").join(format!("FileStatusListSorterTests.{name}.verified.txt"))).unwrap();
        let expected: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap();
        let actual = to_json(root, statuses);
        assert_eq!(actual, expected, "{name}\n{}", serde_json::to_string_pretty(&actual).unwrap());
    }

    fn items(names: &[&str]) -> Vec<GitItemStatus> {
        names.iter().map(|n| GitItemStatus::new(*n)).collect()
    }

    fn cap(b: bool) -> &'static str {
        if b { "True" } else { "False" }
    }

    #[test]
    fn compare_should_not_compare_path_separator() {
        let a = GitItemStatus::new("dir/sub/file");
        let b = GitItemStatus::new("dir.ext/file");
        assert_eq!(path_first_compare(&a, &b), Ordering::Less);
        assert_eq!(path_first_compare(&b, &a), Ordering::Greater);
    }

    #[test]
    fn sort_should_first_add_folders_then_files() {
        let old_folder = "oldfolder/of/renamed/file/";
        let mut statuses = items(&[
            "root_file", ".hidden_root_file", "ext/submodule/", "ext/subfolder/filees", "ext/s/", "ext/file", "ext2/file", "1/file1",
            "1/submodule_sorted_as_file/", "1/subfolder/file1s", "1/2/file12", "1/3/file13", "1/3/4/file134", "5/file1", "5/6/file56b",
            "5/6/file56a", "5/7/file57b", "5/7/file57a",
        ]);
        let mut renamed = GitItemStatus::new(format!("{old_folder}5/7/8/file578"));
        renamed.old_name = Some(renamed.name.clone());
        renamed.name = renamed.name[old_folder.len()..].to_string();
        statuses.push(renamed);
        for flat in [false, true] {
            for merge in [false, true] {
                let root = create_tree_sorted_by_path(&statuses, flat, merge);
                verify(&format!("Sort_should_first_add_folders_then_files_flat={}_mergeSingleItemsWithFolder={}", cap(flat), cap(merge)), &root, &statuses);
            }
        }
    }

    #[test]
    fn sort_should_not_split_folders() {
        let statuses = items(&["core/c.1", "core/c.2", "core.dot/cd.3", "core/api/c_a.0"]);
        verify("Sort_should_not_split_folders", &create_tree_sorted_by_path(&statuses, false, false), &statuses);
    }

    #[test]
    fn sort_should_not_merge_single_file_with_root_node() {
        let statuses = items(&["root_file"]);
        for flat in [false, true] {
            verify(&format!("Sort_should_not_merge_single_file_with_root_node_flat={}", cap(flat)), &create_tree_sorted_by_path(&statuses, flat, true), &statuses);
        }
    }

    #[test]
    fn sort_should_optionally_create_subfolder_nodes_for_single_files() {
        let statuses = items(&["1/2/file12", "1/3/file13", "1/4/file14"]);
        for merge in [false, true] {
            verify(
                &format!("Sort_should_optionally_create_subfolder_nodes_for_single_files_mergeSingleItemsWithFolder={}", cap(merge)),
                &create_tree_sorted_by_path(&statuses, false, merge),
                &statuses,
            );
        }
    }

    #[test]
    fn get_common_path_cases() {
        for (a, b, e) in [
            ("", "", ""),
            ("a", "a", "a"),
            ("a", "b", ""),
            ("a", "ab", ""),
            ("a", "a/b", "a"),
            ("a", "a/b/c", "a"),
            ("a/b", "a/bc", "a"),
            ("a/b", "a/b", "a/b"),
            ("a/b", "a/b/c", "a/b"),
            ("a/b/cc", "a/b/ccd", "a/b"),
            ("a/b/cc", "a/b/cc/de", "a/b/cc"),
        ] {
            assert_eq!(get_common_path(a, b), e, "{a} {b}");
            assert_eq!(get_common_path(b, a), e, "{b} {a}");
        }
    }
}
