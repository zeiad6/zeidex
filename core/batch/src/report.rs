//! The `.xlsx` report a batch search leaves behind.
//!
//! Four sheets, because the questions are different: what happened (summary),
//! what was found (results), what was not (missing), and which names need a
//! human decision (multiple matches). The results sheet is one flat table so
//! Excel's own filter and sort work on it without a helper column; the other
//! three are for reading.
//!
//! Formatting is deliberately restrained: one dark header, one accent, zebra
//! rows, and a status colour that carries meaning. Numbers are stored as
//! numbers with a format (a percentage is a real percentage, a date is a real
//! date), so sorting and summing behave the way a spreadsheet user expects.

use std::path::Path;

use rust_xlsxwriter::{
    Color, ExcelDateTime, Format, FormatAlign, FormatBorder, Workbook, Worksheet,
};

use crate::error::BatchResult;
use crate::matcher::{BatchEntryResult, BatchMatch, BatchReport, BatchStatus, FileKind};

const INK: Color = Color::RGB(0x0F_172A);
const MUTED: Color = Color::RGB(0x64_748B);
const ACCENT: Color = Color::RGB(0x06_B6_D4);
const HEADER_BG: Color = Color::RGB(0x11_1827);
const HEADER_LINE: Color = Color::RGB(0x33_4155);
const ZEBRA: Color = Color::RGB(0xF8_FAFC);
const HAIRLINE: Color = Color::RGB(0xE2_E8F0);
const GOOD: Color = Color::RGB(0x04_78_57);
const WARN: Color = Color::RGB(0xB4_5309);
const BAD: Color = Color::RGB(0xB9_1C1C);

/// What the report says about the run that is not in the results themselves.
pub struct ReportMeta<'a> {
    /// Seconds since the Unix epoch, UTC.
    pub generated_unix_s: i64,
    pub app_version: &'a str,
}

/// The columns of the flat results sheet, in order.
const RESULT_HEADERS: [&str; 11] = [
    "Requested Filename",
    "Status",
    "Matched Filename",
    "Match %",
    "Match Type",
    "Extension",
    "File Size",
    "File Path",
    "Last Modified",
    "Search Time",
    "Notes",
];

const RESULT_WIDTHS: [f64; 11] = [
    34.0, 15.0, 34.0, 9.0, 14.0, 10.0, 12.0, 58.0, 18.0, 12.0, 22.0,
];

/// Writes the whole report to `path`.
pub fn export_report(
    report: &BatchReport,
    meta: &ReportMeta<'_>,
    path: impl AsRef<Path>,
) -> BatchResult<()> {
    let mut workbook = Workbook::new();
    write_summary(&mut workbook, report, meta)?;
    write_results(&mut workbook, report)?;
    write_missing(&mut workbook, report)?;
    write_multiple(&mut workbook, report)?;
    workbook.save(path)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Formats
// ---------------------------------------------------------------------------

fn brand() -> Format {
    Format::new()
        .set_bold()
        .set_font_size(22.0)
        .set_font_color(ACCENT)
}

fn title() -> Format {
    Format::new()
        .set_bold()
        .set_font_size(14.0)
        .set_font_color(INK)
}

fn subtitle() -> Format {
    Format::new().set_font_size(9.0).set_font_color(MUTED)
}

fn kpi_label() -> Format {
    Format::new()
        .set_font_size(10.0)
        .set_font_color(MUTED)
        .set_border_bottom(FormatBorder::Thin)
        .set_border_bottom_color(HAIRLINE)
}

fn kpi_value() -> Format {
    Format::new()
        .set_bold()
        .set_font_size(11.0)
        .set_font_color(INK)
        .set_align(FormatAlign::Right)
        .set_border_bottom(FormatBorder::Thin)
        .set_border_bottom_color(HAIRLINE)
}

fn header() -> Format {
    Format::new()
        .set_bold()
        .set_font_size(10.0)
        .set_font_color(Color::White)
        .set_background_color(HEADER_BG)
        .set_border_bottom(FormatBorder::Medium)
        .set_border_bottom_color(HEADER_LINE)
        .set_align(FormatAlign::Left)
        .set_text_wrap()
}

fn cell() -> Format {
    Format::new().set_font_size(10.0).set_font_color(INK)
}

fn zebra() -> Format {
    cell().set_background_color(ZEBRA)
}

fn centered() -> Format {
    cell().set_align(FormatAlign::Center)
}

fn centered_zebra() -> Format {
    zebra().set_align(FormatAlign::Center)
}

fn percent() -> Format {
    Format::new()
        .set_font_size(10.0)
        .set_font_color(INK)
        .set_align(FormatAlign::Center)
        .set_num_format("0%")
}

fn percent_zebra() -> Format {
    percent().set_background_color(ZEBRA)
}

fn datetime() -> Format {
    Format::new()
        .set_font_size(10.0)
        .set_font_color(INK)
        .set_num_format("yyyy-mm-dd hh:mm")
}

fn datetime_zebra() -> Format {
    datetime().set_background_color(ZEBRA)
}

fn duration() -> Format {
    Format::new()
        .set_font_size(10.0)
        .set_font_color(MUTED)
        .set_align(FormatAlign::Right)
        .set_num_format("0\" ms\"")
}

fn duration_zebra() -> Format {
    duration().set_background_color(ZEBRA)
}

/// Status text in its own colour, on the row's own background.
fn status_format(status: BatchStatus, on_zebra: bool) -> Format {
    // Exhaustive on purpose: a new status must be given a colour here rather
    // than silently inheriting the muted one.
    let color = match status {
        BatchStatus::Exact => GOOD,
        BatchStatus::High => WARN,
        BatchStatus::Possible => MUTED,
        BatchStatus::NotFound | BatchStatus::Error => BAD,
    };
    let base = if on_zebra { zebra() } else { cell() };
    base.set_bold().set_font_color(color)
}

fn section_note() -> Format {
    Format::new()
        .set_font_size(9.0)
        .set_font_color(MUTED)
        .set_text_wrap()
}

// ---------------------------------------------------------------------------
// Sheet 1 - Summary
// ---------------------------------------------------------------------------

fn write_summary(
    workbook: &mut Workbook,
    report: &BatchReport,
    meta: &ReportMeta<'_>,
) -> BatchResult<()> {
    let summary = &report.summary;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Summary")?;
    sheet.set_column_width(0, 30.0)?;
    sheet.set_column_width(1, 34.0)?;
    sheet.set_column_width(2, 4.0)?;

    sheet.write_string_with_format(0, 0, "ZEIDEX", &brand())?;
    sheet.write_string_with_format(1, 0, "Batch File Search Report", &title())?;
    sheet.write_string_with_format(
        2,
        0,
        format!(
            "Generated {} UTC  ·  Zeidex {}",
            stamp(meta.generated_unix_s),
            meta.app_version
        ),
        &subtitle(),
    )?;

    // The chart reads from this block, so the labels and values live here rather
    // than being formatted inline into a sentence. The four outcome rows sit
    // together and are mutually exclusive (exact + approximate + not found +
    // errors = requested), so the chart cannot count a found file twice: "Found"
    // is the sum of the two match rows and is deliberately not plotted.
    let rows: [(&str, f64, &str); 10] = [
        ("Requested names", summary.total as f64, "#,##0"),
        ("Found", summary.found as f64, "#,##0"),
        ("Exact matches", summary.exact as f64, "#,##0"),
        ("Approximate matches", summary.approximate as f64, "#,##0"),
        ("Not found", summary.not_found as f64, "#,##0"),
        ("Errors", summary.errors as f64, "#,##0"),
        ("Multiple-match cases", summary.multiple as f64, "#,##0"),
        ("Success rate", summary.success_percent / 100.0, "0.0%"),
        (
            "Matching threshold",
            f64::from(report.threshold) / 100.0,
            "0%",
        ),
        (
            "Search duration",
            summary.duration_ms as f64,
            "#,##0\" ms\"",
        ),
    ];

    let mut row: u32 = 4;
    for (label, value, number_format) in rows {
        sheet.write_string_with_format(row, 0, label, &kpi_label())?;
        sheet.write_number_with_format(
            row,
            1,
            value,
            &kpi_value().set_num_format(number_format),
        )?;
        row += 1;
    }

    // Scope is text, not a number, so it sits in the same grid with the value
    // starting in column B like every other row.
    sheet.write_string_with_format(row, 0, "Search scope", &kpi_label())?;
    sheet.write_string_with_format(
        row,
        1,
        format!(
            "{} indexed files and folders",
            thousands(report.indexed_files)
        ),
        &kpi_value().set_align(FormatAlign::Left),
    )?;

    // One series over the four outcome rows (exact, approximate, not found,
    // errors). They are contiguous and mutually exclusive, so the bars add up to
    // the number of requested names. "Found" is the sum of the first two and is
    // deliberately outside the range: including it made the chart report more
    // outcomes than there were requests.
    let mut chart = rust_xlsxwriter::Chart::new(rust_xlsxwriter::ChartType::Column);
    chart.title().set_name("Outcome of the batch");
    chart.set_width(430);
    chart.set_height(250);
    chart.set_style(10);
    chart.legend().set_hidden();
    chart
        .add_series()
        .set_name("Names")
        .set_values(("Summary", 6, 1, 9, 1))
        .set_categories(("Summary", 6, 0, 9, 0));
    sheet.insert_chart(3, 3, &chart)?;

    // The explanation spans A:C: wrapped inside a 30-character column it would
    // be a tower of five lines per sentence.
    let notes_row = row + 3;
    let notes = [
        "How matching works",
        "Exact means the normalized name, or the name without its extension, equals the request after case, diacritic and Arabic letter-variant folding.",
        "Contains, All words and Fuzzy scores are below 100% by construction, so a 100% threshold accepts equality only.",
        "A name that matched exactly is not padded with fuzzy neighbours: the exact answer is the answer.",
        "Times are UTC. Percentages are stored as numbers, so the sheet sorts and filters them as numbers.",
    ];
    for (offset, line) in notes.iter().enumerate() {
        let format = if offset == 0 {
            title().set_font_size(11.0)
        } else {
            section_note()
        };
        let at = notes_row + offset as u32;
        sheet.merge_range(at, 0, at, 2, line, &format)?;
        // Two lines at 68 characters wide is enough for every note above; the
        // extra height keeps the wrap from clipping.
        sheet.set_row_height(at, if offset == 0 { 20.0 } else { 26.0 })?;
    }

    sheet.set_landscape();
    sheet.set_print_scale(85);
    sheet.set_margins(0.5, 0.5, 0.6, 0.6, 0.3, 0.3);
    sheet.set_footer("&CZeidex · Batch Search · &P / &N");

    Ok(())
}

// ---------------------------------------------------------------------------
// Sheet 2 - Results
// ---------------------------------------------------------------------------

fn write_results(workbook: &mut Workbook, report: &BatchReport) -> BatchResult<()> {
    let sheet = workbook.add_worksheet();
    sheet.set_name("Results")?;
    for (column, width) in RESULT_WIDTHS.iter().enumerate() {
        sheet.set_column_width(column as u16, *width)?;
    }
    for (column, label) in RESULT_HEADERS.iter().enumerate() {
        sheet.write_string_with_format(0, column as u16, *label, &header())?;
    }
    sheet.set_row_height(0, 28.0)?;
    // Row 1 and column A: scrolling right through the path keeps the header and
    // the requested name in view.
    sheet.set_freeze_panes(1, 1)?;

    let mut row: u32 = 1;
    for entry in &report.entries {
        if entry.matches.is_empty() {
            write_result_row(sheet, row, entry, None)?;
            row += 1;
            continue;
        }
        for matched in &entry.matches {
            write_result_row(sheet, row, entry, Some(matched))?;
            row += 1;
        }
    }

    // `row` is one past the last written row; the filter needs the last used
    // row, and a sheet with only the header still gets a valid range.
    let last_row = row.saturating_sub(1);
    sheet.autofilter(0, 0, last_row, RESULT_HEADERS.len() as u16 - 1)?;
    // Printing a 108-row table: landscape, the header on every page, and the
    // header row repeated. Without this a PDF export is a wall of columns split
    // across pages with no labels.
    sheet.set_landscape();
    sheet.set_print_scale(55);
    sheet.set_repeat_rows(0, 0)?;
    sheet.set_print_area(0, 0, last_row, RESULT_HEADERS.len() as u16 - 1)?;
    sheet.set_footer("&CZeidex · Batch Search · &P / &N");
    Ok(())
}

fn write_result_row(
    sheet: &mut Worksheet,
    row: u32,
    entry: &BatchEntryResult,
    matched: Option<&BatchMatch>,
) -> BatchResult<()> {
    let striped = row.is_multiple_of(2);
    let text = if striped { zebra() } else { cell() };
    let middle = if striped {
        centered_zebra()
    } else {
        centered()
    };
    let share = if striped { percent_zebra() } else { percent() };
    let time = if striped {
        datetime_zebra()
    } else {
        datetime()
    };
    let took = if striped {
        duration_zebra()
    } else {
        duration()
    };

    sheet.write_string_with_format(row, 0, cell_text(&entry.requested), &text)?;
    sheet.write_string_with_format(
        row,
        1,
        status_label(entry.status),
        &status_format(entry.status, striped),
    )?;

    match matched {
        Some(found) => {
            sheet.write_string_with_format(row, 2, cell_text(&found.name), &text)?;
            sheet.write_number_with_format(row, 3, f64::from(found.score) / 100.0, &share)?;
            sheet.write_string_with_format(row, 4, found.match_type.label(), &text)?;
            sheet.write_string_with_format(
                row,
                5,
                found.extension.as_deref().unwrap_or("—"),
                &middle,
            )?;
            sheet.write_string_with_format(
                row,
                6,
                size_label(found.size_bytes, found.kind),
                &middle,
            )?;
            sheet.write_string_with_format(row, 7, cell_text(&found.path), &text)?;
            match found.modified_unix_s.and_then(excel_datetime) {
                Some(stamp) => {
                    sheet.write_datetime_with_format(row, 8, &stamp, &time)?;
                }
                None => {
                    sheet.write_string_with_format(row, 8, "—", &middle)?;
                }
            }
        }
        None => {
            for column in 2..9u16 {
                sheet.write_string_with_format(row, column, "", &text)?;
            }
        }
    }

    sheet.write_number_with_format(row, 9, entry.duration_ms as f64, &took)?;
    sheet.write_string_with_format(row, 10, note_for(entry, matched), &text)?;
    Ok(())
}

fn status_label(status: BatchStatus) -> &'static str {
    match status {
        BatchStatus::Exact => "Exact",
        BatchStatus::High => "High confidence",
        BatchStatus::Possible => "Possible",
        BatchStatus::NotFound => "Not found",
        BatchStatus::Error => "Error",
    }
}

fn note_for(entry: &BatchEntryResult, matched: Option<&BatchMatch>) -> String {
    if entry.status == BatchStatus::Error {
        return "No searchable characters: the name is empty after normalization.".to_string();
    }
    if entry.matches.is_empty() {
        return "No file at or above the threshold.".to_string();
    }
    if entry.total_matches > entry.matches.len() {
        return format!(
            "{} candidates; showing the best {}.",
            entry.total_matches,
            entry.matches.len()
        );
    }
    if entry.total_matches > 1 {
        return format!("{} candidates.", entry.total_matches);
    }
    match matched {
        Some(found) => format!("{} · {}%", found.match_type.label(), found.score),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Sheet 3 - Missing
// ---------------------------------------------------------------------------

fn write_missing(workbook: &mut Workbook, report: &BatchReport) -> BatchResult<()> {
    let sheet = workbook.add_worksheet();
    sheet.set_name("Missing")?;
    sheet.set_column_width(0, 46.0)?;
    sheet.set_column_width(1, 12.0)?;
    sheet.set_column_width(2, 58.0)?;
    for (column, label) in ["Requested Filename", "Extension", "Note"]
        .iter()
        .enumerate()
    {
        sheet.write_string_with_format(0, column as u16, *label, &header())?;
    }
    sheet.set_row_height(0, 24.0)?;
    sheet.set_freeze_panes(1, 0)?;

    let mut row: u32 = 1;
    for entry in report
        .entries
        .iter()
        // An entry with no searchable characters is not a missing file: it was
        // never searched, and listing it here would send the user looking for a
        // name the index could never have matched.
        .filter(|entry| entry.matches.is_empty() && entry.status != BatchStatus::Error)
    {
        let striped = row.is_multiple_of(2);
        let text = if striped { zebra() } else { cell() };
        sheet.write_string_with_format(row, 0, cell_text(&entry.requested), &text)?;
        sheet.write_string_with_format(row, 1, extension_of(&entry.requested), &text)?;
        sheet.write_string_with_format(
            row,
            2,
            format!("Not in the index at the {}% threshold.", report.threshold),
            &text,
        )?;
        row += 1;
    }

    if row == 1 {
        // Error entries are deliberately absent from this sheet, so "nothing
        // missing" is not the same as "everything was found": a batch whose only
        // non-matches were unsearchable names used to print the success message.
        let message = if report.summary.errors > 0 {
            format!(
                "No missing files. {} name(s) could not be searched; see the Results sheet.",
                report.summary.errors
            )
        } else {
            "Every requested name was found.".to_string()
        };
        sheet.write_string_with_format(1, 0, message, &cell().set_font_color(GOOD))?;
    } else {
        sheet.autofilter(0, 0, row - 1, 2)?;
    }
    sheet.set_landscape();
    sheet.set_print_scale(90);
    sheet.set_footer("&CZeidex · Batch Search · &P / &N");
    Ok(())
}

// ---------------------------------------------------------------------------
// Sheet 4 - Multiple matches
// ---------------------------------------------------------------------------

fn write_multiple(workbook: &mut Workbook, report: &BatchReport) -> BatchResult<()> {
    let sheet = workbook.add_worksheet();
    sheet.set_name("Multiple Matches")?;
    let widths = [34.0, 34.0, 9.0, 14.0, 64.0, 18.0, 30.0];
    for (column, width) in widths.iter().enumerate() {
        sheet.set_column_width(column as u16, *width)?;
    }
    let headers = [
        "Requested Filename",
        "Matched Filename",
        "Match %",
        "Match Type",
        "File Path",
        "Last Modified",
        "Notes",
    ];
    for (column, label) in headers.iter().enumerate() {
        sheet.write_string_with_format(0, column as u16, *label, &header())?;
    }
    sheet.set_row_height(0, 24.0)?;
    sheet.set_freeze_panes(1, 0)?;

    let mut row: u32 = 1;
    for entry in report
        .entries
        .iter()
        .filter(|entry| entry.total_matches > 1)
    {
        for (position, found) in entry.matches.iter().enumerate() {
            let striped = row.is_multiple_of(2);
            let text = if striped { zebra() } else { cell() };
            let share = if striped { percent_zebra() } else { percent() };
            let time = if striped {
                datetime_zebra()
            } else {
                datetime()
            };
            sheet.write_string_with_format(row, 0, cell_text(&entry.requested), &text)?;
            sheet.write_string_with_format(row, 1, cell_text(&found.name), &text)?;
            sheet.write_number_with_format(row, 2, f64::from(found.score) / 100.0, &share)?;
            sheet.write_string_with_format(row, 3, found.match_type.label(), &text)?;
            sheet.write_string_with_format(row, 4, cell_text(&found.path), &text)?;
            match found.modified_unix_s.and_then(excel_datetime) {
                Some(stamp) => {
                    sheet.write_datetime_with_format(row, 5, &stamp, &time)?;
                }
                None => {
                    sheet.write_string_with_format(row, 5, "—", &text)?;
                }
            }
            // The sheet lists the candidates that were kept, not every one that
            // reached the threshold. Saying so on each row is what stops a
            // truncated list from reading as the whole answer.
            // On the first row of each group only, so the note reads as a
            // caption rather than a repeated column.
            let note = if position == 0 {
                if entry.total_matches > entry.matches.len() {
                    format!(
                        "{} candidates; showing the best {}.",
                        entry.total_matches,
                        entry.matches.len()
                    )
                } else {
                    format!("{} candidates.", entry.total_matches)
                }
            } else {
                String::new()
            };
            sheet.write_string_with_format(row, 6, note, &text)?;
            row += 1;
        }
    }

    if row == 1 {
        sheet.write_string_with_format(
            1,
            0,
            "No requested name matched more than one file.",
            &cell().set_font_color(GOOD),
        )?;
    } else {
        sheet.autofilter(0, 0, row - 1, 6)?;
    }
    sheet.set_landscape();
    sheet.set_print_scale(75);
    sheet.set_repeat_rows(0, 0)?;
    sheet.set_footer("&CZeidex · Batch Search · &P / &N");
    Ok(())
}

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

fn excel_datetime(unix_s: i64) -> Option<ExcelDateTime> {
    ExcelDateTime::from_timestamp(unix_s).ok()
}

/// `2026-11-22 00:33` in UTC, computed without a date crate: the civil-from-days
/// algorithm is small and this is the only place that needs it.
fn stamp(unix_s: i64) -> String {
    let days = unix_s.div_euclid(86_400);
    let seconds = unix_s.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        seconds / 3600,
        (seconds % 3600) / 60
    )
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a calendar date.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `12345` -> `12,345`, so a large index reads as a number rather than a digit
/// run.
fn thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn size_label(bytes: Option<u64>, kind: FileKind) -> String {
    if kind == FileKind::Folder {
        return "—".to_string();
    }
    let Some(bytes) = bytes else {
        return "—".to_string();
    };
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let value = bytes as f64;
    if value >= GB {
        format!("{:.1} GB", value / GB)
    } else if value >= MB {
        format!("{:.1} MB", value / MB)
    } else if value >= KB {
        format!("{:.1} KB", value / KB)
    } else {
        format!("{bytes} B")
    }
}

/// The extension as typed in the request, so a missing `.pdf` is visible even
/// when no file was found to read it from.
fn extension_of(requested: &str) -> String {
    match requested.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && !extension.is_empty() => {
            extension.to_string()
        }
        _ => "—".to_string(),
    }
}

/// Excel refuses a cell longer than 32 767 UTF-16 units, and the library turns
/// that into an error that aborts the whole export. A single pasted paragraph
/// with no line breaks is one entry, so one long request used to cost the entire
/// report. Truncating keeps the workbook: the visible text still identifies the
/// request, and the ellipsis says it was cut.
///
/// The limit is in UTF-16 units because that is what Excel counts, and a
/// supplementary-plane character is two of them.
fn cell_text(value: &str) -> std::borrow::Cow<'_, str> {
    const MAX_UNITS: usize = 32_767;
    // One unit is left for the ellipsis, which is one UTF-16 unit.
    const LIMIT: usize = MAX_UNITS - 1;

    let units: usize = value.encode_utf16().count();
    if units <= MAX_UNITS {
        return std::borrow::Cow::Borrowed(value);
    }
    let mut used = 0usize;
    let mut cut = value.len();
    for (offset, ch) in value.char_indices() {
        let width = ch.len_utf16();
        if used + width > LIMIT {
            cut = offset;
            break;
        }
        used += width;
    }
    let mut out = String::with_capacity(cut + 3);
    out.push_str(&value[..cut]);
    out.push('…');
    std::borrow::Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn the_stamp_is_a_utc_calendar_date() {
        assert_eq!(stamp(0), "1970-01-01 00:00");
        assert_eq!(stamp(1_700_000_000), "2023-11-14 22:13");
        // A leap day, to catch an off-by-one in the civil algorithm.
        assert_eq!(stamp(1_709_164_800), "2024-02-29 00:00");
    }

    #[test]
    fn an_unsearchable_entry_is_labelled_and_explained() {
        let entry = BatchEntryResult {
            requested: "\u{0640}".to_string(),
            status: BatchStatus::Error,
            matches: Vec::new(),
            total_matches: 0,
            duration_ms: 1,
        };
        assert_eq!(status_label(entry.status), "Error");
        assert!(note_for(&entry, None).contains("No searchable characters"));
    }

    #[test]
    fn sizes_are_readable_at_every_scale() {
        assert_eq!(size_label(Some(512), FileKind::File), "512 B");
        assert_eq!(size_label(Some(2048), FileKind::File), "2.0 KB");
        assert_eq!(size_label(Some(5 * 1024 * 1024), FileKind::File), "5.0 MB");
        assert_eq!(size_label(None, FileKind::File), "—");
        assert_eq!(size_label(Some(10), FileKind::Folder), "—");
    }

    #[test]
    fn a_large_count_reads_with_separators() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(12_345), "12,345");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn an_extension_is_read_from_the_request_when_nothing_matched() {
        assert_eq!(extension_of("report.final.pdf"), "pdf");
        assert_eq!(extension_of("README"), "—");
        assert_eq!(extension_of(".gitignore"), "—");
    }

    /// Excel rejects a cell over 32 767 UTF-16 units, and the library turns that
    /// into an error that aborts the whole export. A single pasted paragraph is
    /// one entry, so one long request used to cost the entire report.
    #[test]
    fn a_cell_is_cut_to_the_excel_limit_instead_of_failing_the_export() {
        let long = "a".repeat(40_000);
        let cut = cell_text(&long);
        assert_eq!(cut.encode_utf16().count(), 32_767);
        assert!(cut.ends_with('…'), "the cut is visible");

        // A supplementary-plane character is two UTF-16 units, so the cut has to
        // land on a character boundary and stay inside the budget.
        let wide = "𝕏".repeat(20_000);
        let cut = cell_text(&wide);
        assert_eq!(cut.encode_utf16().count(), 32_767);
        assert!(cut.ends_with('…'));
        assert!(cut.chars().rev().nth(1).is_some_and(|ch| ch == '𝕏'));

        // Short values are returned untouched, without an allocation.
        let short = "invoice.pdf";
        assert!(matches!(cell_text(short), std::borrow::Cow::Borrowed(_)));
        assert_eq!(cell_text(short), short);
    }
}
