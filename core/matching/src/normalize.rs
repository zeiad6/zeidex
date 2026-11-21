//! Folding text so a search matches what a reader means.
//!
//! Lives in the matching crate, not the engine, because more than search needs
//! it: the AI tiers match a meeting title and a contact name against typed
//! text, and folding differently there would mean `hop` finds a FILE called
//! `Họp` while `join hop` finds nothing.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Lowercased, with diacritics stripped: `Họp` and `hop` compare equal, as do
/// `Café` and `cafe`.
///
/// Vietnamese needs one rule Unicode does not give for free: `đ` is a letter in
/// its own right rather than a `d` with a mark, so NFKD leaves it alone and it
/// has to be mapped by hand. Without that, `dien thoai` misses `Điện thoại`.
///
/// Arabic needs the same treatment for letters people type inconsistently.
/// NFKD already folds the hamza forms (`أ`→`ا`) and strips the harakat, but it
/// leaves alone the pairs Arabic writers substitute freely: `ة`/`ه`, `ى`/`ي`,
/// and the tatweel used to stretch a word. Folding those on both sides is what
/// makes `مدرسه` find `مدرسة` and `مصطفي` find `مصطفى`; the Arabic-Indic digits
/// fold to ASCII so `١٢٣` and `123` are the same query.
pub fn normalize_for_search(input: &str) -> String {
    // Fast path: pure ASCII avoids Unicode NFKD overhead
    if input.is_ascii() {
        let mut out = input.to_owned();
        out.make_ascii_lowercase();
        return out;
    }

    let mut out = String::with_capacity(input.len());

    for ch in input.nfkd() {
        if is_combining_mark(ch) {
            continue;
        }

        match ch {
            'đ' | 'Đ' => out.push('d'),
            // Decorative elongation, never part of the word.
            '\u{0640}' => {}
            // Ta marbuta is typed as ha in casual writing.
            '\u{0629}' => out.push('\u{0647}'),
            // Alef maqsura and yeh are the same letter to a searcher.
            '\u{0649}' => out.push('\u{064A}'),
            // Persian/Urdu kaf and yeh, common in mixed Arabic documents.
            '\u{06A9}' => out.push('\u{0643}'),
            '\u{06CC}' => out.push('\u{064A}'),
            '\u{0660}'..='\u{0669}' => out.push(digit_from(ch, '\u{0660}')),
            '\u{06F0}'..='\u{06F9}' => out.push(digit_from(ch, '\u{06F0}')),
            _ => out.extend(ch.to_lowercase()),
        }
    }

    out
}

/// `'0'`-based ASCII digit for a character in a ten-digit run starting at `zero`.
fn digit_from(ch: char, zero: char) -> char {
    char::from(b'0' + (ch as u32 - zero as u32) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_is_just_lowercased() {
        assert_eq!(normalize_for_search("Safari"), "safari");
        assert_eq!(normalize_for_search("main.go"), "main.go");
    }

    #[test]
    fn vietnamese_folds_to_what_people_type() {
        assert_eq!(normalize_for_search("Họp"), "hop");
        assert_eq!(normalize_for_search("Điện thoại"), "dien thoai");
        assert_eq!(normalize_for_search("Đà Nẵng"), "da nang");
    }

    #[test]
    fn other_latin_marks_fold_too() {
        assert_eq!(normalize_for_search("Café"), "cafe");
        assert_eq!(normalize_for_search("Müller"), "muller");
        assert_eq!(normalize_for_search("RÉSUMÉ"), "resume");
    }

    #[test]
    fn scripts_without_case_or_marks_pass_through() {
        // Han and kana have nothing to fold; the point is that they survive.
        assert_eq!(normalize_for_search("会议"), "会议");
        assert_eq!(normalize_for_search("いぬ"), "いぬ");
    }

    #[test]
    fn arabic_hamza_forms_fold_to_the_bare_alef() {
        // NFKD splits the hamza off, and the combining mark is then dropped.
        assert_eq!(normalize_for_search("أحمد"), normalize_for_search("احمد"));
        assert_eq!(
            normalize_for_search("إبراهيم"),
            normalize_for_search("ابراهيم")
        );
        assert_eq!(normalize_for_search("آدم"), normalize_for_search("ادم"));
    }

    #[test]
    fn arabic_letters_people_type_interchangeably_fold_together() {
        // Typed without the ta marbuta: the most common Arabic search mistake.
        assert_eq!(normalize_for_search("مدرسة"), normalize_for_search("مدرسه"));
        // Alef maqsura typed as yeh.
        assert_eq!(normalize_for_search("مصطفى"), normalize_for_search("مصطفي"));
        assert_eq!(normalize_for_search("على"), normalize_for_search("علي"));
        // Persian/Urdu forms in an otherwise Arabic document.
        assert_eq!(normalize_for_search("کتاب"), normalize_for_search("كتاب"));
    }

    #[test]
    fn arabic_tatweel_is_not_a_letter() {
        assert_eq!(
            normalize_for_search("كتابــــ"),
            normalize_for_search("كتاب")
        );
    }

    #[test]
    fn arabic_indic_digits_fold_to_ascii_so_a_number_is_one_query() {
        assert_eq!(normalize_for_search("١٢٣"), "123");
        assert_eq!(normalize_for_search("۱۲۳"), "123");
        // A mixed string keeps its order and its letters.
        assert_eq!(normalize_for_search("فاتورة ٢٠٢٦"), "فاتوره 2026");
    }

    #[test]
    fn arabic_and_latin_keep_their_relative_order() {
        assert_eq!(normalize_for_search("تقرير PDF 2026"), "تقرير pdf 2026");
    }
}
