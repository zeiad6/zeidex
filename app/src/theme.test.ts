import assert from "node:assert/strict";
import test from "node:test";
import { parseTheme, resolveTheme } from "./theme";

test("a stored value that is not a choice means dark, never a surprise flip", () => {
  assert.equal(parseTheme("dark"), "dark");
  assert.equal(parseTheme("light"), "light");
  assert.equal(parseTheme("system"), "system");
  assert.equal(parseTheme(null), "dark");
  assert.equal(parseTheme(""), "dark");
  assert.equal(parseTheme("LIGHT"), "dark");
  assert.equal(parseTheme("sepia"), "dark");
});

test("system follows the OS in both directions", () => {
  assert.equal(resolveTheme("system", true), "light");
  assert.equal(resolveTheme("system", false), "dark");
});

test("an explicit choice ignores the OS", () => {
  assert.equal(resolveTheme("dark", true), "dark");
  assert.equal(resolveTheme("light", false), "light");
});
