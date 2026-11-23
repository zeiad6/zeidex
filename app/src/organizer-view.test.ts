import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

import type { BatchExtraction, Extraction, OrganizerWarning } from "./api";
import {
  DOCUMENT_KINDS,
  applyFieldEdit,
  batchAsTsv,
  batchReviewCount,
  confidencePercent,
  confidenceTone,
  fieldsAsTsv,
  kindLabel,
  kindMeta,
  languageLabel,
  needsReviewCount,
  subjectForTab,
  warningCodes,
  warningLabel,
} from "./organizer-view";

function field(overrides: Partial<Extraction["fields"][number]> = {}) {
  return {
    key: "full_name",
    label: "Full name",
    label_ar: "الاسم",
    value: "أحمد محمد علي",
    confidence: 0.9,
    needs_review: false,
    source_index: 0,
    page: 0,
    ...overrides,
  };
}

function extraction(overrides: Partial<Extraction> = {}): Extraction {
  return {
    schema: 1,
    kind: "id_card",
    confidence: 0.88,
    language: "arabic",
    fields: [field()],
    warnings: [],
    ...overrides,
  };
}

function batch(overrides: Partial<BatchExtraction> = {}): BatchExtraction {
  return {
    schema: 1,
    kind: "id_card",
    confidence: 0.9,
    language: "arabic",
    columns: [
      { key: "full_name", label: "Full name", label_ar: "الاسم" },
      { key: "national_id", label: "National ID", label_ar: "الرقم الوطني" },
    ],
    rows: [
      { page: 0, values: ["أحمد", "29801011234567"], confidence: 1, needs_review: false },
      { page: 1, values: ["منى", "30105234567890"], confidence: 1, needs_review: true },
    ],
    warnings: [],
    ...overrides,
  };
}

test("every profile has a unique id and a label in both languages", () => {
  const ids = new Set<string>();
  for (const meta of DOCUMENT_KINDS) {
    assert.ok(!ids.has(meta.id), `duplicate profile id ${meta.id}`);
    ids.add(meta.id);
    assert.ok(meta.label.trim().length > 0, `${meta.id} has no English label`);
    assert.ok(meta.labelAr.trim().length > 0, `${meta.id} has no Arabic label`);
  }
  assert.ok(ids.has("id_card"));
  assert.ok(ids.has("unknown"));
});

test("an unknown profile falls back to a document, never to a wrong type", () => {
  assert.equal(kindMeta("unknown").id, "unknown");
  assert.equal(kindLabel("unknown", true), "مستند");
  assert.equal(kindLabel("unknown", false), "Document");
  assert.equal(kindLabel("invoice", true), "فاتورة");
});

test("every warning the organizer emits has a readable label in both languages", () => {
  const codes = [
    "empty_document",
    "empty_page",
    "table_needs_review",
    "ambiguous_field",
    "no_fields",
    "total_missing",
    "field_needs_review",
    "duplicate_value",
    "national_id_missing",
    "name_missing",
  ];
  for (const code of codes) {
    assert.notEqual(warningLabel(code, false), code, `${code} has no English label`);
    assert.notEqual(warningLabel(code, true), code, `${code} has no Arabic label`);
  }
});

test("a warning code this UI does not know is shown, not swallowed", () => {
  assert.equal(warningLabel("future_warning", true), "future_warning");
});

test("every warning code in the Rust organizer has a label here", () => {
  // The panel must not silently print `national_id_missing` at a user because
  // the crate grew a code the UI never learned. Scan the crate the same way the
  // contrast test scans the components.
  const dir = "../core/organizer/src";
  const codes = new Set<string>();
  for (const file of readdirSync(dir)) {
    if (!file.endsWith(".rs")) continue;
    const source = readFileSync(join(dir, file), "utf8");
    for (const match of source.matchAll(/code: "([a-z_]+)"/g)) codes.add(match[1]);
    for (const match of source.matchAll(/"([a-z_]+_missing)"/g)) codes.add(match[1]);
  }

  // A scan that found nothing would pass vacuously.
  assert.ok(codes.has("empty_document"), `scan found no codes in ${dir}`);
  assert.ok(codes.size >= 8, `only ${codes.size} codes found`);

  for (const code of codes) {
    assert.notEqual(warningLabel(code, false), code, `${code} has no English label`);
    assert.notEqual(warningLabel(code, true), code, `${code} has no Arabic label`);
  }
});

test("warnings are deduplicated in first-seen order", () => {
  const warnings: OrganizerWarning[] = [
    { code: "empty_page", message: "page 2" },
    { code: "duplicate_value", message: "id twice" },
    { code: "empty_page", message: "page 4" },
  ];
  assert.deepEqual(warningCodes(warnings), ["empty_page", "duplicate_value"]);
});

test("confidence is a clamped whole percentage", () => {
  assert.equal(confidencePercent(0.876), 88);
  assert.equal(confidencePercent(0), 0);
  assert.equal(confidencePercent(1), 100);
  assert.equal(confidencePercent(1.4), 100);
  assert.equal(confidencePercent(-0.2), 0);
  assert.equal(confidencePercent(Number.NaN), 0);
});

test("the tone crosses at the extractor's own review threshold", () => {
  assert.equal(confidenceTone(0.59), "low");
  assert.equal(confidenceTone(0.6), "medium");
  // The tone follows the percentage the user sees, so 84.9% (shown as 85%) is
  // high and 84.4% (shown as 84%) is not.
  assert.equal(confidenceTone(0.844), "medium");
  assert.equal(confidenceTone(0.849), "high");
  assert.equal(confidenceTone(0.85), "high");
  assert.equal(confidenceTone(0.1), "low");
});

test("a manual correction clears its review flag and touches nothing else", () => {
  const original = extraction({
    fields: [
      field({ needs_review: true, confidence: 0.4 }),
      field({ key: "national_id", label: "National ID", label_ar: "الرقم الوطني", value: "2980" }),
    ],
  });
  const edited = applyFieldEdit(original, 0, "أحمد محمد");

  assert.equal(edited.fields[0].value, "أحمد محمد");
  assert.equal(edited.fields[0].needs_review, false);
  // The OCR confidence is a fact about the read; it is not rewritten by an edit.
  assert.equal(edited.fields[0].confidence, 0.4);
  assert.equal(edited.fields[1].value, "2980");
  assert.equal(edited.fields[1].needs_review, false);
  // The original stays untouched, so undo history holds real snapshots.
  assert.equal(original.fields[0].value, "أحمد محمد علي");
  assert.equal(original.fields[0].needs_review, true);
});

test("review counts describe the data the user must check", () => {
  assert.equal(
    needsReviewCount(extraction({ fields: [field({ needs_review: true }), field()] })),
    1,
  );
  assert.equal(batchReviewCount(batch()), 1);
});

test("the field table copies as TSV with tabs and newlines flattened", () => {
  const tsv = fieldsAsTsv(
    extraction({
      fields: [
        field({ value: "line one\nline two" }),
        field({ key: "note", label: "Note", label_ar: "ملاحظة", value: "a\tb" }),
      ],
    }),
    true,
  );
  const lines = tsv.split("\n");
  assert.equal(lines[0], "الحقل\tالقيمة");
  assert.equal(lines[1], "الاسم\tline one line two");
  assert.equal(lines[2], "ملاحظة\ta b");
  assert.equal(lines.length, 3);
});

test("the batch table copies as TSV with a page column and page order kept", () => {
  const tsv = batchAsTsv(batch(), true);
  const lines = tsv.split("\n");
  assert.equal(lines[0], "الصفحة\tالاسم\tالرقم الوطني");
  assert.equal(lines[1], "1\tأحمد\t29801011234567");
  assert.equal(lines[2], "2\tمنى\t30105234567890");
});

test("each tab exports through the subject it shows", () => {
  assert.equal(subjectForTab("fields"), "extraction");
  assert.equal(subjectForTab("json"), "extraction");
  assert.equal(subjectForTab("csv"), "extraction");
  assert.equal(subjectForTab("markdown"), "extraction");
  assert.equal(subjectForTab("batch"), "batch");
  assert.equal(subjectForTab("organized"), "organized");
});

test("language is named in the reader's language", () => {
  assert.equal(languageLabel("arabic", true), "عربي");
  assert.equal(languageLabel("arabic", false), "Arabic");
  assert.equal(languageLabel("mixed", true), "مختلط");
  assert.equal(languageLabel("unknown", false), "Unknown");
});
