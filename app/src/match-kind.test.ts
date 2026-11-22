import assert from "node:assert/strict";
import test from "node:test";
import type { MatchKind } from "./api";
import { MATCH_KINDS, demoMatchKind, isMatchKind } from "./match-kind";

test("every tier has a label in both languages and a chip tone", () => {
  const kinds: MatchKind[] = [
    "exact",
    "prefix",
    "suffix",
    "contains",
    "fuzzy",
    "approximate",
  ];
  for (const kind of kinds) {
    const info = MATCH_KINDS[kind];
    assert.ok(info.label.length > 0, `${kind} has an English label`);
    assert.ok(info.labelAr.length > 0, `${kind} has an Arabic label`);
    assert.ok(info.hint.length > 0, `${kind} has an English hint`);
    assert.ok(info.hintAr.length > 0, `${kind} has an Arabic hint`);
    assert.match(info.tone, /border-/, `${kind} has a chip border tone`);
  }
});

test("the demo mirror names the same tiers the Rust matcher does", () => {
  assert.equal(demoMatchKind("report", "report"), "exact");
  assert.equal(demoMatchKind("report", "report-2026.docx"), "prefix");
  // The end of the stem, with the extension ignored.
  assert.equal(demoMatchKind("report", "annual-report.pdf"), "suffix");
  assert.equal(demoMatchKind("2026-014", "invoice-2026-014.pdf"), "suffix");
  // A query that is only the extension is a format filter, not an ending.
  assert.equal(demoMatchKind("pdf", "invoice-2026-014.pdf"), "contains");
  // A longer query that includes the extension still ends the name.
  assert.equal(demoMatchKind("2026-014.pdf", "invoice-2026-014.pdf"), "suffix");
  // Inside the name, but neither at the start nor at the end.
  assert.equal(demoMatchKind("report", "annual-report-2025.pdf"), "contains");
  assert.equal(demoMatchKind("nnual", "annual-report.pdf"), "contains");
  // A longer name that still starts with the query is a prefix, not contains.
  assert.equal(demoMatchKind("report", "report-archive-2025.pdf"), "prefix");
  // Letters in order with gaps.
  assert.equal(demoMatchKind("vsc", "visual studio code"), "fuzzy");
  // One transposed letter: close, not a miss.
  assert.equal(demoMatchKind("reprot", "report-2026.docx"), "approximate");
  assert.equal(demoMatchKind("invoise", "invoice-2026-014.pdf"), "approximate");
  assert.equal(demoMatchKind("zzzz", "annual-report.pdf"), null);
});

test("a single letter is never a suffix or an approximate match", () => {
  // Every plural would otherwise be labelled "End".
  assert.notEqual(demoMatchKind("s", "notes.txt"), "suffix");
  // Four letters is the shortest typo-tolerant query.
  assert.equal(demoMatchKind("nop", "notes.txt"), null);
});

test("classification of an empty query is null, not a tier", () => {
  assert.equal(demoMatchKind("   ", "report.pdf"), null);
  assert.equal(demoMatchKind("report", ""), null);
});

test("isMatchKind accepts the wire values and rejects anything else", () => {
  assert.equal(isMatchKind("suffix"), true);
  assert.equal(isMatchKind("approximate"), true);
  assert.equal(isMatchKind("ends_with"), false);
  assert.equal(isMatchKind(""), false);
});
