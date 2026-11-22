import assert from "node:assert/strict";
import test from "node:test";
import { formatBytes, formatCer, formatSpeed, percentOf } from "./model-format";

test("megabytes round to a readable figure", () => {
  assert.equal(formatBytes(20_400_000), "19 MB");
  assert.equal(formatBytes(129_000_000), "123 MB");
});

test("gigabytes keep one decimal below ten", () => {
  assert.equal(formatBytes(1_288_490_188), "1.2 GB");
  assert.equal(formatBytes(32 * 1024 * 1024 * 1024), "32 GB");
});

test("a zero or negative size is zero megabytes, never NaN", () => {
  assert.equal(formatBytes(0), "0 MB");
  assert.equal(formatBytes(-5), "0 MB");
  assert.equal(formatBytes(Number.NaN), "0 MB");
});

test("speed switches unit at one megabyte per second", () => {
  assert.equal(formatSpeed(860_160), "840 KB/s");
  assert.equal(formatSpeed(3_355_443), "3.2 MB/s");
  assert.equal(formatSpeed(0), "—");
});

test("progress is clamped and rounded", () => {
  assert.equal(percentOf(0, 100), 0);
  assert.equal(percentOf(50, 200), 25);
  assert.equal(percentOf(300, 200), 100);
  assert.equal(percentOf(10, 0), 0);
});

test("character error rate is shown as a percentage", () => {
  assert.equal(formatCer(0), "0.0%");
  assert.equal(formatCer(0.0421), "4.2%");
  assert.equal(formatCer(2), "100.0%");
  assert.equal(formatCer(Number.NaN), "—");
});
