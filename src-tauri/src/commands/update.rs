//! Release check against the project's public GitHub repository.
//!
//! This is the one outbound request the app makes on its own schedule. It sends
//! nothing about the user: no file names, no paths, no index counts, no
//! identifier. The response is the public release record, and the UI only ever
//! shows its tag, date and notes.
//!
//! The check is deliberately cheap and quiet: one HTTPS GET 15 seconds after
//! start (so it never competes with the first index walk), then every six
//! hours. A failure is reported to the caller that asked; the background check
//! simply stays silent, because "GitHub is unreachable" is not something to
//! interrupt a search for.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Public releases endpoint. A constant, never user input.
pub const RELEASES_API: &str = "https://api.github.com/repos/zeiad6/zeidex-search/releases/latest";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Delay before the first background check: long enough for the window and the
/// first walk to finish.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(15);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub available: bool,
    /// The release page, opened by the existing `open_url` command.
    pub url: String,
    pub notes: String,
    pub published_at: String,
    pub checked_at: i64,
    /// Why the check could not answer. `None` on success, including "up to
    /// date": an unreachable server is the only error state.
    pub error: Option<String>,
}

/// Compares dotted numeric versions. `0.4.0` vs `0.4.1` is the only shape the
/// project publishes; a tag that is not numeric is treated as "not newer"
/// rather than as an update, so a typo in a tag cannot nag every user.
pub fn is_newer(latest: &str, current: &str) -> bool {
    fn parts(value: &str) -> Option<Vec<u64>> {
        let trimmed = value.trim().trim_start_matches(['v', 'V']);
        let core = trimmed.split(['-', '+']).next().unwrap_or(trimmed);
        let mut numbers = Vec::new();
        for piece in core.split('.') {
            numbers.push(piece.parse::<u64>().ok()?);
        }
        (!numbers.is_empty()).then_some(numbers)
    }
    let (Some(latest), Some(current)) = (parts(latest), parts(current)) else {
        return false;
    };
    let width = latest.len().max(current.len());
    for index in 0..width {
        let left = latest.get(index).copied().unwrap_or(0);
        let right = current.get(index).copied().unwrap_or(0);
        if left != right {
            return left > right;
        }
    }
    false
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn failure(message: String) -> UpdateInfo {
    UpdateInfo {
        current: current_version(),
        latest: String::new(),
        available: false,
        url: String::new(),
        notes: String::new(),
        published_at: String::new(),
        checked_at: now(),
        error: Some(message),
    }
}

/// The request itself, free of Tauri so it can be exercised from a test or a
/// diagnostics command.
pub async fn fetch_latest() -> UpdateInfo {
    let client = match reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(format!("zeidex-search/{}", current_version()))
        .build()
    {
        Ok(client) => client,
        Err(err) => return failure(format!("could not create the HTTP client: {err}")),
    };
    let response = match client
        .get(RELEASES_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) => return failure(format!("could not reach GitHub: {err}")),
    };
    if !response.status().is_success() {
        return failure(format!(
            "GitHub answered {} for the latest release",
            response.status().as_u16()
        ));
    }
    let body = match response.text().await {
        Ok(body) => body,
        Err(err) => return failure(format!("could not read the release response: {err}")),
    };
    let json: serde_json::Value = match serde_json::from_str(&body) {
        Ok(json) => json,
        Err(err) => return failure(format!("the release response was not JSON: {err}")),
    };

    let tag = json["tag_name"].as_str().unwrap_or("").trim().to_string();
    if tag.is_empty() {
        return failure("the latest release has no tag".to_string());
    }
    let current = current_version();
    UpdateInfo {
        available: is_newer(&tag, &current),
        latest: tag,
        url: json["html_url"].as_str().unwrap_or("").to_string(),
        notes: json["body"].as_str().unwrap_or("").to_string(),
        published_at: json["published_at"].as_str().unwrap_or("").to_string(),
        checked_at: now(),
        current,
        error: None,
    }
}

/// The Settings button. Always answers, never fails the command: the UI shows
/// `error` instead of a toast that says nothing.
#[tauri::command(async)]
pub async fn check_for_updates() -> UpdateInfo {
    fetch_latest().await
}

/// Starts the quiet background check. Called once from `setup`.
pub fn spawn_background_check(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("update-check".to_string())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK_DELAY);
            loop {
                let info = tauri::async_runtime::block_on(fetch_latest());
                // Only a real update is worth an event. A failed check would
                // otherwise pop a notification every six hours on a machine
                // that is offline, which trains the user to ignore it.
                if info.available
                    && info.error.is_none()
                    && let Err(err) = app.emit("zeidex://update", info)
                {
                    eprintln!("zeidex: could not emit the update notice: {err}");
                }
                std::thread::sleep(CHECK_INTERVAL);
            }
        });
    if let Err(err) = spawned {
        eprintln!("zeidex: update check did not start: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn a_higher_patch_minor_or_major_is_an_update() {
        assert!(is_newer("v0.4.1", "0.4.0"));
        assert!(is_newer("0.5.0", "0.4.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(is_newer("0.4", "0.3.9"));
    }

    #[test]
    fn the_same_version_or_an_older_one_is_not_an_update() {
        assert!(!is_newer("v0.4.0", "0.4.0"));
        assert!(!is_newer("0.3.9", "0.4.0"));
        assert!(!is_newer("0.4.0", "0.4"));
    }

    #[test]
    fn a_non_numeric_tag_is_not_treated_as_an_update() {
        assert!(!is_newer("nightly", "0.4.0"));
        assert!(!is_newer("", "0.4.0"));
        assert!(!is_newer("v0.4.x", "0.4.0"));
    }

    #[test]
    fn a_prerelease_suffix_compares_on_its_numeric_core() {
        assert!(is_newer("v0.5.0-beta.1", "0.4.0"));
        assert!(!is_newer("v0.4.0-rc.1", "0.4.0"));
    }
}
