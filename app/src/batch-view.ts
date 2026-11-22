/**
 * The Batch Search view's pure parts: filtering, ordering and formatting.
 *
 * Kept out of the component so the rules can be tested without a DOM, and so
 * the component only decides how things look.
 */

import type { BatchEntryResult, BatchMatch, BatchStatus } from "./api";

export type BatchFilter =
  | "all"
  | "found"
  | "missing"
  | "exact"
  | "approximate"
  | "multiple"
  | "error";
export type BatchSort = "input" | "status" | "score" | "name" | "size" | "type" | "path";

/** The percentage a row is ordered and filtered by: its best candidate. */
export function bestScore(entry: BatchEntryResult): number {
  return entry.matches[0]?.score ?? 0;
}

export function bestMatch(entry: BatchEntryResult): BatchMatch | null {
  return entry.matches[0] ?? null;
}

export function filterEntries(
  entries: BatchEntryResult[],
  filter: BatchFilter,
): BatchEntryResult[] {
  switch (filter) {
    case "all":
      return entries;
    case "found":
      return entries.filter((entry) => entry.matches.length > 0);
    case "missing":
      // An unsearchable entry is not a missing file: the search never ran, so
      // it belongs in its own category rather than in the follow-up list.
      return entries.filter(
        (entry) => entry.matches.length === 0 && entry.status !== "error",
      );
    case "error":
      return entries.filter((entry) => entry.status === "error");
    case "exact":
      return entries.filter((entry) => entry.status === "exact");
    case "approximate":
      return entries.filter(
        (entry) => entry.matches.length > 0 && entry.status !== "exact",
      );
    case "multiple":
      // More than one candidate at or above the threshold: the row a user has
      // to decide about, whether or not the best one is exact.
      return entries.filter((entry) => entry.total_matches > 1);
  }
}

/**
 * Orders a copy. `input` keeps the order the names were pasted in, which is the
 * only order that means anything when the list came from a spreadsheet.
 *
 * The sort is stable by construction (Array#sort is stable), so equal scores
 * keep their input order instead of shuffling between renders.
 */
export function sortEntries(entries: BatchEntryResult[], sort: BatchSort): BatchEntryResult[] {
  const copy = entries.slice();
  switch (sort) {
    case "input":
      return copy;
    case "status":
      return copy.sort((a, b) => statusRank(a.status) - statusRank(b.status));
    case "score":
      return copy.sort((a, b) => bestScore(b) - bestScore(a));
    case "name":
      return copy.sort((a, b) => a.requested.localeCompare(b.requested));
    case "size":
      return copy.sort(
        (a, b) => (bestMatch(b)?.size_bytes ?? -1) - (bestMatch(a)?.size_bytes ?? -1),
      );
    case "type":
      // A row with no match, or a matched folder, has no extension; both sort
      // after every typed file so the list reads as a document inventory first.
      return copy.sort((a, b) =>
        compareOptional(
          bestMatch(a)?.extension?.toLowerCase(),
          bestMatch(b)?.extension?.toLowerCase(),
        ),
      );
    case "path":
      return copy.sort((a, b) =>
        compareOptional(bestMatch(a)?.path, bestMatch(b)?.path),
      );
  }
}

/**
 * Narrows the table without changing the report: the minimum percentage is
 * applied to the best score, so an entry with no match disappears as soon as
 * the user asks for any floor above zero, and the type filter keeps only rows
 * whose best match has that extension.
 */
export function refineEntries(
  entries: BatchEntryResult[],
  minScore: number,
  extension: string | null,
): BatchEntryResult[] {
  const wanted = extension?.trim().toLowerCase() || null;
  return entries.filter((entry) => {
    if (bestScore(entry) < minScore) return false;
    if (!wanted) return true;
    const best = bestMatch(entry);
    return best !== null && (best.extension?.toLowerCase() ?? "") === wanted;
  });
}

/** Orders present values first, then the rows that have none. */
function compareOptional(left: string | undefined, right: string | undefined): number {
  if (left === undefined && right === undefined) return 0;
  if (left === undefined) return 1;
  if (right === undefined) return -1;
  return left.localeCompare(right);
}

function statusRank(status: BatchStatus): number {
  switch (status) {
    case "exact":
      return 0;
    case "high":
      return 1;
    case "possible":
      return 2;
    case "not_found":
      return 3;
    case "error":
      return 4;
  }
}

export function statusLabel(status: BatchStatus, rtl: boolean): string {
  switch (status) {
    case "exact":
      return rtl ? "مطابقة تامة" : "Exact";
    case "high":
      return rtl ? "ثقة عالية" : "High confidence";
    case "possible":
      return rtl ? "احتمال" : "Possible";
    case "not_found":
      return rtl ? "غير موجود" : "Not found";
    case "error":
      return rtl ? "خطأ في البحث" : "Search error";
  }
}

export function matchKindLabel(kind: BatchMatch["match_type"], rtl: boolean): string {
  if (!rtl) {
    switch (kind) {
      case "exact":
        return "Exact";
      case "stem":
        return "Exact name";
      case "containment":
        return "Contains";
      case "tokens":
        return "All words";
      case "fuzzy":
        return "Fuzzy";
    }
  }
  switch (kind) {
    case "exact":
      return "تامة";
    case "stem":
      return "الاسم بلا امتداد";
    case "containment":
      return "يحتوي";
    case "tokens":
      return "كل الكلمات";
    case "fuzzy":
      return "تقريبي";
  }
}

/** `1.4 MB`, `512 B`, or an em dash when the size is not known. */
export function formatBytes(bytes: number | null): string {
  if (bytes === null) return "—";
  const KB = 1024;
  const MB = KB * 1024;
  const GB = MB * 1024;
  if (bytes >= GB) return `${(bytes / GB).toFixed(1)} GB`;
  if (bytes >= MB) return `${(bytes / MB).toFixed(1)} MB`;
  if (bytes >= KB) return `${(bytes / KB).toFixed(1)} KB`;
  return `${bytes} B`;
}

/** `report.final.pdf` -> `pdf`; no extension -> an em dash. */
export function extensionOf(name: string): string {
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || dot === name.length - 1) return "—";
  return name.slice(dot + 1);
}

/**
 * The word shown beside the slider. The threshold is a percentage, but "78%"
 * alone does not tell a user whether that is strict or loose.
 */
export function thresholdLabel(value: number, rtl: boolean): string {
  if (value >= 100) return rtl ? "مطابقة تامة فقط" : "Exact only";
  if (value >= 90) return rtl ? "صارم جداً" : "Very strict";
  if (value >= 80) return rtl ? "ثقة عالية" : "High confidence";
  if (value >= 65) return rtl ? "متوازن" : "Balanced";
  if (value >= 55) return rtl ? "مرن" : "Flexible";
  return rtl ? "واسع" : "Broad";
}

/** The last path segment, for a row that has to fit in a narrow column. */
export function folderOf(path: string): string {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return cut > 0 ? path.slice(0, cut) : path;
}

/** Paths of every match in the report, in display order. */
export function allPaths(entries: BatchEntryResult[]): string[] {
  return entries.flatMap((entry) => entry.matches.map((match) => match.path));
}
