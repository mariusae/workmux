use anyhow::{Context, Result};
use nix::fcntl::{Flock, FlockArg};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use super::VcsKind;

#[derive(Debug, Default, Serialize, Deserialize)]
struct RepositoryMetadata {
    repository: PathBuf,
    vcs: Option<VcsKind>,
    #[serde(default)]
    worktrees: HashMap<String, HashMap<String, String>>,
}

/// Repository-scoped metadata for values that used to live exclusively in
/// `.git/config`. The file is shared by Git and Sapling backends so workflow
/// code does not need to know how repository metadata is represented.
pub struct WorktreeMetadataStore {
    repository: PathBuf,
    vcs: VcsKind,
    path: PathBuf,
    lock_path: PathBuf,
}

impl WorktreeMetadataStore {
    pub fn new(vcs: VcsKind, repository: &Path) -> Result<Self> {
        let base = crate::xdg::state_dir()?.join("repositories");
        Self::with_base(vcs, repository, &base)
    }

    fn with_base(vcs: VcsKind, repository: &Path, base: &Path) -> Result<Self> {
        fs::create_dir_all(base).context("Failed to create repository state directory")?;
        let canonical = repository
            .canonicalize()
            .unwrap_or_else(|_| repository.to_path_buf());
        let id = stable_path_hash(&canonical);
        Ok(Self {
            repository: canonical,
            vcs,
            path: base.join(format!("{id:016x}.json")),
            lock_path: base.join(format!("{id:016x}.lock")),
        })
    }

    fn lock(&self) -> Result<Flock<File>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&self.lock_path)
            .with_context(|| {
                format!("Failed to open metadata lock: {}", self.lock_path.display())
            })?;
        Flock::lock(file, FlockArg::LockExclusive)
            .map_err(|(_file, errno)| errno)
            .context("Failed to lock worktree metadata")
    }

    fn load(&self) -> Result<RepositoryMetadata> {
        let content = match fs::read_to_string(&self.path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RepositoryMetadata {
                    repository: self.repository.clone(),
                    vcs: Some(self.vcs),
                    worktrees: HashMap::new(),
                });
            }
            Err(error) => return Err(error).context("Failed to read worktree metadata"),
        };
        serde_json::from_str(&content).context("Invalid worktree metadata")
    }

    fn update(&self, change: impl FnOnce(&mut RepositoryMetadata)) -> Result<()> {
        let _lock = self.lock()?;
        let mut metadata = self.load()?;
        metadata.repository = self.repository.clone();
        metadata.vcs = Some(self.vcs);
        change(&mut metadata);
        let content = serde_json::to_vec_pretty(&metadata)?;
        crate::state::write_atomic(&self.path, &content)
    }

    pub fn get(&self, handle: &str, key: &str) -> Option<String> {
        self.load().ok()?.worktrees.get(handle)?.get(key).cloned()
    }

    pub fn get_all(&self, key: &str) -> HashMap<String, String> {
        self.load()
            .map(|metadata| {
                metadata
                    .worktrees
                    .into_iter()
                    .filter_map(|(handle, values)| {
                        values.get(key).cloned().map(|value| (handle, value))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn set(&self, handle: &str, key: &str, value: &str) -> Result<()> {
        self.update(|metadata| {
            metadata
                .worktrees
                .entry(handle.to_string())
                .or_default()
                .insert(key.to_string(), value.to_string());
        })
    }

    pub fn remove(&self, handle: &str) -> Result<()> {
        self.update(|metadata| {
            metadata.worktrees.remove(handle);
        })
    }

    pub fn migrate(&self, old_handle: &str, new_handle: &str) -> Result<()> {
        if old_handle == new_handle {
            return Ok(());
        }
        self.update(|metadata| {
            if let Some(values) = metadata.worktrees.remove(old_handle) {
                metadata
                    .worktrees
                    .entry(new_handle.to_string())
                    .or_default()
                    .extend(values);
            }
        })
    }
}

fn stable_path_hash(path: &Path) -> u64 {
    // FNV-1a keeps repository filenames compact without adding another hash
    // dependency. The canonical path is retained inside the JSON for diagnosis.
    path.as_os_str()
        .to_string_lossy()
        .bytes()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
}

#[cfg(test)]
mod tests {
    use super::WorktreeMetadataStore;
    use crate::vcs::VcsKind;

    #[test]
    fn stores_and_migrates_worktree_values() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        let state = temp.path().join("state");
        std::fs::create_dir(&repo).unwrap();
        let store = WorktreeMetadataStore::with_base(VcsKind::Sapling, &repo, &state).unwrap();

        store.set("feature", "mode", "session").unwrap();
        store.set("feature", "base", "main").unwrap();
        assert_eq!(store.get("feature", "mode").as_deref(), Some("session"));
        assert_eq!(store.get_all("base")["feature"], "main");

        store.migrate("feature", "renamed").unwrap();
        assert_eq!(store.get("feature", "mode"), None);
        assert_eq!(store.get("renamed", "mode").as_deref(), Some("session"));

        store.remove("renamed").unwrap();
        assert_eq!(store.get("renamed", "mode"), None);
    }
}
