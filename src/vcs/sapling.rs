use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::Worktree;

pub(super) fn list_worktrees_in(workdir: &Path) -> Result<Vec<Worktree>> {
    let output = Command::new("sl")
        .args(["worktree", "list", "-Tjson"])
        .current_dir(workdir)
        .output()
        .context("Failed to execute 'sl worktree list'")?;
    if !output.status.success() {
        return Err(anyhow!(
            "Failed to list Sapling worktrees: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    parse_worktree_list_json(&String::from_utf8(output.stdout)?)
}

fn string_field<'a>(value: &'a Value, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|name| value.get(name)?.as_str())
}

fn bool_field(value: &Value, names: &[&str]) -> Option<bool> {
    names.iter().find_map(|name| value.get(name)?.as_bool())
}

pub(super) fn parse_worktree_list_json(output: &str) -> Result<Vec<Worktree>> {
    let value: Value = serde_json::from_str(output).context("Invalid Sapling worktree JSON")?;
    let entries = value
        .as_array()
        .ok_or_else(|| anyhow!("Sapling worktree list did not return a JSON array"))?;

    let mut worktrees = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let path = string_field(entry, &["path", "worktree_path", "root"])
            .ok_or_else(|| anyhow!("Sapling worktree entry is missing its path"))?;
        let label = string_field(entry, &["label", "name"])
            .filter(|label| !label.is_empty())
            .map(str::to_string);
        let reference = string_field(entry, &["bookmark", "revision", "rev", "node"])
            .filter(|reference| !reference.is_empty())
            .map(str::to_string)
            .or_else(|| label.clone())
            .unwrap_or_else(|| "(detached)".to_string());
        let is_main = bool_field(entry, &["is_main", "main"])
            .or_else(|| {
                string_field(entry, &["kind", "type"])
                    .map(|kind| matches!(kind, "main" | "primary"))
            })
            // Sapling documents the designated main worktree first. Retain
            // that ordering fallback for versions whose JSON omits a flag.
            .unwrap_or(index == 0);

        worktrees.push(Worktree {
            path: PathBuf::from(path),
            reference,
            label,
            is_main,
        });
    }

    Ok(worktrees)
}

#[cfg(test)]
mod tests {
    use super::parse_worktree_list_json;

    #[test]
    fn parses_sapling_worktree_json() {
        let worktrees = parse_worktree_list_json(
            r#"[
                {"path":"/repo/main","label":"","is_main":true,"revision":"abc"},
                {"path":"/repo/wt/feature","label":"feature","is_main":false,"revision":"def"}
            ]"#,
        )
        .unwrap();

        assert_eq!(worktrees.len(), 2);
        assert!(worktrees[0].is_main);
        assert_eq!(worktrees[1].handle(), "feature");
        assert_eq!(worktrees[1].reference, "def");
    }

    #[test]
    fn accepts_legacy_minimal_json_shape() {
        let worktrees = parse_worktree_list_json(
            r#"[{"worktree_path":"/repo/main"},{"worktree_path":"/repo/wt/a","name":"a"}]"#,
        )
        .unwrap();

        assert!(worktrees[0].is_main);
        assert_eq!(worktrees[1].handle(), "a");
    }
}
