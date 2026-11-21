import assert from "node:assert/strict";
import test from "node:test";
import { parsePageRange } from "./page-range";

test("empty inputs mean every page", () => {
  assert.deepEqual(parsePageRange("", ""), { kind: "all" });
  assert.deepEqual(parsePageRange("  ", "\t"), { kind: "all" });
});

test("a complete window is parsed as numbers", () => {
  assert.deepEqual(parsePageRange("3", "9"), { kind: "range", first: 3, last: 9 });
  assert.deepEqual(parsePageRange(" 1 ", "1"), { kind: "range", first: 1, last: 1 });
});

test("one filled box is never silently treated as every page", () => {
  const first = parsePageRange("4", "");
  assert.equal(first.kind, "invalid");
  const last = parsePageRange("", "7");
  assert.equal(last.kind, "invalid");
});

test("rejects values the backend would have to reject anyway", () => {
  for (const [first, last] of [
    ["0", "3"],
    ["3", "0"],
    ["-1", "3"],
    ["2.5", "4"],
    ["one", "2"],
    ["9", "3"],
    ["1e3", "1e3"],
    ["+2", "4"],
    ["99999999999999999999", "99999999999999999999"],
  ]) {
    assert.equal(parsePageRange(first, last).kind, "invalid", `${first}..${last}`);
  }
});

test("a window starting at one is still just a window", () => {
  assert.deepEqual(parsePageRange("1", "999"), { kind: "range", first: 1, last: 999 });
});
