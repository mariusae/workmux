use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

mod metadata;
mod sapling;

pub use metadata::WorktreeMetadataStore;

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

/// List linked working copies using the selected backend.
pub fn list_worktrees_in(kind: VcsKind, workdir: &Path) -> Result<Vec<Worktree>> {
    match kind {
        VcsKind::Git => {
            let entries = crate::git::list_worktrees_in(Some(workdir))?;
            let main_path = crate::git::get_main_worktree_root_in(Some(workdir))?;
            Ok(entries
                .into_iter()
                .map(|(path, reference)| Worktree {
                    is_main: paths_equal(&path, &main_path),
                    path,
                    reference,
                    label: None,
                })
                .collect())
        }
        VcsKind::Sapling => sapling::list_worktrees_in(workdir),
    }
}

pub fn main_worktree_root_in(kind: VcsKind, workdir: &Path) -> Result<PathBuf> {
    list_worktrees_in(kind, workdir)?
        .into_iter()
        .find(|worktree| worktree.is_main)
        .map(|worktree| worktree.path)
        .ok_or_else(|| anyhow!("No main {} worktree found", kind.name()))
}

/// Find a worktree by handle/label first, then by its displayed reference.
pub fn find_worktree_in(kind: VcsKind, name: &str, workdir: &Path) -> Result<Worktree> {
    let worktrees = list_worktrees_in(kind, workdir)?;
    worktrees
        .iter()
        .find(|worktree| worktree.handle() == name)
        .or_else(|| worktrees.iter().find(|worktree| worktree.reference == name))
        .cloned()
        .ok_or_else(|| crate::git::WorktreeNotFound(name.to_string()).into())
}

pub struct CreateWorktree<'a> {
    pub path: &'a Path,
    pub handle: &'a str,
    /// Git branch name. Sapling uses this only as a fallback revision.
    pub reference: &'a str,
    pub create_reference: bool,
    pub base: Option<&'a str>,
    pub track_upstream: bool,
}

pub fn create_worktree_in(
    kind: VcsKind,
    request: &CreateWorktree<'_>,
    workdir: &Path,
) -> Result<()> {
    match kind {
        VcsKind::Git => crate::git::create_worktree_in(
            request.path,
            request.reference,
            request.create_reference,
            request.base,
            request.track_upstream,
            Some(workdir),
        ),
        VcsKind::Sapling => sapling::create_worktree_in(
            request.path,
            request.handle,
            request.base.or(Some(".")),
            workdir,
        ),
    }
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}
