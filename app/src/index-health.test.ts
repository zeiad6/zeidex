import assert from "node:assert/strict";
import test from "node:test";
import { indexHealth } from "./index-health";
import type { IndexStatus } from "./index-status";

const status = (overrides: Partial<IndexStatus> = {}): IndexStatus => ({
  indexing: false,
  paused: false,
  count: 120,
  skipped: 0,
  error: null,
  ...overrides,
});

test("a missing status asks for a refresh, not a healthy claim", () => {
  assert.equal(indexHealth(null), "needs-refresh");
});

test("an error outranks the walk flags", () => {
  assert.equal(indexHealth(status({ error: "disk unreadable", indexing: true })), "error");
});

test("a parked walk is partial", () => {
  assert.equal(indexHealth(status({ indexing: true, paused: true })), "partial");
});

test("a running walk is updating", () => {
  assert.equal(indexHealth(status({ indexing: true })), "updating");
});

test("an empty index needs a refresh", () => {
  assert.equal(indexHealth(status({ count: 0 })), "needs-refresh");
});

test("rows and no error is healthy", () => {
  assert.equal(indexHealth(status()), "healthy");
});
