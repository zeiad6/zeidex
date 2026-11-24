//! Local volumes, so the Drives screen offers "index this disk" instead of
//! asking the user to type a root by hand.
//!
//! The list is read from the platform (`sysinfo`), not from the config: a drive
//! that was unplugged must disappear from the screen, and a drive that was
//! plugged in must appear without a restart. Whether a drive is *indexed* is a
//! separate answer, taken from the configured scan roots, so the toggle shows
//! what the walker actually does rather than what the user once clicked.

use serde::Serialize;
use sysinfo::Disks;
use tauri::State;

use crate::state::AppState;
use zeidex_engine::config::RuntimeConfig;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveInfo {
    /// The mount point as the walker would see it (`C:\`, `/`, `/Volumes/X`).
    pub root: String,
    /// The volume label, or the root when the platform exposes none.
    pub label: String,
    pub removable: bool,
    /// `ssd`, `hdd` or `unknown`; presentation only.
    pub kind: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// True when this drive root is itself a configured scan root, i.e. the
    /// whole disk is indexed, not just a folder on it.
    pub scan_root: bool,
}

/// Normalises a mount point so a drive can be compared with a configured root.
/// Windows reports `C:\` with a trailing separator and `C:` without; both name
/// the same volume.
fn normalized_root(root: &str) -> String {
    let trimmed = root.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        // The POSIX root is the one path whose separator is the whole path.
        root.to_string()
    } else {
        trimmed.to_string()
    }
}

fn same_root(left: &str, right: &str) -> bool {
    let left = normalized_root(left);
    let right = normalized_root(right);
    if cfg!(target_os = "windows") {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}

/// Reads the volumes and marks the ones the walker already covers.
///
/// A failure to enumerate disks is not an error the UI can act on: an empty
/// list says "the platform would not tell us", and the Settings screen still
/// has the folder picker.
#[tauri::command(async)]
pub fn system_drives(_state: State<'_, AppState>) -> Vec<DriveInfo> {
    let config = RuntimeConfig::load_cached();
    let roots = config.file_scan_root_list();
    let Ok(disks) = Disks::new_with_refreshed_list() else {
        return Vec::new();
    };

    let mut drives: Vec<DriveInfo> = disks
        .iter()
        .map(|disk| {
            let root = disk.mount_point().to_string_lossy().into_owned();
            let label = {
                let name = disk.name().to_string_lossy().trim().to_string();
                if name.is_empty() { root.clone() } else { name }
            };
            let kind = match disk.kind() {
                sysinfo::DiskKind::SSD => "ssd",
                sysinfo::DiskKind::HDD => "hdd",
                _ => "unknown",
            };
            // Whole-disk indexing means the drive root itself is a configured
            // root. A folder that merely lives on the drive (the default home
            // roots) does not make the disk indexed, so `C:\Users\me\Desktop`
            // must not light up the C: toggle.
            let scan_root = roots.iter().any(|configured| same_root(configured, &root));
            DriveInfo {
                label,
                removable: disk.is_removable(),
                kind: kind.to_string(),
                total_bytes: disk.total_space(),
                free_bytes: disk.available_space(),
                scan_root,
                root,
            }
        })
        .collect();

    // Stable order: system volume first, then the rest by path. `Disks` makes no
    // ordering promise and a list that reshuffles between visits reads as a bug.
    drives.sort_by(|left, right| {
        let system = |drive: &DriveInfo| !drive.removable && drive.root.len() <= 3;
        system(right)
            .cmp(&system(left))
            .then_with(|| left.root.cmp(&right.root))
    });
    drives
}

#[cfg(test)]
mod tests {
    use super::{normalized_root, same_root};

    #[test]
    fn a_trailing_separator_does_not_make_two_names_for_one_volume() {
        assert_eq!(normalized_root(r"C:\"), r"C:");
        assert_eq!(normalized_root("/"), "/");
        assert_eq!(normalized_root("/home/"), "/home");
        assert!(same_root(r"C:\", r"C:"));
        assert!(same_root("/home", "/home/"));
        assert!(!same_root("/home", "/home/other"));
    }
}
