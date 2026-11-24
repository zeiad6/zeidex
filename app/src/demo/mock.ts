import type { LaunchResult, MatchKind, PageResult } from "../api";
import { demoMatchKind } from "../match-kind";
import { DEMO_LIST, parseList, searchBatch } from "./batch";
import {
  ADVISOR,
  CLIPS,
  MODELS,
  MODEL_PACKS,
  RESULTS,
  SAMPLE_PAGE,
  SAMPLE_PREVIEW,
  TRANSLATIONS,
} from "./fixtures";
import { DEMO_ORGANIZED, demoBatch, demoExtraction } from "./organizer";

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
  // The Models screen mutates its catalog; a demo visit should not leak that
  // state into the next one, so it is copied once per install.
  let demoPacks = MODEL_PACKS.map((pack) => ({ ...pack, files: pack.files.map((file) => ({ ...file })) }));
  let demoActive = "ppocrv4-mobile";
  let demoSettings = {
    version: "0.4.0",
    platform: "windows",
    configPath: "C:\\Users\\you\\AppData\\Roaming\\zeidex\\config.json",
    dbPath: "C:\\Users\\you\\AppData\\Roaming\\zeidex\\zeidex.db",
    candidateCount: 12804,
    fileScanDepth: 4,
    fileScanLimit: 60000,
    fileScanExtraRoots: ["D:\\Projects"],
    lazyIndexingEnabled: true,
    startMinimized: true,
    localizedAppNames: true,
    clipboardHistoryLimit: 200,
    clipboardFilterSecrets: true,
    clipboardRetentionDays: 30,
    clipboardPaused: false,
    webSearchEnabled: true,
    webSearchEngine: "duckduckgo",
    searchEngines: ["duckduckgo", "google", "bing"],
    limits: {
      fileScanDepth: [1, 12],
      fileScanLimit: [1000, 500000],
      clipboardHistoryLimit: [20, 5000],
      clipboardRetentionDays: [1, 365],
    },
  };
  let demoHotkey = {
    accelerator: "Alt+Space",
    registered: true,
    error: null as string | null,
    default: "Alt+Space",
  };
  let demoAutostart = {
    supported: true,
    enabled: true,
    mechanism: "Windows registry",
    location: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
    command: "C:\\Users\\you\\AppData\\Local\\Zeidex\\zeidex.exe",
    development: false,
    error: null as string | null,
  };
  // The PDF run a Cancel click would stop, if one is in flight.
  let pdfRun: { cancelled: boolean } | null = null;
  // Index Exclusions, as the Settings screen reads them. The demo keeps the
  // same shape the commands return so the real component runs unchanged.
  let demoExclusions = [
    {
      id: "ex-demo-1",
      kind: "folder",
      value: "D:\\Projects\\node_modules",
      enabled: true,
      added_at: 1763200000,
    },
    {
      id: "ex-demo-2",
      kind: "extension",
      value: "tmp",
      enabled: true,
      added_at: 1763200100,
    },
    {
      id: "ex-demo-3",
      kind: "pattern",
      value: "**/build/**",
      enabled: false,
      added_at: 1763200200,
    },
  ];
  let demoDrives = [
    {
      root: "C:\\",
      label: "System",
      removable: false,
      kind: "ssd",
      totalBytes: 512_000_000_000,
      freeBytes: 148_000_000_000,
      scanRoot: false,
    },
    {
      root: "D:\\",
      label: "Projects",
      removable: false,
      kind: "ssd",
      totalBytes: 1_000_000_000_000,
      freeBytes: 410_000_000_000,
      scanRoot: true,
    },
    {
      root: "E:\\",
      label: "Backup",
      removable: true,
      kind: "hdd",
      totalBytes: 2_000_000_000_000,
      freeBytes: 900_000_000_000,
      scanRoot: false,
    },
  ];
  const demoPresets = [
    {
      id: "development",
      name: "Development files",
      nameAr: "ملفات التطوير",
      description: "Dependency and build folders that are never worth searching by name.",
      descriptionAr: "مجلدات الاعتماديات والبناء التي لا تستحق البحث عنها بالاسم.",
      rules: [
        { kind: "folder", value: "node_modules" },
        { kind: "folder", value: ".git" },
        { kind: "folder", value: "target" },
        { kind: "folder", value: "dist" },
        { kind: "folder", value: "build" },
      ],
    },
    {
      id: "temporary",
      name: "Temporary files",
      nameAr: "الملفات المؤقتة",
      description: "Caches, temp folders and logs.",
      descriptionAr: "ذاكرات مؤقتة ومجلدات temp وسجلات.",
      rules: [
        { kind: "extension", value: "tmp" },
        { kind: "extension", value: "log" },
        { kind: "pattern", value: "**/Cache/**" },
      ],
    },
    {
      id: "system",
      name: "System noise",
      nameAr: "ضجيج النظام",
      description: "Windows folders that add thousands of rows and no useful result.",
      descriptionAr: "مجلدات Windows تضيف آلاف الصفوف دون نتيجة مفيدة.",
      rules: [
        { kind: "folder", value: "C:\\Windows\\WinSxS" },
        { kind: "folder", value: "C:\\$Recycle.Bin" },
      ],
    },
  ];

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

  // The demo bridge mirrors the Rust tiers so the screenshots show the same
  // "Start / End / Inside / Close" chips the real launcher does.
  const MATCH_ORDER: Record<MatchKind, number> = {
    exact: 0,
    prefix: 1,
    suffix: 2,
    contains: 3,
    fuzzy: 4,
    approximate: 5,
  };

  const search = (query: string): LaunchResult[] => {
    const needle = query.trim();
    if (!needle) return [];
    const rows: LaunchResult[] = [];
    for (const result of RESULTS) {
      const match_kind = demoMatchKind(needle, result.title);
      if (match_kind) {
        rows.push({ ...result, match_kind });
      }
    }
    rows.sort((a, b) => MATCH_ORDER[a.match_kind!] - MATCH_ORDER[b.match_kind!]);
    return rows.slice(0, 24);
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
        // The demo has no disk; the path is the one the real command would
        // have returned so the flow stays demonstrable.
        return "/home/you/Documents/zeidex-batch-2026-11-22.xlsx";
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
      case "models_catalog":
        return demoPacks;
      case "models_advisor":
        return ADVISOR;
      case "models_download": {
        const id = String(args.packId ?? "");
        const pack = demoPacks.find((entry) => entry.id === id);
        if (!pack) throw `unknown model pack: ${id}`;
        const total = pack.download_bytes;
        const file = pack.files[0]?.name ?? null;
        for (const fraction of [0.08, 0.34, 0.61, 0.88]) {
          await delay(240);
          emit("models-progress", {
            pack_id: id,
            phase: "downloading",
            file,
            file_index: 1,
            file_count: pack.files.length,
            downloaded: Math.round(total * fraction),
            total,
            speed_bps: 4_600_000,
            message: null,
          });
        }
        for (const phase of ["verifying", "installing"] as const) {
          emit("models-progress", {
            pack_id: id,
            phase,
            file,
            file_index: pack.files.length,
            file_count: pack.files.length,
            downloaded: total,
            total,
            speed_bps: 0,
            message: null,
          });
          await delay(320);
        }
        demoPacks = demoPacks.map((entry) =>
          entry.id === id
            ? {
                ...entry,
                state: "installed",
                installed_bytes: total,
                files: entry.files.map((item) => ({
                  ...item,
                  present: true,
                  actual_bytes: item.expected_bytes,
                })),
              }
            : entry,
        );
        emit("models-progress", {
          pack_id: id,
          phase: "done",
          file: null,
          file_index: pack.files.length,
          file_count: pack.files.length,
          downloaded: total,
          total,
          speed_bps: 0,
          message: null,
        });
        return demoPacks.find((entry) => entry.id === id);
      }
      case "models_cancel_download":
        return true;
      case "models_activate": {
        demoActive = String(args.packId ?? demoActive);
        demoPacks = demoPacks.map((entry) => ({ ...entry, active: entry.id === demoActive }));
        return demoPacks;
      }
      case "models_remove": {
        const id = String(args.packId ?? "");
        demoPacks = demoPacks.map((entry) =>
          entry.id === id
            ? { ...entry, state: "not_installed", installed_bytes: 0, active: false }
            : entry,
        );
        if (demoActive === id) demoActive = "ppocrv4-mobile";
        demoPacks = demoPacks.map((entry) => ({ ...entry, active: entry.id === demoActive }));
        return demoPacks;
      }
      case "models_verify": {
        const id = String(args.packId ?? "");
        const pack = demoPacks.find((entry) => entry.id === id);
        return {
          pack_id: id,
          ok: true,
          problems: [],
          checked_files: pack?.files.length ?? 0,
          checked_bytes: pack?.installed_bytes ?? 0,
        };
      }
      case "models_benchmark": {
        const id = String(args.packId ?? "");
        const pack = demoPacks.find((entry) => entry.id === id);
        if (!pack) throw `unknown model pack: ${id}`;
        await delay(900);
        return {
          pack_id: id,
          pack_name: pack.name,
          load_ms: pack.bundled ? 240 : 410,
          total_ms: pack.bundled ? 1_240 : 1_680,
          memory_mb: pack.ram_mb,
          samples: [
            {
              name: "english",
              milliseconds: 380,
              characters: 52,
              expected_characters: 53,
              cer: 0.019,
              text: "Zeidex reads documents offline. Invoice 2024 total: 1250.00 USD.",
            },
            {
              name: "arabic",
              milliseconds: 520,
              characters: 48,
              expected_characters: 49,
              cer: 0.041,
              text: "زايدكس يقرأ المستندات دون اتصال بالإنترنت. الفاتورة رقم 2024.",
            },
          ],
        };
      }
      // The Studio's picker goes through the Rust command (the dialog's
      // extension filter lives there), so the browser demo has to answer that
      // command as well as the plugin one or "Open file" silently does nothing.
      case "ocr_pick_file":
        return DEMO_PDF;
      // Settings and the shortcut card read real commands; answering them keeps
      // the screen reviewable in a browser instead of showing its error state.
      case "settings_snapshot":
        return demoSettings;
      case "settings_update": {
        const patch = (args.patch ?? {}) as Record<string, unknown>;
        demoSettings = { ...demoSettings, ...patch } as typeof demoSettings;
        return { settings: demoSettings, reindexRequired: false, warning: null };
      }
      case "hotkey_status":
        return demoHotkey;
      case "hotkey_set": {
        const accelerator = args.accelerator;
        demoHotkey =
          accelerator === null || accelerator === undefined
            ? { ...demoHotkey, accelerator: demoHotkey.default, registered: true, error: null }
            : { ...demoHotkey, accelerator: String(accelerator), registered: true, error: null };
        return demoHotkey;
      }
      case "autostart_status":
        return demoAutostart;
      case "autostart_set":
        demoAutostart = { ...demoAutostart, enabled: Boolean(args.enabled) };
        return demoAutostart;
      case "settings_pick_folder":
      case "settings_pick_file":
        // A browser has no native dialog. Returning `null` is the same answer
        // the command gives when the user dismisses the picker.
        return null;
      case "exclusions_snapshot":
        return {
          entries: demoExclusions,
          active: demoExclusions.filter((entry) => entry.enabled).length,
          presets: demoPresets.map((preset) => ({
            ...preset,
            alreadyApplied: preset.rules.filter((rule) =>
              demoExclusions.some(
                (entry) =>
                  entry.kind === rule.kind &&
                  entry.value.toLowerCase() === rule.value.toLowerCase(),
              ),
            ).length,
          })),
          configPath: demoSettings.configPath,
          indexPath: demoSettings.dbPath,
        };
      case "exclusions_add": {
        const kind = String(args.kind ?? "folder");
        const value = String(args.value ?? "").trim();
        if (!value) throw "An exclusion needs a value.";
        demoExclusions = [
          ...demoExclusions,
          {
            id: `ex-demo-${demoExclusions.length + 1}-${Date.now()}`,
            kind,
            value,
            enabled: true,
            added_at: Math.floor(Date.now() / 1000),
          },
        ];
        return {
          entries: demoExclusions,
          active: demoExclusions.filter((entry) => entry.enabled).length,
          removed: 0,
          reindexing: false,
        };
      }
      case "exclusions_update": {
        const id = String(args.id ?? "");
        demoExclusions = demoExclusions.map((entry) =>
          entry.id === id
            ? {
                ...entry,
                value: args.value === undefined ? entry.value : String(args.value),
                enabled: args.enabled === undefined ? entry.enabled : Boolean(args.enabled),
              }
            : entry,
        );
        return {
          entries: demoExclusions,
          active: demoExclusions.filter((entry) => entry.enabled).length,
          removed: 0,
          reindexing: false,
        };
      }
      case "exclusions_remove": {
        const id = String(args.id ?? "");
        demoExclusions = demoExclusions.filter((entry) => entry.id !== id);
        return {
          entries: demoExclusions,
          active: demoExclusions.filter((entry) => entry.enabled).length,
          removed: 0,
          reindexing: false,
        };
      }
      case "exclusions_clear":
        demoExclusions = [];
        return { entries: [], active: 0, removed: 0, reindexing: false };
      case "exclusions_apply_preset": {
        const preset = demoPresets.find((entry) => entry.id === args.preset);
        if (preset) {
          for (const rule of preset.rules) {
            const exists = demoExclusions.some(
              (entry) =>
                entry.kind === rule.kind &&
                entry.value.toLowerCase() === rule.value.toLowerCase(),
            );
            if (!exists) {
              demoExclusions.push({
                id: `ex-demo-${demoExclusions.length + 1}-${Date.now()}`,
                kind: rule.kind,
                value: rule.value,
                enabled: true,
                added_at: Math.floor(Date.now() / 1000),
              });
            }
          }
        }
        return {
          entries: demoExclusions,
          active: demoExclusions.filter((entry) => entry.enabled).length,
          removed: 0,
          reindexing: false,
        };
      }
      case "exclusions_preview":
        return 0;
      case "system_drives":
        return demoDrives;
      case "diagnostics_snapshot":
        return {
          app: {
            name: "Zeidex Search",
            version: "0.4.0",
            build: "release",
            tauri: "2.12",
            developer: "Zeidexsoft",
            maintainer: "@zeiad6",
            website: "https://zeidexsoft.com",
            repository: "https://github.com/zeiad6/zeidex-search",
            developerCard: "https://zeidexlink.zeidexsoft.com/zeidex",
            license: "GPL-3.0-or-later",
            copyright: "Copyright © 2026 Zeidexsoft",
          },
          platform: {
            os: "Windows 11 Pro",
            arch: "x86_64",
            family: "windows",
            executable: "C:\\Users\\you\\AppData\\Local\\Zeidex\\zeidex.exe",
          },
          paths: {
            config: demoSettings.configPath,
            database: demoSettings.dbPath,
            modelsBundled: "C:\\Users\\you\\AppData\\Local\\Zeidex\\models",
            modelsUser: "C:\\Users\\you\\AppData\\Roaming\\zeidex\\models",
            onnxRuntime: "C:\\Users\\you\\AppData\\Local\\Zeidex\\onnxruntime.dll",
            pdfium: "C:\\Users\\you\\AppData\\Local\\Zeidex\\pdfium.dll",
            startupLog: "C:\\Users\\you\\AppData\\Roaming\\zeidex\\zeidex-startup.log",
          },
          index: {
            candidates: demoSettings.candidateCount,
            documents: 214,
            databaseBytes: 48_300_000,
            scanRoots: demoSettings.fileScanExtraRoots,
            exclusionRules: demoExclusions.length,
          },
        };
      case "onboarding_status":
        // `?welcome=1` shows the first-run guide in the browser demo; a normal
        // demo visit is a returning user, so the guide stays out of the way.
        return new URLSearchParams(window.location.search).get("welcome") !== "1";
      case "onboarding_complete":
        return true;
      case "check_for_updates":
        return {
          current: "0.4.0",
          latest: "0.4.0",
          available: false,
          url: "https://github.com/zeiad6/zeidex-search/releases/tag/v0.4.0",
          notes: "",
          publishedAt: "2026-11-20T10:00:00Z",
          checkedAt: Math.floor(Date.now() / 1000),
          error: null,
        };
      case "index_clear": {
        const removed = demoSettings.candidateCount;
        demoSettings = { ...demoSettings, candidateCount: 0 };
        return removed;
      }
      case "search_history_clear":
        return 0;
      case "index_pause":
        return true;
      case "index_resume":
        return true;
      case "index_cancel":
        return true;
      case "index_control_state":
        return "idle";
      case "document_index_status":
        return { enabled: true, count: 214 };
      case "document_set_indexing":
        return Boolean(args.enabled);
      case "document_text_clear":
        return 0;
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
      // The organizer runs in Rust; the demo answers with the fixture that
      // describes the same demo pages, so the panel can be reviewed in a
      // browser. Exports and previews are not faked: the real bodies come from
      // `core/organizer/src/render.rs` and only exist when Rust is present.
      case "organize_pages":
        return DEMO_ORGANIZED;
      case "extract_document":
        return demoExtraction(args.profile);
      case "extract_document_batch":
        return demoBatch(args.profile);
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
      case "plugin:event|listen": {
        const event = String(args.event ?? "");
        const handler = Number(args.handler ?? 0);
        listeners.set(event, handler);
        // `?demo=1&update=1` makes the mock announce a release newer than this
        // build, so the notification toast can be reviewed without cutting a
        // release. Dev-only, like the rest of this file.
        if (
          event === "zeidex://update" &&
          new URLSearchParams(window.location.search).get("update") === "1"
        ) {
          window.setTimeout(() => {
            emit("zeidex://update", {
              current: "0.4.0",
              latest: "0.4.1",
              available: true,
              url: "https://github.com/zeiad6/zeidex-search/releases/tag/v0.4.1",
              notes: "A calmer light theme and a fix for drives that report no free space.",
              publishedAt: "2026-11-22T09:00:00Z",
              checkedAt: Math.floor(Date.now() / 1000),
              error: null,
            });
          }, 1200);
        }
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
