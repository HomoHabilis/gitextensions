//! Port of `AppSettings`: application settings persisted as JSON in the user's config directory.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::commands::{GitRefsSortBy, GitRefsSortOrder, LocalChangesAction};
use crate::revision_reader::RevisionSortOrder;

/// The configuration directory of Git Extensions (`~/.config/GitExtensions` on Linux).
pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("GITEXT_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    dirs::config_dir().map(|d| d.join("GitExtensions"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PullAction {
    #[default]
    Merge,
    Rebase,
    Fetch,
    FetchAll,
    FetchPruneAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BranchFilterMode {
    /// All branches (`--all`).
    #[default]
    All,
    /// Only the current branch.
    Current,
    /// Branches given by `branch_filter`.
    Specific,
}

/// Graph drawing style (`RevisionGraphDrawStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GraphDrawStyle {
    #[default]
    Normal,
    DrawNonRelativesGray,
    HighlightSelected,
}

/// A commit template (`CommitTemplateItem`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitTemplate {
    pub name: String,
    pub text: String,
}

/// A user script (`ScriptInfo`), run from the revision context menu or toolbar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserScript {
    pub name: String,
    pub command: String,
    pub arguments: String,
    #[serde(default)]
    pub ask_confirmation: bool,
    #[serde(default)]
    pub run_in_background: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    // General
    pub git_command: String,
    pub theme: Theme,
    pub ui_scale: f32,
    pub font_size: f32,
    pub monospace_font_size: f32,
    pub recent_repositories_history_size: usize,
    pub sort_recent_repos_alphabetically: bool,
    pub close_process_dialog_on_success: bool,
    pub default_clone_destination: String,
    pub follow_renames_in_file_history: bool,
    pub show_git_command_line: bool,
    pub check_for_uncommitted_changes_in_checkout: bool,
    pub default_local_changes_action: LocalChangesAction,
    pub auto_stash_on_checkout: bool,

    // Revision grid
    pub show_author_column: bool,
    pub show_date_column: bool,
    pub show_id_column: bool,
    pub show_relative_date: bool,
    pub show_author_date: bool,
    pub show_current_branch_only: bool,
    pub branch_filter_mode: BranchFilterMode,
    pub branch_filter: String,
    pub show_remote_branches: bool,
    pub show_tags: bool,
    pub show_stashes: bool,
    pub show_reflog_references: bool,
    pub show_git_notes: bool,
    pub show_artificial_commits: bool,
    pub show_first_parent: bool,
    pub show_merge_commits: bool,
    pub show_superproject_refs: bool,
    pub revision_sort_order: RevisionSortOrder,
    pub max_revision_graph_commits: usize,
    pub refs_sort_by: GitRefsSortBy,
    pub refs_sort_order: GitRefsSortOrder,

    // Revision graph
    pub merge_graph_lanes_having_common_parent: bool,
    pub render_graph_with_diagonals: bool,
    pub straighten_graph_diagonals: bool,
    pub straighten_graph_segments_limit: i32,
    pub graph_draw_style: GraphDrawStyle,
    pub highlight_author_commits: bool,
    pub lane_width: f32,
    pub row_height: f32,

    // Diff viewer
    pub ignore_whitespace: bool,
    pub ignore_whitespace_changes: bool,
    pub context_lines: u32,
    pub show_entire_file: bool,
    pub use_histogram_diff: bool,
    pub show_line_numbers: bool,
    pub word_wrap_diff: bool,
    pub show_whitespace: bool,

    // Blame
    pub detect_copy_in_file_on_blame: bool,
    pub detect_copy_in_all_on_blame: bool,
    pub ignore_whitespace_on_blame: bool,

    // Commit dialog
    pub remember_amend_commit_state: bool,
    pub ensure_commit_message_second_line_empty: bool,
    pub commit_validation_max_cnt_chars_first_line: usize,
    pub commit_validation_max_cnt_chars_per_line: usize,
    pub commit_validation_second_line_must_be_empty: bool,
    pub sign_off_by_default: bool,
    pub push_after_commit: bool,
    pub refresh_commit_dialog_on_form_focus: bool,
    pub commit_templates: Vec<CommitTemplate>,
    pub commit_message_history_size: usize,
    pub show_untracked_files: bool,

    // Remote operations
    pub pull_action: PullAction,
    pub auto_stash_on_pull: bool,
    pub prune_on_fetch: bool,
    pub fetch_tags: bool,
    pub force_push_with_lease: bool,
    pub recurse_submodules_on_push: u8,
    pub update_submodules_on_checkout: bool,

    // Tools
    pub merge_tool: String,
    pub diff_tool: String,
    pub editor: String,
    pub terminal: String,
    pub file_manager: String,
    pub user_scripts: Vec<UserScript>,

    // Window layout
    pub left_panel_visible: bool,
    pub commit_info_position_right: bool,
    pub last_browse_tab: usize,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            git_command: "git".into(),
            theme: Theme::System,
            ui_scale: 1.0,
            font_size: 13.5,
            monospace_font_size: 13.0,
            recent_repositories_history_size: 30,
            sort_recent_repos_alphabetically: false,
            close_process_dialog_on_success: true,
            default_clone_destination: dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default(),
            follow_renames_in_file_history: true,
            show_git_command_line: false,
            check_for_uncommitted_changes_in_checkout: true,
            default_local_changes_action: LocalChangesAction::DontChange,
            auto_stash_on_checkout: false,
            show_author_column: true,
            show_date_column: true,
            show_id_column: true,
            show_relative_date: true,
            show_author_date: true,
            show_current_branch_only: false,
            branch_filter_mode: BranchFilterMode::All,
            branch_filter: String::new(),
            show_remote_branches: true,
            show_tags: true,
            show_stashes: true,
            show_reflog_references: false,
            show_git_notes: false,
            show_artificial_commits: true,
            show_first_parent: false,
            show_merge_commits: true,
            show_superproject_refs: true,
            revision_sort_order: RevisionSortOrder::GitDefault,
            max_revision_graph_commits: 100_000,
            refs_sort_by: GitRefsSortBy::Default,
            refs_sort_order: GitRefsSortOrder::Descending,
            merge_graph_lanes_having_common_parent: true,
            render_graph_with_diagonals: true,
            straighten_graph_diagonals: true,
            straighten_graph_segments_limit: 80,
            graph_draw_style: GraphDrawStyle::DrawNonRelativesGray,
            highlight_author_commits: true,
            lane_width: 16.0,
            row_height: 22.0,
            ignore_whitespace: false,
            ignore_whitespace_changes: false,
            context_lines: 3,
            show_entire_file: false,
            use_histogram_diff: false,
            show_line_numbers: true,
            word_wrap_diff: false,
            show_whitespace: false,
            detect_copy_in_file_on_blame: true,
            detect_copy_in_all_on_blame: false,
            ignore_whitespace_on_blame: true,
            remember_amend_commit_state: true,
            ensure_commit_message_second_line_empty: true,
            commit_validation_max_cnt_chars_first_line: 0,
            commit_validation_max_cnt_chars_per_line: 0,
            commit_validation_second_line_must_be_empty: false,
            sign_off_by_default: false,
            push_after_commit: false,
            refresh_commit_dialog_on_form_focus: true,
            commit_templates: Vec::new(),
            commit_message_history_size: 30,
            show_untracked_files: true,
            pull_action: PullAction::Merge,
            auto_stash_on_pull: false,
            prune_on_fetch: false,
            fetch_tags: true,
            force_push_with_lease: true,
            recurse_submodules_on_push: 0,
            update_submodules_on_checkout: false,
            merge_tool: String::new(),
            diff_tool: String::new(),
            editor: String::new(),
            terminal: String::new(),
            file_manager: String::new(),
            user_scripts: Vec::new(),
            left_panel_visible: true,
            commit_info_position_right: false,
            last_browse_tab: 0,
        }
    }
}

impl AppSettings {
    pub fn default_path() -> Option<PathBuf> {
        config_dir().map(|d| d.join("settings.json"))
    }

    /// Loads settings; missing/invalid files yield defaults (unknown keys are ignored).
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).unwrap_or_default())?;
        std::fs::rename(tmp, path)
    }

    pub fn graph_config(&self) -> GraphConfig {
        GraphConfig {
            merge_graph_lanes_having_common_parent: self.merge_graph_lanes_having_common_parent,
            render_graph_with_diagonals: self.render_graph_with_diagonals,
            straighten_graph_diagonals: self.straighten_graph_diagonals,
            straighten_graph_segments_limit: self.straighten_graph_segments_limit,
        }
    }
}

/// Graph settings (mirrors `gitext_graph::RevisionGraphConfig` without the dependency).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphConfig {
    pub merge_graph_lanes_having_common_parent: bool,
    pub render_graph_with_diagonals: bool,
    pub straighten_graph_diagonals: bool,
    pub straighten_graph_segments_limit: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_original_app_settings() {
        let s = AppSettings::default();
        assert!(s.merge_graph_lanes_having_common_parent);
        assert!(s.render_graph_with_diagonals);
        assert!(s.straighten_graph_diagonals);
        assert_eq!(s.straighten_graph_segments_limit, 80);
        assert_eq!(s.recent_repositories_history_size, 30);
        assert!(s.show_stashes);
    }

    #[test]
    fn save_and_load_roundtrip_and_tolerate_partial_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut s = AppSettings::default();
        s.theme = Theme::Dark;
        s.commit_templates.push(CommitTemplate { name: "fix".into(), text: "fix: ".into() });
        s.save(&path).unwrap();
        assert_eq!(AppSettings::load(&path), s);
        std::fs::write(&path, r#"{"show_tags": false, "unknown": 1}"#).unwrap();
        let l = AppSettings::load(&path);
        assert!(!l.show_tags);
        assert!(l.show_stashes);
        std::fs::write(&path, "garbage").unwrap();
        assert_eq!(AppSettings::load(&path), AppSettings::default());
    }
}
