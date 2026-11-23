use zeidex_organizer::{
    BlockKind, Language, OrganizedDocument, SourceBlock, SourceCell, SourceDocument, SourcePage,
    SourceTable, organize, organize_text,
};

fn page(blocks: Vec<SourceBlock>) -> SourceDocument {
    SourceDocument {
        pages: vec![SourcePage { blocks }],
    }
}

fn cell(row: usize, col: usize, text: &str) -> SourceCell {
    SourceCell {
        row,
        col,
        row_span: 1,
        col_span: 1,
        text: text.to_string(),
    }
}

fn tokens(text: &str) -> Vec<String> {
    let mut tokens: Vec<String> = text
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_lowercase())
        .collect();
    tokens.sort();
    tokens
}

#[test]
fn titles_headings_lists_and_key_values_are_separated() {
    let document = organize(&page(vec![
        SourceBlock::titled("Zeidex Report"),
        SourceBlock::text("Monthly Report"),
        SourceBlock::text(
            "This paragraph is long enough that the line before it reads as a heading, and it keeps going for a while.",
        ),
        SourceBlock::text("- Milk"),
        SourceBlock::text("- Bread"),
        SourceBlock::text("Total: 42"),
    ]));

    assert_eq!(document.blocks[0].kind, BlockKind::Title);
    assert_eq!(document.blocks[0].level, Some(1));
    assert_eq!(document.blocks[1].kind, BlockKind::Heading);
    assert_eq!(document.blocks[2].kind, BlockKind::Paragraph);
    assert_eq!(document.blocks[3].kind, BlockKind::List);
    assert_eq!(document.blocks[3].items, vec!["Milk", "Bread"]);
    assert_eq!(document.blocks[4].kind, BlockKind::KeyValue);
    assert_eq!(document.blocks[4].key.as_deref(), Some("Total"));
    assert_eq!(document.blocks[4].value.as_deref(), Some("42"));
}

#[test]
fn a_pipe_table_is_detected_with_its_header() {
    let document = organize(&page(vec![SourceBlock::text(
        "| Item | Qty |\n| Pen | 3 |\n| Ink | 2 |",
    )]));
    let table = document.blocks[0]
        .table
        .as_ref()
        .expect("a pipe grid is a table");

    assert_eq!(table.header, vec!["Item", "Qty"]);
    assert_eq!(table.rows, vec![vec!["Pen", "3"], vec!["Ink", "2"]]);
    assert!(!table.needs_review);
    assert!(table.confidence > 0.8);
}

#[test]
fn a_ragged_table_is_flagged_instead_of_repaired_silently() {
    let document = organize(&page(vec![SourceBlock::text(
        "| A | B | C |\n| 1 | 2 | 3 |\n| 4 | 5 | 6 |\n| 7 | 8 |",
    )]));
    let table = document.blocks[0]
        .table
        .as_ref()
        .expect("three consistent rows out of four is a table");

    assert!(table.needs_review, "confidence {:.2}", table.confidence);
    assert!(table.confidence < 0.75);
    assert!(
        document
            .warnings
            .iter()
            .any(|warning| warning.code == "table_needs_review"),
        "the user must be told the table needs a look"
    );
}

#[test]
fn arabic_and_english_together_are_mixed() {
    let document = organize_text("Invoice رقم 12");
    assert_eq!(document.language, Language::Mixed);
    assert!(document.stats.arabic_ratio > 0.0);
}

#[test]
fn dates_amounts_and_contacts_are_classified_without_touching_them() {
    let document = organize(&page(vec![
        SourceBlock::text("Invoice No: 2026-001"),
        SourceBlock::text("2026-01-15"),
        SourceBlock::text("$1,250.00"),
        SourceBlock::text("support@zeidex.example"),
        SourceBlock::text("+20 123 456 7890"),
    ]));

    assert_eq!(document.blocks[0].kind, BlockKind::KeyValue);
    assert_eq!(document.blocks[1].kind, BlockKind::Date);
    assert_eq!(document.blocks[1].text, "2026-01-15");
    assert_eq!(document.blocks[2].kind, BlockKind::Amount);
    assert_eq!(document.blocks[2].text, "$1,250.00");
    assert_eq!(document.blocks[3].kind, BlockKind::Contact);
    assert_eq!(document.blocks[4].kind, BlockKind::Contact);
}

#[test]
fn csv_quotes_commas_and_quotes() {
    let source = SourceTable {
        rows: 2,
        cols: 2,
        cells: vec![
            cell(0, 0, "Name"),
            cell(0, 1, "Note"),
            cell(1, 0, "Smith, John"),
            cell(1, 1, "He said \"hi\""),
        ],
    };
    let document = organize(&page(vec![SourceBlock {
        kind: "table".to_string(),
        text: String::new(),
        table: Some(source),
        confidence: 1.0,
    }]));

    let csv = document.to_csv();
    assert!(csv.contains("\"Smith, John\""), "{csv}");
    assert!(csv.contains("\"He said \"\"hi\"\"\""), "{csv}");
}

#[test]
fn json_round_trips_the_whole_document() {
    let document = organize(&page(vec![
        SourceBlock::titled("Report"),
        SourceBlock::text("Total: 42"),
        SourceBlock::text("| A | B |\n| 1 | 2 |"),
    ]));
    let back = OrganizedDocument::from_json(&document.to_json()).expect("valid JSON");
    assert_eq!(back, document);
}

#[test]
fn every_renderer_keeps_every_token() {
    let source = page(vec![
        SourceBlock::titled("Sales Report 2026"),
        SourceBlock::text("Prepared for the Cairo office."),
        SourceBlock::text("- First item\n- Second item"),
        SourceBlock::text("Total: 1,250.00 EGP"),
        SourceBlock::text("| Region | Amount |\n| Cairo | 900 |\n| Giza | 350 |"),
    ]);
    let document = organize(&source);

    let mut input = String::new();
    for block in &source.pages[0].blocks {
        input.push_str(&block.text);
        input.push('\n');
        if let Some(table) = &block.table {
            for cell in &table.cells {
                input.push_str(&cell.text);
                input.push('\n');
            }
        }
    }

    for output in [document.plain_text(), document.to_markdown()] {
        assert_eq!(
            tokens(&input),
            tokens(&output),
            "an export changed the content: {output}"
        );
    }

    // HTML carries markup and CSS, so compare content containment rather than
    // token equality: every source token must still be present in the page.
    let html = document.to_html().to_lowercase();
    for token in tokens(&input) {
        assert!(html.contains(&token), "the HTML export dropped {token:?}");
    }

    // CSV is a table export; it must keep every cell, and it is allowed to
    // leave prose out because a CSV of paragraphs is not a spreadsheet.
    let table_tokens = tokens("Region Amount Cairo 900 Giza 350");
    assert_eq!(table_tokens, tokens(&document.to_csv()));
}

#[test]
fn html_escapes_markup() {
    let document = organize_text("<script>alert('x')</script>");
    let html = document.to_html();
    assert!(html.contains("&lt;script&gt;"), "{html}");
    assert!(!html.contains("<script>alert"), "{html}");
}

#[test]
fn merged_cells_from_the_layout_keep_their_grid() {
    let source = SourceTable {
        rows: 2,
        cols: 2,
        cells: vec![
            SourceCell {
                row: 0,
                col: 0,
                row_span: 1,
                col_span: 2,
                text: "Name".to_string(),
            },
            cell(1, 0, "Ali"),
            cell(1, 1, "3"),
        ],
    };
    let document = organize(&page(vec![SourceBlock {
        kind: "table".to_string(),
        text: String::new(),
        table: Some(source),
        confidence: 1.0,
    }]));
    let table = document.blocks[0].table.as_ref().expect("table");

    assert_eq!(table.header, vec!["Name", ""]);
    assert_eq!(table.rows, vec![vec!["Ali", "3"]]);
    assert!(!table.needs_review);
    assert!(table.confidence > 0.8);
}

#[test]
fn empty_source_blocks_do_not_shift_the_source_index() {
    let document = organize(&page(vec![
        SourceBlock::text("First"),
        SourceBlock::text("   "),
        SourceBlock::text("Second"),
    ]));
    assert_eq!(document.blocks.len(), 2);
    assert_eq!(document.blocks[0].source_index, 0);
    assert_eq!(document.blocks[1].source_index, 2);
}

#[test]
fn an_empty_document_is_reported_rather_than_exported_as_success() {
    let document = organize(&SourceDocument::default());
    assert!(document.blocks.is_empty());
    assert!(document.stats.blocks == 0);
    assert!(
        document
            .warnings
            .iter()
            .any(|warning| warning.code == "empty_document")
    );
}
