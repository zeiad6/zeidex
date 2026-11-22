/**
 * A browser-side stand-in for `zeidex-batch`.
 *
 * The real matching runs in Rust against the real index; this only exists so
 * the Batch Search screen can be opened and clicked in a plain browser
 * (`?demo=1`) without the shell. It mirrors the same rules — normalize, then
 * exact / stem / containment / tokens / bounded edit distance — so the screen
 * shows believable numbers instead of random ones. Dev-only.
 */

const MAX_ENTRIES = 2000;
const MAX_EDITS = 4;

/** Case, diacritics, Arabic letter variants and Arabic-Indic digits. */
export function normalizeForSearch(input: string): string {
  let text = input.normalize("NFKD");
  // Strip combining marks (harakat, Latin accents) and tatweel.
  text = text.replace(/[\u0300-\u036f\u064b-\u065f\u0670\u0640]/g, "");
  text = text.toLowerCase();
  text = text
    .replace(/\u0623|\u0625|\u0622/g, "\u0627")
    .replace(/\u0629/g, "\u0647")
    .replace(/\u0649/g, "\u064a")
    .replace(/\u0624/g, "\u0648")
    .replace(/\u0626/g, "\u064a");
  text = text.replace(/[\u0660-\u0669]/g, (digit) =>
    String.fromCharCode(digit.charCodeAt(0) - 0x0660 + 48),
  );
  return text.replace(/\s+/g, " ").trim();
}

export interface DemoParsed {
  entries: string[];
  dropped_empty: number;
  duplicates: number;
  overflow: number;
}

export function parseList(text: string): DemoParsed {
  const seen = new Set<string>();
  const entries: string[] = [];
  let dropped = 0;
  let duplicates = 0;
  let overflow = 0;
  for (const raw of text.split(/[\r\n\t]+/)) {
    const trimmed = raw.replace(/^\uFEFF/, "").trim();
    const unquoted = /^(["']).*\1$/s.test(trimmed) ? trimmed.slice(1, -1).trim() : trimmed;
    if (!unquoted) {
      dropped += 1;
      continue;
    }
    const key = normalizeForSearch(unquoted);
    if (seen.has(key)) {
      duplicates += 1;
      continue;
    }
    if (entries.length >= MAX_ENTRIES) {
      overflow += 1;
      continue;
    }
    seen.add(key);
    entries.push(unquoted);
  }
  return { entries, dropped_empty: dropped, duplicates, overflow };
}

export interface DemoMatch {
  path: string;
  name: string;
  extension: string | null;
  size_bytes: number | null;
  modified_unix_s: number | null;
  kind: "file" | "folder";
  score: number;
  match_type: "exact" | "stem" | "containment" | "tokens" | "fuzzy";
}

export interface DemoEntry {
  requested: string;
  status: "exact" | "high" | "possible" | "not_found";
  matches: DemoMatch[];
  total_matches: number;
  duration_ms: number;
}

export interface DemoReport {
  threshold: number;
  indexed_files: number;
  entries: DemoEntry[];
  summary: {
    total: number;
    found: number;
    not_found: number;
    exact: number;
    approximate: number;
    multiple: number;
    success_percent: number;
    duration_ms: number;
  };
}

const FOLDER = "/home/you/Documents";

function file(name: string, size: number, folder = FOLDER): DemoMatch {
  const dot = name.lastIndexOf(".");
  return {
    path: `${folder}/${name}`,
    name,
    extension: dot > 0 ? name.slice(dot + 1) : null,
    size_bytes: size,
    modified_unix_s: 1_763_000_000,
    kind: "file",
    score: 0,
    match_type: "exact",
  };
}

/** A small but mixed index: Arabic and English, duplicates, folders. */
const INDEX: DemoMatch[] = [
  file("invoice-2024-final.pdf", 284_112, "/home/you/Documents/Invoices"),
  file("invoice-2024-014.pdf", 91_204, "/home/you/Documents/Invoices"),
  file("invoice-2023-summary.xlsx", 48_512, "/home/you/Documents/Invoices"),
  file("quarterly-report.pdf", 1_204_882),
  file("quarterly-report-Q3.docx", 88_120),
  file("report.pdf", 12_004, "/home/you/Downloads"),
  file("report.pdf", 4_882, "/home/you/Archive/2024"),
  file("meeting-notes-2026-03.docx", 22_410),
  file("budget-2026.xlsx", 71_002, "/home/you/Finance"),
  file("project-plan.pdf", 402_110),
  file("contract-signed.pdf", 688_402, "/home/you/Legal"),
  file("تقرير-الاجتماع.docx", 31_220, "/home/you/Documents/العمل"),
  file("فاتورة-2024.pdf", 120_884, "/home/you/Documents/الفواتير"),
  file("مدرسة-الأطفال.pdf", 55_140, "/home/you/Documents/العمل"),
  file("كتاب-الطالب.pdf", 2_884_000, "/home/you/Documents/كتب"),
  file("خطة-المشروع-2026.pdf", 190_002, "/home/you/Documents/العمل"),
  file("notes.txt", 1_240),
  file("README.md", 3_012, "/home/you/Projects/zeidex"),
  {
    path: "/home/you/Documents/Invoices",
    name: "Invoices",
    extension: null,
    size_bytes: null,
    modified_unix_s: 1_763_000_000,
    kind: "folder",
    score: 0,
    match_type: "exact",
  },
];

export const DEMO_LIST = [
  "invoice-2024-final.pdf",
  "quarterly-report.pdf",
  "report.pdf",
  "فاتورة-2024.pdf",
  "مدرسه-الاطفال.pdf",
  "budget-2026.xlsx",
  "meeting-notes.docx",
  "contract-2025.pdf",
  "خطة-المشروع.pdf",
  "missing-file.txt",
].join("\n");

function stemOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}

function tokens(text: string): string[] {
  return text.split(/[^\p{L}\p{N}]+/u).filter(Boolean);
}

function levenshtein(a: string[], b: string[], max: number): number {
  if (Math.abs(a.length - b.length) > max) return max + 1;
  let previous: number[] = Array.from({ length: b.length + 1 }, (_, index) => index);
  for (let i = 1; i <= a.length; i += 1) {
    const current = new Array<number>(b.length + 1).fill(max + 1);
    current[0] = i;
    let rowMin = current[0];
    for (let j = 1; j <= b.length; j += 1) {
      current[j] =
        a[i - 1] === b[j - 1]
          ? previous[j - 1]
          : 1 + Math.min(previous[j - 1], previous[j], current[j - 1]);
      rowMin = Math.min(rowMin, current[j]);
    }
    if (rowMin > max) return max + 1;
    previous = current;
  }
  return previous[b.length];
}

function scoreOne(candidate: string, query: string, threshold: number): [number, DemoMatch["match_type"]] | null {
  const queryChars = [...query];
  const candidateChars = [...candidate];
  const maxLen = Math.max(queryChars.length, candidateChars.length);
  if (maxLen === 0) return null;
  const allowed = Math.min(MAX_EDITS, Math.floor((maxLen * (100 - threshold)) / 100));

  const shorter = query.length <= candidate.length ? query : candidate;
  const longer = query.length <= candidate.length ? candidate : query;
  if ([...shorter].length >= 3 && longer.includes(shorter)) {
    const ratio = [...shorter].length / [...longer].length;
    let score = 60 + 40 * ratio;
    if (longer.startsWith(shorter)) score += 20;
    return [Math.min(99, Math.round(score)), "containment"];
  }
  const queryTokens = tokens(query);
  if (queryTokens.length > 1) {
    const haystack = tokens(candidate);
    if (queryTokens.every((token) => haystack.includes(token))) return [85, "tokens"];
  }
  if (Math.abs(queryChars.length - candidateChars.length) > allowed) return null;
  const distance = levenshtein(queryChars, candidateChars, allowed);
  if (distance > allowed) return null;
  const score = Math.min(99, Math.round((1 - distance / maxLen) * 100));
  return score >= threshold ? [score, "fuzzy"] : null;
}

export function searchBatch(entries: string[], threshold: number): DemoReport {
  const started = Date.now();
  const results: DemoEntry[] = entries.map((requested) => {
    const query = normalizeForSearch(requested);
    const found: DemoMatch[] = [];
    if (query) {
      for (const item of INDEX) {
        const name = normalizeForSearch(item.name);
        const stem = normalizeForSearch(stemOf(item.name));
        let best: [number, DemoMatch["match_type"]] | null = null;
        if (name === query) best = [100, "exact"];
        else if (stem === query) best = [100, "stem"];
        else {
          for (const text of new Set([name, stem])) {
            const scored = scoreOne(text, query, threshold);
            if (scored && (!best || scored[0] > best[0])) best = scored;
          }
        }
        if (best) found.push({ ...item, score: best[0], match_type: best[1] });
      }
    }
    found.sort((a, b) => b.score - a.score || a.name.localeCompare(b.name));
    const total = found.length;
    const best = found[0];
    const status: DemoEntry["status"] = !best
      ? "not_found"
      : best.score >= 100
        ? "exact"
        : best.score >= 85
          ? "high"
          : "possible";
    return {
      requested,
      status,
      matches: found.slice(0, 8),
      total_matches: total,
      duration_ms: 1,
    };
  });

  const found = results.filter((entry) => entry.matches.length > 0).length;
  return {
    threshold,
    indexed_files: INDEX.length,
    entries: results,
    summary: {
      total: results.length,
      found,
      not_found: results.length - found,
      exact: results.filter((entry) => entry.status === "exact").length,
      approximate: results.filter((entry) => entry.matches.length > 0 && entry.status !== "exact").length,
      multiple: results.filter((entry) => entry.total_matches > 1).length,
      success_percent: results.length ? Math.round((found / results.length) * 1000) / 10 : 0,
      duration_ms: Math.max(1, Date.now() - started),
    },
  };
}
