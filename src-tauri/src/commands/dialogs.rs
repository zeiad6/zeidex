//! Native file dialogs and the safe file replacement every export shares.
//!
//! The picker runs here rather than in the webview. A command that takes a path
//! from the page is a command the page can point anywhere, and `save()` followed
//! by `export_*(path)` means a script in the webview can overwrite a file the
//! user never chose. Opening the dialog in Rust binds the write to a path the
//! user just picked in a native window, so the export commands need no fs-plugin
//! scope at all.
//!
//! Writes go to a sibling staging file and are renamed into place, so a failure
//! halfway through an export cannot truncate the workbook or document that was
//! already there.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

use super::policy::PathPolicy;

/// The window that owns every native dialog this module opens.
///
/// An unowned dialog is a separate top-level window on Windows: the OS has no
/// reason to keep it above the application that asked for it, which is how a
/// picker ends up behind Zeidex. `set_parent` passes the main window handle to
/// the platform dialog, making it modal to that window and keeping it in front.
/// This replaces the old workaround of forcing the launcher always-on-top,
/// which only made the ordering worse for the dialog.
fn dialog_parent(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    app.get_webview_window("main")
}

/// Puts keyboard focus back on the main window after a native dialog closes.
///
/// On Windows the dialog is a separate top-level window owned by the app. When
/// it closes, the foreground window can stay with the desktop or the previous
/// application, so the next keystroke goes somewhere else and the launcher
/// looks frozen. Re-activating the owner is the supported fix; the dialog is
/// already parented, so this only restores the focus the dialog took.
fn restore_focus(app: &AppHandle) {
    if let Some(window) = dialog_parent(app) {
        let _ = window.set_focus();
    }
}

/// Opens a single-file picker and returns the chosen path.
///
/// `title` and `filter_name` are cosmetic and come from the UI so the dialog
/// speaks the user's language. The extensions are fixed by the caller in Rust:
/// the page may choose the wording, never what the picker will hand back.
///
/// The chosen path is minted into `policy` here, not at the call site, so a new
/// picker cannot forget to: the dialog is the only way a path outside the index
/// becomes readable.
///
/// `Ok(None)` means the user dismissed the dialog, which is not an error.
pub fn pick_file(
    app: &AppHandle,
    policy: &PathPolicy,
    title: Option<String>,
    filter_name: Option<String>,
    extensions: &[&str],
) -> Result<Option<PathBuf>, String> {
    let Some(picked) = builder(app, title, filter_name, extensions).blocking_pick_file() else {
        restore_focus(app);
        return Ok(None);
    };
    let path = picked.into_path().map_err(|err| err.to_string())?;
    policy.mint(&path);
    restore_focus(app);
    Ok(Some(path))
}

/// Opens a folder picker and returns the chosen directory, minted.
///
/// Used by the Settings Center's "add a scan folder". The dialog runs here for
/// the same reason the file picker does: the page asks for a folder, the user
/// chooses it, and only then is the path readable.
pub fn pick_folder(
    app: &AppHandle,
    policy: &PathPolicy,
    title: Option<String>,
) -> Result<Option<PathBuf>, String> {
    let mut builder = app.dialog().file();
    if let Some(window) = dialog_parent(app) {
        builder = builder.set_parent(&window);
    }
    if let Some(title) = title.filter(|value| !value.trim().is_empty()) {
        builder = builder.set_title(title);
    }
    let Some(picked) = builder.blocking_pick_folder() else {
        restore_focus(app);
        return Ok(None);
    };
    let path = picked.into_path().map_err(|err| err.to_string())?;
    policy.mint(&path);
    restore_focus(app);
    Ok(Some(path))
}

/// Opens a save dialog and returns the path to write.
///
/// The suggested name is sanitised and given `extension` before it reaches the
/// dialog, so a name from the UI cannot steer the dialog into another directory
/// and a workbook is never saved without the extension its filter promises.
pub fn save_file(
    app: &AppHandle,
    title: Option<String>,
    filter_name: Option<String>,
    extension: &str,
    default_name: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    let suggested = safe_default_name(default_name.unwrap_or_default(), extension);
    let Some(picked) = builder(app, title, filter_name, &[extension])
        .set_file_name(suggested)
        .blocking_save_file()
    else {
        restore_focus(app);
        return Ok(None);
    };
    let path = picked.into_path().map_err(|err| err.to_string())?;
    restore_focus(app);
    Ok(Some(with_extension(path, extension)))
}

fn builder(
    app: &AppHandle,
    title: Option<String>,
    filter_name: Option<String>,
    extensions: &[&str],
) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
    let mut builder = app.dialog().file();
    if let Some(window) = dialog_parent(app) {
        builder = builder.set_parent(&window);
    }
    if let Some(title) = title.filter(|value| !value.trim().is_empty()) {
        builder = builder.set_title(title);
    }
    builder.add_filter(
        filter_name.unwrap_or_else(|| "Files".to_string()),
        extensions,
    )
}

/// Writes through a sibling staging file and renames it over the target.
///
/// The rename is atomic on one filesystem, so the user either keeps the file
/// they had or gets the new one — never a half-written workbook.
pub fn write_atomically(
    target: &Path,
    write: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let staging = staging_path(target);
    if let Err(err) = write(&staging) {
        let _ = std::fs::remove_file(&staging);
        return Err(err);
    }
    if let Err(err) = std::fs::rename(&staging, target) {
        let _ = std::fs::remove_file(&staging);
        return Err(format!(
            "the file was built but could not be saved to {}: {err}",
            target.display()
        ));
    }
    Ok(())
}

/// `path`, given `extension` if it does not already end with it.
///
/// The save dialog appends the filter's extension on Windows and macOS but not
/// on every Linux desktop, and a workbook called `report` is one the file
/// manager cannot open by double-clicking.
pub fn with_extension(path: PathBuf, extension: &str) -> PathBuf {
    let wanted = format!(".{extension}");
    if path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().to_lowercase().ends_with(&wanted))
    {
        return path;
    }
    let mut name = path.into_os_string();
    name.push(wanted);
    PathBuf::from(name)
}

/// A name a save dialog may suggest: the last path component of `name`, with
/// `extension` and a fallback if nothing usable is left.
///
/// The name comes from the UI, so it must not be able to place the dialog in a
/// directory of its choosing (`../../.bashrc`) or suggest an empty name.
pub fn safe_default_name(name: &str, extension: &str) -> String {
    let stem = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim()
        .trim_start_matches('.')
        .to_string();
    if stem.is_empty() {
        return format!("zeidex.{extension}");
    }
    with_extension(PathBuf::from(stem), extension)
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("zeidex.{extension}"))
}

/// A sibling of `target` to write first, so the rename stays on one filesystem
/// and a crash mid-export cannot leave a half-written file in its place.
fn staging_path(target: &Path) -> PathBuf {
    let mut name = target
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(format!(".{}.part", std::process::id()));
    target.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::{safe_default_name, staging_path, with_extension};
    use std::path::{Path, PathBuf};

    #[test]
    fn a_report_name_gets_an_extension_only_when_it_has_none() {
        assert_eq!(
            with_extension(PathBuf::from("/tmp/report"), "xlsx"),
            PathBuf::from("/tmp/report.xlsx")
        );
        assert_eq!(
            with_extension(PathBuf::from("/tmp/report.xlsx"), "xlsx"),
            PathBuf::from("/tmp/report.xlsx")
        );
        // Some desktops return the name in whatever case the user typed.
        assert_eq!(
            with_extension(PathBuf::from("/tmp/REPORT.XLSX"), "xlsx"),
            PathBuf::from("/tmp/REPORT.XLSX")
        );
        // A dotfile has no extension, so it keeps its name and gains one.
        assert_eq!(
            with_extension(PathBuf::from("/tmp/.hidden"), "xlsx"),
            PathBuf::from("/tmp/.hidden.xlsx")
        );
    }

    #[test]
    fn a_suggested_name_cannot_choose_its_own_directory() {
        assert_eq!(safe_default_name("zeidex.docx", "docx"), "zeidex.docx");
        assert_eq!(
            safe_default_name("../../.bashrc", "docx"),
            "bashrc.docx",
            "a suggested name must be a file name, not a path"
        );
        assert_eq!(safe_default_name("", "xlsx"), "zeidex.xlsx");
        assert_eq!(safe_default_name("   ", "xlsx"), "zeidex.xlsx");
        assert_eq!(safe_default_name("..", "xlsx"), "zeidex.xlsx");
        assert_eq!(safe_default_name("تقرير", "docx"), "تقرير.docx");
        // A name with the wrong extension gets the right one rather than both.
        assert_eq!(safe_default_name("report.xlsx", "docx"), "report.xlsx.docx");
    }

    #[test]
    fn the_staging_file_sits_beside_the_target() {
        let target = Path::new("/home/user/report.xlsx");
        let staging = staging_path(target);
        assert_eq!(staging.parent(), target.parent());
        assert_ne!(staging, target);
        assert!(
            staging
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".part")),
            "a half-written workbook must not look like the real one: {staging:?}"
        );
    }
}
