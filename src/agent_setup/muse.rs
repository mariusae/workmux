//! Muse status tracking setup.
//!
//! Detects Muse via the `muse` executable or its config directory
//! (`$XDG_CONFIG_HOME/muse`, else `~/.config/muse`).
//! Installs hooks by merging into the muse `settings.json`.
//! Muse honors user `hooks` from that file using the same schema as
//! Claude Code (`SessionStart`, `UserPromptSubmit`, `PreToolUse`,
//! `PostToolUse`, `Stop`); there is no `Notification` event, so only
//! working/done states are tracked (no waiting state).

use anyhow::Result;
use serde_json::Value;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::StatusCheck;
use crate::agent_setup::json_config::{
    self, EmptyJsonRoot, JsonHookInstallSpec, JsonHookUninstallSpec,
};

/// Hooks configuration embedded at compile time.
const HOOKS_JSON: &str = include_str!("../../resources/muse/settings.json");

/// Resolve the muse config directory, honoring `XDG_CONFIG_HOME`.
pub fn muse_config_dir_with_env(
    home: &Path,
    get_env: impl Fn(&str) -> Option<OsString>,
) -> PathBuf {
    if let Some(xdg) = get_env("XDG_CONFIG_HOME").map(PathBuf::from) {
        return xdg.join("muse");
    }
    home.join(".config/muse")
}

fn muse_config_dir() -> Option<PathBuf> {
    home::home_dir().map(|home| muse_config_dir_with_env(&home, |key| std::env::var_os(key)))
}

fn settings_path() -> Option<PathBuf> {
    muse_config_dir().map(|d| d.join("settings.json"))
}

/// Detect if Muse is present via filesystem.
///
/// Checks the `muse` executable first, then falls back to the config
/// directory (which exists after first launch even when `muse` is not
/// on `PATH` in the current shell).
/// Returns the reason string if detected, None otherwise.
pub fn detect() -> Option<&'static str> {
    if which::which("muse").is_ok() {
        return Some("found muse executable");
    }

    if muse_config_dir().is_some_and(|d| d.is_dir()) {
        return Some("found muse config directory");
    }

    None
}

/// Check if workmux hooks are installed in muse settings.json.
pub fn check() -> Result<StatusCheck> {
    let Some(path) = settings_path() else {
        return Ok(StatusCheck::NotInstalled);
    };

    json_config::check_hook_file(
        &path,
        "Failed to read muse settings.json",
        "muse settings.json is not valid JSON",
    )
}

/// Remove workmux hooks from muse settings.json.
///
/// Uses shared JSON helpers to surgically remove only workmux entries,
/// preserving any user-configured hooks. Returns a description of what
/// was done.
pub fn uninstall() -> Result<String> {
    let Some(path) = settings_path() else {
        return Ok("Muse config dir not found, nothing to uninstall".to_string());
    };
    uninstall_at(path)
}

fn uninstall_at(path: PathBuf) -> Result<String> {
    json_config::json_hook_uninstall(
        &path,
        &JsonHookUninstallSpec {
            messages: json_config::JsonHookUninstallMessages {
                file_missing: "No muse settings.json found",
                not_found: "No workmux hooks found in muse settings",
                soft_read_error: None,
                soft_parse_error: None,
            },
            delete_if_no_hooks_remain: false,
            remove_plugins: false,
            soft_errors: false,
        },
    )
}

fn load_hooks() -> Result<Value> {
    json_config::hooks_from_embedded(HOOKS_JSON, "hooks config missing hooks key")
}

/// Install workmux hooks into the muse `settings.json`.
///
/// Merges hook groups into existing hooks without clobbering or creating
/// duplicates. Returns a description of what was done.
pub fn install() -> Result<String> {
    let path =
        settings_path().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;

    json_config::json_hook_install(
        &path,
        &load_hooks()?,
        &JsonHookInstallSpec {
            read_context: "Failed to read muse settings.json",
            parse_context: "muse settings.json is not valid JSON",
            write_context: "Failed to write muse settings.json",
            mkdir_context: "Failed to create muse config directory",
            empty_root: EmptyJsonRoot::Object,
        },
    )?;

    Ok(format!("Installed hooks to {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hooks_json_is_valid() {
        let parsed: serde_json::Value =
            serde_json::from_str(HOOKS_JSON).expect("embedded hooks config is valid JSON");
        let hooks = parsed.get("hooks").unwrap().as_object().unwrap();
        assert!(hooks.contains_key("UserPromptSubmit"));
        assert!(hooks.contains_key("PreToolUse"));
        assert!(hooks.contains_key("Stop"));
        // Muse has no Notification event: no waiting state.
        assert!(!hooks.contains_key("Notification"));
    }

    #[test]
    fn test_hooks_json_contains_workmux_command() {
        assert!(HOOKS_JSON.contains("workmux set-window-status"));
    }

    #[test]
    fn test_load_hooks() {
        let hooks = load_hooks().unwrap();
        let obj = hooks.as_object().unwrap();
        assert!(obj.contains_key("UserPromptSubmit"));
        assert!(obj.contains_key("PreToolUse"));
        assert!(obj.contains_key("Stop"));
    }

    #[test]
    fn test_muse_config_dir_defaults_to_home() {
        let dir = muse_config_dir_with_env(Path::new("/home/test"), |_| None);
        assert_eq!(dir, PathBuf::from("/home/test/.config/muse"));
    }

    #[test]
    fn test_muse_config_dir_respects_xdg() {
        let dir = muse_config_dir_with_env(Path::new("/home/test"), |key| {
            (key == "XDG_CONFIG_HOME").then(|| OsString::from("/tmp/xdg"))
        });
        assert_eq!(dir, PathBuf::from("/tmp/xdg/muse"));
    }

    #[test]
    fn test_uninstall_no_settings_file() {
        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.json");
        let result = uninstall_at(settings_path).unwrap();
        assert!(result.contains("No muse settings.json"));
    }

    #[test]
    fn test_uninstall_removes_hooks_only() {
        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.json");
        std::fs::write(
            &settings_path,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"workmux set-window-status done"}]},{"hooks":[{"type":"command","command":"my-hook"}]}]},"telemetry":{"enabled":true}}"#,
        )
        .unwrap();
        let result = uninstall_at(settings_path.clone()).unwrap();
        assert!(result.contains("Removed workmux hooks"));
        let content = std::fs::read_to_string(&settings_path).unwrap();
        let config: Value = serde_json::from_str(&content).unwrap();
        let stop = config["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 1);
        assert!(
            stop[0]["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .contains("my-hook")
        );
        // Unrelated settings survive.
        assert_eq!(config["telemetry"]["enabled"], true);
    }

    #[test]
    fn test_uninstall_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.json");
        std::fs::write(
            &settings_path,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"workmux set-window-status done"}]}]}}"#,
        )
        .unwrap();
        let result1 = uninstall_at(settings_path.clone()).unwrap();
        assert!(result1.contains("Removed workmux hooks"));
        let result2 = uninstall_at(settings_path).unwrap();
        assert!(result2.contains("No workmux hooks found"));
    }

    #[test]
    fn test_install_merges_without_clobbering() {
        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.json");
        std::fs::write(
            &settings_path,
            r#"{"telemetry":{"enabled":true},"hooks":{"Stop":[{"hooks":[{"type":"command","command":"my-hook"}]}]}}"#,
        )
        .unwrap();
        install_at_for_test(&settings_path);
        let content = std::fs::read_to_string(&settings_path).unwrap();
        let config: Value = serde_json::from_str(&content).unwrap();
        // Pre-existing user hook and settings survive.
        assert_eq!(config["telemetry"]["enabled"], true);
        let stop = config["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        // Workmux events installed.
        assert!(config["hooks"].get("UserPromptSubmit").is_some());
        assert!(config["hooks"].get("PreToolUse").is_some());
        // Idempotent: second install adds nothing.
        install_at_for_test(&settings_path);
        let content = std::fs::read_to_string(&settings_path).unwrap();
        let config: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(config["hooks"]["Stop"].as_array().unwrap().len(), 2);
    }

    fn install_at_for_test(path: &Path) {
        json_config::json_hook_install(
            path,
            &load_hooks().unwrap(),
            &JsonHookInstallSpec {
                read_context: "read",
                parse_context: "parse",
                write_context: "write",
                mkdir_context: "mkdir",
                empty_root: EmptyJsonRoot::Object,
            },
        )
        .unwrap();
    }
}
