import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  api,
  type ModelsStatus,
  type PagePreview,
  type PageResult,
  type PipelineProgress,
} from "../api";
import { count as countLabel } from "../i18n";
import { BrandMark } from "./Brand";
import { Icon } from "./Icons";
import PagePreviewPane from "./ocr/PagePreview";
import RegionEditor from "./ocr/RegionEditor";
import {
  flattenTextRegions,
  replacePage,
  replacePageText,
  updateCellText,
  updateRegionText,
} from "./ocr/edits";
import { parsePageRange } from "../page-range";
import {
  isSupportedDocument,
  isTextDocument,
  textDocumentSource,
} from "../documents";

type Status =
  | { kind: "idle" }
  | { kind: "working"; message: string }
  | { kind: "error"; message: string }
  | { kind: "done"; message: string }
  // A user action with nothing wrong: a cancelled run, a refused drop. Red is
  // for failures, green is for a finished job; this is neither.
  | { kind: "notice"; message: string };

type EditMode = "regions" | "text";

export default function OcrStudio({ onBack, rtl }: { onBack: () => void; rtl: boolean }) {
  const [filePath, setFilePath] = useState<string | null>(null);
  const [pages, setPages] = useState<PageResult[]>([]);
  // Mirrors the "index recognized text" setting, read once per Studio mount.
  // The backend re-checks it on write, so a stale copy can only cause a wasted
  // call, never an unwanted row.
  const indexTextRef = useRef(true);
  const [activePage, setActivePage] = useState(0);
  const [preview, setPreview] = useState<PagePreview | null>(null);
  const [status, setStatus] = useState<Status>({ kind: "idle" });
  // Kept apart from `status`: finishing a document must not turn a ready
  // install back into "models missing".
  const [models, setModels] = useState<ModelsStatus | null>(null);
  const [activeRegion, setActiveRegion] = useState<number | null>(null);
  const [showBoxes, setShowBoxes] = useState(true);
  const [editMode, setEditMode] = useState<EditMode>("regions");
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  // Empty means every page. Kept as text so a partially typed number is not
  // coerced to 0 while the user is still deciding.
  const [firstPage, setFirstPage] = useState("");
  const [lastPage, setLastPage] = useState("");
  // While the plain-text editor is open its value is local: splitting on every
  // keystroke would collapse the newlines the user is still typing.
  const [draft, setDraft] = useState<string | null>(null);
  // A drop target only shows while a file is over the window.
  const [dragging, setDragging] = useState(false);
  // Whether a recognition run is in flight. Kept apart from `status`, because a
  // message (an unsupported drop, a preview failure) must not make the run look
  // finished: the controls stay locked until the command actually returns.
  const [running, setRunning] = useState(false);
  const unlistenRef = useRef<(() => void) | undefined>(undefined);
  const previewCache = useRef(new Map<string, PagePreview>());
  const runningRef = useRef(false);
  // Set when the user asks to stop, so late `page_done` events cannot replace
  // "Cancelling…" with a page counter for work that is being abandoned.
  const cancellingRef = useRef(false);

  const page = pages[activePage];
  const isPdf = filePath?.toLowerCase().endsWith(".pdf") ?? false;
  // A `.docx`, `.xlsx` or text file already holds characters. Running the
  // detector over its bytes would recognize nothing; the reader extracts the
  // text and returns the same page shape, so everything downstream is shared.
  const textDocument = isTextDocument(filePath ?? "");
  // `page_index` is the document's own index, which differs from the position
  // in `pages` after a page-window run.
  const previewPage = page?.page_index ?? activePage;

  // Commit a pending plain-text edit into the region structure the exporters read.
  const commitDraft = useCallback(() => {
    if (draft === null) return pages;
    const current = pages[activePage];
    if (!current) return pages;
    const next = replacePage(pages, activePage, replacePageText(current, draft));
    setPages(next);
    setDraft(null);
    return next;
  }, [draft, pages, activePage]);

  useEffect(() => {
    let active = true;
    api
      .ocrModelsStatus()
      .then((found) => {
        if (active) setModels(found);
      })
      .catch(() => {
        if (active) setModels(null);
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    let live = true;
    api
      .documentIndexStatus()
      .then((status) => {
        if (live) indexTextRef.current = status.enabled;
      })
      .catch(() => {
        // The setting is unreadable: stay with the default, which is "on", and
        // let the write path decide for real.
      });
    return () => {
      live = false;
    };
  }, []);

  useEffect(() => {
    let active = true;
    // A page is OCRed inside a rayon chunk, so `page_done` arrives in completion
    // order, not page order. Counting the completed pages keeps the bar and the
    // message monotonic; using `event.page + 1` made them jump backwards.
    const finished = new Set<number>();
    api
      .onOcrProgress((event: PipelineProgress) => {
        if (!active) return;
        if (event.type === "started") {
          finished.clear();
          setProgress({ done: 0, total: event.pages });
          setStatus({ kind: "working", message: `${event.pages} pages queued` });
        } else if (event.type === "page_done") {
          if (event.page < event.pages) finished.add(event.page);
          const done = Math.min(finished.size, event.pages);
          setProgress({ done, total: event.pages });
          if (!cancellingRef.current) {
            setStatus({
              kind: "working",
              message: `page ${done}/${event.pages}`,
            });
          }
        }
      })
      .then((fn) => {
        if (active) unlistenRef.current = fn;
        else fn();
      });
    return () => {
      active = false;
      unlistenRef.current?.();
    };
  }, []);

  // Load the preview for the active page, cached per file+page. A text document
  // has no page image, so asking the PDF renderer for one would only fail.
  useEffect(() => {
    if (!filePath || pages.length === 0 || isTextDocument(filePath)) {
      setPreview(null);
      return;
    }
    const key = `${filePath}#${previewPage}`;
    const cached = previewCache.current.get(key);
    if (cached) {
      setPreview(cached);
      return;
    }
    let active = true;
    setPreview(null);
    api
      .ocrPagePreview(filePath, previewPage)
      .then((result) => {
        previewCache.current.set(key, result);
        if (active) setPreview(result);
      })
      .catch((err) => {
        if (active) setStatus({ kind: "error", message: String(err) });
      });
    return () => {
      active = false;
    };
  }, [filePath, activePage, pages.length, previewPage]);

  // One place adopts a document, whether it came from the dialog or a drop.
  // Swapping the file while a run is in flight is refused: the old file's
  // results would otherwise land in the new file's panes.
  const loadFile = useCallback((path: string) => {
    if (runningRef.current) {
      setStatus({
        kind: "error",
        message: "Wait for the current recognition run to finish.",
      });
      return;
    }
    previewCache.current.clear();
    setFilePath(path);
    setPages([]);
    setPreview(null);
    setActivePage(0);
    setActiveRegion(null);
    setDraft(null);
    setProgress(null);
    setFirstPage("");
    setLastPage("");
    setStatus({ kind: "idle" });
  }, []);

  const pickFile = async () => {
    if (runningRef.current) {
      setStatus({
        kind: "error",
        message: "Wait for the current recognition run to finish.",
      });
      return;
    }
    try {
      // The dialog and the file's extension filter live in Rust: the page asks
      // for a document, and cannot widen what the picker will hand back.
      const selected = await api.ocrPickFile(
        rtl ? "اختر مستنداً" : "Choose a document",
        rtl ? "المستندات" : "Documents",
      );
      if (selected) loadFile(selected);
    } catch (err) {
      setStatus({ kind: "error", message: String(err) });
    }
  };

  // The README promises that a scan or a PDF can be dropped into the Studio.
  // The listener routes the first dropped path through the same `loadFile` the
  // dialog uses, so both entry points validate and reset identically.
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (!active) return;
        if (event.payload.type === "enter" || event.payload.type === "over") {
          setDragging(true);
        } else if (event.payload.type === "leave") {
          setDragging(false);
        } else if (event.payload.type === "drop") {
          setDragging(false);
          const dropped = event.payload.paths[0];
          if (!dropped) return;
          if (!isSupportedDocument(dropped)) {
            setStatus({
              kind: "error",
              message: `Not an image or PDF: ${dropped}`,
            });
            return;
          }
          loadFile(dropped);
        }
      })
      .then((fn) => {
        if (active) unlisten = fn;
        else fn();
      })
      .catch((err) => {
        // Without the channel a drop does nothing; say so instead of leaving
        // the user dropping files at a window that will never react.
        if (active) {
          setStatus({
            kind: "error",
            message: `Drag and drop is unavailable: ${String(err)}`,
          });
        }
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [loadFile]);

  const runOcr = async () => {
    if (!filePath || runningRef.current) return;
    const range = parsePageRange(firstPage, lastPage);
    if (range.kind === "invalid") {
      setStatus({ kind: "error", message: range.message });
      return;
    }
    setStatus({
      kind: "working",
      message: isTextDocument(filePath)
        ? rtl
          ? "يستخرج النص…"
          : "Extracting text…"
        : rtl
          ? "يعمل…"
          : "Starting…",
    });
    setActiveRegion(null);
    setDraft(null);
    setProgress(null);
    // The ref is set with the state, not by an effect: a drop can arrive before
    // the next render and must still see that a run owns the Studio.
    cancellingRef.current = false;
    runningRef.current = true;
    setRunning(true);
    try {
      const results = isTextDocument(filePath)
        ? await api.documentExtractText(filePath)
        : !isPdf
          ? [await api.ocrProcessImage(filePath)]
          : range.kind === "range"
            ? await api.ocrProcessPdfRange(filePath, range.first, range.last)
            : await api.ocrProcessPdf(filePath);
      previewCache.current.clear();
      setPages(results);
      setActivePage(0);
      setProgress(null);
      setStatus({ kind: "idle" });
      // The recognized text is the only copy of a scan's contents. Keeping it
      // locally makes the file findable by a phrase inside it; it never leaves
      // the machine and the setting can turn it off.
      indexRecognizedText(
        results,
        isTextDocument(filePath) ? textDocumentSource(filePath) : "ocr",
      );
    } catch (err) {
      setProgress(null);
      const message = String(err);
      setStatus(
        message === "cancelled"
          ? { kind: "notice", message: rtl ? "أُلغي التشغيل" : "Recognition cancelled" }
          : { kind: "error", message },
      );
    } finally {
      cancellingRef.current = false;
      runningRef.current = false;
      setRunning(false);
    }
  };

  const cancelRun = async () => {
    if (!runningRef.current) return;
    // Set before the round trip: a `page_done` event for the page already in
    // flight must not overwrite the cancellation message.
    cancellingRef.current = true;
    try {
      const accepted = await api.ocrCancel();
      if (!accepted) {
        // The run ended before the click landed; its own result owns the
        // status, so leave it alone instead of leaving "Cancelling…" behind.
        cancellingRef.current = false;
        return;
      }
      // The flag is read between pages: the click answers at once, and the run
      // ends when the page already inside ONNX Runtime finishes.
      setStatus({ kind: "working", message: rtl ? "جارٍ الإلغاء…" : "Cancelling…" });
    } catch (err) {
      cancellingRef.current = false;
      setStatus({ kind: "error", message: String(err) });
    }
  };

  const editRegionText = (regionIndex: number, text: string) => {
    if (!page) return;
    setPages(replacePage(pages, activePage, updateRegionText(page, regionIndex, text)));
  };

  const editCellText = (regionIndex: number, cellIndex: number, text: string) => {
    if (!page) return;
    setPages(replacePage(pages, activePage, updateCellText(page, regionIndex, cellIndex, text)));
  };

  // Stores the text as it stands. Called after a run and again after an
  // export, because an export is the moment the user finalized their edits:
  // the index should describe what they saved, not the raw first pass.
  const indexRecognizedText = useCallback(
    (
      finalized: PageResult[],
      source: "ocr" | "docx" | "xlsx" | "odt" | "text",
    ) => {
      if (!indexTextRef.current || !filePath) return;
      const body = finalized
        .map((page) => page.text)
        .filter((text) => text.trim().length > 0)
        .join("\n\n");
      if (body.trim().length === 0) return;
      void api
        .documentTextStore(filePath, body, finalized.length, source)
        .catch((err) => console.warn("zeidex: could not index recognized text", err));
    },
    [filePath],
  );

  const exportAs = async (kind: "docx" | "xlsx") => {
    const finalized = commitDraft();
    const name = kind === "docx" ? "zeidex.docx" : "zeidex.xlsx";
    const title = kind === "docx"
      ? rtl ? "حفظ المستند" : "Save the document"
      : rtl ? "حفظ المصنف" : "Save the workbook";
    const filterName = kind === "docx"
      ? rtl ? "مستند Word" : "Word document"
      : rtl ? "مصنف Excel" : "Excel workbook";
    try {
      // Rust opens the save dialog and returns the path it wrote, or null when
      // the dialog was dismissed.
      const target = kind === "docx"
        ? await api.exportDocxPages(finalized, name, title, filterName)
        : await api.exportXlsxPages(finalized, name, title, filterName);
      if (!target) return;
      // Exporting re-stores the text as edited, so the index describes what was
      // saved rather than the run that preceded the edits.
      indexRecognizedText(
        finalized,
        isTextDocument(filePath ?? "") ? textDocumentSource(filePath ?? "") : "ocr",
      );
      setStatus({ kind: "done", message: `${rtl ? "تم التصدير إلى" : "Exported to"} ${target}` });
    } catch (err) {
      setStatus({ kind: "error", message: String(err) });
    }
  };

  const missing = models && !models.complete ? models.missing : [];
  const modelsReady = models?.complete === true;

  const switchMode = (mode: EditMode) => {
    const finalized = commitDraft();
    setEditMode(mode);
    if (mode === "text") {
      const current = finalized[activePage];
      setDraft(current ? flattenTextRegions(current) : null);
    } else {
      setDraft(null);
    }
  };

  const regions = useMemo(() => page?.layout.regions ?? [], [page]);
  const working = running;


  return (
    <div className="flex h-full items-center justify-center px-3 py-3">
      <div className="panel relative flex h-[94vh] w-[min(1460px,97vw)] flex-col">
        {dragging && (
          <div className="pointer-events-none absolute inset-0 z-20 grid place-items-center rounded-[inherit] bg-ink-950/75 backdrop-blur-sm">
            <div className="flex flex-col items-center gap-2 rounded-2xl border border-dashed border-accent-400/40 px-10 py-7 text-center">
              <Icon name="folder" size={20} className="text-accent-300" />
              <p className="text-sm font-medium text-white">
                {rtl ? "أفلت صورة أو ملف PDF لفتحه" : "Drop an image or PDF to open it"}
              </p>
            </div>
          </div>
        )}
        <header className="relative flex items-center gap-3 px-5 pb-3 pt-4" data-tauri-drag-region>
          <span className="pointer-events-none absolute inset-x-8 top-0 h-px bg-gradient-to-r from-transparent via-iris-400/40 to-transparent" />
          <button
            type="button"
            onClick={onBack}
            aria-label={rtl ? "رجوع" : "Back"}
            className="grid h-8 w-8 place-items-center rounded-full border border-white/10 text-zinc-400 transition hover:bg-white/10 hover:text-white"
          >
            <Icon name="chevron" size={15} className={rtl ? "" : "rotate-180"} />
          </button>
          <BrandMark size={26} />
          <div className="flex flex-col leading-none">
            <span className="text-[0.9rem] font-semibold tracking-[-0.01em] text-white">
              {rtl ? "استوديو المستندات" : "Document Studio"}
            </span>
            <span className="mt-1 text-[0.5rem] font-medium uppercase tracking-[0.3em] text-zinc-500">
              Zeidex OCR
            </span>
          </div>

          <span className="ms-2 flex items-center gap-1.5 rounded-full border border-white/10 bg-white/[0.03] px-2.5 py-1 text-[0.625rem] text-zinc-400">
            <span
              className={`h-1.5 w-1.5 rounded-full ${
                modelsReady ? "bg-emerald-400" : "bg-amber-400"
              }`}
            />
            {modelsReady
              ? rtl
                ? "النماذج جاهزة"
                : "Models ready"
              : rtl
                ? "النماذج ناقصة"
                : "Models missing"}
          </span>

          <span className="ms-auto flex items-center gap-1.5">
            {isPdf && (
              <span
                className="flex items-center gap-1 text-[0.62rem] text-zinc-500"
                title={
                  rtl
                    ? "اتركهما فارغين لقراءة كل الصفحات."
                    : "Leave both empty to read every page."
                }
              >
                <label htmlFor="ocr-first-page">{rtl ? "من" : "from"}</label>
                <input
                  id="ocr-first-page"
                  type="number"
                  min={1}
                  inputMode="numeric"
                  value={firstPage}
                  onChange={(event) => setFirstPage(event.target.value)}
                  disabled={working}
                  aria-label={rtl ? "أول صفحة" : "First page"}
                  className="w-12 rounded-md border border-white/10 bg-ink-850 px-1.5 py-1 text-center font-mono text-[0.66rem] text-zinc-200 outline-none transition focus:border-accent-400/50 disabled:opacity-40"
                />
                <label htmlFor="ocr-last-page">{rtl ? "إلى" : "to"}</label>
                <input
                  id="ocr-last-page"
                  type="number"
                  min={1}
                  inputMode="numeric"
                  value={lastPage}
                  onChange={(event) => setLastPage(event.target.value)}
                  disabled={working}
                  aria-label={rtl ? "آخر صفحة" : "Last page"}
                  className="w-12 rounded-md border border-white/10 bg-ink-850 px-1.5 py-1 text-center font-mono text-[0.66rem] text-zinc-200 outline-none transition focus:border-accent-400/50 disabled:opacity-40"
                />
              </span>
            )}
            <button
              type="button"
              onClick={pickFile}
              disabled={working}
              className="flex items-center gap-1.5 rounded-full border border-white/10 px-3 py-1.5 text-xs font-medium text-zinc-300 transition hover:bg-white/10 hover:text-white disabled:cursor-not-allowed disabled:opacity-40"
            >
              <Icon name="folder" size={13} />
              {rtl ? "فتح ملف" : "Open file"}
            </button>
            {working && isPdf ? (
              <button
                type="button"
                onClick={cancelRun}
                className="flex items-center gap-1.5 rounded-full border border-white/15 bg-white/[0.04] px-3.5 py-1.5 text-xs font-semibold text-zinc-200 transition hover:bg-white/10 hover:text-white"
              >
                <Icon name="x" size={13} />
                {rtl ? "إلغاء" : "Cancel"}
              </button>
            ) : (
              <button
                type="button"
                onClick={runOcr}
                disabled={!filePath || working}
                className="flex items-center gap-1.5 rounded-full bg-gradient-to-r from-accent-400 to-iris-400 px-3.5 py-1.5 text-xs font-semibold text-ink-950 shadow-[0_8px_24px_-10px_rgb(34_211_238/0.8)] transition hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-40"
              >
                {/* A single image has no between-page checkpoint, so it shows
                    progress instead of a Cancel button that cannot answer. */}
                <Icon
                  name={working ? "refresh" : "scan"}
                  size={13}
                  className={working ? "animate-spin" : ""}
                />
                {working
                  ? rtl
                    ? "يعمل…"
                    : "Working…"
                  : textDocument
                    ? rtl
                      ? "استخراج النص"
                      : "Extract text"
                    : rtl
                      ? "تشغيل OCR"
                      : "Run OCR"}
              </button>
            )}
          </span>
        </header>

        {missing.length > 0 && (
          <div
            role="status"
            className="mx-5 mb-3 flex flex-wrap items-center gap-2 rounded-lg border border-amber-400/20 bg-amber-400/[0.07] px-3 py-2 text-xs text-amber-200"
          >
            <Icon name="alert" size={13} />
            <span>{rtl ? "النماذج ناقصة:" : "Model files missing:"}</span>
            <span className="text-amber-200/70">{missing.join(", ")}</span>
            <code className="ms-auto rounded bg-black/30 px-1.5 py-0.5 font-mono text-[0.65rem]">
              ./scripts/fetch_models.sh
            </code>
          </div>
        )}
        {(status.kind === "error" ||
          status.kind === "done" ||
          status.kind === "notice") && (
          <div
            role={status.kind === "error" ? "alert" : "status"}
            className={`mx-5 mb-3 flex items-center gap-2 rounded-lg border px-3 py-2 text-xs ${
              status.kind === "error"
                ? "border-red-400/20 bg-red-400/[0.07] text-red-200"
                : status.kind === "notice"
                  ? "border-white/10 bg-white/[0.04] text-zinc-300"
                  : "border-emerald-400/20 bg-emerald-400/[0.07] text-emerald-200"
            }`}
          >
            <Icon
              name={status.kind === "error" ? "alert" : status.kind === "notice" ? "info" : "check"}
              size={13}
            />
            <span className="truncate">{status.message}</span>
          </div>
        )}

        <div className="grid min-h-0 flex-1 grid-cols-1 gap-px bg-white/[0.06] lg:grid-cols-2">
          <section className="flex min-h-0 flex-col gap-2.5 bg-ink-950/60 p-4">
            <div className="flex items-center gap-2">
              <h2 className="flex items-center gap-1.5 text-[0.62rem] font-semibold micro-label tracking-[0.18em] text-zinc-500">
                <Icon name="eye" size={12} />
                {rtl ? "الصفحة" : "Page"}
              </h2>
              {pages.length > 0 && (
                <span className="rounded-md border border-white/10 px-1.5 py-px font-mono text-[0.6rem] text-zinc-500">
                  {previewPage + 1}
                  {pages.length > 1 && pages[0]?.page_index === 0 ? `/${pages.length}` : ""}
                </span>
              )}
              <span className="ms-auto flex items-center gap-3">
                <label className="flex cursor-pointer items-center gap-1.5 text-[0.68rem] text-zinc-500">
                  <input
                    type="checkbox"
                    checked={showBoxes}
                    onChange={(event) => setShowBoxes(event.target.checked)}
                    className="h-3 w-3 accent-cyan-400"
                  />
                  {rtl ? "المناطق" : "Regions"}
                </label>
                {pages.length > 1 && (
                  <select
                    aria-label={rtl ? "الصفحة" : "Page"}
                    value={activePage}
                    onChange={(event) => {
                      commitDraft();
                      setActivePage(Number(event.target.value));
                      setActiveRegion(null);
                      setDraft(null);
                    }}
                    className="rounded-md border border-white/10 bg-ink-850 px-2 py-1 text-[0.68rem] text-zinc-200"
                  >
                    {pages.map((candidate, index) => (
                      <option key={candidate.page_index} value={index}>
                        {rtl ? `صفحة ${candidate.page_index + 1}` : `Page ${candidate.page_index + 1}`}
                      </option>
                    ))}
                  </select>
                )}
              </span>
            </div>

            <PagePreviewPane
              dataUrl={preview?.data_url ?? null}
              page={preview ?? null}
              regions={regions}
              activeRegion={activeRegion}
              showBoxes={showBoxes}
              onSelect={setActiveRegion}
              rtl={rtl}
              emptyMessage={
                textDocument
                  ? rtl
                    ? "ملف نصي — لا صورة صفحة له"
                    : "A text document has no page image"
                  : undefined
              }
            />

            {working && progress && progress.total > 1 && (
              <div className="flex items-center gap-2">
                <div
                  role="progressbar"
                  aria-label={rtl ? "تقدم قراءة الصفحات" : "Page progress"}
                  aria-valuemin={0}
                  aria-valuemax={progress.total}
                  aria-valuenow={progress.done}
                  className="h-1 flex-1 overflow-hidden rounded-full bg-white/10"
                >
                  <div
                    className="h-full rounded-full bg-gradient-to-r from-accent-400 to-iris-400 transition-[width] duration-300"
                    style={{ width: `${(progress.done / progress.total) * 100}%` }}
                  />
                </div>
                <span className="font-mono text-[0.62rem] tabular-nums text-zinc-500">
                  {progress.done}/{progress.total}
                </span>
              </div>
            )}

            {filePath && (
              <p
                className="truncate font-mono text-[0.64rem] text-zinc-600"
                title={filePath}
                dir="ltr"
              >
                {filePath}
              </p>
            )}
          </section>

          <section className="flex min-h-0 flex-col gap-2.5 bg-ink-950/60 p-4">
            <div className="flex items-center gap-2">
              <h2 className="flex items-center gap-1.5 text-[0.62rem] font-semibold micro-label tracking-[0.18em] text-zinc-500">
                <Icon name="text" size={12} />
                {rtl ? "النص المستخرج" : "Extracted text"}
              </h2>
              <span className="segment ms-auto">
                {(["regions", "text"] as const).map((mode) => (
                  <button
                    key={mode}
                    type="button"
                    onClick={() => switchMode(mode)}
                    aria-pressed={editMode === mode}
                  >
                    {mode === "regions"
                      ? rtl
                        ? "مناطق"
                        : "Regions"
                      : rtl
                        ? "نص"
                        : "Text"}
                  </button>
                ))}
              </span>
            </div>

            {!page ? (
              <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 rounded-xl border border-dashed border-white/10 bg-white/[0.015] px-6 text-center">
                <span className="grid h-12 w-12 place-items-center rounded-2xl bg-gradient-to-br from-accent-400/15 to-iris-400/15 text-accent-300 ring-1 ring-white/10">
                  <Icon name="scan" size={22} />
                </span>
                <p className="text-sm font-medium text-zinc-300">
                  {rtl ? "افتح مستندًا لبدء الاستخراج" : "Open a document to begin"}
                </p>
                <p className="max-w-[24rem] text-xs leading-relaxed text-zinc-600">
                  {rtl
                    ? "صور و PDF تُقرأ محليًا: تخطيط، أسطر، جداول، ثم نص قابل للتحرير والتصدير إلى Word و Excel."
                    : "Images and PDFs are read on-device — layout, lines, tables — then exported as editable Word and Excel files."}
                </p>
                <div className="mt-1 flex items-center gap-1.5 text-[0.6rem] micro-label tracking-[0.16em] text-zinc-600">
                  <Icon name="lock" size={11} />
                  {rtl ? "بلا شبكة" : "No network"}
                </div>
              </div>
            ) : editMode === "regions" ? (
              <RegionEditor
                page={page}
                activeRegion={activeRegion}
                rtl={rtl}
                onSelect={setActiveRegion}
                onRegionText={editRegionText}
                onCellText={editCellText}
              />
            ) : (
              <textarea
                value={draft ?? flattenTextRegions(page)}
                onChange={(event) => setDraft(event.target.value)}
                onBlur={commitDraft}
                dir="auto"
                aria-label={rtl ? "النص المستخرج" : "Extracted text"}
                placeholder={rtl ? "النص يظهر هنا…" : "Recognized text appears here…"}
                className="min-h-0 flex-1 resize-none rounded-xl border border-white/10 bg-ink-900/50 p-3 font-sans text-[0.86rem] leading-relaxed text-zinc-100 outline-none transition placeholder:text-zinc-600 focus:border-accent-400/50"
              />
            )}
          </section>
        </div>

        <footer className="flex flex-wrap items-center gap-2 border-t border-white/[0.07] bg-black/20 px-5 py-2.5 text-[0.7rem] text-zinc-500">
          <span className="flex items-center gap-1.5">
            <Icon name="export" size={12} />
            {rtl ? "تصدير كل الصفحات" : "Export all pages"}
          </span>
          <button
            type="button"
            onClick={() => exportAs("docx")}
            disabled={pages.length === 0}
            className="flex items-center gap-1.5 rounded-full border border-white/10 px-2.5 py-1 text-zinc-300 transition hover:border-sky-400/30 hover:bg-sky-400/10 hover:text-sky-200 disabled:opacity-40"
          >
            <Icon name="file" size={12} />
            Word
          </button>
          <button
            type="button"
            onClick={() => exportAs("xlsx")}
            disabled={pages.length === 0}
            className="flex items-center gap-1.5 rounded-full border border-white/10 px-2.5 py-1 text-zinc-300 transition hover:border-emerald-400/30 hover:bg-emerald-400/10 hover:text-emerald-200 disabled:opacity-40"
          >
            <Icon name="table" size={12} />
            Excel
          </button>
          {pages.length > 0 && (
            <span className="text-zinc-600">
              {countLabel("pages", pages.length, rtl)}
            </span>
          )}
          <span className="ms-auto truncate font-mono text-[0.62rem]" dir="ltr">
            {models?.dir ?? "PP-OCRv4 · SLANet"}
          </span>
        </footer>
      </div>
    </div>
  );
}
