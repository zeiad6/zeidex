//! Reading text that is already text: `.docx`, `.xlsx`, `.odt` and plain text.
//!
//! OCR turns pixels into characters. These formats already hold characters, so
//! running them through a detector would be wrong even if it worked. This
//! module opens them, extracts the text in reading order, and returns the same
//! [`PageResult`] the pipeline returns — so the Studio's text pane, its export
//! and the content index need no second path.
//!
//! Deliberately narrow: no styling, no images, no formulas (only their cached
//! values), no macros, no charts. The goal is "find the invoice by a phrase
//! inside it", not a faithful Word renderer.
//!
//! Nothing here reads the network or writes a temporary file: the package is
//! read into a bounded buffer, the XML is scanned in memory, and the result is
//! text.

use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use crate::error::{OcrError, OcrResult};
use crate::geometry::BBox;
use crate::layout::{PageLayout, Region, RegionKind};
use crate::pipeline::PageResult;

/// Largest package or text file this module will open. A 64 MB `.docx` is a
/// book with images; anything larger is not a document a launcher should index
/// on the UI thread's behalf.
pub const MAX_DOCUMENT_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Largest single zip entry that will be decompressed. `word/document.xml` for
/// a long book is a few megabytes; 32 MB is generous and still refuses a bomb.
const MAX_ENTRY_BYTES: u64 = 32 * 1024 * 1024;

/// Most characters one document may contribute. The content index caps a body
/// at 400k characters; this is the ceiling for what the Studio will display and
/// hand to export, so a malformed file cannot exhaust memory.
pub const MAX_DOCUMENT_CHARS: usize = 2_000_000;

/// Most pages one document may produce. A text file with thousands of form
/// feeds is a log, not a document.
const MAX_PAGES: usize = 500;

/// Widest spreadsheet row kept. Beyond this a row is a data dump, and the
/// Studio's table would be unreadable.
const MAX_XLSX_COLUMNS: usize = 200;

/// Most sheets read from one workbook.
const MAX_XLSX_SHEETS: usize = 100;

/// A text page has no image, so its box is a nominal A4 canvas at 150 dpi. The
/// number exists because a region needs one, not because anything was measured.
const PAGE_WIDTH: u32 = 1240;
const PAGE_HEIGHT: u32 = 1754;

/// Extract the text of a document the OCR pipeline cannot read as an image.
///
/// The extension selects the reader. An unknown extension is an error rather
/// than a guess: opening a binary as text produces mojibake that looks like a
/// successful extraction.
pub fn extract_document(path: &Path) -> OcrResult<Vec<PageResult>> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let bytes = read_bounded(path)?;

    let pages = match extension.as_str() {
        "docx" => docx_pages(&bytes)?,
        "xlsx" => xlsx_pages(&bytes)?,
        "odt" => odt_pages(&bytes)?,
        "txt" | "md" | "markdown" | "csv" | "tsv" | "log" => text_pages(&bytes)?,
        other => {
            return Err(OcrError::Invalid(format!(
                "unsupported document type: .{other}"
            )));
        }
    };

    into_pages(pages)
}

/// Read at most [`MAX_DOCUMENT_FILE_BYTES`], refusing a larger file before it
/// is loaded. `take` plus one extra byte distinguishes "exactly at the limit"
/// from "over it" without trusting a length that can change under us.
fn read_bounded(path: &Path) -> OcrResult<Vec<u8>> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_DOCUMENT_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_DOCUMENT_FILE_BYTES {
        return Err(OcrError::Invalid(format!(
            "the file is larger than {} MB",
            MAX_DOCUMENT_FILE_BYTES / (1024 * 1024)
        )));
    }
    Ok(bytes)
}

/// Decode a plain text file.
///
/// UTF-8 (with or without a BOM) and UTF-16 (with a BOM, or with the zero-byte
/// pattern of BOM-less Latin text) are read directly. A file that is neither is
/// tried as one of the two legacy code pages this product's users actually
/// write: Windows-1256 for Arabic and Windows-1252 for Western text. Refusing
/// them outright, which this function used to do, means a `.txt` saved by
/// Arabic Windows opens as an error — a real gap for an Arabic-first tool.
///
/// The guess is deliberately narrow and stated, not statistical: both code
/// pages map almost every byte, so both "succeed", and the only question that
/// matters is whether the bytes are Arabic. If at least a tenth of the
/// non-space characters land in the Arabic block under Windows-1256, that is
/// the file's encoding; otherwise the Western reading is used. A file shorter
/// than [`MIN_LEGACY_BYTES`] is refused because two bytes are not evidence, and
/// a file whose decode is mostly control characters or symbols is refused
/// because that is a binary, not text.
fn decode_text(bytes: &[u8]) -> OcrResult<String> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return std::str::from_utf8(rest)
            .map(str::to_owned)
            .map_err(|_| OcrError::Invalid(NOT_UTF8.to_string()));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return Ok(decode_utf16(rest, true));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return Ok(decode_utf16(rest, false));
    }
    // Before UTF-8: a BOM-less UTF-16LE file is valid UTF-8 made of letters and
    // NULs, and reading it as UTF-8 would keep the NULs.
    if looks_like_utf16_le(bytes) {
        return Ok(decode_utf16(bytes, true));
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(text.to_owned());
    }
    decode_legacy(bytes)
}

const NOT_UTF8: &str = "the file is not UTF-8 or UTF-16 text, and its bytes do not read as \
     Windows-1256 or Windows-1252 either; save it as UTF-8 and open it again";

/// Below this, a legacy guess is not evidence: almost any two bytes decode as
/// *something* in a single-byte code page.
const MIN_LEGACY_BYTES: usize = 16;

/// Percent of non-space characters that must be Arabic before Windows-1256 is
/// chosen. Arabic prose runs far above this; a Western file with a few accents
/// reads as a handful of Arabic letters under 1256 and stays below it.
const ARABIC_SHARE_PERCENT: usize = 10;

/// Percent of a decoded file that must be letters, digits, whitespace or ASCII
/// punctuation. A binary renamed `.txt` decodes to control characters and
/// symbols; real prose does not.
const TEXT_SHARE_PERCENT: usize = 75;

fn decode_legacy(bytes: &[u8]) -> OcrResult<String> {
    if bytes.len() < MIN_LEGACY_BYTES {
        return Err(OcrError::Invalid(NOT_UTF8.to_string()));
    }
    // `without_replacement` returns `None` when a byte is undefined in the code
    // page. Real text does not contain those bytes, so a page that cannot map
    // the whole file is not the page it was written in.
    let arabic =
        encoding_rs::WINDOWS_1256.decode_without_bom_handling_and_without_replacement(bytes);
    let western =
        encoding_rs::WINDOWS_1252.decode_without_bom_handling_and_without_replacement(bytes);

    if let Some(text) = &arabic
        && text_share(text) >= TEXT_SHARE_PERCENT
        && arabic_share(text) >= ARABIC_SHARE_PERCENT
    {
        return Ok(text.to_string());
    }
    if let Some(text) = &western
        && text_share(text) >= TEXT_SHARE_PERCENT
    {
        return Ok(text.to_string());
    }
    Err(OcrError::Invalid(NOT_UTF8.to_string()))
}

/// Percent of non-space characters in the Arabic block.
fn arabic_share(text: &str) -> usize {
    let mut arabic = 0usize;
    let mut total = 0usize;
    for ch in text.chars().filter(|ch| !ch.is_whitespace()) {
        total += 1;
        if ('\u{0600}'..='\u{06FF}').contains(&ch) {
            arabic += 1;
        }
    }
    percent(arabic, total)
}

/// Percent of characters that look like text rather than like a decoded binary.
fn text_share(text: &str) -> usize {
    let mut readable = 0usize;
    let mut total = 0usize;
    for ch in text.chars() {
        total += 1;
        if ch.is_alphanumeric() || ch.is_whitespace() || ch.is_ascii_punctuation() {
            readable += 1;
        }
    }
    percent(readable, total)
}

fn percent(part: usize, whole: usize) -> usize {
    part.saturating_mul(100).checked_div(whole).unwrap_or(0)
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> String {
    let (pairs, _) = bytes.as_chunks::<2>();
    let units: Vec<u16> = pairs
        .iter()
        .map(|pair| {
            if little_endian {
                u16::from_le_bytes(*pair)
            } else {
                u16::from_be_bytes(*pair)
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

/// BOM-less UTF-16LE is common on Windows. Latin text in UTF-16LE is half zero
/// bytes on the odd offsets; UTF-8 never has a zero byte in a letter. Arabic in
/// UTF-16LE does not show that pattern, which is why it needs a BOM.
fn looks_like_utf16_le(bytes: &[u8]) -> bool {
    if bytes.len() < 4 || !bytes.len().is_multiple_of(2) {
        return false;
    }
    let pairs = bytes.len() / 2;
    let zeros = bytes.iter().skip(1).step_by(2).filter(|b| **b == 0).count();
    zeros * 2 >= pairs && !bytes.iter().step_by(2).all(|b| *b == 0)
}

/// One event from the small XML scanner below.
#[derive(Clone, Debug, PartialEq, Eq)]
enum XmlEvent<'a> {
    Start {
        name: &'a str,
        attrs: &'a str,
        self_closing: bool,
    },
    End {
        name: &'a str,
    },
    /// Character data with entities already decoded.
    Text(String),
}

/// Scan an XML document and report tags and character data.
///
/// This is not a parser. It is enough for the two XML dialects this module
/// reads (WordprocessingML and SpreadsheetML), both of which are machine-written
/// and well-formed: no CDATA in the parts we read, no namespaces that change
/// meaning, no external entities. A real XML parser would be a dependency for
/// attributes we do not need.
fn scan_xml<F: for<'a> FnMut(XmlEvent<'a>)>(xml: &str, mut on_event: F) {
    let bytes = xml.as_bytes();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if bytes[cursor] != b'<' {
            let end = xml[cursor..]
                .find('<')
                .map(|offset| cursor + offset)
                .unwrap_or(bytes.len());
            on_event(XmlEvent::Text(decode_entities(&xml[cursor..end])));
            cursor = end;
            continue;
        }
        if xml[cursor..].starts_with("<!--") {
            cursor = find_after(xml, cursor + 4, "-->");
            continue;
        }
        if xml[cursor..].starts_with("<![CDATA[") {
            let start = cursor + 9;
            let end = xml[start..]
                .find("]]>")
                .map(|offset| start + offset)
                .unwrap_or(bytes.len());
            on_event(XmlEvent::Text(xml[start..end].to_string()));
            cursor = find_after(xml, end, "]]>");
            continue;
        }
        if xml[cursor..].starts_with("<?") {
            cursor = find_after(xml, cursor + 2, "?>");
            continue;
        }
        if xml[cursor..].starts_with("<!") {
            cursor = find_after(xml, cursor + 2, ">");
            continue;
        }
        let Some(end) = tag_end(xml, cursor) else {
            break;
        };
        let inner = &xml[cursor + 1..end];
        cursor = end + 1;
        if let Some(name) = inner.strip_prefix('/') {
            on_event(XmlEvent::End { name: name.trim() });
            continue;
        }
        let self_closing = inner.trim_end().ends_with('/');
        let inner = inner.trim_end().trim_end_matches('/');
        let name_end = inner
            .find(|ch: char| ch.is_whitespace())
            .unwrap_or(inner.len());
        on_event(XmlEvent::Start {
            name: inner[..name_end].trim(),
            attrs: &inner[name_end..],
            self_closing,
        });
    }
}

fn find_after(xml: &str, from: usize, needle: &str) -> usize {
    xml[from..]
        .find(needle)
        .map(|offset| from + offset + needle.len())
        .unwrap_or(xml.len())
}

/// The index of the `>` that closes the tag starting at `start`, skipping a `>`
/// inside a quoted attribute value.
fn tag_end(xml: &str, start: usize) -> Option<usize> {
    let bytes = xml.as_bytes();
    let mut quote: Option<u8> = None;
    let mut index = start + 1;
    while index < bytes.len() {
        let byte = bytes[index];
        match quote {
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None if byte == b'"' || byte == b'\'' => quote = Some(byte),
            None if byte == b'>' => return Some(index),
            None => {}
        }
        index += 1;
    }
    None
}

/// The value of one attribute, or `None`. Matching starts at a word boundary so
/// a lookup for `id` does not find `r:id`.
fn attribute<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let mut from = 0usize;
    while let Some(offset) = attrs[from..].find(name) {
        let start = from + offset;
        from = start + name.len();
        let boundary_before = start == 0 || attrs.as_bytes()[start - 1].is_ascii_whitespace();
        if !boundary_before {
            continue;
        }
        let rest = &attrs[start + name.len()..];
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(quote) = rest.chars().next().filter(|ch| *ch == '"' || *ch == '\'') else {
            continue;
        };
        let value = &rest[quote.len_utf8()..];
        let end = value.find(quote)?;
        return Some(&value[..end]);
    }
    None
}

/// Decode the five XML entities and numeric references. An unknown entity is
/// kept verbatim: it is more honest to show `&nbsp;` than to drop it.
fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find(';') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let entity = &after[..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => {
                if let Some(hex) = entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                {
                    u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                } else {
                    entity
                        .strip_prefix('#')
                        .and_then(|digits| digits.parse::<u32>().ok())
                        .and_then(char::from_u32)
                }
            }
        };
        match decoded {
            Some(ch) => out.push(ch),
            None => {
                out.push('&');
                out.push_str(entity);
                out.push(';');
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Turn page texts into `PageResult`s, dropping empty pages and enforcing the
/// total character cap.
fn into_pages(texts: Vec<String>) -> OcrResult<Vec<PageResult>> {
    let mut pages = Vec::new();
    let mut budget = MAX_DOCUMENT_CHARS;
    for text in texts {
        if pages.len() >= MAX_PAGES || budget == 0 {
            break;
        }
        let text: String = text.chars().take(budget).collect();
        budget -= text.chars().count();
        let text = text.trim_end().to_string();
        if text.trim().is_empty() {
            continue;
        }
        pages.push(PageResult {
            page_index: pages.len(),
            width: PAGE_WIDTH,
            height: PAGE_HEIGHT,
            layout: PageLayout {
                width: PAGE_WIDTH,
                height: PAGE_HEIGHT,
                regions: vec![Region {
                    kind: RegionKind::Text,
                    bbox: BBox::new(0.0, 0.0, PAGE_WIDTH as f32, PAGE_HEIGHT as f32),
                    confidence: 1.0,
                    text: Some(text.clone()),
                    table: None,
                }],
            },
            text,
        });
    }
    if pages.is_empty() {
        return Err(OcrError::Invalid(
            "the document contains no extractable text".to_string(),
        ));
    }
    Ok(pages)
}

/// Read one entry of a zip package as UTF-8 text, with a size cap.
fn read_zip_entry(package: &[u8], name: &str) -> OcrResult<String> {
    read_zip_entry_optional(package, name)?
        .ok_or_else(|| OcrError::Invalid(format!("the package has no {name}")))
}

fn read_zip_entry_optional(package: &[u8], name: &str) -> OcrResult<Option<String>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(package))
        .map_err(|err| OcrError::Invalid(format!("not a valid Office package: {err}")))?;
    let entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(err) => return Err(OcrError::Invalid(format!("cannot read {name}: {err}"))),
    };
    if entry.size() > MAX_ENTRY_BYTES {
        return Err(OcrError::Invalid(format!(
            "{name} is larger than the reader allows"
        )));
    }
    let mut text = String::new();
    entry
        .take(MAX_ENTRY_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|err| OcrError::Invalid(format!("{name} is not readable text: {err}")))?;
    // The declared size can lie. Reading one byte past the cap catches an entry
    // that expands beyond it, instead of scanning a silently truncated XML and
    // reporting half a document as complete.
    if text.len() as u64 > MAX_ENTRY_BYTES {
        return Err(OcrError::Invalid(format!(
            "{name} expands beyond the reader's limit"
        )));
    }
    Ok(Some(text))
}

/// Pages of a `.docx`, split where Word recorded a page break.
///
/// Word does not store pagination in `document.xml` unless it has laid the
/// document out; it writes `<w:lastRenderedPageBreak/>` when it has, and an
/// explicit `<w:br w:type="page"/>` always. A document with neither becomes one
/// page, which is honest: the text is complete, the page count is unknown.
fn docx_pages(package: &[u8]) -> OcrResult<Vec<String>> {
    let xml = read_zip_entry(package, "word/document.xml")?;
    let mut pages: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_text = false;

    scan_xml(&xml, |event| match event {
        XmlEvent::Start { name: "w:t", .. } => in_text = true,
        XmlEvent::End { name: "w:t" } => in_text = false,
        XmlEvent::Text(text) if in_text => current.push_str(&text),
        XmlEvent::Start { name: "w:tab", .. } => current.push('\t'),
        XmlEvent::Start { name: "w:cr", .. } => current.push('\n'),
        XmlEvent::Start {
            name: "w:br",
            attrs,
            ..
        } => {
            if attrs.contains("page") {
                pages.push(std::mem::take(&mut current));
            } else {
                current.push('\n');
            }
        }
        XmlEvent::Start {
            name: "w:lastRenderedPageBreak",
            ..
        } => pages.push(std::mem::take(&mut current)),
        XmlEvent::End { name: "w:p" } => current.push('\n'),
        _ => {}
    });
    pages.push(current);
    Ok(pages)
}

/// Pages of an `.odt`, split where the writer recorded a soft page break.
///
/// ODF keeps text in `<text:p>` and `<text:h>` inside `<office:text>`; the
/// pagination of the original layout is not in the file unless a soft break was
/// written, so a document without one becomes a single page — the same honesty
/// as a `.docx` with no recorded break. Runs of spaces are `<text:s text:c="n"/>`
/// rather than literal spaces, and `<text:tab/>` / `<text:line-break/>` are the
/// other whitespace events; all three are restored so a table row keeps its
/// columns and a paragraph keeps its shape.
fn odt_pages(package: &[u8]) -> OcrResult<Vec<String>> {
    let xml = read_zip_entry(package, "content.xml")?;
    let mut pages: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_paragraph = false;

    scan_xml(&xml, |event| match event {
        XmlEvent::Start { name, .. } if name == "text:p" || name == "text:h" => {
            in_paragraph = true;
        }
        XmlEvent::End { name } if name == "text:p" || name == "text:h" => {
            in_paragraph = false;
            if !current.ends_with('\n') {
                current.push('\n');
            }
        }
        XmlEvent::Text(text) if in_paragraph => current.push_str(&text),
        XmlEvent::Start { name, .. } if in_paragraph && name == "text:tab" => current.push('\t'),
        XmlEvent::Start { name, .. } if in_paragraph && name == "text:line-break" => {
            current.push('\n');
        }
        XmlEvent::Start {
            name,
            attrs,
            self_closing: _,
        } if in_paragraph && name == "text:s" => {
            // `<text:s/>` is one space; `text:c` is how many more. The cap keeps
            // a malformed count from allocating a page of spaces.
            let count = attribute(attrs, "text:c")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(1)
                .min(64);
            for _ in 0..count {
                current.push(' ');
            }
        }
        XmlEvent::Start {
            name: "text:soft-page-break",
            ..
        } => {
            pages.push(std::mem::take(&mut current));
        }
        _ => {}
    });

    if !current.trim().is_empty() {
        pages.push(current);
    }
    Ok(pages)
}

/// Sheets of a `.xlsx`, one page each: `(sheet name, rows of cells)`.
///
/// Only values are read. A formula shows its cached result, because the cell's
/// `<v>` is what Excel last computed; the formula text itself is not content a
/// user searches for, and evaluating it is out of scope.
/// How a numeric cell should be written out, taken from its style.
///
/// Only the distinctions that change what a reader sees are kept: a date that
/// would otherwise read `45000`, a percentage that would otherwise read `0.5`, a
/// time that would otherwise read `0.75`. Everything else stays exactly as
/// stored, because inventing a format for a plain number would be a guess.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NumberStyle {
    General,
    Date,
    Time,
    DateTime,
    Percent(usize),
}

/// The number formats a workbook declares, and which of them each cell style
/// uses. Absent `styles.xml` means every cell is `General`, which is exactly
/// what a workbook written without styles means.
#[derive(Default)]
struct CellStyles {
    /// Cell style index (the `s` attribute) to number-format id.
    style_num_fmt: Vec<u32>,
    /// Custom format codes by id. Built-in ids are recognised by number.
    codes: std::collections::HashMap<u32, String>,
}

impl CellStyles {
    fn number_style(&self, style_index: Option<usize>) -> NumberStyle {
        let Some(index) = style_index else {
            return NumberStyle::General;
        };
        let Some(id) = self.style_num_fmt.get(index).copied() else {
            return NumberStyle::General;
        };
        match self.codes.get(&id) {
            Some(code) => style_from_code(code),
            None => style_from_builtin(id),
        }
    }
}

/// The built-in formats that change the displayed value. The ids are fixed by
/// ECMA-376; 27–36 and 50–58 are the East Asian date variants, and leaving them
/// out would make a Japanese or Korean workbook print serial numbers.
fn style_from_builtin(id: u32) -> NumberStyle {
    match id {
        9 => NumberStyle::Percent(0),
        10 => NumberStyle::Percent(2),
        14..=17 | 27..=36 | 50..=58 => NumberStyle::Date,
        18..=21 | 45..=47 => NumberStyle::Time,
        22 => NumberStyle::DateTime,
        _ => NumberStyle::General,
    }
}

/// Classify a custom format code.
///
/// Quoted literals, backslash-escaped characters and bracketed sections are
/// removed first: `0"%"` is a plain number followed by the letter, and `[h]:mm`
/// is a duration whose `h` is inside a bracket. What remains is the pattern.
fn style_from_code(code: &str) -> NumberStyle {
    let mut bare = String::new();
    let mut chars = code.chars();
    let mut in_quotes = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' => in_quotes = !in_quotes,
            '\\' => {
                let _ = chars.next();
            }
            '[' if !in_quotes => {
                for inner in chars.by_ref() {
                    if inner == ']' {
                        break;
                    }
                }
            }
            _ if !in_quotes => bare.push(ch.to_ascii_lowercase()),
            _ => {}
        }
    }
    if bare.contains('%') {
        return NumberStyle::Percent(decimals_before_percent(&bare));
    }
    let has_date = bare.contains('y') || bare.contains('d');
    let has_time = bare.contains('h') || bare.contains('s');
    match (has_date, has_time) {
        (true, true) => NumberStyle::DateTime,
        (true, false) => NumberStyle::Date,
        (false, true) => NumberStyle::Time,
        (false, false) => NumberStyle::General,
    }
}

/// Decimal places the format asks for before the percent sign: `0.00%` is two.
fn decimals_before_percent(code: &str) -> usize {
    let Some(percent) = code.find('%') else {
        return 0;
    };
    let head = &code[..percent];
    let Some(dot) = head.rfind('.') else {
        return 0;
    };
    head[dot + 1..]
        .chars()
        .filter(|ch| *ch == '0' || *ch == '#')
        .count()
        .min(6)
}

fn cell_styles(package: &[u8]) -> OcrResult<CellStyles> {
    let Some(xml) = read_zip_entry_optional(package, "xl/styles.xml")? else {
        return Ok(CellStyles::default());
    };
    let mut styles = CellStyles::default();
    let mut in_cell_xfs = false;
    scan_xml(&xml, |event| match event {
        XmlEvent::Start {
            name: "numFmt",
            attrs,
            ..
        } => {
            let id = attribute(attrs, "numFmtId").and_then(|value| value.parse::<u32>().ok());
            let code = attribute(attrs, "formatCode");
            if let (Some(id), Some(code)) = (id, code) {
                styles.codes.insert(id, code.to_string());
            }
        }
        XmlEvent::Start {
            name: "cellXfs", ..
        } => in_cell_xfs = true,
        XmlEvent::End { name: "cellXfs" } => in_cell_xfs = false,
        // `<xf>` also appears under `<cellStyleXfs>`; only the cell formats in
        // `<cellXfs>` are indexed by a cell's `s` attribute.
        XmlEvent::Start {
            name: "xf", attrs, ..
        } if in_cell_xfs => {
            styles.style_num_fmt.push(
                attribute(attrs, "numFmtId")
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(0),
            );
        }
        _ => {}
    });
    Ok(styles)
}

/// Whether the workbook counts days from 1904 (the old Mac default).
fn workbook_uses_1904(workbook: &str) -> bool {
    let mut uses = false;
    scan_xml(workbook, |event| {
        if let XmlEvent::Start {
            name: "workbookPr",
            attrs,
            ..
        } = event
        {
            uses = matches!(attribute(attrs, "date1904"), Some("1") | Some("true"));
        }
    });
    uses
}

/// Render a stored value the way Excel would display it, for the three styles
/// that change the reading. A non-numeric value under a numeric style is left
/// alone: the format describes a number that is not there.
fn format_cell(raw: &str, style: NumberStyle, date1904: bool) -> String {
    if style == NumberStyle::General {
        return raw.to_string();
    }
    let Ok(value) = raw.trim().parse::<f64>() else {
        return raw.to_string();
    };
    if !value.is_finite() {
        return raw.to_string();
    }
    match style {
        NumberStyle::General => raw.to_string(),
        NumberStyle::Percent(decimals) => format!("{:.*}%", decimals, value * 100.0),
        NumberStyle::Date => excel_date(value, date1904),
        NumberStyle::Time => excel_time(value),
        NumberStyle::DateTime => format!("{} {}", excel_date(value, date1904), excel_time(value)),
    }
}

/// A date as `YYYY-MM-DD`.
///
/// ISO rather than the workbook's own pattern: the output is text for search and
/// for the Studio's pane, and an ISO date reads and sorts unambiguously in both
/// languages. The pattern was used to decide *that* this is a date, which is the
/// only question the pattern can answer reliably.
fn excel_date(value: f64, date1904: bool) -> String {
    let serial = value.floor() as i64;
    // Excel's 1900 system contains a 29 February 1900 that never existed, and
    // every serial from 61 on is one day ahead of the real calendar. Serial 60
    // is that phantom day; printing it as a date would be a lie, so it keeps
    // Excel's own label instead.
    if !date1904 && serial == 60 {
        return "1900-02-29".to_string();
    }
    let unix_days = if date1904 {
        serial - 24_107
    } else if serial < 60 {
        serial - 25_568
    } else {
        serial - 25_569
    };
    let (year, month, day) = civil_from_days(unix_days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The time of day in a serial, as `HH:MM:SS`.
fn excel_time(value: f64) -> String {
    let fraction = value - value.floor();
    let seconds = (fraction * 86_400.0).round() as i64;
    let seconds = seconds.rem_euclid(86_400);
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

/// Days since 1970-01-01 to a civil date, by Howard Hinnant's algorithm. No
/// calendar library is needed for one conversion, and this one is exact for
/// every day a spreadsheet can hold.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn xlsx_pages(package: &[u8]) -> OcrResult<Vec<String>> {
    let shared = match read_zip_entry_optional(package, "xl/sharedStrings.xml")? {
        Some(xml) => shared_strings(&xml),
        None => Vec::new(),
    };
    let workbook = read_zip_entry(package, "xl/workbook.xml")?;
    let rels = read_zip_entry_optional(package, "xl/_rels/workbook.xml.rels")?.unwrap_or_default();
    let sheets = workbook_sheets(&workbook, &rels);
    if sheets.is_empty() {
        return Err(OcrError::Invalid(
            "the workbook lists no sheets".to_string(),
        ));
    }

    let multiple = sheets.len() > 1;
    let styles = cell_styles(package)?;
    let date1904 = workbook_uses_1904(&workbook);
    let mut pages = Vec::new();
    for (name, path) in sheets.iter().take(MAX_XLSX_SHEETS) {
        let xml = match read_zip_entry_optional(package, path)? {
            Some(xml) => xml,
            None => continue,
        };
        let rows = sheet_rows(&xml, &shared, &styles, date1904);
        if rows.is_empty() {
            continue;
        }
        let mut text = String::new();
        if multiple {
            text.push_str(name);
            text.push('\n');
        }
        text.push_str(&rows.join("\n"));
        pages.push(text);
    }
    Ok(pages)
}

/// The shared string table, in index order. Phonetic runs (`<rPh>`) are
/// pronunciation guides, not cell content, and are skipped.
fn shared_strings(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_si = false;
    let mut phonetic_depth = 0usize;
    let mut in_text = false;

    scan_xml(xml, |event| match event {
        XmlEvent::Start { name: "si", .. } => {
            in_si = true;
            current.clear();
        }
        XmlEvent::End { name: "si" } => {
            if in_si {
                out.push(std::mem::take(&mut current));
                in_si = false;
            }
        }
        XmlEvent::Start { name: "rPh", .. } => phonetic_depth += 1,
        XmlEvent::End { name: "rPh" } => phonetic_depth = phonetic_depth.saturating_sub(1),
        XmlEvent::Start { name: "t", .. } if in_si && phonetic_depth == 0 => in_text = true,
        XmlEvent::End { name: "t" } => in_text = false,
        XmlEvent::Text(text) if in_si && in_text && phonetic_depth == 0 => current.push_str(&text),
        _ => {}
    });
    out
}

/// Sheet names paired with the package path of their XML, in workbook order.
///
/// The relationship id in `workbook.xml` points into `workbook.xml.rels`; when
/// the rels are missing or do not mention a sheet, the conventional
/// `xl/worksheets/sheet{n}.xml` is used so a minimal package still opens.
fn workbook_sheets(workbook: &str, rels: &str) -> Vec<(String, String)> {
    let mut relationships: Vec<(String, String)> = Vec::new();
    scan_xml(rels, |event| {
        if let XmlEvent::Start {
            name: "Relationship",
            attrs,
            ..
        } = event
            && let (Some(id), Some(target)) = (attribute(attrs, "Id"), attribute(attrs, "Target"))
        {
            relationships.push((id.to_string(), resolve_part(target)));
        }
    });

    let mut sheets = Vec::new();
    scan_xml(workbook, |event| {
        if let XmlEvent::Start {
            name: "sheet",
            attrs,
            ..
        } = event
        {
            let sheet_name = attribute(attrs, "name").unwrap_or("Sheet").to_string();
            let target = attribute(attrs, "r:id")
                .and_then(|id| {
                    relationships
                        .iter()
                        .find(|(relationship, _)| relationship == id)
                        .map(|(_, path)| path.clone())
                })
                .unwrap_or_else(|| format!("xl/worksheets/sheet{}.xml", sheets.len() + 1));
            sheets.push((sheet_name, target));
        }
    });
    sheets
}

/// A relationship target is relative to `xl/` unless it is absolute. `.` and
/// `..` segments are resolved: a writer is allowed to say
/// `../xl/worksheets/sheet1.xml`, and a zip lookup does not normalize paths.
fn resolve_part(target: &str) -> String {
    let joined = if let Some(absolute) = target.strip_prefix('/') {
        absolute.to_string()
    } else if target.starts_with("xl/") {
        target.to_string()
    } else {
        format!("xl/{target}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// One sheet's cells as tab-separated rows. Cells are placed by their `r`
/// reference so a sparse row keeps its columns aligned; trailing empty cells
/// are dropped.
fn sheet_rows(xml: &str, shared: &[String], styles: &CellStyles, date1904: bool) -> Vec<String> {
    let mut rows: Vec<String> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut next_column = 0usize;
    let mut cell_column = 0usize;
    let mut cell_type = String::new();
    let mut cell_style: Option<usize> = None;
    let mut value = String::new();
    let mut in_value = false;
    let mut in_inline = false;
    let mut in_inline_text = false;

    scan_xml(xml, |event| match event {
        XmlEvent::Start { name: "row", .. } => {
            row.clear();
            next_column = 0;
        }
        XmlEvent::Start {
            name: "c",
            attrs,
            self_closing,
        } => {
            cell_column = attribute(attrs, "r")
                .and_then(column_index)
                .unwrap_or(next_column)
                .min(MAX_XLSX_COLUMNS.saturating_sub(1));
            cell_type = attribute(attrs, "t").unwrap_or_default().to_string();
            cell_style = attribute(attrs, "s").and_then(|value| value.parse::<usize>().ok());
            value.clear();
            in_value = false;
            in_inline = false;
            in_inline_text = false;
            // A self-closing cell is empty and still occupies its column.
            if self_closing {
                next_column = cell_column + 1;
            }
        }
        XmlEvent::Start { name: "v", .. } => in_value = true,
        XmlEvent::End { name: "v" } => in_value = false,
        XmlEvent::Start { name: "is", .. } => in_inline = true,
        XmlEvent::Start { name: "t", .. } if in_inline => in_inline_text = true,
        XmlEvent::End { name: "t" } => in_inline_text = false,
        XmlEvent::Text(text) => {
            if in_value || in_inline_text {
                value.push_str(&text);
            }
        }
        XmlEvent::End { name: "c" } => {
            while row.len() < cell_column {
                row.push(String::new());
            }
            let cell = match cell_type.as_str() {
                "s" => value
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| shared.get(index).cloned())
                    .unwrap_or_default(),
                "b" => match value.trim() {
                    "1" => "TRUE".to_string(),
                    "0" => "FALSE".to_string(),
                    other => other.to_string(),
                },
                // A number (typed or produced by a formula) is the only kind a
                // format can change; text, booleans and errors are already what
                // the reader sees.
                "" | "n" => format_cell(&value, styles.number_style(cell_style), date1904),
                _ => value.clone(),
            };
            row.push(cell);
            next_column = cell_column + 1;
        }
        XmlEvent::End { name: "row" } => {
            while row.last().is_some_and(|cell| cell.is_empty()) {
                row.pop();
            }
            if !row.is_empty() {
                rows.push(row.join("\t"));
            }
            row.clear();
        }
        _ => {}
    });
    rows
}

/// Zero-based column of a cell reference such as `B7` or `AA12`.
fn column_index(reference: &str) -> Option<usize> {
    let mut column = 0usize;
    let mut letters = 0usize;
    for ch in reference.chars() {
        let upper = ch.to_ascii_uppercase();
        if !upper.is_ascii_uppercase() {
            break;
        }
        column = column
            .checked_mul(26)?
            .checked_add((upper as u8 - b'A') as usize + 1)?;
        letters += 1;
    }
    (letters > 0).then(|| column - 1)
}

/// Pages of a plain text file, split on the form-feed page break and otherwise
/// returned as one page. `\r\n` becomes `\n`; nothing else is rewritten.
fn text_pages(bytes: &[u8]) -> OcrResult<Vec<String>> {
    let text = decode_text(bytes)?;
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    Ok(text
        .split('\u{000C}')
        .map(|page| page.to_string())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Build an Office package from (name, body) pairs.
    fn package(entries: &[(&str, &str)]) -> Result<Vec<u8>, String> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in entries {
            writer
                .start_file(*name, options)
                .map_err(|err| err.to_string())?;
            writer
                .write_all(body.as_bytes())
                .map_err(|err| err.to_string())?;
        }
        let cursor = writer.finish().map_err(|err| err.to_string())?;
        Ok(cursor.into_inner())
    }

    #[test]
    fn a_docx_keeps_paragraphs_tabs_entities_and_recorded_page_breaks() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:r><w:t>فاتورة</w:t><w:tab/><w:t xml:space="preserve">INV &amp; 42</w:t></w:r></w:p>
<w:p><w:r><w:t>second</w:t><w:br w:type="page"/><w:t>third</w:t></w:r></w:p>
<w:p><w:r><w:t>fourth</w:t></w:r></w:p>
</w:body></w:document>"#;
        let bytes = package(&[("word/document.xml", xml)]).unwrap_or_default();
        assert!(!bytes.is_empty());

        let pages = docx_pages(&bytes).unwrap_or_default();
        assert_eq!(pages.len(), 2, "{pages:?}");
        // The break sits mid-paragraph: text before it belongs to page one.
        assert_eq!(pages[0].trim_end(), "فاتورة\tINV & 42\nsecond");
        assert!(pages[1].starts_with("third"), "{:?}", pages[1]);
        assert!(pages[1].contains("fourth"), "{:?}", pages[1]);
    }

    #[test]
    fn a_docx_without_page_breaks_is_one_page() {
        let xml = r#"<w:document><w:body><w:p><w:r><w:t>one</w:t></w:r></w:p>
<w:p><w:r><w:t>two</w:t></w:r></w:p></w:body></w:document>"#;
        let bytes = package(&[("word/document.xml", xml)]).unwrap_or_default();
        let pages = docx_pages(&bytes).unwrap_or_default();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].trim_end(), "one\ntwo");
    }

    #[test]
    fn a_missing_part_is_an_error_not_an_empty_document() {
        let bytes = package(&[("docProps/app.xml", "<Properties/>")]).unwrap_or_default();
        assert!(docx_pages(&bytes).is_err());
        assert!(into_pages(vec!["   ".to_string()]).is_err());
    }

    #[test]
    fn an_xlsx_places_cells_by_reference_and_resolves_shared_strings() {
        let shared = r#"<sst>
<si><t>مرحبا</t></si>
<si><r><t>Hello </t></r><r><t>world</t></r></si>
<si><t>kept</t><rPh><t>phonetic</t></rPh></si>
</sst>"#;
        let workbook = r#"<workbook><sheets>
<sheet name="Data" sheetId="1" r:id="rId1"/>
<sheet name="Second" sheetId="2" r:id="rId2"/>
</sheets></workbook>"#;
        let rels = r#"<Relationships>
<Relationship Id="rId1" Target="worksheets/sheet1.xml"/>
<Relationship Id="rId2" Target="/xl/worksheets/sheet2.xml"/>
</Relationships>"#;
        let sheet1 = r#"<worksheet><sheetData>
<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c>
<c r="C1" t="inlineStr"><is><t>inline</t></is></c><c r="D1" t="s"><v>2</v></c></row>
<row r="2"><c r="A2"><v>42</v></c><c r="C2" t="b"><v>1</v></c></row>
</sheetData></worksheet>"#;
        let sheet2 = r#"<worksheet><sheetData><row r="1"><c r="A1"><v>7</v></c></row></sheetData></worksheet>"#;
        let bytes = package(&[
            ("xl/sharedStrings.xml", shared),
            ("xl/workbook.xml", workbook),
            ("xl/_rels/workbook.xml.rels", rels),
            ("xl/worksheets/sheet1.xml", sheet1),
            ("xl/worksheets/sheet2.xml", sheet2),
        ])
        .unwrap_or_default();

        let pages = xlsx_pages(&bytes).unwrap_or_default();
        assert_eq!(pages.len(), 2, "{pages:?}");
        assert_eq!(
            pages[0],
            "Data\nمرحبا\tHello world\tinline\tkept\n42\t\tTRUE"
        );
        assert_eq!(pages[1], "Second\n7");
    }

    #[test]
    fn a_spreadsheet_date_percentage_and_datetime_read_as_excel_shows_them() {
        let styles = r#"<styleSheet>
<numFmts count="1"><numFmt numFmtId="164" formatCode="0.0%"/></numFmts>
<cellStyleXfs count="1"><xf numFmtId="0"/></cellStyleXfs>
<cellXfs count="4"><xf numFmtId="0"/><xf numFmtId="14"/><xf numFmtId="164"/><xf numFmtId="22"/></cellXfs>
</styleSheet>"#;
        let sheet = r#"<worksheet><sheetData><row r="1">
<c r="A1" s="1"><v>45000</v></c><c r="B1" s="2"><v>0.5</v></c><c r="C1" s="3"><v>45000.5</v></c>
<c r="D1" s="1" t="s"><v>0</v></c><c r="E1"><v>45000</v></c>
</row></sheetData></worksheet>"#;
        let workbook = r#"<workbook><sheets><sheet name="Only" r:id="rId1"/></sheets></workbook>"#;
        let rels = r#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/></Relationships>"#;
        let shared = r#"<sst><si><t>not a date</t></si></sst>"#;
        let bytes = package(&[
            ("xl/workbook.xml", workbook),
            ("xl/_rels/workbook.xml.rels", rels),
            ("xl/worksheets/sheet1.xml", sheet),
            ("xl/styles.xml", styles),
            ("xl/sharedStrings.xml", shared),
        ])
        .unwrap_or_default();

        let pages = xlsx_pages(&bytes).unwrap_or_default();
        assert_eq!(pages.len(), 1);
        // A styled text cell keeps its text, and an unstyled number keeps its
        // digits: only the style decides, never the value's shape.
        assert_eq!(
            pages[0],
            "2023-03-15\t50.0%\t2023-03-15 12:00:00\tnot a date\t45000"
        );
    }

    #[test]
    fn excel_dates_handle_the_1900_leap_bug_and_the_1904_system() {
        assert_eq!(excel_date(1.0, false), "1900-01-01");
        assert_eq!(excel_date(59.0, false), "1900-02-28");
        assert_eq!(excel_date(60.0, false), "1900-02-29");
        assert_eq!(excel_date(61.0, false), "1900-03-01");
        assert_eq!(excel_date(0.0, true), "1904-01-01");
        // A negative serial is not a date Excel can display; the conversion
        // still produces one rather than panicking.
        assert_eq!(excel_date(-1.0, false), "1899-12-30");
    }

    #[test]
    fn a_format_code_decides_percent_date_and_time_without_quoted_literals() {
        assert_eq!(style_from_code("0.00%"), NumberStyle::Percent(2));
        assert_eq!(style_from_code("0%"), NumberStyle::Percent(0));
        assert_eq!(style_from_code(r#"0"%"#), NumberStyle::General);
        assert_eq!(style_from_code("yyyy-mm-dd"), NumberStyle::Date);
        assert_eq!(style_from_code("hh:mm:ss"), NumberStyle::Time);
        assert_eq!(style_from_code("yyyy-mm-dd hh:mm"), NumberStyle::DateTime);
        assert_eq!(style_from_code("[h]:mm:ss"), NumberStyle::Time);
        assert_eq!(style_from_code("#,##0.00"), NumberStyle::General);
        assert_eq!(style_from_builtin(14), NumberStyle::Date);
        assert_eq!(style_from_builtin(22), NumberStyle::DateTime);
        assert_eq!(style_from_builtin(9), NumberStyle::Percent(0));
        assert_eq!(style_from_builtin(0), NumberStyle::General);
    }

    #[test]
    fn a_single_sheet_has_no_name_heading() {
        let workbook = r#"<workbook><sheets><sheet name="Only" r:id="rId1"/></sheets></workbook>"#;
        let rels = r#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/></Relationships>"#;
        let sheet = r#"<worksheet><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#;
        let bytes = package(&[
            ("xl/workbook.xml", workbook),
            ("xl/_rels/workbook.xml.rels", rels),
            ("xl/worksheets/sheet1.xml", sheet),
        ])
        .unwrap_or_default();
        let pages = xlsx_pages(&bytes).unwrap_or_default();
        assert_eq!(pages, vec!["1".to_string()]);
    }

    #[test]
    fn a_text_file_splits_on_form_feed_and_normalizes_line_endings() {
        let pages = text_pages(b"first\r\nline\x0csecond page\r\n").unwrap_or_default();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0], "first\nline");
        assert_eq!(pages[1], "second page\n");
    }

    #[test]
    fn utf16_with_a_bom_is_decoded_and_invalid_utf8_is_refused() {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in "مرحبا".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(decode_text(&bytes).unwrap_or_default(), "مرحبا");

        let err = decode_text(&[0xC3, 0x28]).err().map(|err| err.to_string());
        assert!(
            err.as_ref().is_some_and(|err| err.contains("UTF-8")),
            "{err:?}"
        );
    }

    #[test]
    fn cell_references_map_to_zero_based_columns() {
        assert_eq!(column_index("A"), Some(0));
        assert_eq!(column_index("Z"), Some(25));
        assert_eq!(column_index("AA"), Some(26));
        assert_eq!(column_index("B7"), Some(1));
        assert_eq!(column_index("7"), None);
        assert_eq!(column_index(""), None);
    }

    #[test]
    fn relationship_targets_resolve_dots_and_absolute_paths() {
        assert_eq!(
            resolve_part("worksheets/sheet1.xml"),
            "xl/worksheets/sheet1.xml"
        );
        assert_eq!(
            resolve_part("/xl/worksheets/sheet1.xml"),
            "xl/worksheets/sheet1.xml"
        );
        assert_eq!(
            resolve_part("../xl/worksheets/sheet1.xml"),
            "xl/worksheets/sheet1.xml"
        );
        assert_eq!(resolve_part("./sheet1.xml"), "xl/sheet1.xml");
    }

    #[test]
    fn entities_decode_and_attribute_lookup_respects_namespaces() {
        assert_eq!(decode_entities("a&amp;b&#1585;&#x645;"), "a&bرم");
        assert_eq!(decode_entities("&unknown;"), "&unknown;");
        let attrs = r#" r:id="rId3" name="x""#;
        assert_eq!(attribute(attrs, "r:id"), Some("rId3"));
        assert_eq!(attribute(attrs, "id"), None);
        assert_eq!(attribute(attrs, "name"), Some("x"));
    }

    #[test]
    fn extract_document_reads_a_real_text_file_and_refuses_unknown_types() {
        let dir = match tempfile::tempdir() {
            Ok(dir) => dir,
            Err(err) => panic!("temp dir: {err}"),
        };
        let path = dir.path().join("فاتورة.txt");
        assert!(std::fs::write(&path, "بند واحد\nبند اثنان\n").is_ok());

        let pages = extract_document(&path).unwrap_or_default();
        assert_eq!(pages.len(), 1);
        assert!(pages[0].text.contains("بند اثنان"), "{}", pages[0].text);
        assert_eq!(pages[0].layout.regions.len(), 1);

        let binary = dir.path().join("data.bin");
        assert!(std::fs::write(&binary, [0u8, 1, 2]).is_ok());
        assert!(extract_document(&binary).is_err());
    }

    #[test]
    fn the_character_budget_stops_a_runaway_page() {
        let huge = "ا".repeat(MAX_DOCUMENT_CHARS + 100);
        let pages = into_pages(vec![huge]).unwrap_or_default();
        assert_eq!(pages[0].text.chars().count(), MAX_DOCUMENT_CHARS);
    }

    /// Bytes from a hex string, so a test can hold a real code-page file.
    fn hex(text: &str) -> Vec<u8> {
        let digits: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        digits
            .chunks(2)
            .filter_map(|pair| {
                let high = (pair[0] as char).to_digit(16)?;
                let low = (pair.get(1).copied()? as char).to_digit(16)?;
                Some((high * 16 + low) as u8)
            })
            .collect()
    }

    #[test]
    fn arabic_windows_1256_text_is_decoded_instead_of_refused() {
        // "فاتورة رقم 123 بتاريخ 2024" as Arabic Windows writes it.
        let bytes = hex("ddc7cae6d1c920d1dee32031323320c8cac7d1edce2032303234");
        assert_eq!(bytes.len(), 26);
        assert_eq!(
            decode_text(&bytes).unwrap_or_default(),
            "فاتورة رقم 123 بتاريخ 2024"
        );
    }

    #[test]
    fn western_windows_1252_text_is_decoded_without_being_read_as_arabic() {
        // "Grüße aus München: Bericht über 2024" — one byte (0xFC) lands in the
        // Arabic block when read as 1256, which is why the share, not the
        // presence of a single Arabic letter, decides.
        let bytes = hex("4772fcdf6520617573204dfc6e6368656e3a204265726963687420fc6265722032303234");
        assert_eq!(
            decode_text(&bytes).unwrap_or_default(),
            "Grüße aus München: Bericht über 2024"
        );
    }

    #[test]
    fn a_binary_file_renamed_to_text_is_refused_rather_than_mojibaked() {
        // A PNG header and the first chunk: every byte maps in 1252, so only the
        // text-share guard can tell this apart from prose.
        let mut bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend_from_slice(&[
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
            0x01, 0x00, 0x08, 0x06, 0x00, 0x00, 0x00,
        ]);
        let err = decode_text(&bytes).err().map(|err| err.to_string());
        assert!(err.is_some(), "a PNG must not decode as text");
    }

    #[test]
    fn an_odt_keeps_paragraphs_tabs_spaces_and_soft_page_breaks() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:text>
<text:h text:outline-level="1">فاتورة</text:h>
<text:p>INV<text:tab/>42<text:s text:c="3"/>end</text:p>
<text:p>second<text:soft-page-break/>third</text:p>
</office:text></office:body></office:document-content>"#;
        let bytes = package(&[("content.xml", xml)]).unwrap_or_default();
        let pages = odt_pages(&bytes).unwrap_or_default();
        assert_eq!(pages.len(), 2, "{pages:?}");
        assert_eq!(pages[0], "فاتورة\nINV\t42   end\nsecond");
        assert_eq!(pages[1], "third\n");
    }

    #[test]
    fn extract_document_dispatches_to_the_odt_reader() {
        let dir = match tempfile::tempdir() {
            Ok(dir) => dir,
            Err(err) => panic!("temp dir: {err}"),
        };
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:text><text:p>بند واحد</text:p></office:text></office:body></office:document-content>"#;
        let bytes = package(&[("content.xml", xml)]).unwrap_or_default();
        let path = dir.path().join("تقرير.odt");
        assert!(std::fs::write(&path, &bytes).is_ok());
        let pages = extract_document(&path).unwrap_or_default();
        assert_eq!(pages.len(), 1);
        assert!(pages[0].text.contains("بند واحد"), "{}", pages[0].text);
    }
}
