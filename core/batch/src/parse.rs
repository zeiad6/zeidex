//! Parsing the list a user pastes into Batch Search.
//!
//! The list usually comes from a spreadsheet, an email or a folder listing, so
//! the parser is deliberately conservative: it only separates entries and
//! trims the noise around them. It never rewrites a name — a filename is data,
//! and a "helpful" fix would search for something the user did not ask for.

use serde::Serialize;
use zeidex_matching::normalize_for_search;

/// Ceiling on one batch. The search is linear in entries times index size, and
/// a pasted novel should fail loudly rather than freeze the window.
pub const MAX_ENTRIES: usize = 2_000;

/// What the review step shows before a search runs.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ParsedList {
    pub entries: Vec<String>,
    /// Lines that were only whitespace or quotes.
    pub dropped_empty: usize,
    /// Entries folded into an earlier one because they normalize the same
    /// (`Invoice.PDF` and `invoice.pdf` are one request).
    pub duplicates: usize,
    /// Entries beyond [`MAX_ENTRIES`] that were not accepted.
    pub overflow: usize,
}

impl ParsedList {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Splits pasted text into search entries.
///
/// Newlines and tabs both separate entries: a single column copied out of a
/// spreadsheet arrives newline-separated, while a row arrives tab-separated.
/// Commas and semicolons do not, because they are ordinary characters in file
/// names (`report, final.pdf`). A UTF-8 BOM from Notepad is dropped, CRLF is
/// handled, and a wrapping pair of quotes is removed — that is what a CSV or a
/// mail client adds around a name, not part of it.
///
/// A bare carriage return separates entries too. Old Mac exports and some
/// mail-client copies use CR alone, and splitting only on LF turned such a file
/// into a single entry containing every name.
pub fn parse_list(text: &str) -> ParsedList {
    let mut parsed = ParsedList::default();
    let mut seen = std::collections::HashSet::new();

    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // CRLF is one separator, not two. Splitting on both characters separately
    // would count an empty entry between them for every line of a Windows file,
    // and the review step would report a file full of blank lines.
    let normalized;
    let text = if text.contains("\r\n") {
        normalized = text.replace("\r\n", "\n");
        normalized.as_str()
    } else {
        text
    };
    for raw in text.split(['\n', '\r', '\t']) {
        let entry = clean_entry(raw);
        if entry.is_empty() {
            parsed.dropped_empty += 1;
            continue;
        }
        let key = normalize_for_search(&entry);
        if !seen.insert(key) {
            parsed.duplicates += 1;
            continue;
        }
        if parsed.entries.len() == MAX_ENTRIES {
            parsed.overflow += 1;
            continue;
        }
        parsed.entries.push(entry);
    }
    parsed
}

/// Decodes a list file's bytes.
///
/// Excel's "Unicode text" export and Windows Notepad write UTF-16LE with a BOM,
/// so refusing anything that is not UTF-8 would reject the most likely `.txt` a
/// spreadsheet produces. A BOM decides the encoding; otherwise UTF-8 is assumed
/// and invalid bytes are replaced rather than failing the whole import, because
/// one damaged name should not cost the user the other 999.
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16(rest, u16::from_be_bytes);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

fn decode_utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    // `as_chunks` drops a trailing odd byte, which is the right trade: a
    // truncated file still imports its complete names.
    let (pairs, _trailing) = bytes.as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().map(|pair| unit(*pair)).collect();
    String::from_utf16_lossy(&units)
}

/// Trims whitespace, a trailing `\r`, and one wrapping pair of quotes.
fn clean_entry(raw: &str) -> String {
    let trimmed = raw.trim();
    let unquoted = match (trimmed.chars().next(), trimmed.chars().last()) {
        (Some(first @ ('"' | '\'')), Some(last)) if first == last && trimmed.len() >= 2 => {
            &trimmed[first.len_utf8()..trimmed.len() - last.len_utf8()]
        }
        _ => trimmed,
    };
    unquoted.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::{MAX_ENTRIES, decode_text, parse_list};

    #[test]
    fn newlines_and_tabs_both_separate_entries() {
        let parsed = parse_list("report.pdf\ninvoice_2024.xlsx\tproject_final.docx\n");
        assert_eq!(
            parsed.entries,
            ["report.pdf", "invoice_2024.xlsx", "project_final.docx"]
        );
    }

    #[test]
    fn commas_are_part_of_a_name() {
        let parsed = parse_list("report, final.pdf\nnotes; v2.txt");
        assert_eq!(parsed.entries, ["report, final.pdf", "notes; v2.txt"]);
    }

    #[test]
    fn crlf_bom_and_wrapping_quotes_are_removed() {
        let parsed = parse_list("\u{feff}\"report.pdf\"\r\n'notes.txt'\r\n");
        assert_eq!(parsed.entries, ["report.pdf", "notes.txt"]);
    }

    #[test]
    fn duplicates_fold_the_way_search_folds_text() {
        // Case, Arabic letter variants and Arabic-Indic digits all normalize
        // the same, so each pair is one request rather than two.
        let parsed = parse_list("Invoice.PDF\ninvoice.pdf\nمدرسة.pdf\nمدرسه.pdf\n١٢٣.pdf\n123.pdf");
        assert_eq!(parsed.entries, ["Invoice.PDF", "مدرسة.pdf", "١٢٣.pdf"]);
        assert_eq!(parsed.duplicates, 3);
    }

    #[test]
    fn empty_lines_are_counted_not_searched() {
        let parsed = parse_list("\n\n   \nreal.pdf\n\n");
        assert_eq!(parsed.entries, ["real.pdf"]);
        assert_eq!(parsed.dropped_empty, 5);
    }

    #[test]
    fn a_list_beyond_the_ceiling_reports_the_overflow() {
        let text = (0..MAX_ENTRIES + 7)
            .map(|index| format!("file-{index}.pdf"))
            .collect::<Vec<_>>()
            .join("\n");
        let parsed = parse_list(&text);
        assert_eq!(parsed.entries.len(), MAX_ENTRIES);
        assert_eq!(parsed.overflow, 7);
    }

    #[test]
    fn an_imported_file_is_decoded_by_its_bom() {
        let utf8 = "report.pdf\nتقرير.pdf";
        assert_eq!(decode_text(utf8.as_bytes()), utf8);

        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice(utf8.as_bytes());
        assert_eq!(decode_text(&with_bom), utf8);

        // What Excel writes for "Unicode text".
        let mut utf16 = vec![0xFF, 0xFE];
        for unit in utf8.encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(decode_text(&utf16), utf8);

        let mut utf16_be = vec![0xFE, 0xFF];
        for unit in utf8.encode_utf16() {
            utf16_be.extend_from_slice(&unit.to_be_bytes());
        }
        assert_eq!(decode_text(&utf16_be), utf8);
    }

    #[test]
    fn an_imported_file_with_a_bad_byte_still_yields_its_other_names() {
        let mut bytes = b"first.pdf\n".to_vec();
        bytes.push(0xFF);
        bytes.extend_from_slice(b"\nlast.pdf");
        let decoded = decode_text(&bytes);
        let parsed = parse_list(&decoded);
        assert!(parsed.entries.contains(&"first.pdf".to_string()));
        assert!(parsed.entries.contains(&"last.pdf".to_string()));
    }

    #[test]
    fn arabic_names_survive_untouched() {
        let parsed = parse_list("تقرير الاجتماع.pdf\nكتاب الطالب.docx");
        assert_eq!(parsed.entries, ["تقرير الاجتماع.pdf", "كتاب الطالب.docx"]);
    }

    /// A classic-Mac export or an old mail-client copy separates names with a
    /// bare carriage return. Splitting only on LF turned such a file into one
    /// entry containing every name, which then searched for nothing and exported
    /// as `_x000D_`-escaped noise.
    #[test]
    fn a_cr_only_list_is_split_into_names() {
        let parsed = parse_list("a.pdf\rb.pdf\rتقرير.pdf");
        assert_eq!(parsed.entries, ["a.pdf", "b.pdf", "تقرير.pdf"]);
        assert_eq!(parsed.dropped_empty, 0);
    }

    /// CRLF must not become a doubled empty entry, and mixed endings must all
    /// separate.
    #[test]
    fn mixed_line_endings_do_not_create_phantom_entries() {
        let parsed = parse_list("a.pdf\r\nb.pdf\nc.pdf\rd.pdf");
        assert_eq!(parsed.entries, ["a.pdf", "b.pdf", "c.pdf", "d.pdf"]);
        assert_eq!(parsed.dropped_empty, 0);
    }
}
