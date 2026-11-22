//! Installing, verifying and removing model packs on disk.
//!
//! A pack is only ever "installed" when a manifest written after a full
//! verification is present. Downloads land in a staging folder and are moved
//! into place with a rename, so an interrupted download can never be activated
//! and can never damage the pack that is currently working.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::catalog::{self, FileFormat, ModelPack, PackFile};

/// Name of the marker file that makes a folder an installed pack.
pub const MANIFEST: &str = "manifest.json";

/// Name of the file that records which pack the engine should load.
pub const ACTIVE: &str = "active-pack.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub pack_id: String,
    /// Pack format version. Bumped when the file layout changes so an old
    /// install is re-fetched instead of half-read.
    pub version: u32,
    pub installed_at_unix_s: u64,
    /// File name → SHA-256 of the installed bytes.
    pub files: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallState {
    /// No manifest: nothing usable is in the folder.
    NotInstalled,
    /// Some files are present but the manifest is missing (an interrupted
    /// download, or a folder copied by hand).
    Partial,
    /// The manifest is present and every file it lists exists with the right
    /// size. Hashes are checked separately, because that reads every byte.
    Installed,
}

#[derive(Clone, Debug, Serialize)]
pub struct FileStatus {
    pub role: crate::catalog::FileRole,
    pub name: String,
    pub present: bool,
    pub expected_bytes: u64,
    pub actual_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PackStatus {
    pub pack_id: String,
    pub state: InstallState,
    pub dir: PathBuf,
    pub files: Vec<FileStatus>,
    /// Bytes still to download for this pack, from the catalog.
    pub missing_bytes: u64,
}

impl PackStatus {
    pub fn is_installed(&self) -> bool {
        self.state == InstallState::Installed
    }
}

/// Where a pack's files live.
pub fn dir_for(models_root: &Path, pack: &ModelPack) -> PathBuf {
    catalog::pack_dir(models_root, pack.id)
}

/// Staging folder for an in-progress download. One per pack, so cancelling one
/// pack never touches another.
pub fn staging_dir(models_root: &Path, pack: &ModelPack) -> PathBuf {
    models_root
        .join("packs")
        .join(format!(".staging-{}", pack.id))
}

/// Where a pack's raw download for one file is kept while it is in flight.
pub fn part_path(dir: &Path, file: &PackFile) -> PathBuf {
    dir.join(format!("{}.part", file.name))
}

/// Inspects a pack folder without reading file contents.
pub fn status(models_root: &Path, pack: &ModelPack) -> PackStatus {
    let dir = dir_for(models_root, pack);
    let manifest = read_manifest(&dir).ok();
    let mut files = Vec::with_capacity(pack.files.len());
    let mut missing_bytes = 0;
    let mut all_present = true;

    for file in pack.files {
        let path = dir.join(file.name);
        let actual = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        // A dictionary extracted from YAML is not the downloaded file, so its
        // catalog size is only a lower bound; presence is what matters.
        let present = path.is_file() && (actual > 0 || file.bytes == 0);
        if !present {
            all_present = false;
            missing_bytes += file.bytes;
        }
        files.push(FileStatus {
            role: file.role,
            name: file.name.to_string(),
            present,
            expected_bytes: file.bytes,
            actual_bytes: actual,
        });
    }

    let state = match manifest {
        Some(manifest) if manifest.pack_id == pack.id && all_present => InstallState::Installed,
        _ if files.iter().any(|file| file.present) => InstallState::Partial,
        _ => InstallState::NotInstalled,
    };

    PackStatus {
        pack_id: pack.id.to_string(),
        state,
        dir,
        files,
        missing_bytes,
    }
}

/// Reads and validates the manifest. A manifest naming a different pack is
/// treated as absent rather than trusted.
pub fn read_manifest(dir: &Path) -> Result<Manifest, String> {
    let bytes = std::fs::read(dir.join(MANIFEST)).map_err(|err| err.to_string())?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|err| err.to_string())?;
    if manifest.version != 1 {
        return Err(format!(
            "unsupported pack manifest version {}",
            manifest.version
        ));
    }
    Ok(manifest)
}

/// Full verification: every file in the manifest exists and hashes to the
/// recorded digest, and every catalog file is in the manifest.
///
/// This reads the whole pack (up to ~300 MB), so it runs after an install and
/// on an explicit "verify" action, not on every screen open.
pub fn verify(models_root: &Path, pack: &ModelPack) -> Result<Manifest, Vec<String>> {
    let dir = dir_for(models_root, pack);
    let manifest = match read_manifest(&dir) {
        Ok(manifest) if manifest.pack_id == pack.id => manifest,
        Ok(_) => return Err(vec!["manifest belongs to another pack".to_string()]),
        Err(err) => return Err(vec![format!("manifest: {err}")]),
    };

    let mut problems = Vec::new();
    for file in pack.files {
        match manifest.files.get(file.name) {
            Some(expected) => match hash_file(&dir.join(file.name)) {
                Ok(actual) if actual == *expected => {}
                Ok(actual) => problems.push(format!("{0}: digest {1}", file.name, &actual[..12])),
                Err(err) => problems.push(format!("{}: {err}", file.name)),
            },
            None => problems.push(format!("{}: not in manifest", file.name)),
        }
    }
    for name in manifest.files.keys() {
        if !pack.files.iter().any(|file| file.name == name) {
            problems.push(format!("{name}: unknown file in manifest"));
        }
    }

    if problems.is_empty() {
        Ok(manifest)
    } else {
        Err(problems)
    }
}

/// Turns one downloaded file into the bytes that belong in the pack folder.
///
/// A raw dictionary is used as published. A YAML model config has its
/// `character_dict` extracted, because the v5 Arabic recognizer publishes no
/// standalone list.
pub fn prepare_file(file: &PackFile, downloaded: &Path) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(downloaded).map_err(|err| err.to_string())?;
    match file.format {
        FileFormat::Raw => Ok(bytes),
        FileFormat::CharacterDictYaml => {
            let text =
                String::from_utf8(bytes).map_err(|_| "model config is not UTF-8".to_string())?;
            let characters = catalog::extract_character_dict(&text)?;
            let mut out = String::new();
            for character in characters {
                out.push_str(&character);
                out.push('\n');
            }
            Ok(out.into_bytes())
        }
    }
}

/// Writes a manifest for a staged folder whose files were just verified.
pub fn write_manifest(dir: &Path, pack: &ModelPack) -> Result<Manifest, String> {
    let mut files = BTreeMap::new();
    for file in pack.files {
        let path = dir.join(file.name);
        let digest = hash_file(&path)?;
        files.insert(file.name.to_string(), digest);
    }
    let manifest = Manifest {
        pack_id: pack.id.to_string(),
        version: 1,
        installed_at_unix_s: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0),
        files,
    };
    let encoded = serde_json::to_vec_pretty(&manifest).map_err(|err| err.to_string())?;
    std::fs::write(dir.join(MANIFEST), encoded).map_err(|err| err.to_string())?;
    Ok(manifest)
}

/// Moves a verified staging folder into place.
///
/// The previous pack, if any, is moved aside first and removed only after the
/// new one is in place, so a failure leaves either the old pack or the new one —
/// never neither.
pub fn promote(models_root: &Path, pack: &ModelPack) -> Result<PathBuf, String> {
    let staging = staging_dir(models_root, pack);
    let target = dir_for(models_root, pack);
    if !staging.is_dir() {
        return Err("nothing to install: the staging folder is missing".to_string());
    }
    // Verify before the swap: a half-written staging folder must not replace a
    // working install.
    verify_manifest_in(&staging, pack)?;

    let parent = target.parent().ok_or("pack folder has no parent")?;
    std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;

    let retired = parent.join(format!(".retired-{}", pack.id));
    if retired.exists() {
        std::fs::remove_dir_all(&retired).map_err(|err| err.to_string())?;
    }
    if target.exists() {
        std::fs::rename(&target, &retired).map_err(|err| err.to_string())?;
    }
    match std::fs::rename(&staging, &target) {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&retired);
            Ok(target)
        }
        Err(err) => {
            // Put the old pack back before reporting, so the user keeps a
            // working model.
            if retired.exists() {
                let _ = std::fs::rename(&retired, &target);
            }
            Err(format!("could not move the pack into place: {err}"))
        }
    }
}

fn verify_manifest_in(dir: &Path, pack: &ModelPack) -> Result<(), String> {
    let manifest = read_manifest(dir)?;
    if manifest.pack_id != pack.id {
        return Err("manifest belongs to another pack".to_string());
    }
    for file in pack.files {
        let expected = manifest
            .files
            .get(file.name)
            .ok_or_else(|| format!("{} is not in the manifest", file.name))?;
        let actual = hash_file(&dir.join(file.name))?;
        if actual != *expected {
            return Err(format!("{} failed its digest check", file.name));
        }
    }
    Ok(())
}

/// Removes an installed pack. The bundled pack cannot be removed, and the
/// caller must refuse that before it gets here.
pub fn remove(models_root: &Path, pack: &ModelPack) -> Result<(), String> {
    if pack.bundled {
        return Err("the bundled pack cannot be removed".to_string());
    }
    let dir = dir_for(models_root, pack);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|err| err.to_string())?;
    }
    let staging = staging_dir(models_root, pack);
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|err| err.to_string())?;
    }
    // A removed pack must not stay the active one: the next OCR run would fail
    // to load it. The bundled pack is the safe fallback.
    if active_pack(models_root).as_deref() == Some(pack.id) {
        clear_active(models_root)?;
    }
    Ok(())
}

#[derive(Deserialize, Serialize)]
struct ActiveMarker {
    pack_id: String,
}

/// The pack id the user last activated. `None` means the bundled default.
///
/// The id is returned as stored: resolving it against the catalog and falling
/// back to the bundled pack when it is unknown is the caller's job, because only
/// the caller knows where the bundled folder is. This keeps a hand-edited or
/// stale marker harmless instead of an error the UI would have to explain.
pub fn active_pack(models_root: &Path) -> Option<String> {
    let bytes = std::fs::read(models_root.join(ACTIVE)).ok()?;
    let marker: ActiveMarker = serde_json::from_slice(&bytes).ok()?;
    Some(marker.pack_id)
}

/// Records a pack as active after proving it is complete and unmodified.
///
/// Activation reads every byte of a downloaded pack: a model that was damaged
/// on disk after install must not be selected just because its manifest is
/// still there. The bundled pack is checked by the shell, which knows where its
/// folder is, so only downloaded packs are hashed here.
pub fn set_active(models_root: &Path, pack: &ModelPack) -> Result<(), String> {
    if !pack.bundled {
        verify(models_root, pack).map_err(|problems| {
            format!("{} failed verification: {}", pack.name, problems.join("; "))
        })?;
    }
    let encoded = serde_json::to_vec_pretty(&ActiveMarker {
        pack_id: pack.id.to_string(),
    })
    .map_err(|err| err.to_string())?;
    std::fs::create_dir_all(models_root).map_err(|err| err.to_string())?;
    // Write beside the target and rename: a crash mid-write leaves either the
    // old marker or the new one, never half a JSON document.
    let temporary = models_root.join(format!("{ACTIVE}.tmp"));
    std::fs::write(&temporary, encoded).map_err(|err| err.to_string())?;
    std::fs::rename(&temporary, models_root.join(ACTIVE)).map_err(|err| err.to_string())
}

/// Returns to the bundled pack.
pub fn clear_active(models_root: &Path) -> Result<(), String> {
    match std::fs::remove_file(models_root.join(ACTIVE)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

/// SHA-256 of a file, lowercase hex. Streams, so a 300 MB model does not have to
/// fit in memory.
pub fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Total bytes a pack occupies once installed, dictionaries included.
pub fn installed_bytes(models_root: &Path, pack: &ModelPack) -> u64 {
    let dir = dir_for(models_root, pack);
    pack.files
        .iter()
        .filter_map(|file| std::fs::metadata(dir.join(file.name)).ok())
        .map(|meta| meta.len())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{FileFormat, FileRole};

    fn test_pack(files: &'static [PackFile]) -> ModelPack {
        ModelPack {
            id: "test-pack",
            tier: crate::catalog::Tier::Balanced,
            name: "Test",
            name_ar: "اختبار",
            summary: "",
            summary_ar: "",
            ram_mb: 100,
            bundled: false,
            files,
            rec_ar_visual: false,
            source: "",
            license: "",
        }
    }

    static FILES: &[PackFile] = &[
        PackFile {
            role: FileRole::Det,
            name: "det.onnx",
            url: "https://example.com/det",
            sha256: "0",
            bytes: 0,
            format: FileFormat::Raw,
        },
        PackFile {
            role: FileRole::RecEn,
            name: "rec_en.onnx",
            url: "https://example.com/en",
            sha256: "0",
            bytes: 0,
            format: FileFormat::Raw,
        },
        PackFile {
            role: FileRole::RecAr,
            name: "rec_ar.onnx",
            url: "https://example.com/ar",
            sha256: "0",
            bytes: 0,
            format: FileFormat::Raw,
        },
        PackFile {
            role: FileRole::DictEn,
            name: "rec_en_dict.txt",
            url: "https://example.com/endict",
            sha256: "0",
            bytes: 0,
            format: FileFormat::Raw,
        },
        PackFile {
            role: FileRole::DictAr,
            name: "rec_ar_dict.txt",
            url: "https://example.com/ardict",
            sha256: "0",
            bytes: 0,
            format: FileFormat::Raw,
        },
    ];

    fn staged(root: &Path, pack: &ModelPack) -> PathBuf {
        let dir = staging_dir(root, pack);
        std::fs::create_dir_all(&dir).expect("staging");
        for file in pack.files {
            std::fs::write(dir.join(file.name), format!("bytes of {}", file.name)).expect("write");
        }
        dir
    }

    #[test]
    fn an_empty_folder_is_not_installed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pack = test_pack(FILES);
        let status = status(temp.path(), &pack);
        assert_eq!(status.state, InstallState::NotInstalled);
        assert!(!status.is_installed());
    }

    #[test]
    fn files_without_a_manifest_are_only_partial() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pack = test_pack(FILES);
        let dir = staging_dir(temp.path(), &pack);
        std::fs::create_dir_all(&dir).expect("staging");
        std::fs::write(dir.join("det.onnx"), b"x").expect("write");
        // Staging is not the install folder; a stray install folder is.
        let target = dir_for(temp.path(), &pack);
        std::fs::create_dir_all(&target).expect("target");
        std::fs::write(target.join("det.onnx"), b"x").expect("write");
        assert_eq!(status(temp.path(), &pack).state, InstallState::Partial);
    }

    #[test]
    fn promote_installs_and_verifies() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pack = test_pack(FILES);
        let dir = staged(temp.path(), &pack);
        write_manifest(&dir, &pack).expect("manifest");
        let installed = promote(temp.path(), &pack).expect("promote");
        assert!(installed.join(MANIFEST).is_file());
        assert_eq!(status(temp.path(), &pack).state, InstallState::Installed);
        verify(temp.path(), &pack).expect("verify");
    }

    #[test]
    fn a_corrupt_staging_folder_is_never_promoted() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pack = test_pack(FILES);
        let dir = staged(temp.path(), &pack);
        write_manifest(&dir, &pack).expect("manifest");
        std::fs::write(dir.join("rec_en.onnx"), b"tampered").expect("tamper");
        assert!(promote(temp.path(), &pack).is_err());
        assert!(!dir_for(temp.path(), &pack).exists());
    }

    #[test]
    fn an_installed_pack_that_changed_on_disk_fails_verification() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pack = test_pack(FILES);
        let dir = staged(temp.path(), &pack);
        write_manifest(&dir, &pack).expect("manifest");
        promote(temp.path(), &pack).expect("install");
        std::fs::write(dir_for(temp.path(), &pack).join("rec_en.onnx"), b"tampered")
            .expect("tamper");
        let problems = verify(temp.path(), &pack).expect_err("verify must fail");
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("rec_en.onnx")),
            "{problems:?}"
        );
    }

    #[test]
    fn promote_keeps_the_old_pack_when_the_new_one_is_bad() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pack = test_pack(FILES);
        let dir = staged(temp.path(), &pack);
        write_manifest(&dir, &pack).expect("manifest");
        promote(temp.path(), &pack).expect("first install");
        let installed = dir_for(temp.path(), &pack);
        let before = std::fs::read(installed.join("det.onnx")).expect("read");

        // A second staging that fails its digest check must leave the first
        // install untouched.
        let staged_again = staging_dir(temp.path(), &pack);
        std::fs::create_dir_all(&staged_again).expect("staging");
        for file in pack.files {
            std::fs::write(staged_again.join(file.name), b"new").expect("write");
        }
        write_manifest(&staged_again, &pack).expect("manifest");
        std::fs::write(staged_again.join("det.onnx"), b"changed after manifest").expect("write");
        assert!(promote(temp.path(), &pack).is_err());
        assert_eq!(
            std::fs::read(installed.join("det.onnx")).expect("read"),
            before
        );
    }

    #[test]
    fn remove_deletes_an_installed_pack_and_refuses_the_bundled_one() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pack = test_pack(FILES);
        let dir = staged(temp.path(), &pack);
        write_manifest(&dir, &pack).expect("manifest");
        promote(temp.path(), &pack).expect("install");
        remove(temp.path(), &pack).expect("remove");
        assert!(!dir_for(temp.path(), &pack).exists());

        let bundled = crate::catalog::default_pack();
        assert!(remove(temp.path(), bundled).is_err());
    }

    #[test]
    fn activation_requires_a_verified_install() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pack = test_pack(FILES);
        assert!(
            set_active(temp.path(), &pack).is_err(),
            "an uninstalled pack must not activate"
        );
        assert_eq!(active_pack(temp.path()), None);

        let dir = staged(temp.path(), &pack);
        write_manifest(&dir, &pack).expect("manifest");
        promote(temp.path(), &pack).expect("install");
        set_active(temp.path(), &pack).expect("activate");
        assert_eq!(active_pack(temp.path()).as_deref(), Some(pack.id));
        clear_active(temp.path()).expect("clear");
        assert_eq!(active_pack(temp.path()), None);
    }

    #[test]
    fn a_tampered_pack_cannot_be_activated() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pack = test_pack(FILES);
        let dir = staged(temp.path(), &pack);
        write_manifest(&dir, &pack).expect("manifest");
        promote(temp.path(), &pack).expect("install");
        std::fs::write(dir_for(temp.path(), &pack).join("det.onnx"), b"tampered").expect("tamper");
        let error = set_active(temp.path(), &pack).expect_err("tampered pack");
        assert!(error.contains("det.onnx"), "{error}");
        assert_eq!(active_pack(temp.path()), None);
    }

    #[test]
    fn removing_the_active_pack_returns_to_the_bundled_one() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pack = test_pack(FILES);
        let dir = staged(temp.path(), &pack);
        write_manifest(&dir, &pack).expect("manifest");
        promote(temp.path(), &pack).expect("install");
        set_active(temp.path(), &pack).expect("activate");
        remove(temp.path(), &pack).expect("remove");
        assert_eq!(active_pack(temp.path()), None);
    }

    #[test]
    fn a_stale_active_marker_is_returned_for_the_caller_to_resolve() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join(ACTIVE), br#"{"pack_id":"deleted-pack"}"#).expect("marker");
        assert_eq!(active_pack(temp.path()).as_deref(), Some("deleted-pack"));
        // The shell resolves the id; an unknown one falls back to bundled.
        assert!(crate::catalog::pack("deleted-pack").is_none());
    }

    #[test]
    fn a_yaml_config_becomes_a_dictionary() {
        let temp = tempfile::tempdir().expect("tempdir");
        let yaml = "PostProcess:\n  character_dict:\n";
        let mut text = String::from(yaml);
        for ch in "abcdefghijklmnopqrstuvwxyz0123456789".chars() {
            text.push_str(&format!("  - '{ch}'\n"));
        }
        let path = temp.path().join("inference.yml");
        std::fs::write(&path, text).expect("write");
        let file = PackFile {
            role: FileRole::DictAr,
            name: "rec_ar_dict.txt",
            url: "https://example.com/yml",
            sha256: "0",
            bytes: 0,
            format: FileFormat::CharacterDictYaml,
        };
        let prepared = prepare_file(&file, &path).expect("prepare");
        let text = String::from_utf8(prepared).expect("utf8");
        assert_eq!(text.lines().count(), 36);
        assert_eq!(text.lines().next(), Some("a"));
    }

    #[test]
    fn hashing_matches_a_known_digest() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("f");
        std::fs::write(&path, b"abc").expect("write");
        assert_eq!(
            hash_file(&path).expect("hash"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
