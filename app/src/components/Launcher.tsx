import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, type Calculation, type ClipEntry, type LaunchResult, type Tab } from "../api";
import type { IndexStatus } from "../index-status";
import { count as countLabel } from "../i18n";
import { Wordmark } from "./Brand";
import ClipboardPanel from "./ClipboardPanel";
import { Icon } from "./Icons";
import ResultList from "./ResultList";
import WindowControls from "./WindowControls";
import TranslationPanel from "./TranslationPanel";

const TABS: { id: Tab; label: string; rtlLabel: string }[] = [
  { id: "all", label: "All", rtlLabel: "الكل" },
  { id: "apps", label: "Apps", rtlLabel: "التطبيقات" },
  { id: "files", label: "Files", rtlLabel: "الملفات" },
  { id: "clipboard", label: "Clipboard", rtlLabel: "الحافظة" },
];

const ACTION_LABEL: Record<string, string> = {
  open: "Open",
  open_folder: "Open folder",
  reveal: "Reveal",
  web_search: "Search web",
  translate: "Translate",
  perform_block: "Run",
};

const ACTION_LABEL_AR: Record<string, string> = {
  open: "فتح",
  open_folder: "فتح مجلد",
  reveal: "كشف",
  web_search: "بحث",
  translate: "ترجمة",
  perform_block: "تشغيل",
};

const HINTS: { icon: "search" | "sparkle" | "translate"; title: string; titleAr: string; body: string; bodyAr: string }[] = [
  {
    icon: "search",
    title: "Apps, files, folders",
    titleAr: "تطبيقات وملفات ومجلدات",
    body: "Start typing a name.",
    bodyAr: "ابدأ بكتابة الاسم.",
  },
  {
    icon: "sparkle",
    title: "Calculator",
    titleAr: "الحاسبة",
    body: "(18 * 4) + 7.5",
    bodyAr: "(18 * 4) + 7.5",
  },
  {
    icon: "translate",
    title: "Translate",
    titleAr: "الترجمة",
    body: 'tr"hello world',
    bodyAr: 'tr"hello world',
  },
];

function usageAction(result: LaunchResult): string {
  switch (result.action.type) {
    case "open":
      if (result.kind === "app") return "open_app";
      if (result.kind === "folder") return "open_folder";
      return "open_file";
    case "open_folder":
      return "open_folder";
    case "reveal":
      return "reveal";
    case "web_search":
      return "web_search";
    case "translate":
      return "translate";
    case "perform_block":
      return "execute";
  }
}

export default function Launcher({
  rtl,
  indexStatus,
  onToggleRtl,
  onOpenOcr,
  onOpenBatch,
  onOpenModels,
  onOpenSettings,
}: {
  rtl: boolean;
  indexStatus: IndexStatus | null;
  onToggleRtl: () => void;
  onOpenOcr: () => void;
  onOpenBatch: () => void;
  onOpenModels: () => void;
  onOpenSettings: () => void;
}) {
  const [query, setQuery] = useState("");
  const [tab, setTab] = useState<Tab>("all");
  const [results, setResults] = useState<LaunchResult[]>([]);
  const [selected, setSelected] = useState(0);
  const [loading, setLoading] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [clipboard, setClipboard] = useState<ClipEntry[]>([]);
  const [clipboardPaused, setClipboardPaused] = useState(false);
  const [indexing, setIndexing] = useState(false);
  const [count, setCount] = useState<number | null>(null);
  const statusSeen = useRef(false);
  const [calc, setCalc] = useState<Calculation | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  // Ticket for clipboard list reads: a response that started before a delete, a
  // pin, or a newer read must not overwrite the newer state.
  const clipboardSeq = useRef(0);

  // A hotkey or the tray's Show raises the window with whatever had focus
  // before, which is never the query box. A launcher is only useful if typing
  // goes straight into search, so every focus regain re-arms the input and
  // selects the old query: the next keystroke replaces it instead of appending.
  useEffect(() => {
    const win = getCurrentWindow();
    let active = true;
    let unlisten: (() => void) | undefined;
    win
      .onFocusChanged(({ payload: focused }) => {
        if (!focused) return;
        inputRef.current?.focus();
        inputRef.current?.select();
      })
      .then((fn) => {
        if (active) unlisten = fn;
        else fn();
      })
      .catch(() => {
        // Browser demo: no native window to subscribe to.
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  // `tr"` switches the panel from search to translation; the prefix is consumed
  // and the rest of the query is the text to translate.
  const translateText = query.startsWith('tr"') ? query.slice(3) : null;

  // A tab change returns focus to the query box and re-arms the selection:
  // clicking a filter moved focus to the button, and every shortcut in this
  // panel is typed into the input.
  useEffect(() => {
    inputRef.current?.focus();
    setSelected(0);
  }, [tab]);

  useEffect(() => {
    let cancelled = false;
    const handle = window.setTimeout(async () => {
      if (query.trim().length === 0 || query.startsWith('tr"')) {
        setResults([]);
        return;
      }
      setLoading(true);
      try {
        const found = await api.search(query, 24);
        if (!cancelled) {
          setResults(found);
          setSelected(0);
        }
      } catch (err) {
        if (!cancelled) setNotice(String(err));
      } finally {
        if (!cancelled) setLoading(false);
      }
    }, 140);
    return () => {
      cancelled = true;
      window.clearTimeout(handle);
    };
  }, [query]);

  // The inline calculator. Pure and synchronous in the core, so it runs on
  // every keystroke; `None` (not arithmetic) leaves the results untouched.
  useEffect(() => {
    if (translateText !== null) {
      setCalc(null);
      return;
    }
    let cancelled = false;
    api
      .calcEval(query)
      .then((value) => {
        if (!cancelled) setCalc(value);
      })
      .catch(() => {
        if (!cancelled) setCalc(null);
      });
    return () => {
      cancelled = true;
    };
  }, [query, translateText]);

  useEffect(() => {
    if (tab !== "clipboard") return;
    let stopped = false;
    const refresh = async () => {
      const ticket = ++clipboardSeq.current;
      const clips = await api.clipboardList("text", "", 100);
      if (!stopped && ticket === clipboardSeq.current) setClipboard(clips);
    };
    const safeRefresh = () => {
      refresh().catch(() => {
        // Clipboard history is optional; a headless session has no pasteboard.
      });
    };
    // The backend monitor records clips; polling once before the first read is
    // what makes a clip copied immediately before the tab opened show up now
    // instead of on the next tick.
    api
      .clipboardPoll()
      .catch(() => undefined)
      .then(safeRefresh);
    api
      .clipboardPaused()
      .then((value) => {
        if (!stopped) setClipboardPaused(value);
      })
      .catch(() => undefined);
    const interval = window.setInterval(safeRefresh, 1000);
    return () => {
      stopped = true;
      window.clearInterval(interval);
    };
  }, [tab]);

  useEffect(() => {
    api
      .candidateCount()
      .then((value) => {
        // A status event that landed while this request was in flight is
        // newer than the snapshot the request returned; do not overwrite it.
        if (!statusSeen.current) setCount(value);
      })
      .catch(() => undefined);
  }, []);

  // The worker emits the count after every walk and every watched change; the
  // footer shows that number rather than the one sampled at mount.
  useEffect(() => {
    if (!indexStatus) return;
    statusSeen.current = true;
    setCount(indexStatus.count);
  }, [indexStatus]);

  const visible = results.filter((result) => {
    if (tab === "apps") return result.kind === "app";
    if (tab === "files")
      return (
        result.kind === "file" || result.kind === "folder" || result.kind === "document"
      );
    return true;
  });

  const runSelected = async () => {
    const result = visible[selected];
    if (!result) return;
    try {
      // A web row is composed by the shell and has no candidate row, so there
      // is nothing for `usage_events` to point at (the foreign key would fail
      // and abort the open).
      if (result.action.type !== "web_search") {
        await api.recordUsage(result.id, usageAction(result));
      }
      switch (result.action.type) {
        case "open":
        case "open_folder":
          await api.openTarget(result.action.path);
          break;
        case "reveal":
          await api.revealTarget(result.action.path);
          break;
        case "web_search":
          await api.openUrl(await api.webSearchUrl(result.action.query));
          break;
        default:
          // The engine does not emit `translate` or `perform_block` in this
          // build, so this is unreachable today; it is a guard, not a silent
          // no-op, so a future producer cannot look like it worked.
          setNotice("This action is not available in this build.");
          return;
      }
      await getCurrentWindow().hide();
    } catch (err) {
      setNotice(String(err));
    }
  };

  const toggleClipboardPause = async () => {
    try {
      // The backend is the source of truth: the switch shows what it stored,
      // not what the click hoped for.
      setClipboardPaused(await api.clipboardSetPaused(!clipboardPaused));
    } catch {
      setNotice(rtl ? "تعذّر تغيير حالة التسجيل" : "Could not change recording");
    }
  };

  const clearClipboard = async () => {
    try {
      await api.clipboardClear();
    } catch {
      // Fall through to the re-read: if the wipe failed the rows come back and
      // the user can see it did not happen.
    }
    try {
      // A list read that started before the wipe must not land after it and
      // put the rows back; this ticket is newer than any of those.
      const ticket = ++clipboardSeq.current;
      const clips = await api.clipboardList("text", "", 100);
      if (ticket === clipboardSeq.current) setClipboard(clips);
    } catch {
      // Leave the list as it is; the next poll refreshes it.
    }
  };

  // Enter on the calculator row copies the bare, re-parseable value (`raw`),
  // not the grouped display (`1,000,000`), so pasting it elsewhere still works.
  const copyCalc = async (value: Calculation) => {
    try {
      await navigator.clipboard.writeText(value.raw);
      setNotice(rtl ? "نُسخت النتيجة" : "Result copied");
    } catch {
      setNotice(rtl ? "تعذّر النسخ" : "Copy failed");
    }
    await getCurrentWindow().hide();
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setSelected((value) => Math.min(value + 1, visible.length - 1));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setSelected((value) => Math.max(value - 1, 0));
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (calc) {
        void copyCalc(calc);
      } else {
        void runSelected();
      }
    } else if (event.key === "Escape") {
      void getCurrentWindow().hide();
    } else if (event.key === "Tab") {
      event.preventDefault();
      setTab((current) => {
        const index = TABS.findIndex((entry) => entry.id === current);
        return TABS[(index + 1) % TABS.length].id;
      });
    }
  };

  const reindex = async () => {
    setIndexing(true);
    setNotice(null);
    try {
      const total = await api.bootstrapIndex();
      setCount(total);
      setNotice(`Indexed ${total} candidates.`);
    } catch (err) {
      setNotice(String(err));
    } finally {
      setIndexing(false);
    }
  };

  const idle = query.trim().length === 0;
  // The button's own request and the worker's status are both "busy"; showing
  // only the local flag would let a startup walk look idle.
  const busy = indexing || (indexStatus?.indexing ?? false);

  return (
    <div className="flex h-full items-start justify-center bg-transparent px-4 pt-[10vh]">
      <div className="panel w-[min(780px,94vw)]">
        <header
          className="relative flex items-center gap-4 px-5 pb-3 pt-4"
          data-tauri-drag-region
        >
          <span className="pointer-events-none absolute inset-x-8 top-0 h-px bg-gradient-to-r from-transparent via-accent-400/40 to-transparent" />
          <Wordmark />

          <nav className="segment ms-1" aria-label={rtl ? "المرشحات" : "Filters"}>
            {TABS.map((entry) => (
              <button
                key={entry.id}
                type="button"
                onClick={() => setTab(entry.id)}
                aria-pressed={tab === entry.id}
              >
                {rtl ? entry.rtlLabel : entry.label}
              </button>
            ))}
          </nav>

          <div className="ms-auto flex items-center gap-1.5">
            <button
              type="button"
              onClick={onOpenOcr}
              className="group flex items-center gap-1.5 rounded-full border border-white/10 bg-white/[0.03] px-3 py-1.5 text-xs font-medium text-zinc-300 transition hover:border-accent-400/30 hover:bg-accent-400/10 hover:text-accent-200"
            >
              <Icon name="scan" size={13} />
              {rtl ? "استوديو OCR" : "OCR Studio"}
            </button>
            <button
              type="button"
              onClick={onOpenBatch}
              className="group flex items-center gap-1.5 rounded-full border border-white/10 bg-white/[0.03] px-3 py-1.5 text-xs font-medium text-zinc-300 transition hover:border-iris-400/30 hover:bg-iris-400/10 hover:text-iris-400"
            >
              <Icon name="table" size={13} />
              {rtl ? "البحث الدفعي" : "Batch Search"}
            </button>
            <button
              type="button"
              onClick={onOpenModels}
              aria-label={rtl ? "نماذج OCR" : "OCR Models"}
              title={rtl ? "نماذج OCR" : "OCR Models"}
              className="grid h-8 w-8 place-items-center rounded-full border border-white/10 text-zinc-400 transition hover:bg-iris-400/10 hover:text-iris-400"
            >
              <Icon name="cpu" size={14} />
            </button>
            <button
              type="button"
              onClick={onOpenSettings}
              aria-label={rtl ? "الإعدادات" : "Settings"}
              title={rtl ? "الإعدادات" : "Settings"}
              className="grid h-8 w-8 place-items-center rounded-full border border-white/10 text-zinc-400 transition hover:bg-white/10 hover:text-white"
            >
              <Icon name="tune" size={14} />
            </button>
            <button
              type="button"
              onClick={onToggleRtl}
              aria-label={rtl ? "Switch to English" : "التبديل إلى العربية"}
              className="grid h-8 w-8 place-items-center rounded-full border border-white/10 text-xs font-semibold text-zinc-400 transition hover:bg-white/10 hover:text-white"
            >
              {rtl ? "EN" : "ع"}
            </button>
            <WindowControls rtl={rtl} />
          </div>
        </header>

        <div className="px-5 pb-3">
          <div className="group relative flex items-center rounded-xl border border-white/10 bg-ink-900/70 transition focus-within:border-accent-400/40 focus-within:bg-ink-900/90 focus-within:shadow-[0_0_0_3px_rgb(34_211_238/0.08)]">
            <span className="pointer-events-none ps-3.5 text-zinc-500 transition group-focus-within:text-accent-400">
              <Icon name={loading ? "refresh" : "search"} size={17} />
            </span>
            <input
              ref={inputRef}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={onKeyDown}
              role="combobox"
              aria-expanded={!idle}
              aria-controls="zeidex-results"
              aria-autocomplete="list"
              aria-activedescendant={idle ? undefined : `zeidex-option-${selected}`}
              aria-label={rtl ? "ابحث في التطبيقات والملفات" : "Search apps and files"}
              placeholder={rtl ? "ابحث في التطبيقات والملفات…" : "Search apps, files and folders…"}
              className="search-input w-full bg-transparent px-3 py-3 text-[0.95rem] text-white outline-none placeholder:text-zinc-600"
            />
            {query ? (
              <button
                type="button"
                onClick={() => {
                  setQuery("");
                  inputRef.current?.focus();
                }}
                aria-label={rtl ? "مسح البحث" : "Clear search"}
                className="me-1 grid h-7 w-7 place-items-center rounded-md text-zinc-500 transition hover:bg-white/10 hover:text-zinc-200"
              >
                <Icon name="x" size={13} />
              </button>
            ) : (
              <span className="me-3 hidden shrink-0 items-center gap-1 sm:flex">
                <kbd className="kbd">Esc</kbd>
              </span>
            )}
          </div>
        </div>

        {calc && (
          <div className="px-5 pb-3">
            <button
              type="button"
              onClick={() => void copyCalc(calc)}
              className="flex w-full items-center gap-3 rounded-xl border border-accent-400/20 bg-accent-400/[0.06] px-3.5 py-2 text-start transition hover:bg-accent-400/10"
            >
              <span className="text-[0.6rem] font-semibold micro-label tracking-[0.16em] text-accent-400/80">
                {rtl ? "حاسبة" : "Calculator"}
              </span>
              <span
                className="min-w-0 flex-1 truncate font-mono text-sm text-zinc-100"
                dir="ltr"
              >
                {calc.display}
              </span>
              <span className="kbd shrink-0">⏎</span>
            </button>
          </div>
        )}

        <div className="min-h-[9rem]">
          {translateText !== null ? (
            <TranslationPanel text={translateText} rtl={rtl} />
          ) : tab === "clipboard" ? (
            <ClipboardPanel
              entries={clipboard}
              rtl={rtl}
              paused={clipboardPaused}
              onTogglePause={() => void toggleClipboardPause()}
              onClear={() => void clearClipboard()}
              onError={(message) => setNotice(message)}
              onDeleted={(id) => {
                // Invalidate any list read that is already in flight, or it can
                // land after this update and put the row back.
                clipboardSeq.current += 1;
                setClipboard((items) => items.filter((item) => item.id !== id));
              }}
              onToggled={(entry) => {
                clipboardSeq.current += 1;
                setClipboard((items) =>
                  items.map((item) => (item.id === entry.id ? entry : item)),
                );
              }}
            />
          ) : idle ? (
            <div className="animate-rise grid grid-cols-1 gap-2 px-5 pb-4 sm:grid-cols-3">
              {HINTS.map((hint) => (
                <div
                  key={hint.title}
                  className="rounded-xl border border-white/[0.07] bg-white/[0.02] p-3"
                >
                  <span className="flex items-center gap-1.5 text-[0.62rem] font-semibold micro-label tracking-[0.14em] text-zinc-500">
                    <Icon name={hint.icon} size={12} />
                    {rtl ? hint.titleAr : hint.title}
                  </span>
                  <span
                    dir="ltr"
                    className="mt-1.5 block truncate font-mono text-[0.72rem] text-zinc-400"
                  >
                    {rtl ? hint.bodyAr : hint.body}
                  </span>
                </div>
              ))}
            </div>
          ) : calc && visible.length === 0 ? (
            // The calculator row above is already the answer; an empty-state
            // illustration under it would read as an error.
            <div aria-hidden="true" />
          ) : (
            <ResultList
              results={visible}
              selected={selected}
              onHover={setSelected}
              onRun={(result) => {
                const index = visible.indexOf(result);
                if (index >= 0) setSelected(index);
                void runSelected();
              }}
              actionLabel={rtl ? ACTION_LABEL_AR : ACTION_LABEL}
              rtl={rtl}
            />
          )}
        </div>

        <footer className="flex items-center gap-3 border-t border-white/[0.07] bg-black/20 px-5 py-2.5 text-[0.7rem] text-zinc-500">
          <span className="flex items-center gap-1.5">
            <span className="live-dot h-1.5 w-1.5 rounded-full bg-accent-400" />
            {translateText !== null
              ? rtl
                ? "ترجمة"
                : "Translation"
              : calc
                ? rtl
                  ? "حاسبة"
                  : "Calculator"
                : loading
                  ? rtl
                    ? "يبحث…"
                    : "Searching…"
                  : tab === "clipboard"
                    ? countLabel("clips", clipboard.length, rtl)
                    : countLabel("results", visible.length, rtl)}
          </span>
          {busy ? (
            <span className="text-zinc-600">
              {indexStatus?.paused ? (rtl ? "متوقفة مؤقتاً" : "Indexing paused") : rtl ? "يفهرس…" : "Indexing…"}
            </span>
          ) : (
            count !== null && (
              <span className="text-zinc-600">
                {rtl ? `${count} مفهرس` : `${count.toLocaleString()} indexed`}
              </span>
            )
          )}

          <span className="ms-auto flex items-center gap-2">
            {indexStatus?.error && (
              <span
                role="status"
                title={indexStatus.error}
                aria-label={
                  rtl
                    ? `تحذير الفهرس: ${indexStatus.error}`
                    : `Index warning: ${indexStatus.error}`
                }
                className="text-amber-300"
              >
                <Icon name="alert" size={12} />
              </span>
            )}
            {notice && (
              <span role="status" aria-live="polite" className="max-w-[16rem] truncate text-zinc-400">
                {notice}
              </span>
            )}
            <span className="hidden items-center gap-1.5 sm:flex">
              <kbd className="kbd">↑</kbd>
              <kbd className="kbd">↓</kbd>
              <span className="text-zinc-600">{rtl ? "تنقّل" : "move"}</span>
              <kbd className="kbd">⏎</kbd>
              <span className="text-zinc-600">{rtl ? "فتح" : "open"}</span>
            </span>
            <button
              type="button"
              onClick={reindex}
              disabled={busy}
              className="flex items-center gap-1.5 rounded-full px-2 py-1 text-zinc-400 transition hover:bg-white/10 hover:text-zinc-200 disabled:opacity-50"
            >
              <Icon name="refresh" size={12} className={busy ? "animate-spin" : ""} />
              {busy ? (rtl ? "يفهرس…" : "Indexing…") : rtl ? "إعادة الفهرسة" : "Reindex"}
            </button>
          </span>
        </footer>
      </div>
    </div>
  );
}
