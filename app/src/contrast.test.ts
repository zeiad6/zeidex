import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

/**
 * WCAG contrast is a property of the design tokens, not of a screenshot, so it
 * can be tested. The stylesheet re-points `--color-*` for the light theme, and
 * every utility in the app compiles to `var(--color-…)`, so a token table is a
 * faithful description of the pairs the interface paints.
 *
 * The list below is not a sample: `every text colour the components use must
 * appear here` scans the sources, so adding `text-zinc-800` to a component
 * fails this suite until its contrast is declared and checked.
 */

const css = readFileSync("src/styles.css", "utf8");

/** Tailwind's stock ramp, which the dark theme inherits unchanged. */
const TAILWIND_DEFAULTS: Record<string, string> = {
  "zinc-100": "#f4f4f5",
  "zinc-200": "#e4e4e7",
  "zinc-300": "#d4d4d8",
  "zinc-400": "#a1a1aa",
  "zinc-500": "#71717a",
  "zinc-600": "#52525b",
  "zinc-700": "#3f3f46",
  "zinc-800": "#27272a",
  "zinc-900": "#18181b",
  "zinc-950": "#09090b",
  "amber-200": "#fde68a",
  "amber-300": "#fcd34d",
  "red-200": "#fecaca",
  "red-300": "#fca5a5",
  "rose-200": "#fecdd3",
  "rose-300": "#fda4af",
  "emerald-200": "#a7f3d0",
  "emerald-300": "#6ee7b7",
  "sky-200": "#bae6fd",
  "sky-300": "#7dd3fc",
  "violet-300": "#c4b5fd",
  "cyan-300": "#67e8f9",
  "pink-300": "#f9a8d4",
  "orange-300": "#fdba74",
  "indigo-300": "#a5b4fc",
  "fuchsia-300": "#f0abfc",
};

function tokensIn(selector: string): Record<string, string> {
  const start = css.indexOf(selector);
  assert.notEqual(start, -1, `the stylesheet must define ${selector}`);
  const open = css.indexOf("{", start);
  const close = css.indexOf("}", open);
  const body = css.slice(open + 1, close);
  const tokens: Record<string, string> = {};
  for (const match of body.matchAll(/--color-([a-z0-9-]+):\s*(#[0-9a-fA-F]{6})\s*;/g)) {
    tokens[match[1]] = match[2].toLowerCase();
  }
  return tokens;
}

const dark: Record<string, string> = {
  ...TAILWIND_DEFAULTS,
  white: "#ffffff",
  ...tokensIn("@theme {"),
};
const light: Record<string, string> = {
  ...dark,
  ...tokensIn('html[data-theme="light"] {'),
};

function channel(value: number): number {
  const c = value / 255;
  return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

function luminance(hex: string): number {
  const value = hex.replace("#", "");
  const r = Number.parseInt(value.slice(0, 2), 16);
  const g = Number.parseInt(value.slice(2, 4), 16);
  const b = Number.parseInt(value.slice(4, 6), 16);
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

function contrast(foreground: string, background: string): number {
  const a = luminance(foreground);
  const b = luminance(background);
  const [hi, lo] = a > b ? [a, b] : [b, a];
  return (hi + 0.05) / (lo + 0.05);
}

/** Every token the components paint as text, and where it is used. */
const TEXT_TOKENS: Array<[string, string]> = [
  ["white", "the strongest label"],
  ["zinc-100", "result title"],
  ["zinc-200", "result title"],
  ["zinc-300", "body text"],
  ["zinc-400", "secondary text"],
  ["zinc-500", "muted label, footer, hint"],
  ["zinc-600", "placeholder and empty-state copy"],
  ["accent-100", "accent label"],
  ["accent-200", "accent label"],
  ["accent-300", "accent label"],
  ["accent-400", "accent icon and label"],
  ["iris-400", "secondary accent"],
  ["amber-200", "warning text"],
  ["amber-300", "warning text"],
  ["red-200", "error text"],
  ["red-300", "error text"],
  ["rose-200", "destructive text"],
  ["rose-300", "destructive text"],
  ["emerald-200", "success text"],
  ["emerald-300", "success text"],
  ["sky-200", "info text"],
  ["sky-300", "info text"],
  ["cyan-300", "info text"],
  ["violet-300", "category text"],
  ["pink-300", "category text"],
  ["orange-300", "category text"],
  ["indigo-300", "category text"],
  ["fuchsia-300", "category text"],
];

/** Pairs that are not text-on-panel: a label inside a solid accent fill. */
const FILL_PAIRS: Array<[foreground: string, background: string, why: string]> = [
  ["ink-950", "accent-400", "label inside the primary button"],
  ["ink-950", "accent-300", "label inside the primary button on hover"],
];

function check(theme: Record<string, string>, name: string): void {
  const panel = theme["ink-850"];
  assert.ok(panel, `${name}: ink-850 must be defined`);
  for (const [token, why] of TEXT_TOKENS) {
    const foreground = theme[token];
    assert.ok(foreground, `${name}: ${token} must be defined (${why})`);
    const minimum = token === "white" ? 7 : 4.5;
    const ratio = contrast(foreground, panel);
    assert.ok(
      ratio >= minimum,
      `${name}: ${token} (${foreground}) on the panel (${panel}) is ` +
        `${ratio.toFixed(2)}:1, needs ${minimum}:1 for ${why}`,
    );
  }
  for (const [foreground, background, why] of FILL_PAIRS) {
    const fg = theme[foreground];
    const bg = theme[background];
    assert.ok(fg && bg, `${name}: ${foreground} and ${background} must be defined`);
    const ratio = contrast(fg, bg);
    assert.ok(
      ratio >= 4.5,
      `${name}: ${foreground} (${fg}) on ${background} (${bg}) is ` +
        `${ratio.toFixed(2)}:1, needs 4.5:1 for ${why}`,
    );
  }
}

test("dark theme text meets WCAG AA on the panel", () => {
  check(dark, "dark");
});

test("light theme text meets WCAG AA on paper", () => {
  check(light, "light");
});

test("the muted ramp stays ordered, so lifting it did not flatten hierarchy", () => {
  const themes: Array<[string, Record<string, string>]> = [
    ["dark", dark],
    ["light", light],
  ];
  for (const [name, theme] of themes) {
    const steps = ["zinc-400", "zinc-500", "zinc-600", "zinc-700"].map((key) => ({
      key,
      value: luminance(theme[key]),
    }));
    for (let i = 1; i < steps.length; i += 1) {
      const previous = steps[i - 1];
      const current = steps[i];
      assert.notEqual(
        previous.value,
        current.value,
        `${name}: ${previous.key} and ${current.key} must not collapse to one colour`,
      );
      // In the dark theme each step is dimmer; the light theme inverts the ramp.
      const brighter = current.value > previous.value;
      assert.equal(
        brighter,
        name === "light",
        `${name}: ${current.key} must be ${name === "light" ? "lighter" : "dimmer"} than ${previous.key}`,
      );
    }
  }
});

function sourceFiles(dir: string): string[] {
  const found: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) found.push(...sourceFiles(path));
    else if (entry.name.endsWith(".tsx")) found.push(path);
  }
  return found;
}

test("every text colour the components use is declared in the contrast table", () => {
  // A fill foreground (`text-ink-950` on a cyan button) is not text on the
  // panel, but it must still be declared and checked.
  const declared = new Set([
    ...TEXT_TOKENS.map(([token]) => token),
    ...FILL_PAIRS.map(([foreground]) => foreground),
  ]);
  const used = new Set<string>();
  for (const file of sourceFiles("src")) {
    const source = readFileSync(file, "utf8");
    // `text-zinc-500`, `placeholder:text-zinc-600`, `hover:text-accent-300` …
    for (const match of source.matchAll(/\btext-([a-z]+)-(\d{2,3})\b/g)) {
      used.add(`${match[1]}-${match[2]}`);
    }
  }
  assert.ok(used.size > 0, "the scan must find text colours; an empty scan proves nothing");
  const undeclared = [...used].filter((token) => !declared.has(token)).sort();
  assert.deepEqual(
    undeclared,
    [],
    "these text colours are painted but never contrast-checked; add them to TEXT_TOKENS",
  );
});

test("a token that is not defined is a test failure, not a silent pass", () => {
  assert.throws(() => tokensIn('html[data-theme="sepia"]'), /must define/);
});
