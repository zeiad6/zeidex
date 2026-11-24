//! Launch at login.
//!
//! This is an operating-system fact, not a value in `~/.zeidex/config`: the
//! truth is the registry value, the XDG desktop entry or the LaunchAgent on
//! disk. The UI reads it back after every write, so a policy that silently
//! removes the entry shows up as "inactive" instead of a switch that lies.
//!
//! The app starts hidden (see `tauri.conf.json`), which is what makes this
//! pleasant: signing in puts the launcher in the tray, one shortcut away,
//! instead of throwing a window in front of whatever the user is doing.
//!
//! Nothing here installs a service or a second executable. On Windows the entry
//! is `HKCU\...\Run`, on Linux an XDG autostart file, on macOS a user
//! LaunchAgent. Disabling removes exactly the entry this app created.

use std::path::Path;

use serde::Serialize;

/// The value name under the Windows `Run` key. A constant, never user input.
#[allow(dead_code)] // Used on Windows; compiled everywhere so the shape is checked.
const WINDOWS_RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[allow(dead_code)] // Used on Windows; compiled everywhere so the shape is checked.
const WINDOWS_VALUE: &str = "Zeidex";
/// Reverse-DNS label of the macOS LaunchAgent; also the plist file name.
const MAC_LABEL: &str = "com.zeidexsoft.zeidex";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutostartStatus {
    pub supported: bool,
    pub enabled: bool,
    /// Human label for the mechanism ("Windows registry", "XDG autostart", …).
    pub mechanism: String,
    /// The registry key or file the entry lives in, when there is one.
    pub location: Option<String>,
    /// The executable the entry points at.
    pub command: String,
    /// A debug build points at `target/debug`; enabling it is a trap.
    pub development: bool,
    pub error: Option<String>,
}

#[tauri::command(async)]
pub fn autostart_status() -> AutostartStatus {
    let exe = std::env::current_exe().ok();
    platform::status(exe.as_deref())
}

/// Adds or removes the login entry and returns the state read back from the OS.
///
/// A write that "succeeds" but leaves no entry (group policy, a read-only home)
/// is reported as the state it actually has, not as the state we asked for.
#[tauri::command(async)]
pub fn autostart_set(enabled: bool) -> Result<AutostartStatus, String> {
    let exe = std::env::current_exe()
        .map_err(|err| format!("could not resolve the Zeidex executable: {err}"))?;
    if !exe.is_file() {
        return Err(format!(
            "the Zeidex executable is missing: {}",
            exe.display()
        ));
    }
    platform::set(enabled, &exe)?;
    Ok(platform::status(Some(&exe)))
}

fn status_with(
    exe: Option<&Path>,
    mechanism: &str,
    location: Option<String>,
    enabled: bool,
    error: Option<String>,
) -> AutostartStatus {
    AutostartStatus {
        supported: true,
        enabled,
        mechanism: mechanism.to_string(),
        location,
        command: exe
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        development: cfg!(debug_assertions),
        error,
    }
}

/// The `Run` value: the executable, quoted so a path with spaces stays one
/// argument, followed by `--autostart` so a login start can stay in the tray
/// while a normal double-click shows the window. A Windows path cannot contain a
/// double quote.
#[allow(dead_code)] // Used on Windows; tested on every host.
fn windows_value(exe: &Path) -> String {
    format!("\"{}\" --autostart", exe.display())
}

/// An XDG desktop entry. `Exec` follows the Desktop Entry spec: always quoted,
/// with the four characters the spec reserves inside quotes escaped.
#[allow(dead_code)] // Used on Linux; tested on every host.
fn desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name=Zeidex Search\n\
         Comment=Offline launcher and document OCR studio\n\
         Exec={} --autostart\n\
         Icon=zeidex\n\
         Terminal=false\n\
         StartupNotify=false\n\
         X-GNOME-Autostart-enabled=true\n",
        desktop_quote(exe)
    )
}

#[allow(dead_code)] // Used on Linux; tested on every host.
fn desktop_quote(exe: &Path) -> String {
    let raw = exe.to_string_lossy();
    let mut quoted = String::with_capacity(raw.len() + 2);
    quoted.push('"');
    for character in raw.chars() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '`' => quoted.push_str("\\`"),
            '$' => quoted.push_str("\\$"),
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

/// A per-user LaunchAgent. It runs at login without an installer or an admin
/// prompt, and removing the file removes the behaviour.
#[allow(dead_code)] // Used on macOS; tested on every host.
fn launch_agent_plist(exe: &Path) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \x20 <key>Label</key>\n\
         \x20 <string>{MAC_LABEL}</string>\n\
         \x20 <key>ProgramArguments</key>\n\
         \x20 <array>\n\
         \x20   <string>{}</string>\n\
         \x20   <string>--autostart</string>\n\
         \x20 </array>\n\
         \x20 <key>RunAtLoad</key>\n\
         \x20 <true/>\n\
         \x20 <key>ProcessType</key>\n\
         \x20 <string>Interactive</string>\n\
         </dict>\n\
         </plist>\n",
        xml_escape(&exe.to_string_lossy())
    )
}

#[allow(dead_code)] // Used on macOS; tested on every host.
fn xml_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(target_os = "windows")]
mod platform {
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::Output;

    use super::{AutostartStatus, WINDOWS_RUN_KEY, WINDOWS_VALUE, status_with, windows_value};

    /// `reg.exe` is a console program. Spawning it from a GUI build without
    /// this flag flashes a console window for every toggle.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn run(args: &[&str]) -> Result<Output, String> {
        std::process::Command::new("reg")
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|err| format!("could not run reg.exe: {err}"))
    }

    fn query_enabled() -> Result<bool, String> {
        let output = run(&["query", WINDOWS_RUN_KEY, "/v", WINDOWS_VALUE])?;
        if output.status.success() {
            // `reg query` can succeed with an empty value list on some builds;
            // the name in the output is the proof the value exists.
            return Ok(String::from_utf8_lossy(&output.stdout).contains(WINDOWS_VALUE));
        }
        // 1 is "the value or key was not found", which is a valid "off".
        if output.status.code() == Some(1) {
            return Ok(false);
        }
        Err(format!(
            "reg query failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }

    pub fn status(exe: Option<&Path>) -> AutostartStatus {
        match query_enabled() {
            Ok(enabled) => status_with(
                exe,
                "Windows registry",
                Some(WINDOWS_RUN_KEY.to_string()),
                enabled,
                None,
            ),
            Err(err) => status_with(
                exe,
                "Windows registry",
                Some(WINDOWS_RUN_KEY.to_string()),
                false,
                Some(err),
            ),
        }
    }

    pub fn set(enabled: bool, exe: &Path) -> Result<(), String> {
        if enabled {
            let value = windows_value(exe);
            let output = run(&[
                "add",
                WINDOWS_RUN_KEY,
                "/v",
                WINDOWS_VALUE,
                "/t",
                "REG_SZ",
                "/d",
                &value,
                "/f",
            ])?;
            if output.status.success() {
                return Ok(());
            }
            return Err(format!(
                "could not add the startup entry: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        let output = run(&["delete", WINDOWS_RUN_KEY, "/v", WINDOWS_VALUE, "/f"])?;
        // 1 means the value was already absent, which is the state asked for.
        if output.status.success() || output.status.code() == Some(1) {
            return Ok(());
        }
        Err(format!(
            "could not remove the startup entry: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{AutostartStatus, desktop_entry, status_with};

    fn desktop_path() -> Result<PathBuf, String> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(dirs::config_dir)
            .ok_or_else(|| "could not resolve the user configuration directory".to_string())?;
        Ok(base.join("autostart").join("zeidex.desktop"))
    }

    pub fn status(exe: Option<&Path>) -> AutostartStatus {
        match desktop_path() {
            Ok(path) => status_with(
                exe,
                "XDG autostart",
                Some(path.display().to_string()),
                path.is_file(),
                None,
            ),
            Err(err) => status_with(exe, "XDG autostart", None, false, Some(err)),
        }
    }

    pub fn set(enabled: bool, exe: &Path) -> Result<(), String> {
        apply(enabled, exe, &desktop_path()?)
    }

    fn apply(enabled: bool, exe: &Path, path: &Path) -> Result<(), String> {
        if enabled {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|err| format!("could not create {}: {err}", parent.display()))?;
            }
            return fs::write(path, desktop_entry(exe))
                .map_err(|err| format!("could not write {}: {err}", path.display()));
        }
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("could not remove {}: {err}", path.display())),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::time::{SystemTime, UNIX_EPOCH};

        #[test]
        fn the_desktop_entry_is_written_read_back_and_removed() {
            let base = std::env::temp_dir().join(format!(
                "zeidex-autostart-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos()
            ));
            let path = base.join("autostart").join("zeidex.desktop");
            let exe = Path::new("/opt/Zeidex Search/zeidex");

            apply(true, exe, &path).expect("write entry");
            assert!(path.is_file(), "the entry must exist");
            let written = fs::read_to_string(&path).expect("read entry");
            assert!(written.contains(r#"Exec="/opt/Zeidex Search/zeidex""#));
            assert!(written.contains("X-GNOME-Autostart-enabled=true"));

            apply(false, exe, &path).expect("remove entry");
            assert!(!path.exists(), "the entry must be gone");
            // Disabling twice is the state the user asked for, not an error.
            apply(false, exe, &path).expect("remove an absent entry");

            let _ = fs::remove_dir_all(&base);
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{AutostartStatus, MAC_LABEL, launch_agent_plist, status_with};

    fn plist_path() -> Result<PathBuf, String> {
        dirs::home_dir()
            .map(|home| {
                home.join("Library")
                    .join("LaunchAgents")
                    .join(format!("{MAC_LABEL}.plist"))
            })
            .ok_or_else(|| "could not resolve the home directory".to_string())
    }

    pub fn status(exe: Option<&Path>) -> AutostartStatus {
        match plist_path() {
            Ok(path) => status_with(
                exe,
                "macOS LaunchAgent",
                Some(path.display().to_string()),
                path.is_file(),
                None,
            ),
            Err(err) => status_with(exe, "macOS LaunchAgent", None, false, Some(err)),
        }
    }

    pub fn set(enabled: bool, exe: &Path) -> Result<(), String> {
        let path = plist_path()?;
        if enabled {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|err| format!("could not create {}: {err}", parent.display()))?;
            }
            return fs::write(&path, launch_agent_plist(exe))
                .map_err(|err| format!("could not write {}: {err}", path.display()));
        }
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("could not remove {}: {err}", path.display())),
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
mod platform {
    use std::path::Path;

    use super::AutostartStatus;

    pub fn status(exe: Option<&Path>) -> AutostartStatus {
        AutostartStatus {
            supported: false,
            enabled: false,
            mechanism: "Unsupported".to_string(),
            location: None,
            command: exe
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            development: cfg!(debug_assertions),
            error: Some("launch at login is not supported on this platform".to_string()),
        }
    }

    pub fn set(_enabled: bool, _exe: &Path) -> Result<(), String> {
        Err("launch at login is not supported on this platform".to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{desktop_entry, launch_agent_plist, windows_value, xml_escape};

    #[test]
    fn the_windows_value_quotes_a_path_with_spaces() {
        assert_eq!(
            windows_value(Path::new(r"C:\Program Files\Zeidex\zeidex.exe")),
            r#""C:\Program Files\Zeidex\zeidex.exe" --autostart"#
        );
    }

    #[test]
    fn the_desktop_entry_quotes_and_escapes_the_executable() {
        let entry = desktop_entry(Path::new("/home/u/My Apps/$zeidex`x/zeidex"));
        assert!(
            entry.contains(r#"Exec="/home/u/My Apps/\$zeidex\`x/zeidex" --autostart"#),
            "{entry}"
        );
        assert!(entry.contains("Terminal=false"));
        assert!(entry.contains("X-GNOME-Autostart-enabled=true"));
    }

    #[test]
    fn the_launch_agent_is_a_plist_with_an_escaped_path() {
        let plist = launch_agent_plist(Path::new("/Applications/Zeidex & Co/zeidex"));
        assert!(
            plist.contains("<string>/Applications/Zeidex &amp; Co/zeidex</string>"),
            "{plist}"
        );
        assert!(plist.contains("<key>RunAtLoad</key>"));
        assert!(plist.contains("<true/>"));
    }

    #[test]
    fn xml_escaping_covers_the_five_entities() {
        assert_eq!(
            xml_escape("a&b<c>d\"e'f"),
            "a&amp;b&lt;c&gt;d&quot;e&apos;f"
        );
    }
}
