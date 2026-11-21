import test from "node:test";
import assert from "node:assert/strict";
import type { AppSettings } from "./api";
import { diff, readInteger, toDraft } from "./settings-draft";

function settings(overrides: Partial<AppSettings> = {}): AppSettings {
  return {
    version: "0.1.0",
    platform: "linux",
    configPath: "/home/u/.zeidex/config",
    dbPath: "/home/u/.zeidex/zeidex.db",
    candidateCount: 12,
    fileScanDepth: 4,
    fileScanLimit: 5000,
    fileScanExtraRoots: ["/data/projects"],
    lazyIndexingEnabled: true,
    localizedAppNames: false,
    clipboardHistoryLimit: 200,
    clipboardFilterSecrets: true,
    clipboardRetentionDays: 0,
    clipboardPaused: false,
    webSearchEnabled: true,
    webSearchEngine: "duckduckgo",
    searchEngines: ["duckduckgo", "google", "bing"],
    limits: {
      fileScanDepth: [1, 12],
      fileScanLimit: [100, 100_000],
      clipboardHistoryLimit: [10, 1000],
      clipboardRetentionDays: [0, 365],
    },
    ...overrides,
  };
}

test("an untouched draft is not dirty and reports no errors", () => {
  const current = settings();
  const change = diff(toDraft(current), current);
  assert.equal(change.dirty, false);
  assert.deepEqual(change.patch, {});
  assert.deepEqual(change.errors, {});
});

test("an out-of-range value from a hand-edited config is not an error until edited", () => {
  // 99 is outside [1, 12]; the field is reported as-is, not flagged.
  const current = settings({ fileScanDepth: 99 });
  const change = diff(toDraft(current), current);
  assert.equal(change.dirty, false);
  assert.equal(change.errors.fileScanDepth, undefined);
});

test("editing a number to another valid value becomes a patch", () => {
  const current = settings();
  const draft = { ...toDraft(current), fileScanDepth: "6" };
  const change = diff(draft, current);
  assert.equal(change.patch.fileScanDepth, 6);
  assert.equal(change.dirty, true);
  assert.deepEqual(change.errors, {});
});

test("an emptied field is required, not zero", () => {
  const current = settings();
  const draft = { ...toDraft(current), clipboardHistoryLimit: "  " };
  const change = diff(draft, current);
  assert.equal(change.errors.clipboardHistoryLimit, "required");
  assert.equal("clipboardHistoryLimit" in change.patch, false);
  assert.equal(change.dirty, true);
});

test("a fractional value is refused instead of rounded", () => {
  const current = settings();
  const draft = { ...toDraft(current), fileScanLimit: "5000.5" };
  const change = diff(draft, current);
  assert.equal(change.errors.fileScanLimit, "integer");
});

test("a value above the server bound is refused here too", () => {
  const current = settings();
  const draft = { ...toDraft(current), clipboardHistoryLimit: "1001" };
  const change = diff(draft, current);
  assert.equal(change.errors.clipboardHistoryLimit, "range");
});

test("a value seen only as text equal to the saved number is not a change", () => {
  // "05000" parses to 5000, which is what is already saved: no write.
  const current = settings();
  const draft = { ...toDraft(current), fileScanLimit: "05000" };
  const change = diff(draft, current);
  assert.equal("fileScanLimit" in change.patch, false);
  assert.equal(change.dirty, false);
});

test("equal roots in a different order are a change, because order is what gets written", () => {
  const current = settings({ fileScanExtraRoots: ["/a", "/b"] });
  const draft = { ...toDraft(current), fileScanExtraRoots: ["/b", "/a"] };
  const change = diff(draft, current);
  assert.deepEqual(change.patch.fileScanExtraRoots, ["/b", "/a"]);
});

test("removing every root sends an empty list, not an omitted key", () => {
  const current = settings({ fileScanExtraRoots: ["/a"] });
  const draft = { ...toDraft(current), fileScanExtraRoots: [] };
  const change = diff(draft, current);
  assert.deepEqual(change.patch.fileScanExtraRoots, []);
  assert.equal(change.dirty, true);
});

test("booleans and the web-search pair travel only when they differ", () => {
  const current = settings();
  const draft = {
    ...toDraft(current),
    lazyIndexingEnabled: false,
    clipboardFilterSecrets: false,
    webSearchEnabled: false,
    webSearchEngine: "bing",
  };
  const change = diff(draft, current);
  assert.equal(change.patch.lazyIndexingEnabled, false);
  assert.equal(change.patch.clipboardFilterSecrets, false);
  assert.deepEqual(change.patch.webSearch, { enabled: false, engine: "bing" });
  assert.equal(change.patch.localizedAppNames, undefined);
});

test("an engine change alone still sends the enabled flag", () => {
  const current = settings();
  const draft = { ...toDraft(current), webSearchEngine: "google" };
  const change = diff(draft, current);
  assert.deepEqual(change.patch.webSearch, { enabled: true, engine: "google" });
});

function errorOf(raw: string, bounds: [number, number]): string | undefined {
  const parsed = readInteger(raw, bounds);
  return "error" in parsed ? parsed.error : undefined;
}

test("valid values are accepted at both ends of every bound", () => {
  assert.deepEqual(readInteger("1", [1, 12]), { value: 1 });
  assert.deepEqual(readInteger("12", [1, 12]), { value: 12 });
  assert.deepEqual(readInteger("0", [0, 365]), { value: 0 });
  assert.equal(errorOf("13", [1, 12]), "range");
  assert.equal(errorOf("0", [1, 12]), "range");
  assert.equal(errorOf("", [0, 1]), "required");
  assert.equal(errorOf("abc", [0, 10]), "integer");
  assert.equal(errorOf("NaN", [0, 10]), "integer");
});
