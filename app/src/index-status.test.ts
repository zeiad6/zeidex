import assert from "node:assert/strict";
import test from "node:test";
import { parseIndexStatus } from "./index-status";

test("accepts the status the shell emits", () => {
  assert.deepEqual(parseIndexStatus({ indexing: true, count: 812, error: null }), {
    indexing: true,
    paused: false,
    count: 812,
    error: null,
  });
  assert.deepEqual(parseIndexStatus({ indexing: false, count: 0, error: "walk failed" }), {
    indexing: false,
    paused: false,
    count: 0,
    error: "walk failed",
  });
});

test("a missing error field is treated as no error, not as a rejection", () => {
  assert.deepEqual(parseIndexStatus({ indexing: false, count: 3 }), {
    indexing: false,
    paused: false,
    count: 3,
    error: null,
  });
});

test("rejects payloads that cannot drive the footer", () => {
  assert.equal(parseIndexStatus(null), null);
  assert.equal(parseIndexStatus(undefined), null);
  assert.equal(parseIndexStatus("indexing"), null);
  assert.equal(parseIndexStatus({}), null);
  assert.equal(parseIndexStatus({ indexing: "yes", count: 1 }), null);
  assert.equal(parseIndexStatus({ indexing: true }), null);
  assert.equal(parseIndexStatus({ indexing: true, count: -1 }), null);
  assert.equal(parseIndexStatus({ indexing: true, count: Number.NaN }), null);
  assert.equal(parseIndexStatus({ indexing: true, count: Number.POSITIVE_INFINITY }), null);
  assert.equal(parseIndexStatus({ indexing: true, count: 1, error: 42 }), null);
});

test("truncates a fractional count instead of rendering a decimal", () => {
  assert.deepEqual(parseIndexStatus({ indexing: false, count: 12.9 }), {
    indexing: false,
    paused: false,
    count: 12,
    error: null,
  });
});

test("a parked walk is reported as paused, and a malformed flag is not", () => {
  assert.deepEqual(parseIndexStatus({ indexing: true, paused: true, count: 5 }), {
    indexing: true,
    paused: true,
    count: 5,
    error: null,
  });
  // Anything other than a real `true` is "not paused": the UI must not offer a
  // Resume for a walk that is still running.
  assert.equal(parseIndexStatus({ indexing: true, paused: "yes", count: 5 })?.paused, false);
});
