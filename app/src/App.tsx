import { useCallback, useEffect, useRef, useState } from "react";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "./api";
import type { IndexStatus } from "./index-status";
import BatchSearch from "./components/BatchSearch";
import Launcher from "./components/Launcher";
import Models from "./components/Models";
import OcrStudio from "./components/OcrStudio";
import Settings from "./components/Settings";
import ErrorBoundary from "./ErrorBoundary";
import { clampToScreen, logicalScreen } from "./window-size";
import {
  applyTheme,
  resolveTheme,
  saveTheme,
  storedTheme,
  systemPrefersLight,
  type ThemeChoice,
} from "./theme";

type Mode = "launcher" | "ocr" | "settings" | "batch" | "models";

const DIRECTION_KEY = "zeidex.direction";

/**
 * The screen's own name, used for the document's single level-one heading.
 *
 * Every screen draws its title as styled text, which a screen reader cannot
 * navigate by; a visually hidden heading gives the document the outline that
 * landmark and heading navigation expect without changing the design. The
 * visible titles stay as they are, so the hierarchy a sighted user sees is
 * unchanged.
 */
const SCREEN_TITLE: Record<Mode, { en: string; ar: string }> = {
  launcher: { en: "Zeidex Search", ar: "Zeidex Search" },
  ocr: { en: "Document Studio", ar: "استوديو المستندات" },
  batch: { en: "Batch Search", ar: "البحث الدفعي" },
  models: { en: "OCR Models", ar: "نماذج OCR" },
  settings: { en: "Settings", ar: "الإعدادات" },
};

/**
 * The launcher is a small floating panel; the Studio needs two panes; Settings
 * needs room for a nav column. Resizing the same window keeps one surface (and
 * one hotkey) instead of juggling a second window.
 */
const WINDOW_SIZE: Record<Mode, { width: number; height: number }> = {
  launcher: { width: 760, height: 520 },
  ocr: { width: 1280, height: 880 },
  settings: { width: 1140, height: 820 },
  batch: { width: 1280, height: 880 },
  models: { width: 1280, height: 880 },
};

/**
 * The interface direction is a user preference, so it outlives the window. It
 * lives in the webview's local storage: on this device, no account, no network.
 * A storage failure (private mode, a wiped profile) falls back to LTR for this
 * run instead of breaking startup.
 */
function storedDirection(): boolean {
  try {
    return window.localStorage.getItem(DIRECTION_KEY) === "rtl";
  } catch {
    return false;
  }
}

export default function App() {
  const [mode, setMode] = useState<Mode>("launcher");
  const [rtl, setRtl] = useState(storedDirection);
  const [theme, setTheme] = useState<ThemeChoice>(storedTheme);
  const [osPrefersLight, setOsPrefersLight] = useState(systemPrefersLight);
  const [indexStatus, setIndexStatus] = useState<IndexStatus | null>(null);

  // `system` must keep following the OS while the window is open, so the media
  // query is a subscription rather than a value read once at boot.
  useEffect(() => {
    let query: MediaQueryList;
    try {
      query = window.matchMedia("(prefers-color-scheme: light)");
    } catch {
      return;
    }
    const onChange = () => setOsPrefersLight(query.matches);
    setOsPrefersLight(query.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);

  useEffect(() => {
    applyTheme(resolveTheme(theme, osPrefersLight));
  }, [theme, osPrefersLight]);

  const setThemeChoice = useCallback((next: ThemeChoice) => {
    setTheme(next);
    saveTheme(next);
  }, []);

  const setDirection = useCallback((next: boolean) => {
    setRtl(next);
    try {
      window.localStorage.setItem(DIRECTION_KEY, next ? "rtl" : "ltr");
    } catch {
      // The choice still applies to this session; it just will not be remembered.
    }
  }, []);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    api
      .onMode((next) => {
        if (active) setMode(next === "ocr" ? "ocr" : "launcher");
      })
      .then((fn) => {
        if (active) unlisten = fn;
        else fn();
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  // The index worker is the only source of the count. Both screens adopt what
  // it reports, so a watcher refresh or a settings-triggered rebuild updates
  // the footer without a second request and without a guessed number.
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    api
      .onIndexStatus((status) => {
        if (active) setIndexStatus(status);
      })
      .then((fn) => {
        if (active) unlisten = fn;
        else fn();
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  // Keyboard focus follows the screen. Clicking "Settings" removes the button
  // that had focus, so without this the next Tab starts at the top of the
  // document and a keyboard user has to walk the whole chrome again to reach
  // the screen they just opened. The launcher is excluded: its query box takes
  // focus on mount, and taking it back would make the launcher untypeable after
  // a round trip.
  const mainRef = useRef<HTMLElement | null>(null);
  useEffect(() => {
    if (mode === "launcher") return;
    mainRef.current?.focus();
  }, [mode]);

  useEffect(() => {
    void (async () => {
      try {
        const win = getCurrentWindow();
        // Setting a client size on Windows leaves a maximized window on screen
        // but clears the maximized state tao tracks, so the caption button
        // would offer "maximize" on a window the user can no longer restore.
        // A mode switch has no size to apply while the window is maximized.
        if (await win.isMaximized()) return;
        // Settings is larger than a 1366x768 laptop can show, so the request is
        // fitted to the monitor before it is applied. The panel scrolls, so a
        // shorter window keeps every control reachable.
        const screen = logicalScreen(await currentMonitor());
        const { width, height } = clampToScreen(WINDOW_SIZE[mode], screen);
        await win.setSize(new LogicalSize(width, height));
        await win.center();
      } catch {
        // Browser demo and headless runs have no native window.
      }
    })();
  }, [mode]);

  return (
    <div dir={rtl ? "rtl" : "ltr"} lang={rtl ? "ar" : "en"} className="h-full">
      <main
        ref={mainRef}
        tabIndex={-1}
        aria-labelledby="screen-title"
        className="h-full outline-none"
      >
        <h1 id="screen-title" className="sr-only">
          {rtl ? SCREEN_TITLE[mode].ar : SCREEN_TITLE[mode].en}
        </h1>
        <ErrorBoundary scope={mode} rtl={rtl} onReset={() => setMode("launcher")}>
          {mode === "ocr" ? (
            <OcrStudio rtl={rtl} onBack={() => setMode("launcher")} />
          ) : mode === "batch" ? (
            <BatchSearch rtl={rtl} onBack={() => setMode("launcher")} />
          ) : mode === "models" ? (
            <Models rtl={rtl} onBack={() => setMode("launcher")} />
          ) : mode === "settings" ? (
            <Settings
              rtl={rtl}
              indexStatus={indexStatus}
              onSetRtl={setDirection}
              theme={theme}
              onSetTheme={setThemeChoice}
              onBack={() => setMode("launcher")}
            />
          ) : (
            <Launcher
              rtl={rtl}
              indexStatus={indexStatus}
              onToggleRtl={() => setDirection(!rtl)}
              onOpenOcr={() => setMode("ocr")}
              onOpenBatch={() => setMode("batch")}
              onOpenModels={() => setMode("models")}
              onOpenSettings={() => setMode("settings")}
            />
          )}
        </ErrorBoundary>
      </main>
    </div>
  );
}
