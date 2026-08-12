use crate::vcs;
use anyhow::{Result, anyhow};

pub fn run(name: &str) -> Result<()> {
    // Smart resolution: try handle first, then branch name
    let cwd = std::env::current_dir()?;
    let kind = vcs::detect_in(&cwd)?;
    let worktree = vcs::find_worktree_in(kind, name, &cwd).map_err(|_| {
        anyhow!(
            "Worktree '{}' not found. Use 'workmux list' to see available worktrees.",
            name
        )
    })?;
    println!("{}", worktree.path.display());
    Ok(())
}
