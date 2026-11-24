import assert from "node:assert/strict";
import test from "node:test";
import { menuPosition } from "./menu-position";

test("a menu opened inside the window keeps the point it was opened at", () => {
  assert.deepEqual(menuPosition(120, 90, 208, 120, 1024, 768), { left: 120, top: 90 });
});

test("a menu opened at the right edge is pulled back inside", () => {
  assert.deepEqual(menuPosition(1000, 40, 208, 120, 1024, 768), { left: 808, top: 40 });
});

test("a menu opened at the bottom edge is pulled up inside", () => {
  assert.deepEqual(menuPosition(40, 760, 208, 120, 1024, 768), { left: 40, top: 640 });
});

test("a point outside the window still lands on the margin", () => {
  assert.deepEqual(menuPosition(-30, -12, 208, 120, 1024, 768), { left: 8, top: 8 });
});

test("a window smaller than the menu does not produce a negative offset", () => {
  const { left, top } = menuPosition(10, 10, 300, 300, 200, 200);
  assert.equal(left, 8);
  assert.equal(top, 8);
});
