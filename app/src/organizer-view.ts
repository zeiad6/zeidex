/**
 * Presentation rules for the Data Intelligence panel.
 *
 * The Rust organizer owns every decision about content: which document a page
 * is, what a field is called, and what a value is. This module only decides how
 * to show those decisions — labels, tones, and the small pure edits the panel
 * needs — so the panel itself stays a view and these rules stay testable.
 */
import type {
  BatchExtraction,
  DocumentKind,
  Extraction,
  ExtractionField,
  OrganizerLanguage,
  OrganizerWarning,
} from "./api";

export interface DocumentKindMeta {
  id: DocumentKind;
  label: string;
  labelAr: string;
}

/** Every profile the extractor ships, in the order the picker shows them. */
export const DOCUMENT_KINDS: readonly DocumentKindMeta[] = [
  { id: "id_card", label: "ID card", labelAr: "بطاقة هوية" },
  { id: "passport", label: "Passport", labelAr: "جواز سفر" },
  { id: "invoice", label: "Invoice", labelAr: "فاتورة" },
  { id: "receipt", label: "Receipt", labelAr: "إيصال" },
  { id: "bank_statement", label: "Bank statement", labelAr: "كشف حساب" },
  { id: "certificate", label: "Certificate", labelAr: "شهادة" },
  { id: "business_card", label: "Business card", labelAr: "بطاقة عمل" },
  { id: "letter", label: "Letter", labelAr: "خطاب" },
  { id: "form", label: "Form", labelAr: "نموذج" },
  { id: "table", label: "Table", labelAr: "جدول" },
  { id: "unknown", label: "Document", labelAr: "مستند" },
];

const UNKNOWN_KIND: DocumentKindMeta = {
  id: "unknown",
  label: "Document",
  labelAr: "مستند",
};

export function kindMeta(kind: DocumentKind): DocumentKindMeta {
  return DOCUMENT_KINDS.find((meta) => meta.id === kind) ?? UNKNOWN_KIND;
}

export function kindLabel(kind: DocumentKind, rtl: boolean): string {
  const meta = kindMeta(kind);
  return rtl ? meta.labelAr : meta.label;
}

/**
 * Warning codes are stable identifiers from `core/organizer`. A code this UI
 * has not learned yet is shown verbatim rather than hidden, so a new detector
 * warning can never disappear silently.
 */
const WARNINGS: Record<string, { label: string; labelAr: string }> = {
  empty_document: {
    label: "No text was recognized",
    labelAr: "لم يُستخرج أي نص",
  },
  empty_page: {
    label: "A page produced no fields; its row is kept blank",
    labelAr: "صفحة بلا حقول — صفّها محفوظ فارغًا",
  },
  table_needs_review: {
    label: "A table needs review",
    labelAr: "جدول يحتاج مراجعة",
  },
  ambiguous_field: {
    label: "The same label carried more than one value",
    labelAr: "تكرر نفس الحقل بأكثر من قيمة",
  },
  no_fields: {
    label: "No known fields in this profile",
    labelAr: "لا حقول معروفة في هذا النوع",
  },
  total_missing: {
    label: "No total was printed; none was invented",
    labelAr: "الإجمالي غير مطبوع — لم يُحسب تلقائيًا",
  },
  field_needs_review: {
    label: "A low-confidence field needs review",
    labelAr: "حقل منخفض الثقة يحتاج مراجعة",
  },
  duplicate_value: {
    label: "The same identifier appears on more than one page",
    labelAr: "قيمة مكررة بين الصفحات",
  },
  national_id_missing: {
    label: "No national ID was found for this profile",
    labelAr: "لم يُعثر على رقم قومي لهذا النوع",
  },
  name_missing: {
    label: "No name was found for this profile",
    labelAr: "لم يُعثر على اسم لهذا النوع",
  },
};

export function warningLabel(code: string, rtl: boolean): string {
  const entry = WARNINGS[code];
  if (!entry) return code;
  return rtl ? entry.labelAr : entry.label;
}

/** Codes in first-seen order, so one repeated warning is shown once. */
export function warningCodes(warnings: OrganizerWarning[]): string[] {
  const seen: string[] = [];
  for (const warning of warnings) {
    if (!seen.includes(warning.code)) seen.push(warning.code);
  }
  return seen;
}

export function confidencePercent(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(100, Math.round(value * 100)));
}

export type ConfidenceTone = "low" | "medium" | "high";

/** Below the extractor's review threshold is low; a solid read is high. */
export function confidenceTone(value: number): ConfidenceTone {
  const percent = confidencePercent(value);
  if (percent < 60) return "low";
  if (percent < 85) return "medium";
  return "high";
}

export function languageLabel(language: OrganizerLanguage, rtl: boolean): string {
  switch (language) {
    case "arabic":
      return rtl ? "عربي" : "Arabic";
    case "english":
      return rtl ? "إنجليزي" : "English";
    case "mixed":
      return rtl ? "مختلط" : "Mixed";
    default:
      return rtl ? "غير محدد" : "Unknown";
  }
}

export function fieldLabel(field: ExtractionField, rtl: boolean): string {
  return rtl && field.label_ar ? field.label_ar : field.label;
}

export function needsReviewCount(extraction: Extraction): number {
  return extraction.fields.filter((field) => field.needs_review).length;
}

export function batchReviewCount(batch: BatchExtraction): number {
  return batch.rows.filter((row) => row.needs_review).length;
}

/**
 * Applies a manual correction. The edit clears the review flag because a human
 * has now looked at the value; every other field, and the field order, is kept.
 */
export function applyFieldEdit(
  extraction: Extraction,
  index: number,
  value: string,
): Extraction {
  return {
    ...extraction,
    fields: extraction.fields.map((field, position) =>
      position === index ? { ...field, value, needs_review: false } : field,
    ),
  };
}

/** A tab character would split one cell into two, so it becomes a space. */
function cell(text: string): string {
  return text.replace(/\t/g, " ").replace(/\r?\n/g, " ").trim();
}

export function fieldsAsTsv(extraction: Extraction, rtl: boolean): string {
  const header = rtl ? ["الحقل", "القيمة"] : ["Field", "Value"];
  const lines = [header.join("\t")];
  for (const field of extraction.fields) {
    lines.push([cell(fieldLabel(field, rtl)), cell(field.value)].join("\t"));
  }
  return lines.join("\n");
}

/**
 * The batch grid as TSV. The page number is the first column and rows keep page
 * order, so pasting a stack of cards into a spreadsheet lines up row for row.
 */
export function batchAsTsv(batch: BatchExtraction, rtl: boolean): string {
  const header = [rtl ? "الصفحة" : "Page"];
  for (const column of batch.columns) {
    header.push(cell(rtl && column.label_ar ? column.label_ar : column.label));
  }
  const lines = [header.join("\t")];
  for (const row of batch.rows) {
    lines.push([String(row.page + 1), ...row.values.map(cell)].join("\t"));
  }
  return lines.join("\n");
}

/** Which exporter owns the active tab. */
export type ExportSubject = "extraction" | "batch" | "organized";

export function subjectForTab(tab: string): ExportSubject {
  if (tab === "batch") return "batch";
  if (tab === "organized") return "organized";
  return "extraction";
}
