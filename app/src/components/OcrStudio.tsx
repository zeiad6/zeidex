import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
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

type Status =
  | { kind: "idle" }
  | { kind: "working"; message: string }
  | { kind: "error"; message: string }
  | { kind: "done"; message: string };

type EditMode = "regions" | "text";

const IMAGE_EXTENSIONS = ["png", "jpg", "jpeg", "tif", "tiff", "bmp", "webp"];

export default function OcrStudio({ onBack, rtl }: { onBack: () => void; rtl: boolean }) {
  const [filePath, setFilePath] = useState<string | null>(null);
  const [pages, setPages] = useState<PageResult[]>([]);
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
  // While the plain-text editor is open its value is local: splitting on every
  // keystroke would collapse the newlines the user is still typing.
  const [draft, setDraft] = useState<string | null>(null);
  const unlistenRef = useRef<(() => void) | undefined>(undefined);
  const previewCache = useRef(new Map<string, PagePreview>());

  const page = pages[activePage];

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
    let active = true;
    api
      .onOcrProgress((event: PipelineProgress) => {
        if (!active) return;
        if (event.type === "started") {
          setProgress({ done: 0, total: event.pages });
          setStatus({ kind: "working", message: `${event.pages} pages queued` });
        } else if (event.type === "page_done") {
          setProgress({ done: event.page + 1, total: event.pages });
          setStatus({
            kind: "working",
            message: `page ${event.page + 1}/${event.pages}`,
          });
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

  // Load the preview for the active page, cached per file+page.
  useEffect(() => {
    if (!filePath || pages.length === 0) {
      setPreview(null);
      return;
    }
    const key = `${filePath}#${activePage}`;
    const cached = previewCache.current.get(key);
    if (cached) {
      setPreview(cached);
      return;
    }
    let active = true;
    setPreview(null);
    api
      .ocrPagePreview(filePath, activePage)
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
  }, [filePath, activePage, pages.length]);

  const pickFile = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "Documents", extensions: [...IMAGE_EXTENSIONS, "pdf"] }],
      });
      if (typeof selected !== "string") return;
      previewCache.current.clear();
      setFilePath(selected);
      setPages([]);
      setPreview(null);
      setActivePage(0);
      setActiveRegion(null);
      setDraft(null);
      setProgress(null);
    } catch {
      // Dialog cancelled or unavailable.
    }
  };

  const runOcr = async () => {
    if (!filePath) return;
    setStatus({ kind: "working", message: rtl ? "يعمل…" : "Starting…" });
    setActiveRegion(null);
    setDraft(null);
    setProgress(null);
    try {
      const results = filePath.toLowerCase().endsWith(".pdf")
        ? await api.ocrProcessPdf(filePath)
        : [await api.ocrProcessImage(filePath)];
      previewCache.current.clear();
      setPages(results);
      setActivePage(0);
      setProgress(null);
      setStatus({ kind: "idle" });
    } catch (err) {
      setProgress(null);
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

  const exportAs = async (kind: "docx" | "xlsx") => {
    const finalized = commitDraft();
    const target = await save({
      filters: [
        kind === "docx"
          ? { name: "Word", extensions: ["docx"] }
          : { name: "Excel", extensions: ["xlsx"] },
      ],
      defaultPath: kind === "docx" ? "zeidex.docx" : "zeidex.xlsx",
    }).catch(() => null);
    if (!target) return;
    try {
      if (kind === "docx") await api.exportDocxPages(finalized, target);
      else await api.exportXlsxPages(finalized, target);
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
  const working = status.kind === "working";

  return (
    <div className="flex h-full items-center justify-center px-3 py-3">
      <div className="panel flex h-[94vh] w-[min(1460px,97vw)] flex-col">
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
            <button
              type="button"
              onClick={pickFile}
              className="flex items-center gap-1.5 rounded-full border border-white/10 px-3 py-1.5 text-xs font-medium text-zinc-300 transition hover:bg-white/10 hover:text-white"
            >
              <Icon name="folder" size={13} />
              {rtl ? "فتح ملف" : "Open file"}
            </button>
            <button
              type="button"
              onClick={runOcr}
              disabled={!filePath || working}
              className="flex items-center gap-1.5 rounded-full bg-gradient-to-r from-accent-400 to-iris-400 px-3.5 py-1.5 text-xs font-semibold text-ink-950 shadow-[0_8px_24px_-10px_rgb(34_211_238/0.8)] transition hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-40"
            >
              <Icon name={working ? "refresh" : "scan"} size={13} className={working ? "animate-spin" : ""} />
              {working ? (rtl ? "يعمل…" : "Working…") : rtl ? "تشغيل OCR" : "Run OCR"}
            </button>
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
        {(status.kind === "error" || status.kind === "done") && (
          <div
            role={status.kind === "error" ? "alert" : "status"}
            className={`mx-5 mb-3 flex items-center gap-2 rounded-lg border px-3 py-2 text-xs ${
              status.kind === "error"
                ? "border-red-400/20 bg-red-400/[0.07] text-red-200"
                : "border-emerald-400/20 bg-emerald-400/[0.07] text-emerald-200"
            }`}
          >
            <Icon name={status.kind === "error" ? "alert" : "check"} size={13} />
            <span className="truncate">{status.message}</span>
          </div>
        )}

        <div className="grid min-h-0 flex-1 grid-cols-1 gap-px bg-white/[0.06] lg:grid-cols-2">
          <section className="flex min-h-0 flex-col gap-2.5 bg-ink-950/60 p-4">
            <div className="flex items-center gap-2">
              <h2 className="flex items-center gap-1.5 text-[0.62rem] font-semibold uppercase tracking-[0.18em] text-zinc-500">
                <Icon name="eye" size={12} />
                {rtl ? "الصفحة" : "Page"}
              </h2>
              {pages.length > 0 && (
                <span className="rounded-md border border-white/10 px-1.5 py-px font-mono text-[0.6rem] text-zinc-500">
                  {activePage + 1}/{pages.length}
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
                        {rtl ? `صفحة ${index + 1}` : `Page ${index + 1}`}
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
            />

            {working && progress && progress.total > 1 && (
              <div
                role="progressbar"
                aria-valuemin={0}
                aria-valuemax={progress.total}
                aria-valuenow={progress.done}
                className="h-1 overflow-hidden rounded-full bg-white/10"
              >
                <div
                  className="h-full rounded-full bg-gradient-to-r from-accent-400 to-iris-400 transition-[width] duration-300"
                  style={{ width: `${(progress.done / progress.total) * 100}%` }}
                />
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
              <h2 className="flex items-center gap-1.5 text-[0.62rem] font-semibold uppercase tracking-[0.18em] text-zinc-500">
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
                <div className="mt-1 flex items-center gap-1.5 text-[0.6rem] uppercase tracking-[0.16em] text-zinc-600">
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
