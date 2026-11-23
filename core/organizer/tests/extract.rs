use zeidex_organizer::extract::{
    DocumentKind, Extraction, detect_kind, extract, extract_as, extract_batch, extract_batch_as,
};
use zeidex_organizer::{
    Language, SourceBlock, SourceCell, SourceDocument, SourcePage, SourceTable, organize,
};

fn page(blocks: Vec<SourceBlock>) -> SourcePage {
    SourcePage { blocks }
}

fn card(name: &str, national_id: &str) -> SourcePage {
    page(vec![
        SourceBlock::titled("بطاقة تحقيق الشخصية"),
        SourceBlock::text(format!("الاسم: {name}")),
        SourceBlock::text(format!("الرقم القومي: {national_id}")),
        SourceBlock::text("تاريخ الميلاد: 1998-01-01"),
        SourceBlock::text("الجنس: ذكر"),
    ])
}

fn value<'a>(extraction: &'a Extraction, key: &str) -> &'a str {
    extraction
        .field(key)
        .map(|field| field.value.as_str())
        .unwrap_or_else(|| panic!("missing field {key}: {:?}", extraction.fields))
}

#[test]
fn an_arabic_id_card_is_read_into_named_fields() {
    let document = organize(&SourceDocument {
        pages: vec![card("أحمد محمد علي", "29801011234567")],
    });
    let extraction = extract(&document);

    assert_eq!(extraction.kind, DocumentKind::IdCard);
    assert!(extraction.confidence > 0.6, "{}", extraction.confidence);
    assert_eq!(value(&extraction, "full_name"), "أحمد محمد علي");
    assert_eq!(value(&extraction, "national_id"), "29801011234567");
    assert_eq!(value(&extraction, "date_of_birth"), "1998-01-01");
    assert_eq!(value(&extraction, "gender"), "ذكر");

    // Profile order, so every card reads the same way.
    let keys: Vec<&str> = extraction
        .fields
        .iter()
        .map(|field| field.key.as_str())
        .collect();
    assert_eq!(
        keys,
        vec!["full_name", "national_id", "date_of_birth", "gender"]
    );
    assert!(!extraction.field("national_id").expect("id").needs_review);
}

#[test]
fn an_unlabeled_card_is_extracted_but_flagged() {
    let document = organize(&SourceDocument {
        pages: vec![page(vec![
            SourceBlock::text("أحمد محمد علي"),
            SourceBlock::text("29801011234567"),
        ])],
    });
    let extraction = extract(&document);

    assert_eq!(extraction.kind, DocumentKind::IdCard);
    assert_eq!(value(&extraction, "national_id"), "29801011234567");
    assert!(
        extraction.field("national_id").expect("id").needs_review,
        "an unlabeled number must not be presented as certain"
    );
    assert!(extraction.field("full_name").expect("name").needs_review);
    assert!(
        extraction
            .warnings
            .iter()
            .any(|warning| warning.code == "field_needs_review")
    );
}

#[test]
fn an_invoice_keeps_its_numbers_exactly_as_recognized() {
    let document = organize(&SourceDocument {
        pages: vec![page(vec![
            SourceBlock::titled("فاتورة ضريبية"),
            SourceBlock::text("Invoice No: INV-2026-001"),
            SourceBlock::text("Invoice Date: 2026-02-01"),
            SourceBlock::text("Vendor: Nile Office Supplies"),
            SourceBlock::text("Subtotal: 1,000.00"),
            SourceBlock::text("VAT: 140.00"),
            SourceBlock::text("Total: 1,140.00"),
            SourceBlock::text("Currency: EGP"),
        ])],
    });
    let extraction = extract(&document);

    assert_eq!(extraction.kind, DocumentKind::Invoice);
    assert_eq!(value(&extraction, "invoice_number"), "INV-2026-001");
    assert_eq!(value(&extraction, "subtotal"), "1,000.00");
    assert_eq!(value(&extraction, "tax"), "140.00");
    assert_eq!(value(&extraction, "total"), "1,140.00");
    assert_eq!(value(&extraction, "currency"), "EGP");
    assert!(
        !extraction
            .warnings
            .iter()
            .any(|warning| warning.code == "total_missing")
    );
}

#[test]
fn an_invoice_without_a_total_is_reported_not_calculated() {
    let document = organize(&SourceDocument {
        pages: vec![page(vec![
            SourceBlock::titled("فاتورة"),
            SourceBlock::text("Invoice No: 42"),
            SourceBlock::text("Subtotal: 1,000.00"),
            SourceBlock::text("VAT: 140.00"),
        ])],
    });
    let extraction = extract(&document);

    assert_eq!(extraction.kind, DocumentKind::Invoice);
    assert!(extraction.field("total").is_none());
    assert!(
        extraction
            .warnings
            .iter()
            .any(|warning| warning.code == "total_missing")
    );
}

#[test]
fn a_passport_uses_the_passport_profile() {
    let document = organize(&SourceDocument {
        pages: vec![page(vec![
            SourceBlock::titled("Passport"),
            SourceBlock::text("Passport No: A12345678"),
            SourceBlock::text("Name: Ahmed Mohamed Ali"),
            SourceBlock::text("Nationality: Egyptian"),
            SourceBlock::text("Date of Birth: 1998-01-01"),
            SourceBlock::text("Place of Birth: Cairo"),
        ])],
    });
    let extraction = extract(&document);

    assert_eq!(extraction.kind, DocumentKind::Passport);
    assert_eq!(value(&extraction, "passport_number"), "A12345678");
    assert_eq!(value(&extraction, "place_of_birth"), "Cairo");
}

#[test]
fn a_batch_of_id_cards_keeps_one_row_per_card_in_page_order() {
    let names = ["أحمد محمد علي", "منى سعيد حسن", "كريم طارق فؤاد"];
    let ids = ["29801011234567", "30105234567890", "29507129876543"];
    let document = organize(&SourceDocument {
        pages: names
            .iter()
            .zip(ids.iter())
            .map(|(name, id)| card(name, id))
            .collect(),
    });
    let batch = extract_batch(&document);

    assert_eq!(batch.kind, DocumentKind::IdCard);
    assert_eq!(batch.rows.len(), 3);

    let name_column = batch
        .columns
        .iter()
        .position(|column| column.key == "full_name")
        .expect("name column");
    let id_column = batch
        .columns
        .iter()
        .position(|column| column.key == "national_id")
        .expect("id column");

    for (index, row) in batch.rows.iter().enumerate() {
        assert_eq!(row.page, index);
        assert_eq!(row.values[name_column], names[index], "row {index} name");
        assert_eq!(row.values[id_column], ids[index], "row {index} id");
    }
}

#[test]
fn a_blank_card_keeps_its_row_instead_of_shifting_the_rest() {
    let document = organize(&SourceDocument {
        pages: vec![
            card("أحمد محمد علي", "29801011234567"),
            page(vec![SourceBlock::text("   ")]),
            card("كريم طارق فؤاد", "29507129876543"),
        ],
    });
    let batch = extract_batch(&document);

    assert_eq!(batch.rows.len(), 3);
    let name_column = batch
        .columns
        .iter()
        .position(|column| column.key == "full_name")
        .expect("name column");
    let id_column = batch
        .columns
        .iter()
        .position(|column| column.key == "national_id")
        .expect("id column");

    assert_eq!(batch.rows[0].values[name_column], "أحمد محمد علي");
    assert_eq!(batch.rows[1].values[name_column], "");
    assert_eq!(batch.rows[1].values[id_column], "");
    assert!(batch.rows[1].needs_review);
    assert_eq!(batch.rows[2].values[name_column], "كريم طارق فؤاد");
    assert_eq!(batch.rows[2].values[id_column], "29507129876543");
    assert!(
        batch
            .warnings
            .iter()
            .any(|warning| warning.code == "empty_page")
    );
}

#[test]
fn a_repeated_identifier_is_reported_with_both_pages() {
    let document = organize(&SourceDocument {
        pages: vec![
            card("أحمد محمد علي", "29801011234567"),
            card("أحمد محمد علي", "29801011234567"),
        ],
    });
    let batch = extract_batch(&document);

    let warning = batch
        .warnings
        .iter()
        .find(|warning| warning.code == "duplicate_value")
        .expect("duplicate warning");
    assert!(warning.message.contains("29801011234567"));
    assert!(warning.message.contains("1") && warning.message.contains("2"));
}

#[test]
fn the_batch_exports_as_one_real_table() {
    let document = organize(&SourceDocument {
        pages: vec![
            card("أحمد محمد علي", "29801011234567"),
            card("منى سعيد حسن", "30105234567890"),
        ],
    });
    let batch = extract_batch(&document);

    let csv = batch.to_csv();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 3, "header + two cards: {csv}");
    assert!(csv.contains("29801011234567"));
    assert!(csv.contains("30105234567890"));

    let markdown = batch.to_markdown();
    assert!(markdown.contains("---"), "{markdown}");
    assert!(markdown.contains("|الاسم|"), "{markdown}");

    let rendered = batch.to_document();
    let tables: Vec<_> = rendered
        .blocks
        .iter()
        .filter_map(|block| block.table.as_ref())
        .collect();
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].rows.len(), 2);
    assert_eq!(tables[0].columns, batch.columns.len());
}

#[test]
fn a_table_of_labeled_cells_becomes_fields() {
    let source = SourceTable {
        rows: 3,
        cols: 2,
        cells: vec![
            SourceCell {
                row: 0,
                col: 0,
                row_span: 1,
                col_span: 1,
                text: "الاسم".to_string(),
            },
            SourceCell {
                row: 0,
                col: 1,
                row_span: 1,
                col_span: 1,
                text: "أحمد محمد علي".to_string(),
            },
            SourceCell {
                row: 1,
                col: 0,
                row_span: 1,
                col_span: 1,
                text: "الرقم الوطني".to_string(),
            },
            SourceCell {
                row: 1,
                col: 1,
                row_span: 1,
                col_span: 1,
                text: "29801011234567".to_string(),
            },
            SourceCell {
                row: 2,
                col: 0,
                row_span: 1,
                col_span: 1,
                text: "العنوان".to_string(),
            },
            SourceCell {
                row: 2,
                col: 1,
                row_span: 1,
                col_span: 1,
                text: "15 شارع النيل".to_string(),
            },
        ],
    };
    let document = organize(&SourceDocument {
        pages: vec![page(vec![SourceBlock {
            kind: "table".to_string(),
            text: String::new(),
            table: Some(source),
            confidence: 0.9,
        }])],
    });
    let extraction = extract(&document);

    assert_eq!(value(&extraction, "full_name"), "أحمد محمد علي");
    assert_eq!(value(&extraction, "national_id"), "29801011234567");
    assert_eq!(value(&extraction, "address"), "15 شارع النيل");
}

#[test]
fn an_unknown_document_invents_nothing() {
    let document = organize(&SourceDocument {
        pages: vec![page(vec![SourceBlock::text("Hello world")])],
    });
    let extraction = extract(&document);

    assert_eq!(extraction.kind, DocumentKind::Unknown);
    assert!(extraction.fields.is_empty());
    assert!(
        extraction
            .warnings
            .iter()
            .any(|warning| warning.code == "no_fields")
    );
}

#[test]
fn a_custom_label_is_kept_with_its_value() {
    let document = organize(&SourceDocument {
        pages: vec![page(vec![
            SourceBlock::titled("فاتورة"),
            SourceBlock::text("Invoice No: 42"),
            SourceBlock::text("Payment terms: 30 days"),
            SourceBlock::text("Total: 100.00"),
        ])],
    });
    let extraction = extract(&document);

    let custom = extraction
        .fields
        .iter()
        .find(|field| field.key == "custom:payment_terms")
        .expect("custom field");
    assert_eq!(custom.value, "30 days");
    assert!(custom.needs_review);
}

#[test]
fn a_manual_profile_override_is_honored() {
    let document = organize(&SourceDocument {
        pages: vec![card("أحمد محمد علي", "29801011234567")],
    });
    let extraction = extract_as(&document, DocumentKind::Invoice);

    assert_eq!(extraction.kind, DocumentKind::Invoice);
    assert!(extraction.confidence == 1.0);
}

#[test]
fn a_manual_profile_applies_to_every_batch_row() {
    let document = organize(&SourceDocument {
        pages: vec![
            card("أحمد محمد علي", "29801011234567"),
            card("منى سعيد حسن", "30105234567890"),
        ],
    });
    // The cards detect as ID cards; the user insists they are passport pages.
    let batch = extract_batch_as(&document, DocumentKind::Passport);

    assert_eq!(batch.kind, DocumentKind::Passport);
    assert!(batch.confidence == 1.0);
    assert_eq!(batch.rows.len(), 2);
    // The passport profile leads the column order; a field the page really
    // contains may still be appended, but the profile is not ignored.
    assert!(
        batch
            .columns
            .iter()
            .any(|column| column.key == "passport_number"),
        "passport profile columns missing: {:?}",
        batch.columns
    );
    assert!(
        batch.columns.iter().any(|column| column.key == "full_name"),
        "the name column must survive: {:?}",
        batch.columns
    );
    for (index, row) in batch.rows.iter().enumerate() {
        assert_eq!(row.page, index);
    }
}

#[test]
fn detection_is_stable_and_folded() {
    let document = organize(&SourceDocument {
        pages: vec![page(vec![
            SourceBlock::text("بطاقة تحقيق الشخصية"),
            SourceBlock::text("الاسم: أحمد"),
            SourceBlock::text("الرقم القومي: 29801011234567"),
        ])],
    });
    let blocks: Vec<&zeidex_organizer::Block> = document.blocks.iter().collect();
    let (kind, confidence) = detect_kind(&blocks);
    assert_eq!(kind, DocumentKind::IdCard);
    assert!(confidence > 0.5);
    assert_eq!(document.language, Language::Arabic);
}

#[test]
fn a_label_column_in_one_region_does_not_swallow_the_next_label() {
    // One OCR region holding two labeled lines: the first value must not absorb
    // the second label.
    let document = organize(&SourceDocument {
        pages: vec![page(vec![
            SourceBlock::titled("Invoice"),
            SourceBlock::text("Invoice No: INV-9\nTotal: 250.00"),
        ])],
    });
    let extraction = extract(&document);

    assert_eq!(value(&extraction, "invoice_number"), "INV-9");
    assert_eq!(value(&extraction, "total"), "250.00");
}

#[test]
fn a_continuation_line_stays_with_its_single_label() {
    let document = organize(&SourceDocument {
        pages: vec![page(vec![
            SourceBlock::titled("Invoice"),
            SourceBlock::text("Address: 15 Nile Street\nCairo"),
            SourceBlock::text("Total: 10.00"),
        ])],
    });
    let extraction = extract(&document);

    assert_eq!(value(&extraction, "address"), "15 Nile Street\nCairo");
}

#[test]
fn a_batch_without_a_fixed_profile_builds_columns_from_its_own_labels() {
    // A form has no field spec, so the only honest columns are the labels the
    // pages actually carry — and they must still line up per page.
    let document = organize(&SourceDocument {
        pages: vec![
            page(vec![
                SourceBlock::titled("نموذج طلب"),
                SourceBlock::text("Payment terms: 30 days"),
                SourceBlock::text("Project code: ZX-9"),
                SourceBlock::text("Applicant: Ahmed"),
            ]),
            page(vec![
                SourceBlock::titled("نموذج طلب"),
                SourceBlock::text("Payment terms: 45 days"),
                SourceBlock::text("Delivery point: Cairo"),
                SourceBlock::text("Applicant: Mona"),
            ]),
        ],
    });
    let batch = extract_batch(&document);

    assert_eq!(batch.kind, DocumentKind::Form);
    let keys: Vec<&str> = batch
        .columns
        .iter()
        .map(|column| column.key.as_str())
        .collect();
    assert_eq!(
        keys,
        vec![
            "custom:payment_terms",
            "custom:project_code",
            "custom:applicant",
            "custom:delivery_point",
        ]
    );

    assert_eq!(batch.rows.len(), 2);
    assert_eq!(batch.rows[0].page, 0);
    assert_eq!(batch.rows[0].values, vec!["30 days", "ZX-9", "Ahmed", ""]);
    assert_eq!(batch.rows[1].page, 1);
    assert_eq!(batch.rows[1].values, vec!["45 days", "", "Mona", "Cairo"]);
    assert!(batch.rows.iter().all(|row| row.needs_review));
}
