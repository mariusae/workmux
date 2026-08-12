use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::multiplexer::Multiplexer;
use crate::{config, git, vcs};
use tracing::debug;

const AUTO_BASE_BRANCH: &str = "auto";

/// Shared context for workflow operations
///
/// This struct centralizes pre-flight checks and holds essential data
/// needed by workflow modules, reducing code duplication.
pub struct WorkflowContext {
    pub vcs_kind: vcs::VcsKind,
    pub execution_dir: PathBuf,
    pub main_worktree_root: PathBuf,
    pub git_common_dir: PathBuf,
    pub main_branch: String,
    pub prefix: String,
    pub config: config::Config,
    pub mux: Arc<dyn Multiplexer>,
    /// Relative path from repo root to config directory.
    /// Empty if config is at repo root or using defaults.
    pub config_rel_dir: PathBuf,
    /// Absolute path to the directory where config was found.
    /// Used as source for file operations (copy/symlink).
    pub config_source_dir: PathBuf,
}

fn resolve_main_branch(config: &config::Config, repo_path: &Path) -> Result<String> {
    if let Some(ref branch) = config.main_branch {
        Ok(branch.clone())
    } else {
        git::get_default_branch_in(Some(repo_path)).context("Failed to determine the main branch")
    }
}

fn resolve_main_reference(
    kind: vcs::VcsKind,
    config: &config::Config,
    repo_path: &Path,
) -> Result<String> {
    match kind {
        vcs::VcsKind::Git => resolve_main_branch(config, repo_path),
        vcs::VcsKind::Sapling => Ok(config
            .main_branch
            .clone()
            .unwrap_or_else(|| ".".to_string())),
    }
}

pub fn resolve_configured_base_branch(
    config: &config::Config,
    repo_path: &Path,
) -> Result<Option<String>> {
    let Some(base) = config
        .base_branch
        .as_deref()
        .filter(|base| !base.trim().is_empty())
    else {
        return Ok(None);
    };

    if base == AUTO_BASE_BRANCH {
        let kind = vcs::detect_in(repo_path)?;
        resolve_main_reference(kind, config, repo_path).map(Some)
    } else {
        Ok(Some(base.to_string()))
    }
}

fn paths_identify_same_worktree(path: &Path, main_worktree_root: &Path) -> bool {
    match (path.canonicalize(), main_worktree_root.canonicalize()) {
        (Ok(canonical_path), Ok(canonical_main)) => canonical_path == canonical_main,
        _ => path == main_worktree_root,
    }
}

impl WorkflowContext {
    /// Create a new workflow context
    ///
    /// Performs the git repository check and gathers all commonly needed data.
    /// Does NOT check if multiplexer is running or change the current directory - those
    /// are optional operations that can be performed via helper methods.
    pub fn new(
        config: config::Config,
        mux: Arc<dyn Multiplexer>,
        config_location: Option<config::ConfigLocation>,
    ) -> Result<Self> {
        let execution_dir = std::env::current_dir().context("Failed to get current directory")?;
        Self::new_in(&execution_dir, config, mux, config_location)
    }

    /// Create a new workflow context for an explicit repository path
    pub fn new_in(
        repo_path: &Path,
        config: config::Config,
        mux: Arc<dyn Multiplexer>,
        config_location: Option<config::ConfigLocation>,
    ) -> Result<Self> {
        let execution_dir = repo_path.canonicalize().with_context(|| {
            format!(
                "Could not resolve repository path '{}'",
                repo_path.display()
            )
        })?;

        let vcs_kind = vcs::detect_in(&execution_dir)?;

        let main_worktree_root = vcs::main_worktree_root_in(vcs_kind, &execution_dir)
            .context("Could not find the main worktree")?;

        let git_common_dir = match vcs_kind {
            vcs::VcsKind::Git => git::get_git_common_dir_in(Some(&execution_dir))
                .context("Could not find the git common directory")?,
            // Kept for compatibility with deferred-cleanup data. Sapling cleanup
            // never interprets this path as a Git directory.
            vcs::VcsKind::Sapling => main_worktree_root.clone(),
        };

        let main_branch = resolve_main_reference(vcs_kind, &config, &execution_dir)?;

        let prefix = config.window_prefix().to_string();

        let (config_rel_dir, config_source_dir) = match config_location {
            Some(loc) => (loc.rel_dir, loc.config_dir),
            None => (PathBuf::new(), main_worktree_root.clone()),
        };

        debug!(
            execution_dir = %execution_dir.display(),
            vcs = vcs_kind.name(),
            main_worktree_root = %main_worktree_root.display(),
            git_common_dir = %git_common_dir.display(),
            main_branch = %main_branch,
            prefix = %prefix,
            backend = mux.name(),
            config_rel_dir = %config_rel_dir.display(),
            config_source_dir = %config_source_dir.display(),
            "workflow_context:created"
        );

        Ok(Self {
            vcs_kind,
            execution_dir,
            main_worktree_root,
            git_common_dir,
            main_branch,
            prefix,
            config,
            mux,
            config_rel_dir,
            config_source_dir,
        })
    }

    pub fn metadata_store(&self) -> Result<vcs::WorktreeMetadataStore> {
        vcs::WorktreeMetadataStore::new(self.vcs_kind, &self.main_worktree_root)
    }

    /// Read new state first and lazily import legacy Git-config metadata.
    pub fn get_worktree_meta(&self, handle: &str, key: &str) -> Option<String> {
        let store = self.metadata_store().ok()?;
        if let Some(value) = store.get(handle, key) {
            return Some(value);
        }
        if self.vcs_kind == vcs::VcsKind::Git
            && let Some(value) = git::get_worktree_meta_in(handle, key, Some(&self.execution_dir))
        {
            let _ = store.set(handle, key, &value);
            return Some(value);
        }
        None
    }

    pub fn set_worktree_meta(&self, handle: &str, key: &str, value: &str) -> Result<()> {
        self.metadata_store()?.set(handle, key, value)
    }

    pub fn all_worktree_meta(&self, key: &str) -> std::collections::HashMap<String, String> {
        let mut values = if self.vcs_kind == vcs::VcsKind::Git {
            git::get_all_worktree_meta_key_in(Some(&self.execution_dir), key)
        } else {
            std::collections::HashMap::new()
        };
        if let Ok(store) = self.metadata_store() {
            values.extend(store.get_all(key));
        }
        values
    }

    pub fn ensure_window_token(&self, handle: &str) -> Result<String> {
        if let Some(token) = self.get_worktree_meta(handle, "window-token") {
            return Ok(token);
        }
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).context("Failed to generate worktree window token")?;
        let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        self.set_worktree_meta(handle, "window-token", &token)?;
        Ok(token)
    }

    pub fn worktree_mode(&self, handle: &str) -> Option<config::MuxMode> {
        match self.get_worktree_meta(handle, "mode").as_deref() {
            Some("session") => Some(config::MuxMode::Session),
            Some("window") => Some(config::MuxMode::Window),
            _ => None,
        }
    }

    pub fn target_window(&self, handle: &str) -> Option<String> {
        self.get_worktree_meta(handle, "target-window")
    }

    pub fn target_session(&self, handle: &str) -> Option<String> {
        self.get_worktree_meta(handle, "target-session")
    }

    pub fn window_session(&self, handle: &str) -> Option<String> {
        self.get_worktree_meta(handle, "window-session")
    }

    pub fn window_token(&self, handle: &str) -> Option<String> {
        self.get_worktree_meta(handle, "window-token")
    }

    /// Return whether a path identifies the main worktree.
    pub fn is_main_worktree(&self, path: &Path) -> bool {
        paths_identify_same_worktree(path, &self.main_worktree_root)
    }

    /// Ensure the terminal multiplexer is running, returning an error if not
    ///
    /// Call this at the start of workflows that require a multiplexer.
    pub fn ensure_mux_running(&self) -> Result<()> {
        if !self.mux.is_running()? {
            return Err(anyhow!(
                "{} is not running. Please start a {} session first.",
                self.mux.name(),
                self.mux.name()
            ));
        }
        Ok(())
    }

    /// Ensure tmux is running (backward-compat alias for ensure_mux_running)
    #[deprecated(note = "Use ensure_mux_running() instead")]
    #[allow(dead_code)]
    pub fn ensure_tmux_running(&self) -> Result<()> {
        self.ensure_mux_running()
    }

    /// Change working directory to main worktree root
    ///
    /// This is necessary for destructive operations (merge, remove) to prevent
    /// "Unable to read current working directory" errors when the command is run
    /// from within a worktree that is about to be deleted.
    pub fn chdir_to_main_worktree(&self) -> Result<()> {
        debug!(
            safe_cwd = %self.main_worktree_root.display(),
            "workflow_context:changing to main worktree"
        );
        std::env::set_current_dir(&self.main_worktree_root).with_context(|| {
            format!(
                "Could not change directory to '{}'",
                self.main_worktree_root.display()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::paths_identify_same_worktree;
    use tempfile::tempdir;

    #[test]
    fn main_worktree_path_comparison_handles_existing_and_missing_paths() {
        let temp = tempdir().unwrap();
        let main = temp.path().join("main");
        let sibling = temp.path().join("sibling");
        std::fs::create_dir(&main).unwrap();
        std::fs::create_dir(&sibling).unwrap();

        assert!(paths_identify_same_worktree(&main, &main));
        assert!(!paths_identify_same_worktree(&sibling, &main));

        let missing = temp.path().join("missing");
        assert!(paths_identify_same_worktree(&missing, &missing));
        assert!(!paths_identify_same_worktree(
            &temp.path().join("other-missing"),
            &missing
        ));
    }

    #[cfg(unix)]
    #[test]
    fn main_worktree_path_comparison_resolves_symlinks() {
        let temp = tempdir().unwrap();
        let main = temp.path().join("main");
        let alias = temp.path().join("alias");
        std::fs::create_dir(&main).unwrap();
        std::os::unix::fs::symlink(&main, &alias).unwrap();

        assert!(paths_identify_same_worktree(&alias, &main));
    }
}
