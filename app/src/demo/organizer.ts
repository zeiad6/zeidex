/**
 * Demo fixtures for the Data Intelligence panel.
 *
 * The demo bridge has no Rust, so the organizer commands cannot run in a
 * browser. These fixtures describe what the real organizer returns for the
 * demo invoice pages (three identical copies of `SAMPLE_PAGE`): the profile is
 * `invoice`, the identifiers repeat because the pages repeat, and the duplicate
 * warning is the one `core/organizer` would emit. Nothing here is used by the
 * product build; `mock.ts` is imported only in development.
 */
import type {
  BatchExtraction,
  DocumentKind,
  Extraction,
  OrganizerLanguage,
  OrganizedDocument,
} from "../api";

const LANGUAGE: OrganizerLanguage = "english";
const SCHEMA = 1;

export const DEMO_EXTRACTION: Extraction = {
  schema: SCHEMA,
  kind: "invoice",
  confidence: 0.94,
  language: LANGUAGE,
  fields: [
    {
      key: "invoice_number",
      label: "Invoice number",
      label_ar: "رقم الفاتورة",
      value: "2026-014",
      confidence: 0.97,
      needs_review: false,
      source_index: 1,
      page: 0,
    },
    {
      key: "vendor",
      label: "Vendor",
      label_ar: "المورّد",
      value: "AL NOOR TRADING CO.",
      confidence: 0.96,
      needs_review: false,
      source_index: 0,
      page: 0,
    },
    {
      key: "customer",
      label: "Customer",
      label_ar: "العميل",
      value: "Najd Logistics",
      confidence: 0.91,
      needs_review: false,
      source_index: 2,
      page: 0,
    },
    {
      key: "total",
      label: "Total",
      label_ar: "الإجمالي",
      value: "3,030.00",
      // The sample's total is a small, low-contrast line; the real engine flags
      // a read this weak, and the panel shows it as needing review.
      confidence: 0.58,
      needs_review: true,
      source_index: 3,
      page: 0,
    },
    {
      key: "currency",
      label: "Currency",
      label_ar: "العملة",
      value: "SAR",
      confidence: 0.99,
      needs_review: false,
      source_index: 3,
      page: 0,
    },
  ],
  warnings: [
    {
      code: "field_needs_review",
      message: "The total was read at 58% confidence; check it before exporting.",
      block: 3,
    },
  ],
};

export const DEMO_BATCH: BatchExtraction = {
  schema: SCHEMA,
  kind: "invoice",
  confidence: 0.94,
  language: LANGUAGE,
  columns: [
    { key: "invoice_number", label: "Invoice number", label_ar: "رقم الفاتورة" },
    { key: "vendor", label: "Vendor", label_ar: "المورّد" },
    { key: "customer", label: "Customer", label_ar: "العميل" },
    { key: "total", label: "Total", label_ar: "الإجمالي" },
    { key: "currency", label: "Currency", label_ar: "العملة" },
  ],
  // One row per page, in page order. The demo pages are copies, so the same
  // invoice number appears three times — which is exactly what the duplicate
  // warning below reports.
  rows: [
    {
      page: 0,
      values: ["2026-014", "AL NOOR TRADING CO.", "Najd Logistics", "3,030.00", "SAR"],
      confidence: 0.94,
      needs_review: true,
    },
    {
      page: 1,
      values: ["2026-014", "AL NOOR TRADING CO.", "Najd Logistics", "3,030.00", "SAR"],
      confidence: 0.94,
      needs_review: true,
    },
    {
      page: 2,
      values: ["2026-014", "AL NOOR TRADING CO.", "Najd Logistics", "3,030.00", "SAR"],
      confidence: 0.94,
      needs_review: true,
    },
  ],
  warnings: [
    {
      code: "field_needs_review",
      message: "The total was read at 58% confidence; check it before exporting.",
      block: 3,
    },
    {
      code: "duplicate_value",
      message: 'The same Invoice number "2026-014" appears on pages 1 and 2.',
      block: null,
    },
    {
      code: "duplicate_value",
      message: 'The same Invoice number "2026-014" appears on pages 1 and 3.',
      block: null,
    },
  ],
};

export const DEMO_ORGANIZED: OrganizedDocument = {
  schema: SCHEMA,
  pages: 3,
  language: LANGUAGE,
  blocks: [
    {
      kind: "title",
      text: "AL NOOR TRADING CO.",
      level: 1,
      items: [],
      key: null,
      value: null,
      table: null,
      confidence: 0.98,
      source_index: 0,
      page: 0,
    },
    {
      kind: "key_value",
      text: "Invoice no: 2026-014",
      level: null,
      items: [],
      key: "Invoice no",
      value: "2026-014",
      table: null,
      confidence: 0.97,
      source_index: 1,
      page: 0,
    },
    {
      kind: "key_value",
      text: "Total due: 3,030.00 SAR",
      level: null,
      items: [],
      key: "Total due",
      value: "3,030.00 SAR",
      table: null,
      confidence: 0.58,
      source_index: 2,
      page: 0,
    },
    {
      kind: "table",
      text: "",
      level: null,
      items: [],
      key: null,
      value: null,
      table: {
        header: ["Description", "Qty", "Amount"],
        rows: [
          ["Document scanning setup", "1", "1,450.00"],
          ["Arabic OCR licence", "2", "1,580.00"],
        ],
        columns: 3,
        confidence: 0.93,
        needs_review: false,
      },
      confidence: 0.93,
      source_index: 3,
      page: 0,
    },
  ],
  stats: {
    characters: 96,
    words: 17,
    arabic_ratio: 0,
    blocks: 4,
    tables: 1,
    needs_review: 1,
  },
  warnings: [],
};

const NO_FIELDS = {
  code: "no_fields",
  message:
    "No labeled fields were found. The text is still available in the organized view.",
  block: null,
};

/**
 * The profile picker, answered honestly. The demo pages are invoices, so any
 * other profile finds no labeled fields — which is what the real extractor
 * reports for a wrong profile rather than inventing values.
 */
export function demoExtraction(profile: unknown): Extraction {
  if (!profile || profile === "invoice") return DEMO_EXTRACTION;
  return {
    schema: SCHEMA,
    kind: profile as DocumentKind,
    confidence: 1,
    language: LANGUAGE,
    fields: [],
    warnings: [NO_FIELDS],
  };
}

export function demoBatch(profile: unknown): BatchExtraction {
  if (!profile || profile === "invoice") return DEMO_BATCH;
  return {
    schema: SCHEMA,
    kind: profile as DocumentKind,
    confidence: 1,
    language: LANGUAGE,
    // A profile with no matching labels has no columns; the rows still exist,
    // one per page, because a page must keep its slot.
    columns: [],
    rows: DEMO_BATCH.rows.map((row) => ({
      page: row.page,
      values: [],
      confidence: 1,
      needs_review: false,
    })),
    warnings: [NO_FIELDS],
  };
}
