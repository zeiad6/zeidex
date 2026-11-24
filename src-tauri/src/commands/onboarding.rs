//! The first-run guide's state.
//!
//! The guide is shown once per installation: it names the global shortcut, the
//! keys that open and move through results, the tray menu, and where the
//! settings that matter live. That is worth one interruption, and only one.
//!
//! "Seen" is a config key rather than a store row because the shell reads it
//! before the window is shown. A login launch with `start_minimized=true` must
//! still open the window on the very first run: an install that starts silent
//! is indistinguishable from one that failed to start.

use std::path::PathBuf;

use zeidex_engine::config::{self, RuntimeConfig, SettingValue};

fn config_file() -> Result<PathBuf, String> {
    zeidex_engine::config_path::current()
        .map(|resolved| resolved.path)
        .ok_or_else(|| "the Zeidex config folder is not available".to_string())
}

/// True once the guide has been dismissed. A missing key reads as "not seen",
/// which is the safe direction: the worst case is one extra welcome.
#[tauri::command(async)]
pub fn onboarding_status() -> bool {
    RuntimeConfig::load().onboarding_seen
}

/// Records that the guide was dismissed. Idempotent: a second call rewrites the
/// same `true` and nothing else.
#[tauri::command(async)]
pub fn onboarding_complete() -> Result<bool, String> {
    let path = config_file()?;
    mark_seen(&path)?;
    Ok(true)
}

fn mark_seen(path: &std::path::Path) -> Result<(), String> {
    let value = config::canonical_setting("onboarding_seen", SettingValue::Bool(true))?;
    config::write_settings(path, &[("onboarding_seen".to_string(), value)])?;
    // The shell reads the cached copy at startup; drop it so the same process
    // agrees with the file it just wrote.
    RuntimeConfig::invalidate_cache();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "zeidex-onboarding-{name}-{}-{}.config",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(0)
        ));
        path
    }

    #[test]
    fn a_missing_key_reads_as_not_seen() {
        let path = temp_path("missing");
        std::fs::write(&path, "start_minimized=true\n").expect("seed config");
        let config = RuntimeConfig::load_from(&path);
        assert!(!config.onboarding_seen);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn marking_seen_is_persisted_and_keeps_the_rest_of_the_file() {
        let path = temp_path("seen");
        std::fs::write(
            &path,
            "# kept\nstart_minimized=false\nonboarding_seen=false\n",
        )
        .expect("seed config");

        mark_seen(&path).expect("mark seen");

        let written = std::fs::read_to_string(&path).expect("read config");
        assert!(written.contains("# kept"));
        assert!(written.contains("start_minimized=false"));
        assert!(written.contains("onboarding_seen=true"));
        assert_eq!(written.matches("onboarding_seen=").count(), 1);

        let config = RuntimeConfig::load_from(&path);
        assert!(config.onboarding_seen);
        let _ = std::fs::remove_file(&path);
    }
}
