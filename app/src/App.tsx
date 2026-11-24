import { useCallback, useEffect, useRef, useState } from "react";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";
import { api, type UpdateInfo } from "./api";
import type { IndexStatus } from "./index-status";
import BatchSearch from "./components/BatchSearch";
import Launcher from "./components/Launcher";
import Models from "./components/Models";
import OcrStudio from "./components/OcrStudio";
import Settings from "./components/Settings";
import Welcome from "./components/Welcome";
import ErrorBoundary from "./ErrorBoundary";
import { Icon } from "./components/Icons";
import { clampToScreen, logicalScreen } from "./window-size";
import { useTheme } from "./theme-context";

type Mode = "launcher" | "ocr" | "settings" | "batch" | "models";

const MODES: Mode[] = ["launcher", "ocr", "settings", "batch", "models"];

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
  const { choice: theme, setChoice: setThemeChoice } = useTheme();
  const [indexStatus, setIndexStatus] = useState<IndexStatus | null>(null);
  // A newer release, announced once. Dismissing hides the toast for this
  // session; the About screen still shows the answer from the last check.
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  // The first-run guide, and the accelerator it should name. The default is
  // only a placeholder until `hotkey_status` answers.
  const [welcome, setWelcome] = useState(false);
  const [hotkey, setHotkey] = useState("Alt+Space");
  const [version, setVersion] = useState("0.5.0");

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
        if (!active) return;
        // The tray can ask for any screen by name. An unknown value must not
        // silently open the launcher on top of what the user was doing.
        if ((MODES as string[]).includes(next)) setMode(next as Mode);
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

  // Read the guide's state once. The shell already opened the window when the
  // guide is unseen, so this only decides whether to draw it.
  useEffect(() => {
    let active = true;
    api
      .onboardingStatus()
      .then((seen) => {
        if (active && !seen) setWelcome(true);
      })
      .catch(() => {
        // An unreadable config must not block the launcher.
      });
    api
      .hotkeyStatus()
      .then((status) => {
        if (active && status.accelerator) setHotkey(status.accelerator);
      })
      .catch(() => {});
    api
      .settingsSnapshot()
      .then((snapshot) => {
        if (active && snapshot.version) setVersion(snapshot.version);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, []);

  const dismissWelcome = useCallback(() => {
    setWelcome(false);
    // Best effort: if the write fails the guide returns next launch, which is
    // better than pretending it was remembered.
    void api.onboardingComplete().catch(() => {});
  }, []);

  // The background check only emits when a newer release exists, so any event
  // is worth showing. It carries no user data: tag, date, notes, release URL.
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    api
      .onUpdate((info) => {
        if (active && info.available) setUpdate(info);
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
              onShowWelcome={() => setWelcome(true)}
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

        {welcome && (
          <Welcome
            rtl={rtl}
            hotkey={hotkey}
            version={version}
            onDismiss={dismissWelcome}
            onOpenSettings={() => {
              dismissWelcome();
              setMode("settings");
            }}
          />
        )}

        {update?.available && (
          <div
            role="status"
            aria-live="polite"
            className="animate-rise fixed bottom-4 end-4 z-50 flex max-w-[22rem] items-start gap-3 rounded-xl border border-accent-400/25 bg-float/95 px-3.5 py-3 shadow-[var(--shadow-pop)] backdrop-blur"
          >
            <span className="mt-0.5 grid h-7 w-7 shrink-0 place-items-center rounded-full bg-accent-400/15 text-accent-300">
              <Icon name="update" size={14} />
            </span>
            <div className="min-w-0 flex-1">
              <p className="text-[0.78rem] font-medium text-zinc-100">
                {rtl ? "يتوفر تحديث" : "Update available"}
              </p>
              <p className="mt-0.5 text-[0.7rem] leading-relaxed text-zinc-500">
                {rtl
                  ? `الإصدار ${update.latest} جاهز للتنزيل.`
                  : `Zeidex ${update.latest} is ready to download.`}
              </p>
              <div className="mt-2 flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => {
                    if (update.url) void api.openUrl(update.url);
                  }}
                  className="rounded-full border border-accent-400/30 bg-accent-400/10 px-2.5 py-1 text-[0.68rem] font-medium text-accent-200 transition hover:bg-accent-400/20"
                >
                  {rtl ? "افتح صفحة الإصدار" : "Open release page"}
                </button>
                <button
                  type="button"
                  onClick={() => setUpdate(null)}
                  className="rounded-full px-2 py-1 text-[0.68rem] text-zinc-500 transition hover:bg-white/10 hover:text-zinc-300"
                >
                  {rtl ? "لاحقاً" : "Later"}
                </button>
              </div>
            </div>
          </div>
        )}
      </main>
    </div>
  );
}
