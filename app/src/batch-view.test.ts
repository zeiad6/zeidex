import assert from "node:assert/strict";
import test from "node:test";
import type { BatchEntryResult, BatchMatch } from "./api";
import {
  allPaths,
  bestScore,
  extensionOf,
  filterEntries,
  folderOf,
  formatBytes,
  refineEntries,
  sortEntries,
  statusLabel,
  thresholdLabel,
} from "./batch-view";

function match(name: string, score: number, size: number | null = 100): BatchMatch {
  return {
    path: `/docs/${name}`,
    name,
    extension: extensionOf(name) === "—" ? null : extensionOf(name),
    size_bytes: size,
    modified_unix_s: 1_700_000_000,
    kind: "file",
    score,
    match_type: score === 100 ? "exact" : "fuzzy",
  };
}

function entry(
  requested: string,
  status: BatchEntryResult["status"],
  matches: BatchMatch[] = [],
): BatchEntryResult {
  return {
    requested,
    status,
    matches,
    total_matches: matches.length,
    duration_ms: 1,
  };
}

const REPORT = [
  entry("alpha.pdf", "exact", [match("alpha.pdf", 100, 300)]),
  entry("beta", "high", [match("beta-2024.pdf", 88, 900), match("beta.pdf", 80, 100)]),
  entry("gamma.pdf", "not_found"),
  entry("delta", "possible", [match("delta-report.pdf", 62, 50)]),
];

test("filters separate the outcomes a user asks about", () => {
  assert.equal(filterEntries(REPORT, "all").length, 4);
  assert.deepEqual(
    filterEntries(REPORT, "found").map((item) => item.requested),
    ["alpha.pdf", "beta", "delta"],
  );
  assert.deepEqual(
    filterEntries(REPORT, "missing").map((item) => item.requested),
    ["gamma.pdf"],
  );
  assert.deepEqual(
    filterEntries(REPORT, "exact").map((item) => item.requested),
    ["alpha.pdf"],
  );
  // Approximate is "found, but not exactly", so it excludes both the exact row
  // and the missing one.
  assert.deepEqual(
    filterEntries(REPORT, "approximate").map((item) => item.requested),
    ["beta", "delta"],
  );
  // `beta` has two candidates: it is the row that needs a decision.
  assert.deepEqual(
    filterEntries(REPORT, "multiple").map((item) => item.requested),
    ["beta"],
  );
});

test("input order is preserved and every other sort is stable", () => {
  assert.deepEqual(
    sortEntries(REPORT, "input").map((item) => item.requested),
    ["alpha.pdf", "beta", "gamma.pdf", "delta"],
  );
  assert.deepEqual(
    sortEntries(REPORT, "status").map((item) => item.status),
    ["exact", "high", "possible", "not_found"],
  );
  assert.deepEqual(
    sortEntries(REPORT, "score").map((item) => item.requested),
    ["alpha.pdf", "beta", "delta", "gamma.pdf"],
  );
  assert.deepEqual(
    sortEntries(REPORT, "size").map((item) => item.requested),
    ["beta", "alpha.pdf", "delta", "gamma.pdf"],
  );
});

test("the type and path sorts read the best match, not the request", () => {
  assert.deepEqual(
    sortEntries(REPORT, "type").map((item) => item.requested),
    // alpha.pdf and beta both resolve to a .pdf; delta and gamma (no match)
    // keep their input order at the end.
    ["alpha.pdf", "beta", "delta", "gamma.pdf"],
  );
  assert.deepEqual(
    sortEntries(REPORT, "path").map((item) => item.requested),
    ["alpha.pdf", "beta", "delta", "gamma.pdf"],
  );
});

test("the display floor and type filter narrow without touching the report", () => {
  const before = REPORT.map((item) => item.requested);
  assert.deepEqual(
    refineEntries(REPORT, 85, null).map((item) => item.requested),
    ["alpha.pdf", "beta"],
    "85% drops the 62% candidate and every row with no match",
  );
  assert.deepEqual(
    refineEntries(REPORT, 0, "pdf").map((item) => item.requested),
    ["alpha.pdf", "beta", "delta"],
  );
  assert.deepEqual(
    refineEntries(REPORT, 0, "PDF").map((item) => item.requested),
    ["alpha.pdf", "beta", "delta"],
    "the filter is case-insensitive",
  );
  assert.deepEqual(refineEntries(REPORT, 0, "xlsx"), []);
  assert.deepEqual(REPORT.map((item) => item.requested), before);
});

test("sorting never mutates the report it was given", () => {
  const before = REPORT.map((item) => item.requested);
  sortEntries(REPORT, "name");
  assert.deepEqual(REPORT.map((item) => item.requested), before);
});

test("the best score of an unmatched entry is zero, not undefined", () => {
  assert.equal(bestScore(REPORT[0]), 100);
  assert.equal(bestScore(REPORT[2]), 0);
});

test("sizes read the way a file manager shows them", () => {
  assert.equal(formatBytes(512), "512 B");
  assert.equal(formatBytes(2048), "2.0 KB");
  assert.equal(formatBytes(5 * 1024 * 1024), "5.0 MB");
  assert.equal(formatBytes(3 * 1024 * 1024 * 1024), "3.0 GB");
  assert.equal(formatBytes(null), "—");
});

test("extensions come from the name, and a dotfile has none", () => {
  assert.equal(extensionOf("report.final.pdf"), "pdf");
  assert.equal(extensionOf("README"), "—");
  assert.equal(extensionOf(".gitignore"), "—");
  assert.equal(extensionOf("archive."), "—");
});

test("the threshold word matches the number", () => {
  assert.equal(thresholdLabel(50, false), "Broad");
  assert.equal(thresholdLabel(60, false), "Flexible");
  assert.equal(thresholdLabel(70, false), "Balanced");
  assert.equal(thresholdLabel(85, false), "High confidence");
  assert.equal(thresholdLabel(95, false), "Very strict");
  assert.equal(thresholdLabel(100, false), "Same name, with or without its extension");
  assert.equal(thresholdLabel(100, true), "الاسم نفسه، بامتداد أو بدونه");
});

test("status labels are translated, not reused from the backend key", () => {
  assert.equal(statusLabel("not_found", false), "Not found");
  assert.equal(statusLabel("not_found", true), "غير موجود");
});

test("the folder of a path keeps the separators it found", () => {
  assert.equal(folderOf("/home/user/Documents/report.pdf"), "/home/user/Documents");
  assert.equal(folderOf("C:\\Users\\me\\report.pdf"), "C:\\Users\\me");
  assert.equal(folderOf("report.pdf"), "report.pdf");
});

test("every path is collectable for a bulk copy", () => {
  assert.deepEqual(allPaths(REPORT), [
    "/docs/alpha.pdf",
    "/docs/beta-2024.pdf",
    "/docs/beta.pdf",
    "/docs/delta-report.pdf",
  ]);
});

test("an unsearchable entry is its own category, not a missing file", () => {
  const report = [
    entry("report.pdf", "exact", [match("report.pdf", 100, 300)]),
    entry("nothing", "error"),
  ];
  assert.deepEqual(
    filterEntries(report, "missing").map((item) => item.requested),
    [],
  );
  assert.deepEqual(
    filterEntries(report, "error").map((item) => item.requested),
    ["nothing"],
  );
  assert.equal(statusLabel("error", false), "Search error");
  assert.equal(statusLabel("error", true), "خطأ في البحث");
});
