import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";

/**
 * Guards the two layout invariants that a screenshot caught at the 520×360
 * minimum window, and that neither jsdom nor a type check can see:
 *
 * 1. `.panel` must clip with `overflow: clip`, not `overflow: hidden`. A hidden
 *    box is still a scroll container: focusing the Data intelligence overlay
 *    (or clicking the button that opens it) scrolled the studio panel 38 px
 *    sideways and pushed the overlay off-screen. `clip` clips without creating
 *    that scroll box.
 * 2. The launcher panel must be height-constrained (`max-h-full`) so the
 *    results list scrolls instead of the document growing to 605 px inside a
 *    360 px window.
 *
 * Both are one-line regressions, so they get one-line assertions.
 */

// Compiled tests run from `<repo>/.test-build`, one level under the repo root.
const APP_SRC = path.join(__dirname, "..", "app", "src");

function read(file: string): string {
  return readFileSync(path.join(APP_SRC, file), "utf8");
}

/** The body of the `.panel { ... }` rule (not `html[...] .panel {`). */
function panelRule(css: string): string {
  const match = /^\.panel \{/m.exec(css);
  assert.ok(match, "styles.css must define a `.panel` rule at the start of a line");
  const start = match.index;
  const end = css.indexOf("}", start);
  assert.notEqual(end, -1, ".panel rule must be closed");
  return css.slice(start, end);
}

test("the panel clips instead of becoming a scroll container", () => {
  const rule = panelRule(read("styles.css"));
  assert.match(
    rule,
    /overflow:\s*clip;/,
    "`.panel` must use `overflow: clip`; `hidden` lets focus scroll the panel sideways",
  );
  assert.doesNotMatch(rule, /overflow:\s*hidden;/, "`overflow: hidden` regressed the 38 px scroll");
});

test("the launcher panel is bounded by the window height", () => {
  const launcher = read("components/Launcher.tsx");
  const panel = launcher.match(/className="panel [^"]*"/g) ?? [];
  assert.ok(panel.length > 0, "Launcher must render a .panel element");
  assert.ok(
    panel.some((value) => value.includes("max-h-full")),
    "the launcher panel needs `max-h-full` so the list scrolls, not the document",
  );
});
