//! Serializers. Every format is derived from the same block list, so a number
//! or a cell cannot appear in one export and vanish from another.

use crate::model::{BlockKind, Language, OrganizedDocument, Table};

pub fn text(document: &OrganizedDocument) -> String {
    let mut out = String::new();
    for block in &document.blocks {
        let piece = match block.kind {
            BlockKind::List => block
                .items
                .iter()
                .map(|item| format!("- {item}"))
                .collect::<Vec<_>>()
                .join("\n"),
            BlockKind::KeyValue => match (&block.key, &block.value) {
                (Some(key), Some(value)) => format!("{key}: {value}"),
                _ => block.text.clone(),
            },
            BlockKind::Table => block
                .table
                .as_ref()
                .map(table_text)
                .unwrap_or_else(|| block.text.clone()),
            _ => block.text.clone(),
        };
        if piece.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&piece);
    }
    out
}

fn table_text(table: &Table) -> String {
    table
        .all_rows()
        .iter()
        .map(|row| row.join("\t"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn json(document: &OrganizedDocument) -> String {
    serde_json::to_string_pretty(document).unwrap_or_else(|_| "{}".to_string())
}

pub fn csv(document: &OrganizedDocument) -> String {
    let tables: Vec<&Table> = document
        .blocks
        .iter()
        .filter_map(|block| block.table.as_ref())
        .collect();

    if tables.is_empty() {
        let mut out = String::from("kind,text\n");
        for block in &document.blocks {
            out.push_str(block.kind.as_str());
            out.push(',');
            out.push_str(&csv_field(&block.text));
            out.push('\n');
        }
        return out;
    }

    let mut out = String::new();
    for (index, table) in tables.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let rows = table.all_rows();
        let width = rows
            .iter()
            .map(|row| row.len())
            .max()
            .unwrap_or(table.columns)
            .max(1);
        for row in rows {
            let cells: Vec<String> = (0..width)
                .map(|column| csv_field(row.get(column).map(String::as_str).unwrap_or_default()))
                .collect();
            out.push_str(&cells.join(","));
            out.push('\n');
        }
    }
    out
}

fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

pub fn markdown(document: &OrganizedDocument) -> String {
    let mut out = String::new();
    for block in &document.blocks {
        let piece = match block.kind {
            BlockKind::Title => format!("# {}", escape_markdown(&block.text)),
            BlockKind::Heading => format!("## {}", escape_markdown(&block.text)),
            BlockKind::List => block
                .items
                .iter()
                .map(|item| format!("- {}", escape_markdown(item)))
                .collect::<Vec<_>>()
                .join("\n"),
            BlockKind::KeyValue => match (&block.key, &block.value) {
                (Some(key), Some(value)) => {
                    format!("- **{}:** {}", escape_markdown(key), escape_markdown(value))
                }
                _ => escape_markdown(&block.text),
            },
            BlockKind::Table => block
                .table
                .as_ref()
                .map(markdown_table)
                .unwrap_or_else(|| escape_markdown(&block.text)),
            BlockKind::Caption => format!("*{}*", escape_markdown(&block.text)),
            _ => escape_markdown(&block.text),
        };
        if piece.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&piece);
    }
    out
}

fn markdown_table(table: &Table) -> String {
    let rows = table.all_rows();
    if rows.is_empty() {
        return String::new();
    }
    let width = rows.iter().map(|row| row.len()).max().unwrap_or(1).max(1);
    let cell = |row: &[String], column: usize| {
        escape_markdown(row.get(column).map(String::as_str).unwrap_or_default())
            .replace('\n', "<br>")
    };

    let mut out = String::new();
    let header = rows[0];
    let header_cells: Vec<String> = (0..width).map(|column| cell(header, column)).collect();
    out.push('|');
    out.push_str(&header_cells.join("|"));
    out.push_str("|\n|");
    out.push_str(&vec!["---"; width].join("|"));
    out.push_str("|\n");
    for row in &rows[1..] {
        let cells: Vec<String> = (0..width).map(|column| cell(row, column)).collect();
        out.push('|');
        out.push_str(&cells.join("|"));
        out.push_str("|\n");
    }
    out.trim_end().to_string()
}

fn escape_markdown(value: &str) -> String {
    value.replace('|', "\\|")
}

pub fn html(document: &OrganizedDocument) -> String {
    let rtl = matches!(document.language, Language::Arabic | Language::Mixed);
    let dir = if rtl { "rtl" } else { "ltr" };
    let lang = match document.language {
        Language::Arabic => "ar",
        Language::English => "en",
        Language::Mixed => "ar",
        Language::Unknown => "und",
    };

    let mut body = String::new();
    for block in &document.blocks {
        match block.kind {
            BlockKind::Title => {
                body.push_str(&format!("<h1>{}</h1>\n", escape_html(&block.text)));
            }
            BlockKind::Heading => {
                body.push_str(&format!("<h2>{}</h2>\n", escape_html(&block.text)));
            }
            BlockKind::List => {
                body.push_str("<ul>\n");
                for item in &block.items {
                    body.push_str(&format!("<li>{}</li>\n", escape_html(item)));
                }
                body.push_str("</ul>\n");
            }
            BlockKind::KeyValue => match (&block.key, &block.value) {
                (Some(key), Some(value)) => body.push_str(&format!(
                    "<p><strong>{}:</strong> {}</p>\n",
                    escape_html(key),
                    escape_html(value)
                )),
                _ => body.push_str(&format!("<p>{}</p>\n", escape_html(&block.text))),
            },
            BlockKind::Table => {
                if let Some(table) = &block.table {
                    body.push_str(&html_table(table));
                } else {
                    body.push_str(&format!("<p>{}</p>\n", escape_html(&block.text)));
                }
            }
            BlockKind::Caption => {
                body.push_str(&format!(
                    "<p class=\"caption\">{}</p>\n",
                    escape_html(&block.text)
                ));
            }
            _ => body.push_str(&format!("<p>{}</p>\n", escape_html(&block.text))),
        }
    }

    format!(
        "<!doctype html>\n<html lang=\"{lang}\" dir=\"{dir}\">\n<head>\n<meta charset=\"utf-8\">\n\
<title>Zeidex document</title>\n<style>\n\
body{{font-family:system-ui,'Segoe UI',Tahoma,sans-serif;line-height:1.6;margin:2rem auto;max-width:52rem;padding:0 1rem;color:#1b1b1f;background:#fff}}\n\
h1,h2{{line-height:1.25}}\n\
table{{border-collapse:collapse;width:100%;margin:1rem 0}}\n\
th,td{{border:1px solid #c8c8d0;padding:.4rem .6rem;text-align:start;vertical-align:top}}\n\
th{{background:#f2f2f6}}\n\
.caption{{color:#5b5b66;font-size:.9rem}}\n\
@media print{{body{{margin:0;max-width:none}}}}\n\
</style>\n</head>\n<body>\n{body}</body>\n</html>\n"
    )
}

fn html_table(table: &Table) -> String {
    let rows = table.all_rows();
    if rows.is_empty() {
        return String::new();
    }
    let width = rows.iter().map(|row| row.len()).max().unwrap_or(1).max(1);
    let cell = |row: &[String], column: usize, tag: &str| {
        format!(
            "<{tag}>{}</{tag}>",
            escape_html(row.get(column).map(String::as_str).unwrap_or_default())
        )
    };

    let mut out = String::from("<table>\n");
    if !table.header.iter().all(|cell| cell.trim().is_empty()) {
        out.push_str("<thead><tr>");
        for column in 0..width {
            out.push_str(&cell(&table.header, column, "th"));
        }
        out.push_str("</tr></thead>\n");
    }
    out.push_str("<tbody>\n");
    for row in &table.rows {
        out.push_str("<tr>");
        for column in 0..width {
            out.push_str(&cell(row, column, "td"));
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</tbody>\n</table>\n");
    out
}

fn escape_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '\n' => out.push_str("<br>"),
            _ => out.push(ch),
        }
    }
    out
}
