use unicode_bidi::Direction;
use unicode_normalization::UnicodeNormalization;

/// Arabic script blocks: Basic Arabic, Arabic Supplement, Extended-A/B,
/// Presentation Forms-A/B. Persian, Urdu and Hebrew-adjacent Arabic-script
/// languages live in these ranges.
pub fn has_arabic(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(ch as u32,
            0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF |
            0xFB50..=0xFDFF | 0xFE70..=0xFEFF)
    })
}

/// Base paragraph direction from the first strong character. This is what the
/// exporters use to decide RTL paragraph/run properties.
pub fn base_direction(text: &str) -> Direction {
    unicode_bidi::get_base_direction(text)
}

pub fn is_rtl(text: &str) -> bool {
    base_direction(text) == Direction::Rtl
}

/// True when the text mixes RTL and LTR strong runs, which is the common
/// Arabic-with-Latin-numerals case. The exporters keep the logical order and
/// set the paragraph direction; they do not reorder characters.
pub fn is_mixed_direction(text: &str) -> bool {
    let mut saw_rtl = false;
    let mut saw_ltr = false;
    for ch in text.chars() {
        match unicode_bidi::bidi_class(ch) {
            unicode_bidi::BidiClass::R | unicode_bidi::BidiClass::AL => saw_rtl = true,
            unicode_bidi::BidiClass::L => saw_ltr = true,
            _ => {}
        }
    }
    saw_rtl && saw_ltr
}

/// Explicit BiDi control characters that OCR sometimes picks up as glyphs.
fn is_bidi_control(ch: char) -> bool {
    matches!(ch as u32, 0x200E..=0x200F | 0x202A..=0x202E | 0x2066..=0x2069)
}

/// Normalizes recognized text to a stable logical form: NFKC, no BiDi control
/// characters, no zero-width joiners left behind by a bad dictionary.
///
/// The text stays in logical (typing) order. Word/Excel render it correctly
/// once the paragraph direction is set; reordering here would corrupt copy/paste.
pub fn normalize_logical(text: &str) -> String {
    text.nfkc()
        .filter(|ch| !is_bidi_control(*ch) && *ch != '\u{200C}' && *ch != '\u{200D}')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_arabic_and_english() {
        assert!(has_arabic("مرحبا"));
        assert!(has_arabic("بالإنجليزية"));
        assert!(!has_arabic("hello"));
        assert!(!has_arabic("123"));
    }

    #[test]
    fn base_direction_follows_the_first_strong_run() {
        assert!(is_rtl("مرحبا بالعالم"));
        assert!(!is_rtl("hello world"));
        // Digits are weak: the first strong run decides, so Arabic still wins.
        assert!(is_rtl("123 مرحبا"));
        assert!(!is_rtl("123 hello مرحبا"));
    }

    #[test]
    fn mixed_text_is_flagged() {
        assert!(is_mixed_direction("سعر 100 USD"));
        assert!(!is_mixed_direction("مرحبا"));
    }

    #[test]
    fn normalize_strips_controls_and_keeps_logical_order() {
        let dirty = "\u{202A}مرحبا\u{202C} \u{200C}world";
        let clean = normalize_logical(dirty);
        assert_eq!(clean, "مرحبا world");
        // Logical order is preserved: the first Arabic word stays first.
        assert!(clean.find("مرحبا") < clean.find("world"));
    }
}
