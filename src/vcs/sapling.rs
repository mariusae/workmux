use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::Worktree;

pub(super) fn has_uncommitted_changes(worktree: &Path) -> Result<bool> {
    has_uncommitted_changes_with_program(Path::new("sl"), worktree)
}

fn has_uncommitted_changes_with_program(program: &Path, worktree: &Path) -> Result<bool> {
    let output = Command::new(program)
        .arg("status")
        .current_dir(worktree)
        .output()
        .context("Failed to execute 'sl status'")?;
    if !output.status.success() {
        return Err(anyhow!(
            "Failed to inspect Sapling worktree '{}': {}",
            worktree.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(!output.stdout.is_empty())
}

pub(super) fn remove_worktree_in(path: &Path, workdir: &Path) -> Result<()> {
    remove_worktree_with_program(Path::new("sl"), path, workdir)
}

fn remove_worktree_with_program(program: &Path, path: &Path, workdir: &Path) -> Result<()> {
    let output = Command::new(program)
        .args(["worktree", "remove"])
        .arg(path)
        .arg("-y")
        .current_dir(workdir)
        .output()
        .context("Failed to execute 'sl worktree remove'")?;
    if !output.status.success() {
        return Err(anyhow!(
            "Failed to remove Sapling worktree '{}': {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

pub(super) fn label_worktree_in(path: &Path, label: &str, workdir: &Path) -> Result<()> {
    label_worktree_with_program(Path::new("sl"), path, label, workdir)
}

fn label_worktree_with_program(
    program: &Path,
    path: &Path,
    label: &str,
    workdir: &Path,
) -> Result<()> {
    let output = Command::new(program)
        .args(["worktree", "label"])
        .arg(path)
        .arg(label)
        .current_dir(workdir)
        .output()
        .context("Failed to execute 'sl worktree label'")?;
    if !output.status.success() {
        return Err(anyhow!(
            "Failed to label Sapling worktree '{}': {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

pub(super) fn create_worktree_in(
    path: &Path,
    label: &str,
    revision: Option<&str>,
    workdir: &Path,
) -> Result<()> {
    create_worktree_with_program(Path::new("sl"), path, label, revision, workdir)
}

fn create_worktree_with_program(
    program: &Path,
    path: &Path,
    label: &str,
    revision: Option<&str>,
    workdir: &Path,
) -> Result<()> {
    let mut command = Command::new(program);
    command
        .args(["worktree", "add"])
        .arg(path)
        .args(["--label", label]);
    if let Some(revision) = revision.filter(|revision| !revision.is_empty()) {
        command.args(["--rev", revision]);
    }

    let output = command
        .current_dir(workdir)
        .output()
        .context("Failed to execute 'sl worktree add'")?;
    if !output.status.success() {
        return Err(anyhow!(
            "Failed to create Sapling worktree '{}': {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

pub(super) fn list_worktrees_in(workdir: &Path) -> Result<Vec<Worktree>> {
    list_worktrees_with_program(Path::new("sl"), workdir)
}

fn list_worktrees_with_program(program: &Path, workdir: &Path) -> Result<Vec<Worktree>> {
    let output = Command::new(program)
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

    let mut worktrees = parse_worktree_list_json(&String::from_utf8(output.stdout)?)?;

    // Before the first linked worktree is added, Sapling reports a successful
    // but empty worktree list (and prints "this worktree is not part of a
    // group" in the human-readable format). Treat the current checkout as the
    // main worktree so `workmux add` can create and initialize the group.
    if worktrees.is_empty() {
        let output = Command::new(program)
            .arg("root")
            .current_dir(workdir)
            .output()
            .context("Failed to execute 'sl root'")?;
        if !output.status.success() {
            return Err(anyhow!(
                "Failed to find Sapling repository root: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        worktrees.push(Worktree {
            path: PathBuf::from(String::from_utf8(output.stdout)?.trim()),
            reference: ".".to_string(),
            label: None,
            is_main: true,
        });
    }

    Ok(worktrees)
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
                string_field(entry, &["role", "kind", "type"])
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
    use super::{
        create_worktree_with_program, has_uncommitted_changes_with_program,
        label_worktree_with_program, list_worktrees_with_program, parse_worktree_list_json,
        remove_worktree_with_program,
    };
    use std::path::Path;

    #[test]
    fn parses_sapling_worktree_json() {
        let worktrees = parse_worktree_list_json(
            r#"[
                {"path":"/repo/main","role":"main","current":true},
                {"path":"/repo/wt/feature","role":"linked","label":"feature","current":false}
            ]"#,
        )
        .unwrap();

        assert_eq!(worktrees.len(), 2);
        assert!(worktrees[0].is_main);
        assert_eq!(worktrees[1].handle(), "feature");
        assert_eq!(worktrees[1].reference, "feature");
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

    #[cfg(unix)]
    #[test]
    fn treats_ungrouped_checkout_as_main_worktree() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let program = temp.path().join("sl");
        let script = "#!/bin/sh\n\
                      if [ \"$*\" = 'worktree list -Tjson' ]; then\n\
                        printf '%s\\n' '[]'\n\
                      elif [ \"$*\" = 'root' ]; then\n\
                        printf '%s\\n' '/repo/main'\n\
                      fi\n";
        std::fs::write(&program, script).unwrap();
        let mut permissions = std::fs::metadata(&program).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&program, permissions).unwrap();

        let worktrees = list_worktrees_with_program(&program, temp.path()).unwrap();

        assert_eq!(worktrees.len(), 1);
        assert_eq!(worktrees[0].path, Path::new("/repo/main"));
        assert_eq!(worktrees[0].reference, ".");
        assert!(worktrees[0].is_main);
    }

    #[cfg(unix)]
    #[test]
    fn invokes_sapling_worktree_contract() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let program = temp.path().join("sl");
        let log = temp.path().join("commands.log");
        let script = format!(
            "#!/bin/sh\n\
             printf '%s\\n' \"$*\" >> '{}'\n\
             if [ \"$*\" = 'worktree list -Tjson' ]; then\n\
               printf '%s\\n' '[{{\"path\":\"/repo\",\"role\":\"main\",\"current\":true}},{{\"path\":\"/repo/wt/feature\",\"role\":\"linked\",\"label\":\"feature\",\"current\":false}}]'\n\
             elif [ \"$*\" = 'status' ]; then\n\
               printf '%s\\n' 'M file.txt'\n\
             fi\n",
            log.display()
        );
        std::fs::write(&program, script).unwrap();
        let mut permissions = std::fs::metadata(&program).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&program, permissions).unwrap();

        let destination = Path::new("/repo/wt/feature");
        create_worktree_with_program(
            &program,
            destination,
            "feature",
            Some("stable"),
            temp.path(),
        )
        .unwrap();
        let listed = list_worktrees_with_program(&program, temp.path()).unwrap();
        assert_eq!(listed[1].handle(), "feature");
        assert!(has_uncommitted_changes_with_program(&program, temp.path()).unwrap());
        label_worktree_with_program(&program, destination, "renamed", temp.path()).unwrap();
        remove_worktree_with_program(&program, destination, temp.path()).unwrap();

        assert_eq!(
            std::fs::read_to_string(log).unwrap(),
            "worktree add /repo/wt/feature --label feature --rev stable\n\
             worktree list -Tjson\n\
             status\n\
             worktree label /repo/wt/feature renamed\n\
             worktree remove /repo/wt/feature -y\n"
        );
    }
}
