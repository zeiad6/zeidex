import assert from "node:assert/strict";
import { test } from "node:test";
import {
  MIN_WINDOW,
  SCREEN_MARGIN,
  clampToScreen,
  logicalScreen,
  type WindowSize,
} from "./window-size";

const SETTINGS: WindowSize = { width: 1140, height: 820 };

test("keeps the requested size when the platform reports no monitor", () => {
  assert.deepEqual(clampToScreen(SETTINGS, null), SETTINGS);
});

test("keeps the requested size on a monitor with room", () => {
  assert.deepEqual(clampToScreen(SETTINGS, { width: 1920, height: 1080 }), SETTINGS);
});

test("fits Settings on a 1366x768 laptop", () => {
  const fitted = clampToScreen(SETTINGS, { width: 1366, height: 768 });
  assert.deepEqual(fitted, {
    width: 1140,
    height: 768 - SCREEN_MARGIN.height,
  });
  assert.ok(fitted.height < SETTINGS.height);
});

test("fits Settings on a 1280x800 desktop with a dock", () => {
  assert.deepEqual(clampToScreen(SETTINGS, { width: 1280, height: 800 }), {
    width: 1140,
    height: 800 - SCREEN_MARGIN.height,
  });
});

test("never shrinks past the minimum while the screen can still hold it", () => {
  const fitted = clampToScreen(SETTINGS, { width: 900, height: 700 });
  assert.deepEqual(fitted, {
    width: 900 - SCREEN_MARGIN.width,
    height: 700 - SCREEN_MARGIN.height,
  });
  assert.ok(fitted.height >= Math.min(MIN_WINDOW.height, 700 - SCREEN_MARGIN.height));
});

test("leaves the launcher untouched when there is room", () => {
  const launcher = { width: 760, height: 520 };
  assert.deepEqual(clampToScreen(launcher, { width: 1280, height: 800 }), launcher);
});

test("fits the launcher on a short screen too", () => {
  assert.deepEqual(clampToScreen({ width: 760, height: 520 }, { width: 1024, height: 600 }), {
    width: 760,
    height: 600 - SCREEN_MARGIN.height,
  });
});

test("still fits when the screen is smaller than the minimum", () => {
  assert.deepEqual(clampToScreen(SETTINGS, { width: 640, height: 400 }), {
    width: 640 - SCREEN_MARGIN.width,
    height: 400 - SCREEN_MARGIN.height,
  });
});

test("ignores a bogus zero-sized monitor", () => {
  assert.deepEqual(clampToScreen(SETTINGS, { width: 0, height: 0 }), SETTINGS);
});

test("converts physical monitor pixels to logical pixels", () => {
  assert.deepEqual(
    logicalScreen({ size: { width: 2560, height: 1600 }, scaleFactor: 2 }),
    { width: 1280, height: 800 },
  );
});

test("rejects a missing or non-positive scale factor", () => {
  assert.equal(logicalScreen(null), null);
  assert.equal(
    logicalScreen({ size: { width: 1920, height: 1080 }, scaleFactor: 0 }),
    null,
  );
  assert.equal(
    logicalScreen({ size: { width: 0, height: 1080 }, scaleFactor: 1 }),
    null,
  );
});

test("a scaled 4K panel still leaves the requested window size", () => {
  const screen = logicalScreen({
    size: { width: 3840, height: 2160 },
    scaleFactor: 2,
  });
  assert.deepEqual(clampToScreen(SETTINGS, screen), SETTINGS);
});
