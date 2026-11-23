import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";

/**
 * Guards the Windows window contract.
 *
 * The transparent/ghost-window defect was a configuration bug, not a CSS bug:
 * `transparent: true` plus `shadow: true` on an undecorated window makes tao
 * paint through the DWM blur-behind path, and the panel's translucent surface
 * then shows the desktop (or the previous frame) through the window. These
 * assertions make the fix explicit so a later "make it pretty" edit cannot
 * quietly turn transparency back on.
 */

// Compiled tests run from `<repo>/.test-build`, one level under the repo root.
const SRC_TAURI = path.join(__dirname, "..", "src-tauri");

type WindowConfig = Record<string, unknown> & {
  label?: string;
  transparent?: boolean;
  decorations?: boolean;
  shadow?: boolean;
  backgroundColor?: string;
  visible?: boolean;
};

type TauriConfig = {
  app?: { windows?: WindowConfig[] };
  bundle?: {
    licenseFile?: string;
    icon?: string[];
    windows?: {
      nsis?: {
        installerIcon?: string;
        headerImage?: string;
        sidebarImage?: string;
        uninstallerHeaderImage?: string;
        installMode?: string;
        languages?: string[];
      };
    };
  };
};

function load(name: string): TauriConfig {
  return JSON.parse(readFileSync(path.join(SRC_TAURI, name), "utf8")) as TauriConfig;
}

function mainWindow(config: TauriConfig): WindowConfig {
  const windows = config.app?.windows ?? [];
  const main = windows.find((entry) => entry.label === "main");
  assert.ok(main, "the config must define the `main` window");
  return main;
}

/** Reads the BMP header NSIS will read, so a PNG renamed to .bmp fails here. */
function bmpSize(file: string): { width: number; height: number; depth: number } {
  const bytes = readFileSync(path.join(SRC_TAURI, file));
  assert.equal(bytes.toString("ascii", 0, 2), "BM", `${file} is not a BMP`);
  return {
    width: bytes.readInt32LE(18),
    height: bytes.readInt32LE(22),
    depth: bytes.readUInt16LE(28),
  };
}

test("Windows renders an opaque, rectangular window", () => {
  const main = mainWindow(load("tauri.windows.conf.json"));
  assert.equal(main.transparent, false, "transparency is what produced the ghost window");
  assert.equal(main.shadow, false, "a shadow on an undecorated window insets and outlines it");
  assert.equal(main.backgroundColor, "#070910", "the webview needs an opaque base colour");
  // The window is still undecorated: Zeidex draws its own caption buttons.
  assert.equal(main.decorations, false);
  assert.equal(main.visible, false, "the window is shown only after the first paint");
});

test("the Windows override repeats every field of the base window", () => {
  // Tauri merges platform configs with RFC 7396: an array in the platform file
  // replaces the base array instead of merging object-by-object. A partial
  // window entry would silently drop size, min size and the URL.
  const base = mainWindow(load("tauri.conf.json"));
  const windows = mainWindow(load("tauri.windows.conf.json"));
  for (const key of Object.keys(base)) {
    assert.ok(key in windows, `tauri.windows.conf.json drops the base field \`${key}\``);
  }
  for (const key of ["label", "width", "height", "minWidth", "minHeight", "url", "decorations"]) {
    assert.deepEqual(windows[key], base[key], `${key} drifted from the base window`);
  }
});

test("the base config keeps the floating-panel shell for macOS and Linux", () => {
  const main = mainWindow(load("tauri.conf.json"));
  assert.equal(main.transparent, true);
  assert.equal(main.decorations, false);
});

test("the installer artwork exists at the path the bundle config names", () => {
  const nsis = load("tauri.conf.json").bundle?.windows?.nsis;
  assert.ok(nsis, "bundle.windows.nsis is missing");
  const header = bmpSize(nsis.headerImage ?? "");
  assert.deepEqual(header, { width: 150, height: 57, depth: 24 });
  const sidebar = bmpSize(nsis.sidebarImage ?? "");
  assert.deepEqual(sidebar, { width: 164, height: 314, depth: 24 });
  assert.equal(nsis.uninstallerHeaderImage, nsis.headerImage);
  assert.equal(nsis.installMode, "currentUser");
  assert.ok(nsis.languages?.includes("English"));
  assert.ok(nsis.languages?.includes("Arabic"));
});

test("the bundle metadata points at files that ship", () => {
  const bundle = load("tauri.conf.json").bundle;
  assert.ok(bundle?.licenseFile);
  assert.ok(readFileSync(path.join(SRC_TAURI, bundle.licenseFile)).length > 0);
  assert.ok(bundle?.icon?.length);
  for (const icon of bundle?.icon ?? []) {
    assert.ok(readFileSync(path.join(SRC_TAURI, icon)).length > 0, `${icon} is missing`);
  }
  const installerIcon = bundle?.windows?.nsis?.installerIcon;
  assert.ok(installerIcon);
  assert.ok(readFileSync(path.join(SRC_TAURI, installerIcon)).length > 0);
});
