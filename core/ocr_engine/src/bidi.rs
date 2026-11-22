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

fn is_rtl_char(ch: char) -> bool {
    matches!(
        unicode_bidi::bidi_class(ch),
        unicode_bidi::BidiClass::R | unicode_bidi::BidiClass::AL
    )
}

fn is_ltr_char(ch: char) -> bool {
    matches!(
        unicode_bidi::bidi_class(ch),
        unicode_bidi::BidiClass::L
            | unicode_bidi::BidiClass::EN
            | unicode_bidi::BidiClass::AN
    )
}

/// Converts a recognizer's visual-order line back to logical (typing) order.
///
/// `arabic_PP-OCRv5_mobile_rec` emits glyphs in the order they appear on the
/// page, so an RTL line arrives reversed; `arabic_PP-OCRv4_rec` emits logical
/// order already. The inverse for a single line is two steps: reverse every
/// character, then reverse each span that contains no RTL character back.
///
/// The second step is what keeps digits and Latin words readable. In
/// `صفحة 12` the page shows `12 ةحفص`; reversing everything gives
/// `صفحة 21`, and reversing the `21` span gives `صفحة 12`. A trailing full stop
/// returns to the end for the same reason: it sits in a span with no LTR
/// character, so it is reversed once, with the Arabic run.
///
/// This is a line-level inverse of the Unicode Bidirectional Algorithm for the
/// common case the recognizer produces (one line, one base direction, embedded
/// LTR runs). It is not a general reordering of a multi-line paragraph, and it
/// leaves a line with no RTL character untouched.
pub fn visual_to_logical(text: &str) -> String {
    if !text.chars().any(is_rtl_char) {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().rev().collect();
    // Only the LTR characters themselves were reversed by the first step. The
    // neutrals around them (spaces, punctuation) already sit where the logical
    // line wants them, so a span is trimmed to its first and last LTR character
    // before it is reversed back. Reversing the surrounding space would move it
    // from between the words to the end of the line.
    let mut index = 0;
    while index < chars.len() {
        if is_rtl_char(chars[index]) {
            index += 1;
            continue;
        }
        let region_start = index;
        while index < chars.len() && !is_rtl_char(chars[index]) {
            index += 1;
        }
        let region = region_start..index;
        let first_ltr = region.clone().find(|&at| is_ltr_char(chars[at]));
        let last_ltr = region.rev().find(|&at| is_ltr_char(chars[at]));
        if let (Some(first), Some(last)) = (first_ltr, last_ltr) {
            chars[first..=last].reverse();
        }
    }
    chars.into_iter().collect()
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

    #[test]
    fn visual_order_is_reversed_into_logical_order() {
        // The three lines `arabic_PP-OCRv5_mobile_rec` actually emits for the
        // smoke fixture, in the order they appear on the page.
        assert_eq!(
            visual_to_logical("ويمول يف مكب ابحرم"),
            "مرحبا بكم في لوميو"
        );
        assert_eq!(visual_to_logical("بلاطلا باتك"), "كتاب الطالب");
        assert_eq!(
            visual_to_logical("هللا ةمحرو مكيلع مالسلا"),
            "السلام عليكم ورحمة الله"
        );
    }

    #[test]
    fn a_trailing_full_stop_returns_to_the_end() {
        assert_eq!(visual_to_logical(".ابحرم"), "مرحبا.");
    }

    #[test]
    fn digits_and_latin_words_keep_their_internal_order() {
        assert_eq!(visual_to_logical("12 ةحفص"), "صفحة 12");
        assert_eq!(visual_to_logical("PDF 2024 فلم"), "ملف PDF 2024");
        assert_eq!(visual_to_logical("USD 100 رعس"), "سعر USD 100");
    }

    #[test]
    fn punctuation_after_a_latin_run_stays_at_the_end() {
        // The page order for the logical `سعر USD 100.` keeps the full stop at
        // the left edge and the LTR run intact.
        assert_eq!(visual_to_logical(".USD 100 رعس"), "سعر USD 100.");
    }

    #[test]
    fn a_line_without_rtl_text_is_left_alone() {
        assert_eq!(visual_to_logical("report.pdf"), "report.pdf");
        assert_eq!(visual_to_logical("3.14"), "3.14");
        assert_eq!(visual_to_logical(""), "");
    }
}
