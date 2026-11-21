import { useCallback, useEffect, useState } from "react";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "./api";
import type { IndexStatus } from "./index-status";
import Launcher from "./components/Launcher";
import OcrStudio from "./components/OcrStudio";
import Settings from "./components/Settings";
import { clampToScreen, logicalScreen } from "./window-size";

type Mode = "launcher" | "ocr" | "settings";

const DIRECTION_KEY = "zeidex.direction";

/**
 * The launcher is a small floating panel; the Studio needs two panes; Settings
 * needs room for a nav column. Resizing the same window keeps one surface (and
 * one hotkey) instead of juggling a second window.
 */
const WINDOW_SIZE: Record<Mode, { width: number; height: number }> = {
  launcher: { width: 760, height: 520 },
  ocr: { width: 1280, height: 880 },
  settings: { width: 1140, height: 820 },
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
  const [indexStatus, setIndexStatus] = useState<IndexStatus | null>(null);

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

  useEffect(() => {
    void (async () => {
      try {
        const win = getCurrentWindow();
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
      {mode === "ocr" ? (
        <OcrStudio rtl={rtl} onBack={() => setMode("launcher")} />
      ) : mode === "settings" ? (
        <Settings
          rtl={rtl}
          indexStatus={indexStatus}
          onSetRtl={setDirection}
          onBack={() => setMode("launcher")}
        />
      ) : (
        <Launcher
          rtl={rtl}
          indexStatus={indexStatus}
          onToggleRtl={() => setDirection(!rtl)}
          onOpenOcr={() => setMode("ocr")}
          onOpenSettings={() => setMode("settings")}
        />
      )}
    </div>
  );
}
