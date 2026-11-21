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
  let nextCallback = 1;
  let clipboardPaused = false;
  let clips = CLIPS.slice();

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
        return [SAMPLE_PAGE];
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
      case "plugin:dialog|save":
        return "/home/you/Documents/zeidex-export.docx";
      case "plugin:event|listen":
        return nextCallback;
      case "plugin:event|unlisten":
        return null;
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
