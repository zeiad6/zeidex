//! The model packs Zeidex can install, described once and verified by the
//! downloader.
//!
//! A pack is a set of ONNX recognizers plus their character dictionaries. The
//! layout and table models are shared by every pack and always come from the
//! bundled model folder, so a pack download never re-fetches what the installer
//! already shipped.
//!
//! Sizes and digests below are of the exact files the downloader fetches. They
//! were measured from the published artifacts, and a mismatch is treated as a
//! failed install rather than a warning.

use std::path::{Path, PathBuf};

/// What a pack file is for. The downloader uses the role to place the file
/// where the engine expects it, and the engine uses it to decide whether the
/// recognizer emits logical or visual order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileRole {
    Det,
    RecEn,
    RecAr,
    DictEn,
    DictAr,
}

/// How a downloaded file becomes a dictionary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileFormat {
    /// The bytes are the dictionary: one character per line.
    Raw,
    /// A PaddleOCR `inference.yml` whose `PostProcess.character_dict` list is
    /// the dictionary. The Arabic v5 recognizer publishes no standalone list,
    /// so the installer extracts it from the model's own configuration instead
    /// of shipping a copy that could drift.
    CharacterDictYaml,
}

/// One file of a pack.
#[derive(Clone, Copy, Debug)]
pub struct PackFile {
    pub role: FileRole,
    /// File name inside the pack folder.
    pub name: &'static str,
    pub url: &'static str,
    /// Lowercase hex SHA-256 of the published file.
    pub sha256: &'static str,
    pub bytes: u64,
    pub format: FileFormat,
    /// Digest of the file that actually lands in the pack folder, when the
    /// installer derives it instead of copying the download.
    ///
    /// The v5 Arabic dictionary is extracted from the model's `inference.yml`,
    /// so `sha256` describes the YAML and cannot verify what the engine reads.
    /// The extraction is deterministic, so its result is pinned here: without
    /// this the one file that is never published upstream was also the one file
    /// nothing checked.
    pub installed_sha256: Option<&'static str>,
    /// Size of that derived file, for the status screen and the resume check.
    pub installed_bytes: u64,
}

impl PackFile {
    /// The digest and size of the file as it sits in the pack folder.
    pub fn installed(&self) -> (&'static str, u64) {
        match self.installed_sha256 {
            Some(digest) => (digest, self.installed_bytes),
            None => (self.sha256, self.bytes),
        }
    }
}

/// Quality/speed tier. The tiers follow the published PP-OCR mobile/server
/// split: mobile detection and recognition are small and fast, server models
/// are several times larger and read dense or noisy pages more reliably.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Small, CPU-only, low RAM: the bundled default.
    Compact,
    /// Server-side detection with mobile recognition: better on dense scans
    /// while recognition stays fast.
    Balanced,
    /// Server detection and server recognition: the most accurate set, and the
    /// heaviest.
    Precision,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Lightweight · Fast",
            Self::Balanced => "Balanced · Recommended",
            Self::Precision => "Advanced · High accuracy",
        }
    }

    pub fn label_ar(self) -> &'static str {
        match self {
            Self::Compact => "خفيف · سريع",
            Self::Balanced => "متوازن · موصى به",
            Self::Precision => "متقدم · دقة عالية",
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Self::Compact => 1,
            Self::Balanced => 2,
            Self::Precision => 3,
        }
    }
}

/// A downloadable model pack.
#[derive(Clone, Copy, Debug)]
pub struct ModelPack {
    pub id: &'static str,
    pub tier: Tier,
    pub name: &'static str,
    pub name_ar: &'static str,
    pub summary: &'static str,
    pub summary_ar: &'static str,
    /// Peak resident memory the engine needs for this pack, measured as the
    /// model weights plus the ONNX Runtime working set on a CPU run. Used by the
    /// advisor to avoid recommending a pack that will swap.
    pub ram_mb: u64,
    /// The bundled pack ships with the installer and needs no download.
    pub bundled: bool,
    pub files: &'static [PackFile],
    /// The Arabic recognizer emits glyphs in visual (page) order. PP-OCRv5
    /// mobile Arabic does; PP-OCRv4 does not.
    pub rec_ar_visual: bool,
    /// Where the files came from and under which licence, shown in the UI so a
    /// user can audit what the app downloads.
    pub source: &'static str,
    pub license: &'static str,
}

impl ModelPack {
    /// Bytes this pack adds to the models folder.
    pub fn download_bytes(&self) -> u64 {
        self.files.iter().map(|file| file.bytes).sum()
    }

    pub fn file(&self, role: FileRole) -> Option<&'static PackFile> {
        self.files.iter().find(|file| file.role == role)
    }

    /// Roles the pack does not contain. Every pack must carry detection and
    /// both recognizers; a missing role is a catalog bug, and the tests assert
    /// none exists.
    pub fn missing_roles(&self) -> Vec<FileRole> {
        [
            FileRole::Det,
            FileRole::RecEn,
            FileRole::RecAr,
            FileRole::DictEn,
            FileRole::DictAr,
        ]
        .into_iter()
        .filter(|role| self.file(*role).is_none())
        .collect()
    }
}

/// The bundled pack is the folder `scripts/fetch_models.sh` writes, with the
/// names the engine already uses. Keeping it first means a fresh checkout keeps
/// working with no download.
const COMPACT_FILES: &[PackFile] = &[
    PackFile {
        role: FileRole::Det,
        name: "ppocrv4_det.onnx",
        url: "https://huggingface.co/cycloneboy/ch_PP-OCRv4_det_infer/resolve/967f005062008b1389143d015360fe8fdb74e694/model.onnx",
        sha256: "69ce850fec741a2a4568c7c924bb025c9d4f1129e5f96ab428c799ccc5ef2275",
        bytes: 4_729_474,
        format: FileFormat::Raw,
        installed_sha256: None,
        installed_bytes: 0,
    },
    PackFile {
        role: FileRole::RecEn,
        name: "ppocrv4_rec_en.onnx",
        url: "https://huggingface.co/cycloneboy/en_PP-OCRv4_rec_infer/resolve/fe2ad9563440e9001410fc65241cc500ef4265f6/model.onnx",
        sha256: "02672d591fee561a95a2491bd5504d66c812b145d1043026c5658ecc165c7e85",
        bytes: 7_652_836,
        format: FileFormat::Raw,
        installed_sha256: None,
        installed_bytes: 0,
    },
    PackFile {
        role: FileRole::RecAr,
        name: "ppocrv4_rec_ar.onnx",
        url: "https://huggingface.co/cycloneboy/arabic_PP-OCRv4_rec_infer/resolve/99e9558c67eff5e2c54bc073896adf0c91bf01d4/model.onnx",
        sha256: "95c13ea1be830623b8dc506728df0d0218f7c436123ff18b2a5c90254991e788",
        bytes: 7_684_783,
        format: FileFormat::Raw,
        installed_sha256: None,
        installed_bytes: 0,
    },
    PackFile {
        role: FileRole::DictEn,
        name: "ppocr_keys_v1.txt",
        url: "https://huggingface.co/cycloneboy/en_PP-OCRv4_rec_infer/resolve/fe2ad9563440e9001410fc65241cc500ef4265f6/en_dict.txt",
        sha256: "f27a6aa993c9cb67a588e7ea9aea90bb96b8e51dec6ce98bd7e76c104c1829fe",
        bytes: 285,
        format: FileFormat::Raw,
        installed_sha256: None,
        installed_bytes: 0,
    },
    PackFile {
        role: FileRole::DictAr,
        name: "arabic_dict.txt",
        url: "https://huggingface.co/cycloneboy/arabic_PP-OCRv4_rec_infer/resolve/99e9558c67eff5e2c54bc073896adf0c91bf01d4/arabic_dict.txt",
        sha256: "c071bb21fa36f4ee25d1f1aeec6fcc6a2d222b9ab49248329ec606fdaeef901a",
        bytes: 566,
        format: FileFormat::Raw,
        installed_sha256: None,
        installed_bytes: 0,
    },
];

/// Every pack the Models screen can install, in tier order.
pub const PACKS: &[ModelPack] = &[
    ModelPack {
        id: "ppocrv4-mobile",
        tier: Tier::Compact,
        name: "PP-OCRv4 Mobile",
        name_ar: "PP-OCRv4 المحمول",
        summary: "The bundled set: fast on CPU, low memory, good for clean scans and screenshots.",
        summary_ar: "المجموعة المرفقة: سريعة على المعالج، ذاكرة منخفضة، مناسبة للمسح النظيف ولقطات الشاشة.",
        ram_mb: 320,
        bundled: true,
        files: COMPACT_FILES,
        rec_ar_visual: false,
        source: "PaddleOCR PP-OCRv4 (ONNX export by cycloneboy)",
        license: "Apache-2.0",
    },
    ModelPack {
        id: "ppocrv4-server-det",
        tier: Tier::Balanced,
        name: "PP-OCRv4 Server Detection",
        name_ar: "PP-OCRv4 كشف الخادم",
        summary: "Server-grade text detection with the fast mobile recognizers: denser pages, smaller type.",
        summary_ar: "كشف نص بمستوى الخادم مع مُعرِّفات محمولة سريعة: صفحات أكثر كثافة وخطوط أصغر.",
        ram_mb: 900,
        bundled: false,
        files: &[
            PackFile {
                role: FileRole::Det,
                name: "det.onnx",
                url: "https://huggingface.co/cycloneboy/ch_PP-OCRv4_det_server_infer/resolve/9254f56c25190952874a61e38f9008b5b523b134/model.onnx",
                sha256: "56b61e50bd94bbe6a414fc7d187f670e4e3c7b40a0c339d371ea0b392bc1fb84",
                bytes: 113_350_183,
                format: FileFormat::Raw,
                installed_sha256: None,
                installed_bytes: 0,
            },
            PackFile {
                role: FileRole::RecEn,
                name: "rec_en.onnx",
                url: "https://huggingface.co/cycloneboy/en_PP-OCRv4_rec_infer/resolve/fe2ad9563440e9001410fc65241cc500ef4265f6/model.onnx",
                sha256: "02672d591fee561a95a2491bd5504d66c812b145d1043026c5658ecc165c7e85",
                bytes: 7_652_836,
                format: FileFormat::Raw,
                installed_sha256: None,
                installed_bytes: 0,
            },
            PackFile {
                role: FileRole::RecAr,
                name: "rec_ar.onnx",
                url: "https://huggingface.co/cycloneboy/arabic_PP-OCRv4_rec_infer/resolve/99e9558c67eff5e2c54bc073896adf0c91bf01d4/model.onnx",
                sha256: "95c13ea1be830623b8dc506728df0d0218f7c436123ff18b2a5c90254991e788",
                bytes: 7_684_783,
                format: FileFormat::Raw,
                installed_sha256: None,
                installed_bytes: 0,
            },
            PackFile {
                role: FileRole::DictEn,
                name: "rec_en_dict.txt",
                url: "https://huggingface.co/cycloneboy/en_PP-OCRv4_rec_infer/resolve/fe2ad9563440e9001410fc65241cc500ef4265f6/en_dict.txt",
                sha256: "f27a6aa993c9cb67a588e7ea9aea90bb96b8e51dec6ce98bd7e76c104c1829fe",
                bytes: 285,
                format: FileFormat::Raw,
                installed_sha256: None,
                installed_bytes: 0,
            },
            PackFile {
                role: FileRole::DictAr,
                name: "rec_ar_dict.txt",
                url: "https://huggingface.co/cycloneboy/arabic_PP-OCRv4_rec_infer/resolve/99e9558c67eff5e2c54bc073896adf0c91bf01d4/arabic_dict.txt",
                sha256: "c071bb21fa36f4ee25d1f1aeec6fcc6a2d222b9ab49248329ec606fdaeef901a",
                bytes: 566,
                format: FileFormat::Raw,
                installed_sha256: None,
                installed_bytes: 0,
            },
        ],
        rec_ar_visual: false,
        source: "PaddleOCR PP-OCRv4 server detection (ONNX export by cycloneboy)",
        license: "Apache-2.0",
    },
    ModelPack {
        id: "ppocrv5-server",
        tier: Tier::Precision,
        name: "PP-OCRv5 Server",
        name_ar: "PP-OCRv5 خادم",
        summary: "PP-OCRv5 server detection and recognition with the v5 Arabic recognizer: best on noisy, low-resolution and long Arabic lines.",
        summary_ar: "كشف وتعرّف PP-OCRv5 بمستوى الخادم مع مُعرِّف عربي v5: الأفضل للصور المزعجة ومنخفضة الدقة والأسطر العربية الطويلة.",
        ram_mb: 1_600,
        bundled: false,
        files: &[
            PackFile {
                role: FileRole::Det,
                name: "det.onnx",
                url: "https://huggingface.co/breezedeus/cnstd-ppocr-ch_PP-OCRv5_det_server/resolve/5902daf0bdba2e7809cc760f555abcf4f233eeee/ch_PP-OCRv5_det_server_infer.onnx",
                sha256: "0f8846b1d4bba223a2a2f9d9b44022fbc22cc019051a602b41a7fda9667e4cad",
                bytes: 88_118_768,
                format: FileFormat::Raw,
                installed_sha256: None,
                installed_bytes: 0,
            },
            PackFile {
                role: FileRole::RecEn,
                name: "rec_en.onnx",
                url: "https://huggingface.co/breezedeus/cnocr-ppocr-ch_PP-OCRv5_server/resolve/cc74722f9daf71db351161c74cbc0e6bc2396c3c/ch_PP-OCRv5_server_rec_infer.onnx",
                sha256: "e09385400eaaaef34ceff54aeb7c4f0f1fe014c27fa8b9905d4709b65746562a",
                bytes: 84_577_022,
                format: FileFormat::Raw,
                installed_sha256: None,
                installed_bytes: 0,
            },
            PackFile {
                role: FileRole::RecAr,
                name: "rec_ar.onnx",
                url: "https://huggingface.co/itextresearch/itext-arabic_PP-OCRv5_mobile_rec_infer/resolve/44f25a396f7bca2d0a6d375f618f97b134560dc6/inference.onnx",
                sha256: "916385d36898954780b807e3ae0370622b4d608f7f0fae364302cbc65b7b073f",
                bytes: 8_025_038,
                format: FileFormat::Raw,
                installed_sha256: None,
                installed_bytes: 0,
            },
            PackFile {
                role: FileRole::DictEn,
                name: "rec_en_dict.txt",
                url: "https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/a38c087bcb2579f9ccc2068aea02ec893b1c2311/ppocr/utils/dict/ppocrv5_dict.txt",
                sha256: "d1979e9f794c464c0d2e0b70a7fe14dd978e9dc644c0e71f14158cdf8342af1b",
                bytes: 74_012,
                format: FileFormat::Raw,
                installed_sha256: None,
                installed_bytes: 0,
            },
            PackFile {
                role: FileRole::DictAr,
                name: "rec_ar_dict.txt",
                url: "https://huggingface.co/itextresearch/itext-arabic_PP-OCRv5_mobile_rec_infer/resolve/44f25a396f7bca2d0a6d375f618f97b134560dc6/inference.yml",
                sha256: "21368419e6c016c31db55d316d59e11c128e1913e6e6fe10287084710043d3a6",
                bytes: 6_165,
                format: FileFormat::CharacterDictYaml,
                // The extracted list, measured from the pinned YAML above:
                // 747 characters, one per line, 2369 bytes.
                installed_sha256: Some(
                    "7f92f7dbb9b75a4787a83bfb4f6d14a8ab515525130c9d40a9036f61cf6999e9",
                ),
                installed_bytes: 2_369,
            },
        ],
        rec_ar_visual: true,
        source: "PaddleOCR PP-OCRv5 server (ONNX exports by Breezedeus and itextresearch)",
        license: "Apache-2.0",
    },
];

/// Looks a pack up by id. Ids are stable: they are the folder names on disk and
/// the value stored in the settings file.
pub fn pack(id: &str) -> Option<&'static ModelPack> {
    PACKS.iter().find(|pack| pack.id == id)
}

/// The pack a fresh install uses. It is bundled, so it is always installable.
pub fn default_pack() -> &'static ModelPack {
    &PACKS[0]
}

/// Where an installed pack lives: `<models root>/packs/<id>`.
pub fn pack_dir(models_root: &Path, id: &str) -> PathBuf {
    models_root.join("packs").join(id)
}

/// The recognizer paths for one pack, as the engine expects them.
///
/// `bundled_dir` is the read-only folder that ships with the installer (or the
/// checkout `fetch_models.sh` writes); `models_root` is the writable per-user
/// folder packs are installed into. Layout and table models are shared with the
/// bundled folder: a pack only replaces detection and recognition, which is what
/// the tiers differ in.
#[derive(Clone, Debug)]
pub struct PackPaths {
    pub dir: PathBuf,
    pub det: PathBuf,
    pub rec_en: PathBuf,
    pub rec_ar: PathBuf,
    pub dict_en: PathBuf,
    pub dict_ar: PathBuf,
    pub rec_ar_visual: bool,
}

pub fn pack_paths(bundled_dir: &Path, models_root: &Path, pack: &ModelPack) -> PackPaths {
    let dir = if pack.bundled {
        bundled_dir.to_path_buf()
    } else {
        pack_dir(models_root, pack.id)
    };
    let path = |role: FileRole| {
        pack.file(role)
            .map(|file| dir.join(file.name))
            .unwrap_or_else(|| dir.join("missing"))
    };
    let det = path(FileRole::Det);
    let rec_en = path(FileRole::RecEn);
    let rec_ar = path(FileRole::RecAr);
    let dict_en = path(FileRole::DictEn);
    let dict_ar = path(FileRole::DictAr);
    PackPaths {
        dir,
        det,
        rec_en,
        rec_ar,
        dict_en,
        dict_ar,
        rec_ar_visual: pack.rec_ar_visual,
    }
}

/// Extracts `PostProcess.character_dict` from a PaddleOCR `inference.yml`.
///
/// The list is written one character per line, the format the engine's
/// [`Dictionary`](../zeidex_ocr/dict/struct.Dictionary.html) reads. Parsing is
/// deliberately narrow: the key is located, the following list items are taken
/// until the block ends, and each item is unquoted with YAML's single-quote
/// rule (`''` is a literal quote). Anything else is an error, so a changed
/// upstream layout fails the install instead of producing a dictionary of the
/// wrong length.
pub fn extract_character_dict(yaml: &str) -> Result<Vec<String>, String> {
    let mut lines = yaml
        .lines()
        .skip_while(|line| !line.contains("character_dict:"));
    let header = lines
        .next()
        .ok_or("no character_dict key in the model config")?;
    // A flow-style list (`character_dict: [a, b]`) is not what PaddleOCR
    // writes; refuse it rather than half-parsing it.
    if header
        .split_once("character_dict:")
        .map(|(_, rest)| rest.trim().starts_with('['))
        .unwrap_or(false)
    {
        return Err("flow-style character_dict is not supported".to_string());
    }

    let mut characters = Vec::new();
    for line in lines {
        let trimmed = line.trim_start();
        let Some(item) = trimmed.strip_prefix("- ") else {
            if trimmed.starts_with('-') && trimmed.trim() == "-" {
                return Err("empty character_dict entry".to_string());
            }
            if trimmed.is_empty() {
                continue;
            }
            break;
        };
        characters.push(unquote_yaml_scalar(item.trim())?);
    }

    if characters.len() < 32 {
        return Err(format!(
            "character_dict has only {} entries; refusing to install it",
            characters.len()
        ));
    }
    Ok(characters)
}

/// YAML scalars as PaddleOCR writes them: single-quoted with `''` escapes,
/// double-quoted, or bare.
fn unquote_yaml_scalar(raw: &str) -> Result<String, String> {
    let value = if let Some(inner) = raw.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
        inner.replace("''", "'")
    } else if let Some(inner) = raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        // The dictionary contains a backslash; YAML double-quoted strings would
        // escape it, but PaddleOCR emits single-quoted or bare scalars. Accept
        // the common escapes and leave everything else literal.
        inner.replace("\\\\", "\\").replace("\\\"", "\"")
    } else if raw.starts_with('\'') || raw.starts_with('"') {
        return Err(format!("unterminated quoted scalar: {raw}"));
    } else {
        raw.to_string()
    };
    // One line is one character; a multi-character scalar would shift every
    // index after it.
    let mut chars = value.chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) => Ok(ch.to_string()),
        _ => Err(format!("character_dict entry is not one character: {raw}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "\
Global:
  model_name: arabic_PP-OCRv5_mobile_rec
PostProcess:
  name: CTCLabelDecode
  character_dict:
  - '!'
  - '\"'
  - '#'
  - $
  - ''''
  - a
  - b
  - c
  - d
  - e
  - f
  - g
  - h
  - i
  - j
  - k
  - l
  - m
  - n
  - o
  - p
  - q
  - r
  - s
  - t
  - u
  - v
  - w
  - x
  - y
  - z
  - 'م'
";

    #[test]
    fn extracts_a_character_dict_from_the_model_config() {
        let characters = extract_character_dict(CONFIG).expect("config");
        assert_eq!(characters.len(), 32);
        assert_eq!(characters[0], "!");
        assert_eq!(characters[3], "$");
        assert_eq!(characters[4], "'");
        assert_eq!(characters[31], "م");
    }

    #[test]
    fn rejects_a_config_without_a_character_dict() {
        assert!(extract_character_dict("Global:\n  model_name: x\n").is_err());
    }

    #[test]
    fn rejects_short_or_malformed_lists() {
        let short = "PostProcess:\n  character_dict:\n  - a\n  - b\n";
        assert!(extract_character_dict(short).is_err());
        let wide = "PostProcess:\n  character_dict:\n  - ab\n";
        assert!(extract_character_dict(wide).is_err());
    }

    #[test]
    fn every_pack_carries_detection_and_both_recognizers() {
        for pack in PACKS {
            assert!(
                pack.missing_roles().is_empty(),
                "{} is missing {:?}",
                pack.id,
                pack.missing_roles()
            );
        }
    }

    #[test]
    fn pack_ids_are_unique_and_stable() {
        let mut ids: Vec<_> = PACKS.iter().map(|pack| pack.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count);
        for id in &ids {
            assert!(
                id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{id} is not a safe folder name"
            );
        }
    }

    #[test]
    fn digests_are_lowercase_sha256() {
        for pack in PACKS {
            for file in pack.files {
                assert_eq!(file.sha256.len(), 64, "{} {:?}", pack.id, file.role);
                assert!(
                    file.sha256
                        .chars()
                        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
                    "{} {:?} digest is not lowercase hex",
                    pack.id,
                    file.role
                );
            }
        }
    }

    #[test]
    fn every_url_is_https() {
        for pack in PACKS {
            for file in pack.files {
                assert!(
                    file.url.starts_with("https://"),
                    "{} {:?} is not https",
                    pack.id,
                    file.role
                );
            }
        }
    }

    #[test]
    fn tiers_are_ordered_and_distinct() {
        let ranks: Vec<_> = PACKS.iter().map(|pack| pack.tier.rank()).collect();
        assert_eq!(ranks, vec![1, 2, 3]);
    }

    #[test]
    fn the_bundled_pack_needs_no_download() {
        let bundled = PACKS.iter().filter(|pack| pack.bundled).collect::<Vec<_>>();
        assert_eq!(bundled.len(), 1);
        assert_eq!(bundled[0].tier, Tier::Compact);
        assert!(default_pack().bundled);
    }

    #[test]
    fn pack_dirs_are_namespaced() {
        let root = Path::new("/models");
        assert_eq!(
            pack_dir(root, "ppocrv5-server"),
            Path::new("/models/packs/ppocrv5-server")
        );
        let paths = pack_paths(root, root, pack("ppocrv4-server-det").expect("pack"));
        assert_eq!(
            paths.det,
            Path::new("/models/packs/ppocrv4-server-det/det.onnx")
        );
        let bundled = pack_paths(Path::new("/app/models"), root, default_pack());
        assert_eq!(bundled.det, Path::new("/app/models/ppocrv4_det.onnx"));
    }
}
