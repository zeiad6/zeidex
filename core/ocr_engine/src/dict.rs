use std::path::Path;

use crate::error::{OcrError, OcrResult};

/// A CTC character dictionary: one character per line, line 0 reserved for the
/// CTC blank. PP-OCR's `ppocr_keys_v1.txt` / `arabic_dict.txt` follow this
/// convention.
pub struct Dictionary {
    chars: Vec<char>,
}

impl Dictionary {
    /// Loads a dictionary. An empty file is rejected: a recognition model with
    /// no dictionary silently emits blanks, which is worse than failing.
    pub fn load(path: impl AsRef<Path>) -> OcrResult<Self> {
        let contents = std::fs::read_to_string(path)?;
        Self::from_text(&contents)
    }

    pub fn from_text(contents: &str) -> OcrResult<Self> {
        // Index 0 is the CTC blank; the dictionary file's first line is the
        // first real character. Some PP-OCRv4 exports have one more class than
        // the dictionary has lines (the extra class is a space), so a
        // out-of-range index decodes as a space rather than being dropped.
        let mut chars = vec!['\u{0}'];
        chars.extend(contents.lines().filter_map(|line| line.chars().next()));
        if chars.len() < 2 {
            return Err(OcrError::Model(
                "recognition dictionary must have at least a blank and one character".to_string(),
            ));
        }
        Ok(Self { chars })
    }

    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// Greedy CTC decode: collapse repeated indices, drop the blank index (0).
    pub fn decode(&self, indices: &[u32]) -> String {
        let mut out = String::new();
        let mut previous: Option<u32> = None;
        for &index in indices {
            if index == 0 {
                previous = None;
                continue;
            }
            if previous == Some(index) {
                continue;
            }
            previous = Some(index);
            if let Some(&ch) = self.chars.get(index as usize) {
                if ch != '\u{0}' {
                    out.push(ch);
                }
            } else {
                // The export has one class more than the dictionary; it is the
                // space the model uses between words.
                out.push(' ');
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dictionary() -> Dictionary {
        // index 0 = blank (added by `from_text`), 1 = 'a', 2 = 'b', 3 = 'م'
        Dictionary::from_text("a\nb\nم").expect("fixture dictionary")
    }

    #[test]
    fn decode_collapses_repeats_and_drops_blanks() {
        let decoded = dictionary().decode(&[1, 1, 1, 0, 2, 2, 0, 0, 1]);
        assert_eq!(decoded, "aba");
    }

    #[test]
    fn decode_handles_arabic_characters() {
        let decoded = dictionary().decode(&[3, 3, 1]);
        assert_eq!(decoded, "مa");
    }

    #[test]
    fn decode_maps_the_extra_class_to_a_space() {
        // The export has one class more than the dictionary; it is the space.
        let decoded = dictionary().decode(&[1, 99, 2]);
        assert_eq!(decoded, "a b");
    }

    #[test]
    fn rejects_a_dictionary_with_no_characters() {
        assert!(Dictionary::from_text("").is_err());
        assert!(Dictionary::from_text("\n").is_err());
    }
}
