import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Icon } from "./Icons";

/**
 * Minimize, maximize/restore and close for the frameless window.
 *
 * The window is created with `decorations: false`, so the operating system
 * draws no caption and no buttons; without this component the only way to put
 * Zeidex away is the global shortcut or the tray menu, and there is no way to
 * maximize it at all. The three buttons are the ordinary Windows caption
 * actions, reachable by Tab and announced by screen readers.
 *
 * Close is intercepted in Rust: with a tray icon present it hides the window
 * instead of ending the process, and the tray's Quit item is the explicit exit.
 */
export default function WindowControls({ rtl }: { rtl: boolean }) {
  const [maximized, setMaximized] = useState(false);
  const t = (en: string, ar: string) => (rtl ? ar : en);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    const win = getCurrentWindow();
    const sync = () => {
      void win
        .isMaximized()
        .then((value) => {
          if (active) setMaximized(value);
        })
        .catch(() => {
          // A browser demo has no native window; the icon simply stays as-is.
        });
    };
    sync();
    win
      .onResized(sync)
      .then((fn) => {
        if (active) unlisten = fn;
        else fn();
      })
      .catch(() => {
        // Same as above: nothing to subscribe to outside the desktop shell.
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const win = () => getCurrentWindow();

  return (
    <div className="ms-1 flex items-center" role="group" aria-label={t("Window", "النافذة")}>
      <button
        type="button"
        onClick={() => void win().minimize().catch(() => {})}
        aria-label={t("Minimize", "تصغير")}
        title={t("Minimize", "تصغير")}
        className="window-control"
      >
        <Icon name="minimize" size={14} />
      </button>
      <button
        type="button"
        onClick={() => void win().toggleMaximize().catch(() => {})}
        aria-label={maximized ? t("Restore", "استعادة") : t("Maximize", "تكبير")}
        title={maximized ? t("Restore", "استعادة") : t("Maximize", "تكبير")}
        className="window-control"
      >
        <Icon name={maximized ? "restore" : "maximize"} size={12} />
      </button>
      <button
        type="button"
        onClick={() => void win().close().catch(() => {})}
        aria-label={t("Close", "إغلاق")}
        title={t("Close", "إغلاق")}
        className="window-control window-control-close"
      >
        <Icon name="x" size={14} />
      </button>
    </div>
  );
}
