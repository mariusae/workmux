use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Version-control implementation backing the current repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VcsKind {
    Git,
    Sapling,
}

impl VcsKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Git => "git",
            Self::Sapling => "sapling",
        }
    }
}

/// Backend-neutral description of a linked working copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// Git branch or Sapling revision/bookmark shown to the user.
    pub reference: String,
    /// Sapling's worktree label. Git derives the handle from the path.
    pub label: Option<String>,
    pub is_main: bool,
}

impl Worktree {
    pub fn handle(&self) -> String {
        self.label.clone().unwrap_or_else(|| {
            self.path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.reference.clone())
        })
    }
}

/// Detect the repository backend. Git deliberately wins for dot-git
/// repositories because Sapling can operate on those as a compatibility mode;
/// `sl worktree` itself is intended for EdenFS-backed Sapling repositories.
pub fn detect_in(path: &Path) -> Result<VcsKind> {
    if crate::git::is_git_repo_in(Some(path))? {
        return Ok(VcsKind::Git);
    }

    let root = Command::new("sl")
        .args(["root"])
        .current_dir(path)
        .output()
        .context("Failed to execute Sapling while detecting the repository backend")?;
    if !root.status.success() {
        return Err(anyhow!("Not in a Git or Sapling repository"));
    }

    let worktrees = Command::new("sl")
        .args(["worktree", "list", "-Tjson"])
        .current_dir(path)
        .output()
        .context("Failed to probe Sapling worktree support")?;
    if !worktrees.status.success() {
        return Err(anyhow!(
            "Sapling repository does not support linked worktrees: {}",
            String::from_utf8_lossy(&worktrees.stderr).trim()
        ));
    }

    Ok(VcsKind::Sapling)
}

pub fn detect() -> Result<VcsKind> {
    let cwd = std::env::current_dir().context("Failed to resolve current directory")?;
    detect_in(&cwd)
}
