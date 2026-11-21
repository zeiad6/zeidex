import { useEffect, useState } from "react";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "./api";
import Launcher from "./components/Launcher";
import OcrStudio from "./components/OcrStudio";

type Mode = "launcher" | "ocr";

/**
 * The launcher is a small floating panel; the Studio needs two panes. Resizing
 * the same window keeps one surface (and one hotkey) instead of juggling a
 * second window, and the size change is animated by the OS window manager.
 */
const WINDOW_SIZE: Record<Mode, { width: number; height: number }> = {
  launcher: { width: 760, height: 520 },
  ocr: { width: 1280, height: 880 },
};

export default function App() {
  const [mode, setMode] = useState<Mode>("launcher");
  const [rtl, setRtl] = useState(false);

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

  useEffect(() => {
    const { width, height } = WINDOW_SIZE[mode];
    void (async () => {
      try {
        const win = getCurrentWindow();
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
      ) : (
        <Launcher
          rtl={rtl}
          onToggleRtl={() => setRtl((value) => !value)}
          onOpenOcr={() => setMode("ocr")}
        />
      )}
    </div>
  );
}
