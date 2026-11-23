//! Structure detection.
//!
//! Every decision here is rule-based and reproducible. Nothing is rewritten:
//! the classifier only decides what a block *is*, and table detection only
//! groups cells that are already present in the source. A table the detector is
//! not sure about is exported with `needs_review = true` instead of being
//! silently repaired.

use crate::model::{
    Block, BlockKind, Language, OrganizedDocument, SourceBlock, SourceDocument, SourceTable, Stats,
    Table, Warning,
};

pub const SCHEMA: u32 = 2;

/// Confidence below which a table is shown to the user for review.
pub const REVIEW_THRESHOLD: f32 = 0.75;

struct Prepared {
    index: usize,
    page: usize,
    kind: String,
    text: String,
    table: Option<SourceTable>,
    confidence: f32,
}

pub fn run(source: &SourceDocument) -> OrganizedDocument {
    let mut prepared: Vec<Prepared> = Vec::new();
    for (page_index, page) in source.pages.iter().enumerate() {
        for (block_index, block) in page.blocks.iter().enumerate() {
            for part in expand_block(block) {
                if let Some(item) = prepare(block_index, page_index, &part) {
                    prepared.push(item);
                }
            }
        }
    }

    let mut blocks: Vec<Block> = Vec::with_capacity(prepared.len());
    let mut title_seen = false;

    for (position, item) in prepared.iter().enumerate() {
        let next_len = prepared
            .get(position + 1)
            .map(|next| next.text.chars().count())
            .unwrap_or(0);
        let mut block = classify(item, next_len);

        if block.kind == BlockKind::Title {
            if title_seen {
                block.kind = BlockKind::Heading;
                block.level = Some(2);
            } else {
                title_seen = true;
            }
        }

        // Consecutive list rows are one list, not one block per bullet.
        if block.kind == BlockKind::List
            && let Some(previous) = blocks.last_mut()
            && previous.kind == BlockKind::List
            && previous.page == block.page
        {
            previous.items.append(&mut block.items);
            continue;
        }

        blocks.push(block);
    }

    for block in &mut blocks {
        if block.kind == BlockKind::List {
            block.text = block.items.join("\n");
        }
    }

    let mut warnings = Vec::new();
    if blocks.is_empty() {
        warnings.push(Warning {
            code: "empty_document".to_string(),
            message: "No text was recognized.".to_string(),
            block: None,
        });
    }
    for (index, block) in blocks.iter().enumerate() {
        if let Some(table) = &block.table
            && table.needs_review
        {
            warnings.push(Warning {
                code: "table_needs_review".to_string(),
                message: format!(
                    "Table on block {index} is {:.0}% confident; check the cells before exporting.",
                    table.confidence * 100.0
                ),
                block: Some(index),
            });
        }
    }

    let language = detect_language(&blocks);
    let stats = stats(&blocks);

    OrganizedDocument {
        schema: SCHEMA,
        pages: source.pages.len(),
        language,
        blocks,
        stats,
        warnings,
    }
}

/// OCR sometimes returns a whole label column as one region, e.g.
/// `"Invoice No: 7\nTotal: 10.00"`. Classifying that as a single pair makes the
/// first value swallow every following label. When at least two lines are
/// themselves label/value pairs the block is split into one block per line;
/// a continuation line under a single label stays with its value.
fn expand_block(block: &SourceBlock) -> Vec<SourceBlock> {
    if block.table.is_some() {
        return vec![block.clone()];
    }
    let text = block.text.trim();
    if !text.contains('\n') {
        return vec![block.clone()];
    }
    let pairs = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| split_key_value(line).is_some())
        .count();
    if pairs < 2 {
        return vec![block.clone()];
    }
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| SourceBlock {
            kind: block.kind.clone(),
            text: line.to_string(),
            table: None,
            confidence: block.confidence,
        })
        .collect()
}

fn prepare(index: usize, page: usize, block: &SourceBlock) -> Option<Prepared> {
    let text = clean(&block.text);
    if text.is_empty() && block.table.is_none() {
        return None;
    }
    Some(Prepared {
        index,
        page,
        kind: block.kind.trim().to_ascii_lowercase(),
        text,
        table: block.table.clone(),
        confidence: block.confidence.clamp(0.0, 1.0),
    })
}

fn classify(item: &Prepared, next_len: usize) -> Block {
    if let Some(source_table) = &item.table {
        let table = table_from_source(source_table, item.confidence);
        return Block {
            kind: BlockKind::Table,
            text: item.text.clone(),
            level: None,
            items: Vec::new(),
            key: None,
            value: None,
            table: Some(table),
            confidence: item.confidence,
            source_index: item.index,
            page: item.page,
        };
    }

    if let Some((rows, confidence)) = detect_text_table(&item.text) {
        let (header, body) = split_header(&rows);
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        let table = Table {
            header,
            rows: body,
            columns,
            confidence,
            needs_review: confidence < REVIEW_THRESHOLD || columns < 2,
        };
        return Block {
            kind: BlockKind::Table,
            text: item.text.clone(),
            level: None,
            items: Vec::new(),
            key: None,
            value: None,
            table: Some(table),
            confidence: confidence.min(item.confidence),
            source_index: item.index,
            page: item.page,
        };
    }

    let kind = match item.kind.as_str() {
        "title" => Some((BlockKind::Title, Some(1))),
        "caption" => Some((BlockKind::Caption, None)),
        "figure" => Some((BlockKind::Figure, None)),
        "equation" => Some((BlockKind::Equation, None)),
        "header" => Some((BlockKind::Header, None)),
        "footer" => Some((BlockKind::Footer, None)),
        _ => None,
    };

    if let Some((kind, level)) = kind {
        return simple(item, kind, level, item.confidence);
    }

    if let Some(items) = item
        .text
        .lines()
        .map(list_item)
        .collect::<Option<Vec<String>>>()
        && !items.is_empty()
    {
        let mut block = simple(item, BlockKind::List, None, item.confidence);
        block.items = items;
        return block;
    }

    if let Some((key, value)) = split_key_value(&item.text) {
        let mut block = simple(item, BlockKind::KeyValue, None, item.confidence);
        block.key = Some(key);
        block.value = Some(value);
        return block;
    }

    if is_contact(&item.text) {
        return simple(item, BlockKind::Contact, None, 0.8 * item.confidence);
    }
    if is_date_like(&item.text) {
        return simple(item, BlockKind::Date, None, 0.8 * item.confidence);
    }
    if is_amount_like(&item.text) {
        return simple(item, BlockKind::Amount, None, 0.8 * item.confidence);
    }

    if is_heading(&item.text, next_len) {
        return simple(item, BlockKind::Heading, Some(2), 0.6 * item.confidence);
    }

    simple(item, BlockKind::Paragraph, None, item.confidence)
}

fn simple(item: &Prepared, kind: BlockKind, level: Option<u8>, confidence: f32) -> Block {
    Block {
        kind,
        text: item.text.clone(),
        level,
        items: Vec::new(),
        key: None,
        value: None,
        table: None,
        confidence,
        source_index: item.index,
        page: item.page,
    }
}

/// Collapse runs of whitespace inside each line and drop blank lines. Letters
/// and digits are untouched.
pub fn clean(text: &str) -> String {
    text.lines()
        .map(|line| {
            line.split_whitespace()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn clean_inline(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------- tables ----

#[derive(Clone, Copy, PartialEq, Eq)]
enum Delimiter {
    Tab,
    Pipe,
    Spaces,
}

fn detect_text_table(text: &str) -> Option<(Vec<Vec<String>>, f32)> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.len() < 2 {
        return None;
    }
    let delimiter = detect_delimiter(&lines)?;

    let mut rows: Vec<Vec<String>> = Vec::new();
    for line in &lines {
        let cells = split_line(line, delimiter);
        if cells.len() >= 2 && cells.iter().any(|cell| !cell.is_empty()) {
            rows.push(cells);
        }
    }
    // A few stray lines must not turn a paragraph into a table.
    if rows.len() < 2 || rows.len() * 10 < lines.len() * 8 {
        return None;
    }

    let columns = mode(rows.iter().map(Vec::len));
    if columns < 2 {
        return None;
    }
    let consistent = rows.iter().filter(|row| row.len() == columns).count();
    let confidence = consistent as f32 / rows.len() as f32;
    if confidence < 0.6 {
        return None;
    }

    // Keep every cell: pad short rows to the widest row rather than truncating
    // the long ones.
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    for row in &mut rows {
        row.resize(width, String::new());
    }
    Some((rows, (confidence * 0.9).clamp(0.0, 1.0)))
}

fn detect_delimiter(lines: &[&str]) -> Option<Delimiter> {
    let tabs = lines.iter().filter(|line| line.contains('\t')).count();
    if tabs * 2 >= lines.len() {
        return Some(Delimiter::Tab);
    }
    let pipes = lines.iter().filter(|line| line.contains('|')).count();
    if pipes * 2 >= lines.len() {
        return Some(Delimiter::Pipe);
    }
    let spaced = lines
        .iter()
        .filter(|line| line.split("  ").count() >= 2)
        .count();
    if spaced * 2 >= lines.len() {
        return Some(Delimiter::Spaces);
    }
    None
}

fn split_line(line: &str, delimiter: Delimiter) -> Vec<String> {
    match delimiter {
        Delimiter::Tab => line.split('\t').map(clean_inline).collect(),
        Delimiter::Pipe => line
            .trim_matches('|')
            .split('|')
            .map(clean_inline)
            .collect(),
        Delimiter::Spaces => split_on_spaces(line),
    }
}

fn split_on_spaces(line: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut spaces = 0usize;
    for ch in line.chars() {
        if ch == ' ' {
            spaces += 1;
            continue;
        }
        if spaces >= 2 {
            if !current.trim().is_empty() {
                cells.push(current.trim().to_string());
                current.clear();
            }
        } else {
            for _ in 0..spaces {
                current.push(' ');
            }
        }
        spaces = 0;
        current.push(ch);
    }
    if !current.trim().is_empty() {
        cells.push(current.trim().to_string());
    }
    cells
}

fn mode(values: impl Iterator<Item = usize>) -> usize {
    let mut counts: Vec<(usize, usize)> = Vec::new();
    for value in values {
        match counts.iter_mut().find(|(seen, _)| *seen == value) {
            Some((_, count)) => *count += 1,
            None => counts.push((value, 1)),
        }
    }
    counts
        .into_iter()
        .max_by_key(|(value, count)| (*count, *value))
        .map(|(value, _)| value)
        .unwrap_or(0)
}

fn split_header(rows: &[Vec<String>]) -> (Vec<String>, Vec<Vec<String>>) {
    let Some(first) = rows.first() else {
        return (Vec::new(), Vec::new());
    };
    let looks_like_header = first
        .iter()
        .any(|cell| !cell.trim().is_empty() && !is_number_like(cell));
    if looks_like_header {
        (first.clone(), rows[1..].to_vec())
    } else {
        (Vec::new(), rows.to_vec())
    }
}

fn table_from_source(source: &SourceTable, layout_confidence: f32) -> Table {
    let rows = source.rows.max(1);
    let cols = source.cols.max(1);
    let mut grid = vec![vec![String::new(); cols]; rows];
    for cell in &source.cells {
        if cell.row < rows && cell.col < cols {
            grid[cell.row][cell.col] = clean_inline(&cell.text);
        }
    }
    let filled = source
        .cells
        .iter()
        .filter(|cell| !cell.text.trim().is_empty())
        .count();
    let fill = filled as f32 / (rows * cols) as f32;
    let confidence = (layout_confidence * 0.4 + fill * 0.6).clamp(0.0, 1.0);
    let (header, body) = split_header(&grid);
    let empty = header.iter().all(|cell| cell.trim().is_empty()) && body.is_empty();
    Table {
        header,
        rows: body,
        columns: cols,
        confidence,
        needs_review: confidence < REVIEW_THRESHOLD || cols < 2 || empty,
    }
}

// ---------------------------------------------------------- classification --

fn list_item(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    for marker in ["- ", "* ", "• ", "‣ ", "▪ ", "◦ ", "· "] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            let rest = rest.trim();
            if !rest.is_empty() {
                return Some(rest.to_string());
            }
        }
    }

    let first_end = trimmed.find(' ')?;
    let token = trimmed[..first_end].trim_end_matches(['.', ')', '-', '،']);
    let token = token.trim_start_matches('(');
    if token.is_empty() || !token.chars().all(is_arabic_digit_or_ascii) {
        return None;
    }
    let rest = trimmed[first_end..].trim();
    (!rest.is_empty()).then(|| rest.to_string())
}

fn split_key_value(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return None;
    }
    let (position, separator) = trimmed
        .char_indices()
        .find(|(_, ch)| *ch == ':' || *ch == '：')?;
    let key = trimmed[..position].trim();
    let value = trimmed[position + separator.len_utf8()..].trim();
    if key.is_empty() || value.is_empty() || key.chars().count() > 40 {
        return None;
    }
    if !key.chars().any(|ch| ch.is_alphabetic()) {
        return None;
    }
    Some((key.to_string(), value.to_string()))
}

fn is_contact(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.contains('@') {
        let domain = trimmed.split('@').nth(1).unwrap_or_default();
        if domain.contains('.') && !trimmed.contains(' ') {
            return true;
        }
    }
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("www.")
        || trimmed.contains("://")
    {
        return true;
    }
    let digits = trimmed.chars().filter(|ch| ch.is_ascii_digit()).count();
    let separators = trimmed
        .chars()
        .filter(|ch| matches!(ch, '+' | ' ' | '-' | '(' | ')'))
        .count();
    let letters = trimmed.chars().filter(|ch| ch.is_alphabetic()).count();
    (7..=15).contains(&digits) && separators >= 2 && letters == 0 && !is_date_like(trimmed)
}

fn is_date_like(text: &str) -> bool {
    let normalized: String = text
        .trim()
        .chars()
        .map(|ch| match ch {
            '٠'..='٩' => char::from_u32(ch as u32 - 0x0660 + '0' as u32).unwrap_or(ch),
            _ => ch,
        })
        .collect();
    let parts: Vec<&str> = normalized.split(['/', '-', '.']).collect();
    if parts.len() != 3 {
        return false;
    }
    let numbers: Option<Vec<u32>> = parts
        .iter()
        .map(|part| part.trim().parse::<u32>().ok())
        .collect();
    let Some(numbers) = numbers else {
        return false;
    };
    let (a, b, c) = (numbers[0], numbers[1], numbers[2]);
    let year = if a >= 1000 {
        a
    } else if c >= 1000 {
        c
    } else {
        return false;
    };
    let (month, day) = if a >= 1000 { (b, c) } else { (b, a) };
    (1900..=2200).contains(&year) && (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn is_amount_like(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.chars().count() > 40 {
        return false;
    }
    let has_digit = trimmed.chars().any(|ch| ch.is_ascii_digit());
    if !has_digit {
        return false;
    }
    const MARKERS: [&str; 22] = [
        "$",
        "€",
        "£",
        "¥",
        "﷼",
        "ر.س",
        "ر.ع",
        "د.إ",
        "د.ك",
        "ج.م",
        "ريال",
        "درهم",
        "جنيه",
        "دينار",
        "دولار",
        "يورو",
        "USD",
        "EUR",
        "SAR",
        "AED",
        "EGP",
        "GBP",
    ];
    if MARKERS.iter().any(|marker| trimmed.contains(marker)) {
        return true;
    }
    let compact: String = trimmed.chars().filter(|ch| !ch.is_whitespace()).collect();
    let numeric = compact
        .chars()
        .all(|ch| ch.is_ascii_digit() || matches!(ch, ',' | '.' | '+' | '-'));
    numeric
        && compact.contains('.')
        && compact
            .rsplit_once('.')
            .is_some_and(|(_, fraction)| fraction.len() == 2)
}

fn is_heading(text: &str, next_len: usize) -> bool {
    let length = text.chars().count();
    let words = text.split_whitespace().count();
    let ends_sentence = text
        .chars()
        .last()
        .is_some_and(|ch| matches!(ch, '.' | '!' | '?' | ':' | '،' | '؛' | '؟'));
    (3..=80).contains(&length)
        && words <= 12
        && !ends_sentence
        && !text.contains('\n')
        && next_len > length * 2
}

fn is_number_like(text: &str) -> bool {
    let compact: String = text
        .chars()
        .filter(|ch| !ch.is_whitespace() && !matches!(ch, ',' | '.' | '+' | '-' | '$' | '€'))
        .collect();
    !compact.is_empty() && compact.chars().all(|ch| ch.is_ascii_digit())
}

fn is_arabic_digit_or_ascii(ch: char) -> bool {
    ch.is_ascii_digit() || ('\u{0660}'..='\u{0669}').contains(&ch)
}

// ---------------------------------------------------------------- summary ---

pub(crate) fn detect_language(blocks: &[Block]) -> Language {
    let mut arabic = 0usize;
    let mut latin = 0usize;
    for block in blocks {
        for text in block_texts(block) {
            for ch in text.chars() {
                if is_arabic_letter(ch) {
                    arabic += 1;
                } else if ch.is_ascii_alphabetic() {
                    latin += 1;
                }
            }
        }
    }
    match (arabic, latin) {
        (0, 0) => Language::Unknown,
        (0, _) => Language::English,
        (_, 0) => Language::Arabic,
        (a, l) => {
            let smaller = a.min(l) as f32 / (a + l) as f32;
            if smaller >= 0.1 {
                Language::Mixed
            } else if a > l {
                Language::Arabic
            } else {
                Language::English
            }
        }
    }
}

fn is_arabic_letter(ch: char) -> bool {
    matches!(ch,
        '\u{0600}'..='\u{06FF}'
        | '\u{0750}'..='\u{077F}'
        | '\u{08A0}'..='\u{08FF}'
        | '\u{FB50}'..='\u{FDFF}'
        | '\u{FE70}'..='\u{FEFF}'
    ) && ch.is_alphabetic()
}

pub(crate) fn stats(blocks: &[Block]) -> Stats {
    let mut characters = 0usize;
    let mut words = 0usize;
    let mut arabic = 0usize;
    let mut latin = 0usize;
    let mut tables = 0usize;
    let mut needs_review = 0usize;

    for block in blocks {
        if let Some(table) = &block.table {
            tables += 1;
            if table.needs_review {
                needs_review += 1;
            }
        }
        for text in block_texts(block) {
            characters += text.chars().count();
            words += text.split_whitespace().count();
            for ch in text.chars() {
                if is_arabic_letter(ch) {
                    arabic += 1;
                } else if ch.is_ascii_alphabetic() {
                    latin += 1;
                }
            }
        }
    }

    let letters = arabic + latin;
    Stats {
        characters,
        words,
        arabic_ratio: if letters == 0 {
            0.0
        } else {
            arabic as f32 / letters as f32
        },
        blocks: blocks.len(),
        tables,
        needs_review,
    }
}

fn block_texts(block: &Block) -> Vec<&str> {
    let mut texts: Vec<&str> = Vec::new();
    if !block.text.is_empty() {
        texts.push(&block.text);
    }
    texts.extend(block.items.iter().map(String::as_str));
    if let Some(value) = &block.value {
        texts.push(value.as_str());
    }
    if let Some(table) = &block.table {
        texts.extend(table.header.iter().map(String::as_str));
        for row in &table.rows {
            texts.extend(row.iter().map(String::as_str));
        }
    }
    texts
}
