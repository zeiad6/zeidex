import type { PageResult } from "../api";
import { DEMO_LIST, parseList, searchBatch } from "./batch";
import {
  CLIPS,
  MODELS,
  RESULTS,
  SAMPLE_PAGE,
  SAMPLE_PREVIEW,
  TRANSLATIONS,
} from "./fixtures";

type Args = Record<string, unknown>;

const DEMO_PDF = "/home/you/Documents/Invoices/invoice-2026-014.pdf";

/**
 * Stands in for the Tauri IPC bridge so the real components can run in a plain
 * browser. Dev-only: `main.tsx` imports this module lazily and only when
 * `?demo=` is present and Vite is building in development, so nothing here
 * reaches a release bundle.
 */
export function installMock() {
  const callbacks = new Map<number, (payload: unknown) => void>();
  // Event name to the callback id `plugin:event|listen` was given, so the mock
  // can emit the same `ocr-progress` events the shell does.
  const listeners = new Map<string, number>();
  let nextCallback = 1;
  let clipboardPaused = false;
  let clips = CLIPS.slice();
  // The PDF run a Cancel click would stop, if one is in flight.
  let pdfRun: { cancelled: boolean } | null = null;

  const emit = (event: string, payload: unknown) => {
    const id = listeners.get(event);
    if (id === undefined) return;
    callbacks.get(id)?.({ event, id, payload });
  };

  const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

  // Four pages paced like a real run, so the Cancel button has something to
  // stop and the progress bar moves visibly. Pages arrive in order; the real
  // pipeline may not, which is why the Studio counts them instead.
  const processPdf = async (first: number, last: number) => {
    const run = { cancelled: false };
    pdfRun = run;
    const pages = last - first + 1;
    try {
      emit("ocr-progress", { type: "started", pages });
      const results: PageResult[] = [];
      for (let offset = 0; offset < pages; offset += 1) {
        await delay(450);
        // The same between-page checkpoint the pipeline has: a page already
        // inside the runtime would finish, and the next one never starts.
        if (run.cancelled) throw "cancelled";
        emit("ocr-progress", { type: "page_done", page: offset, pages });
        results.push({ ...SAMPLE_PAGE, page_index: first + offset });
      }
      emit("ocr-progress", { type: "finished" });
      return results;
    } finally {
      if (pdfRun === run) pdfRun = null;
    }
  };

  const search = (query: string): typeof RESULTS => {
    const needle = query.trim().toLowerCase();
    if (!needle) return [];
    return RESULTS.filter(
      (result) =>
        result.title.toLowerCase().includes(needle) ||
        (result.subtitle ?? "").toLowerCase().includes(needle),
    ).slice(0, 24);
  };

  const invoke = async (command: string, args: Args = {}) => {
    switch (command) {
      case "search":
        return search(String(args.query ?? ""));
      case "calc_eval":
        return String(args.query ?? "").replace(/\s/g, "") === "(18*4)+7.5"
          ? { display: "79.5", raw: "79.5", value: 79.5 }
          : null;
      case "candidate_count":
        return 12_804;
      case "batch_parse":
        return parseList(String(args.text ?? ""));
      case "batch_import":
        return parseList(DEMO_LIST);
      case "batch_search":
        return searchBatch(
          Array.isArray(args.entries) ? (args.entries as string[]) : [],
          Number(args.threshold ?? 70),
        );
      case "batch_export":
        return null;
      case "open_target":
      case "reveal_target":
        return null;
      case "bootstrap_index":
        return 12_804;
      case "clipboard_list":
        return clips;
      case "clipboard_poll":
        return null;
      case "clipboard_paused":
        return clipboardPaused;
      case "clipboard_set_paused":
        clipboardPaused = Boolean(args.paused);
        return clipboardPaused;
      case "clipboard_set_pinned":
      case "clipboard_delete":
        return true;
      case "clipboard_clear":
        clips = [];
        return CLIPS.length;
      case "ocr_models_status":
        return MODELS;
      case "ocr_page_preview":
        return SAMPLE_PREVIEW;
      case "ocr_process_image":
        return SAMPLE_PAGE;
      case "ocr_process_pdf":
        return processPdf(0, 3);
      case "ocr_process_pdf_range":
        return processPdf(
          Math.max(1, Number(args.firstPage ?? 1)),
          Math.max(1, Number(args.lastPage ?? 4)),
        );
      case "ocr_cancel": {
        if (!pdfRun) return false;
        pdfRun.cancelled = true;
        return true;
      }
      case "translate_text": {
        const lang = String(args.targetLang ?? "en");
        return {
          original: String(args.text ?? ""),
          translated: TRANSLATIONS[lang] ?? TRANSLATIONS.en,
          error: null,
        };
      }
      case "plugin:dialog|open":
        return DEMO_PDF;
      case "plugin:dialog|save": {
        // The plugin passes `{ options: { defaultPath } }`.
        const options = (args.options ?? args) as { defaultPath?: string };
        const suggested = String(options.defaultPath ?? "");
        return suggested.endsWith(".xlsx")
          ? "/home/you/Documents/zeidex-batch-2026-11-22.xlsx"
          : "/home/you/Documents/zeidex-export.docx";
      }
      case "plugin:event|listen": {
        const event = String(args.event ?? "");
        const handler = Number(args.handler ?? 0);
        listeners.set(event, handler);
        return handler;
      }
      case "plugin:event|unlisten": {
        const event = String(args.event ?? "");
        const eventId = Number(args.eventId ?? 0);
        // One event can briefly hold two handlers: a StrictMode remount listens
        // again before the first listener's promise resolves, and that first
        // cleanup must not delete the handler the second listen installed.
        if (listeners.get(event) === eventId) listeners.delete(event);
        return null;
      }
      default:
        return null;
    }
  };

  (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke,
    transformCallback(callback: (payload: unknown) => void, once = false) {
      const id = nextCallback++;
      callbacks.set(id, (payload) => {
        callback(payload);
        if (once) callbacks.delete(id);
      });
      return id;
    },
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { windowLabel: "main", label: "main" },
    },
    convertFileSrc: (path: string) => path,
  };

  // The event API unregisters through its own internals object before it calls
  // the command; without it every unmount rejects with a TypeError.
  (window as unknown as { __TAURI_EVENT_PLUGIN_INTERNALS__: unknown })
    .__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener(event: string, eventId: number) {
      if (listeners.get(event) === eventId) listeners.delete(event);
    },
  };

  // A desktop-like backdrop, so element screenshots are not cut out of a void.
  const style = document.createElement("style");
  style.textContent = `
    body {
      background:
        radial-gradient(60% 60% at 18% 12%, #12314a 0%, transparent 62%),
        radial-gradient(52% 52% at 82% 88%, #2a1d4d 0%, transparent 60%),
        linear-gradient(160deg, #0a0d14 0%, #05070b 70%);
      background-attachment: fixed;
    }
    body::before {
      content: "";
      position: fixed;
      inset: 0;
      pointer-events: none;
      opacity: 0.35;
      background-image:
        linear-gradient(rgb(255 255 255 / 0.035) 1px, transparent 1px),
        linear-gradient(90deg, rgb(255 255 255 / 0.035) 1px, transparent 1px);
      background-size: 48px 48px;
      mask-image: radial-gradient(70% 60% at 50% 40%, #000 20%, transparent 75%);
    }
  `;
  document.head.append(style);
}
