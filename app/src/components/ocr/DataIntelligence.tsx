import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  api,
  type BatchExtraction,
  type DocumentKind,
  type Extraction,
  type OrganizedDocument,
  type PageResult,
} from "../../api";
import {
  DOCUMENT_KINDS,
  applyFieldEdit,
  batchAsTsv,
  batchReviewCount,
  confidencePercent,
  confidenceTone,
  fieldLabel,
  fieldsAsTsv,
  kindLabel,
  languageLabel,
  needsReviewCount,
  subjectForTab,
  warningCodes,
  warningLabel,
} from "../../organizer-view";
import { Icon } from "../Icons";

type Tab = "fields" | "batch" | "organized" | "json" | "csv" | "markdown";
type Format = "txt" | "csv" | "json" | "md" | "html";
type Profile = DocumentKind | "auto";

const TABS: Array<{ id: Tab; label: string; labelAr: string }> = [
  { id: "fields", label: "Fields", labelAr: "الحقول" },
  { id: "batch", label: "Batch", labelAr: "الدفعة" },
  { id: "organized", label: "Organized", labelAr: "المنظّم" },
  { id: "json", label: "JSON", labelAr: "JSON" },
  { id: "csv", label: "CSV", labelAr: "CSV" },
  { id: "markdown", label: "Markdown", labelAr: "Markdown" },
];

const FORMATS: Array<{ id: Format; label: string }> = [
  { id: "txt", label: "TXT" },
  { id: "csv", label: "CSV" },
  { id: "json", label: "JSON" },
  { id: "md", label: "MD" },
  { id: "html", label: "HTML" },
];

const TONE: Record<string, string> = {
  high: "border-emerald-400/25 bg-emerald-400/10 text-emerald-200",
  medium: "border-amber-400/25 bg-amber-400/10 text-amber-200",
  low: "border-red-400/25 bg-red-400/10 text-red-200",
};

const PREVIEW_FORMAT: Record<Exclude<Tab, "fields" | "batch">, Format> = {
  organized: "txt",
  json: "json",
  csv: "csv",
  markdown: "md",
};

function confidenceChip(value: number, rtl: boolean) {
  const percent = confidencePercent(value);
  return (
    <span
      className={`inline-flex shrink-0 items-center rounded-md border px-1.5 py-px font-mono text-[0.58rem] tabular-nums ${TONE[confidenceTone(value)]}`}
      title={
        rtl ? `ثقة الاستخراج ${percent}%` : `Extraction confidence ${percent}%`
      }
    >
      {percent}%
    </span>
  );
}

export default function DataIntelligence({
  pages,
  rtl,
  onClose,
}: {
  pages: PageResult[];
  rtl: boolean;
  onClose: () => void;
}) {
  const t = (en: string, ar: string) => (rtl ? ar : en);
  const [profile, setProfile] = useState<Profile>("auto");
  const [extraction, setExtraction] = useState<Extraction | null>(null);
  const [batch, setBatch] = useState<BatchExtraction | null>(null);
  const [organized, setOrganized] = useState<OrganizedDocument | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("fields");
  const [preview, setPreview] = useState<string | null>(null);
  const [previewBusy, setPreviewBusy] = useState(false);
  const [exporting, setExporting] = useState<Format | null>(null);
  const [showSource, setShowSource] = useState(false);
  const [history, setHistory] = useState<Extraction[]>([]);

  const panelRef = useRef<HTMLDivElement | null>(null);
  const tabRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const requestRef = useRef(0);

  // One analysis pass feeds every tab. The three calls run in parallel and are
  // pure Rust work on pages already in memory: nothing is written, nothing
  // leaves the machine.
  const analyze = useCallback(
    async (chosen: Profile) => {
      const ticket = ++requestRef.current;
      setBusy(true);
      setError(null);
      setNotice(null);
      try {
        const override = chosen === "auto" ? null : chosen;
        const [nextExtraction, nextBatch, nextOrganized] = await Promise.all([
          api.extractDocument(pages, override),
          api.extractDocumentBatch(pages, override),
          api.organizePages(pages),
        ]);
        if (requestRef.current !== ticket) return;
        setExtraction(nextExtraction);
        setBatch(nextBatch);
        setOrganized(nextOrganized);
        setHistory([]);
      } catch (failure) {
        if (requestRef.current !== ticket) return;
        setError(String(failure));
      } finally {
        if (requestRef.current === ticket) setBusy(false);
      }
    },
    [pages],
  );

  useEffect(() => {
    void analyze(profile);
  }, [analyze, profile]);

  // Focus lands inside the panel when it opens, so a keyboard user is not left
  // tabbing through the studio behind it. `preventScroll` keeps the overlay's
  // own position untouched in a short window.
  useEffect(() => {
    panelRef.current?.focus({ preventScroll: true });
  }, []);

  const subject = subjectForTab(tab);
  const activeWarnings = useMemo(() => {
    if (subject === "batch") return batch?.warnings ?? [];
    if (subject === "organized") return organized?.warnings ?? [];
    return extraction?.warnings ?? [];
  }, [subject, batch, organized, extraction]);

  // Every non-table tab shows the exact body the exporter would write, rendered
  // by the same Rust function. A keystroke edit re-renders after a short pause
  // instead of firing a call per character.
  useEffect(() => {
    if (tab === "fields" || tab === "batch") {
      setPreview(null);
      return;
    }
    if (!extraction || !batch || !organized) return;

    type Payload = Parameters<typeof api.previewExport>[0];
    const payload: Payload =
      subject === "batch"
        ? { kind: "batch", batch }
        : subject === "organized"
          ? { kind: "organized", document: organized }
          : { kind: "extraction", extraction };
    const format = PREVIEW_FORMAT[tab];

    let cancelled = false;
    const timer = setTimeout(() => {
      setPreviewBusy(true);
      api
        .previewExport(payload, format)
        .then((body) => {
          if (!cancelled) setPreview(body);
        })
        .catch((failure) => {
          if (!cancelled) {
            setPreview(null);
            setError(String(failure));
          }
        })
        .finally(() => {
          if (!cancelled) setPreviewBusy(false);
        });
    }, 180);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [tab, subject, extraction, batch, organized]);

  const sourceText = useMemo(
    () =>
      pages
        .map((page) => page.text)
        .filter((text) => text.trim() !== "")
        .join("\n\n"),
    [pages],
  );

  const editField = (index: number, value: string) => {
    if (!extraction) return;
    setHistory((stack) => [...stack.slice(-49), extraction]);
    setExtraction(applyFieldEdit(extraction, index, value));
  };

  const undo = () => {
    const previous = history[history.length - 1];
    if (!previous) return;
    setHistory(history.slice(0, -1));
    setExtraction(previous);
  };

  const copy = async () => {
    const body =
      tab === "fields" && extraction
        ? fieldsAsTsv(extraction, rtl)
        : tab === "batch" && batch
          ? batchAsTsv(batch, rtl)
          : tab === "organized" && showSource
            ? sourceText
            : preview;
    if (!body) return;
    try {
      await navigator.clipboard.writeText(body);
      setNotice(t("Copied to the clipboard.", "نُسخ إلى الحافظة."));
    } catch {
      setError(t("Copy failed.", "تعذّر النسخ."));
    }
  };

  const exportAs = async (format: Format) => {
    if (!extraction || !batch || !organized) return;
    setExporting(format);
    setError(null);
    setNotice(null);
    const title =
      format === "html"
        ? t("Export HTML", "تصدير HTML")
        : t("Export data", "تصدير البيانات");
    const filterName = t("Data file", "ملف بيانات");
    try {
      const target =
        subject === "batch"
          ? await api.exportBatch(
              batch,
              format,
              `zeidex-batch.${format === "md" ? "md" : format}`,
              title,
              filterName,
            )
          : subject === "organized"
            ? await api.exportOrganized(
                organized,
                format,
                `zeidex-organized.${format === "md" ? "md" : format}`,
                title,
                filterName,
              )
            : await api.exportExtraction(
                extraction,
                format,
                `zeidex-fields.${format === "md" ? "md" : format}`,
                title,
                filterName,
              );
      if (target) setNotice(`${t("Saved", "حُفظ")}: ${target}`);
    } catch (failure) {
      setError(String(failure));
    } finally {
      setExporting(null);
    }
  };

  const selectTab = (next: Tab) => {
    setTab(next);
    setShowSource(false);
    const index = TABS.findIndex((entry) => entry.id === next);
    tabRefs.current[index]?.focus();
  };

  const onTabKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    const index = TABS.findIndex((entry) => entry.id === tab);
    const forward = rtl ? -1 : 1;
    let next = index;
    if (event.key === "ArrowRight") next = (index + forward + TABS.length) % TABS.length;
    else if (event.key === "ArrowLeft") next = (index - forward + TABS.length) % TABS.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = TABS.length - 1;
    else return;
    event.preventDefault();
    selectTab(TABS[next].id);
  };

  const warnings = warningCodes(activeWarnings);
  const reviewItems =
    subject === "batch" && batch
      ? batchReviewCount(batch)
      : subject === "extraction" && extraction
        ? needsReviewCount(extraction)
        : 0;
  const kind = extraction?.kind ?? batch?.kind ?? "unknown";
  const confidence = subject === "batch" && batch ? batch.confidence : extraction?.confidence ?? 0;
  const language = extraction?.language ?? batch?.language ?? "unknown";

  return (
    <div
      ref={panelRef}
      tabIndex={-1}
      role="region"
      aria-label={t("Data intelligence", "ذكاء البيانات")}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.stopPropagation();
          onClose();
        }
      }}
      className="absolute inset-0 z-30 flex min-h-0 flex-col overflow-hidden rounded-[inherit] bg-ink-950 outline-none"
    >
      <header className="flex flex-wrap items-center gap-2 border-b border-white/[0.07] px-4 py-2.5">
        <span className="grid h-7 w-7 shrink-0 place-items-center rounded-lg bg-gradient-to-br from-accent-400/15 to-iris-400/15 text-accent-300 ring-1 ring-white/10">
          <Icon name="sparkle" size={15} />
        </span>
        <div className="min-w-0 leading-tight">
          <h2 className="truncate text-[0.82rem] font-semibold text-white">
            {t("Data intelligence", "ذكاء البيانات")}
          </h2>
          <p className="truncate text-[0.56rem] micro-label tracking-[0.16em] text-zinc-500">
            {t("OCR organizer · local", "منظّم OCR · محلي")}
          </p>
        </div>

        <span className="flex min-w-0 items-center gap-1.5 rounded-full border border-white/10 bg-white/[0.03] px-2 py-1">
          <span className="truncate text-[0.66rem] font-medium text-zinc-200">
            {kindLabel(kind, rtl)}
          </span>
          {confidenceChip(confidence, rtl)}
        </span>
        <span className="hidden rounded-full border border-white/[0.07] px-2 py-0.5 text-[0.58rem] text-zinc-500 sm:inline">
          {languageLabel(language, rtl)}
        </span>

        <button
          type="button"
          onClick={onClose}
          aria-label={t("Close data intelligence", "إغلاق ذكاء البيانات")}
          className="ms-auto grid h-7 w-7 place-items-center rounded-full border border-white/10 text-zinc-400 transition hover:bg-white/10 hover:text-white"
        >
          <Icon name="x" size={13} />
        </button>
      </header>

      <div className="flex flex-wrap items-center gap-1.5 px-4 py-2">
        <label className="flex items-center gap-1.5 text-[0.62rem] text-zinc-500">
          <span>{t("Profile", "النوع")}</span>
          <select
            value={profile}
            onChange={(event) => setProfile(event.target.value as Profile)}
            disabled={busy}
            className="max-w-[11rem] rounded-md border border-white/10 bg-ink-850 px-2 py-1 text-[0.68rem] text-zinc-200 outline-none transition focus:border-accent-400/50 disabled:opacity-40"
          >
            <option value="auto">{t("Detect automatically", "كشف تلقائي")}</option>
            {DOCUMENT_KINDS.map((meta) => (
              <option key={meta.id} value={meta.id}>
                {rtl ? meta.labelAr : meta.label}
              </option>
            ))}
          </select>
        </label>

        <button
          type="button"
          onClick={() => void analyze(profile)}
          disabled={busy}
          className="flex items-center gap-1.5 rounded-full border border-white/10 px-2.5 py-1 text-[0.66rem] text-zinc-300 transition hover:bg-white/10 hover:text-white disabled:opacity-40"
        >
          <Icon name="refresh" size={12} className={busy ? "animate-spin" : ""} />
          {busy ? t("Analyzing…", "يحلّل…") : t("Re-organize", "إعادة التنظيم")}
        </button>

        {history.length > 0 && (
          <button
            type="button"
            onClick={undo}
            className="flex items-center gap-1.5 rounded-full border border-white/10 px-2.5 py-1 text-[0.66rem] text-zinc-300 transition hover:bg-white/10 hover:text-white"
          >
            <Icon name="enter" size={12} className="rotate-180" />
            {t("Undo edit", "تراجع")}
          </button>
        )}

        {extraction && (
          <span className="ms-auto font-mono text-[0.58rem] text-zinc-600">
            {t("schema", "مخطط")} {extraction.schema}
          </span>
        )}
      </div>

      {error && (
        <div
          role="alert"
          className="mx-4 mb-2 flex items-center gap-2 rounded-lg border border-red-400/20 bg-red-400/[0.07] px-3 py-1.5 text-[0.68rem] text-red-200"
        >
          <Icon name="alert" size={12} />
          <span className="truncate">{error}</span>
        </div>
      )}
      {notice && (
        <div
          role="status"
          className="mx-4 mb-2 flex items-center gap-2 rounded-lg border border-emerald-400/20 bg-emerald-400/[0.07] px-3 py-1.5 text-[0.68rem] text-emerald-200"
        >
          <Icon name="check" size={12} />
          <span className="truncate">{notice}</span>
        </div>
      )}

      <div
        role="tablist"
        aria-label={t("Data views", "عروض البيانات")}
        onKeyDown={onTabKeyDown}
        className="flex shrink-0 gap-0.5 overflow-x-auto border-b border-white/[0.07] px-3"
      >
        {TABS.map((entry, index) => (
          <button
            key={entry.id}
            ref={(node) => {
              tabRefs.current[index] = node;
            }}
            type="button"
            role="tab"
            id={`di-tab-${entry.id}`}
            aria-selected={tab === entry.id}
            aria-controls="di-panel"
            tabIndex={tab === entry.id ? 0 : -1}
            onClick={() => selectTab(entry.id)}
            className={`shrink-0 border-b-2 px-2.5 py-2 text-[0.68rem] font-medium transition ${
              tab === entry.id
                ? "border-accent-400 text-accent-200"
                : "border-transparent text-zinc-500 hover:text-zinc-200"
            }`}
          >
            {rtl ? entry.labelAr : entry.label}
          </button>
        ))}
      </div>

      {warnings.length > 0 && (
        <div
          role="status"
          className="flex flex-wrap items-center gap-1.5 border-b border-amber-400/10 bg-amber-400/[0.04] px-4 py-1.5"
        >
          <Icon name="alert" size={11} className="text-amber-300" />
          {warnings.map((code) => {
            const count = activeWarnings.filter((warning) => warning.code === code).length;
            const detail = activeWarnings
              .filter((warning) => warning.code === code)
              .map((warning) => warning.message)
              .join("\n");
            return (
              <span
                key={code}
                title={detail}
                className="rounded-md border border-amber-400/20 px-1.5 py-px text-[0.6rem] text-amber-200"
              >
                {warningLabel(code, rtl)}
                {count > 1 ? ` ×${count}` : ""}
              </span>
            );
          })}
        </div>
      )}

      <div
        role="tabpanel"
        id="di-panel"
        aria-labelledby={`di-tab-${tab}`}
        className="flex min-h-0 flex-1 flex-col"
      >
        {busy && !extraction ? (
          <div className="grid flex-1 place-items-center">
            <span className="flex items-center gap-2 text-xs text-zinc-500">
              <Icon name="refresh" size={14} className="animate-spin" />
              {t("Reading the recognized pages…", "يقرأ الصفحات المستخرجة…")}
            </span>
          </div>
        ) : tab === "fields" ? (
          <div className="min-h-0 flex-1 overflow-auto">
            {pages.length > 1 && (
              <div className="flex flex-wrap items-center gap-2 border-b border-white/[0.05] px-4 py-1.5 text-[0.62rem] text-zinc-500">
                <span>
                  {t(
                    "Several pages are open: the batch table gives each page its own row.",
                    "عدة صفحات مفتوحة: جدول الدفعة يعطي كل صفحة صفًا مستقلًا.",
                  )}
                </span>
                <button
                  type="button"
                  onClick={() => selectTab("batch")}
                  className="rounded-full border border-accent-400/25 px-2 py-0.5 text-accent-200 transition hover:bg-accent-400/10"
                >
                  {t("Open batch", "افتح الدفعة")}
                </button>
              </div>
            )}
            {!extraction || extraction.fields.length === 0 ? (
              <div className="grid h-full place-items-center px-6 text-center">
                <div className="max-w-[26rem]">
                  <Icon name="text" size={20} className="mx-auto text-zinc-600" />
                  <p className="mt-2 text-xs font-medium text-zinc-300">
                    {t("No fields for this profile", "لا حقول لهذا النوع")}
                  </p>
                  <p className="mt-1 text-[0.68rem] leading-relaxed text-zinc-600">
                    {t(
                      "Pick another document type above, or check the recognized text. Nothing is invented to fill the table.",
                      "اختر نوعًا آخر أعلاه، أو راجع النص المستخرج. لا يُخترع شيء لملء الجدول.",
                    )}
                  </p>
                </div>
              </div>
            ) : (
              <table className="w-full border-collapse text-[0.72rem]">
                <thead className="sticky top-0 z-10 bg-ink-900">
                  <tr className="text-start text-[0.58rem] micro-label tracking-[0.14em] text-zinc-500">
                    <th scope="col" className="px-4 py-1.5 text-start font-medium">
                      {t("Field", "الحقل")}
                    </th>
                    <th scope="col" className="px-2 py-1.5 text-start font-medium">
                      {t("Value", "القيمة")}
                    </th>
                    <th scope="col" className="px-4 py-1.5 text-end font-medium">
                      {t("Confidence", "الثقة")}
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {extraction.fields.map((field, index) => (
                    <tr
                      key={`${field.key}-${index}`}
                      className="border-t border-white/[0.05] align-top"
                    >
                      <th
                        scope="row"
                        className="px-4 py-1.5 text-start font-normal text-zinc-400"
                      >
                        <span className="flex items-center gap-1.5">
                          {field.needs_review && (
                            <span
                              title={t("Needs review", "يحتاج مراجعة")}
                              className="h-1.5 w-1.5 shrink-0 rounded-full bg-amber-400"
                            />
                          )}
                          <span className="truncate">{fieldLabel(field, rtl)}</span>
                        </span>
                        <span className="mt-0.5 block font-mono text-[0.52rem] text-zinc-600">
                          {field.key}
                        </span>
                      </th>
                      <td className="px-2 py-1">
                        <input
                          value={field.value}
                          onChange={(event) => editField(index, event.target.value)}
                          dir="auto"
                          aria-label={fieldLabel(field, rtl)}
                          placeholder="—"
                          className="w-full rounded-md border border-transparent bg-transparent px-1.5 py-1 text-[0.74rem] text-zinc-100 outline-none transition hover:border-white/10 focus:border-accent-400/50 focus:bg-ink-900/60"
                        />
                      </td>
                      <td className="px-4 py-1.5 text-end">
                        <span className="inline-flex items-center gap-1">
                          {field.needs_review && (
                            <span className="text-[0.55rem] text-amber-300">
                              {t("review", "راجع")}
                            </span>
                          )}
                          {confidenceChip(field.confidence, rtl)}
                        </span>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        ) : tab === "batch" ? (
          <div className="min-h-0 flex-1 overflow-auto">
            {!batch || batch.rows.length === 0 ? (
              <div className="grid h-full place-items-center px-6 text-center text-xs text-zinc-500">
                {t("No pages to tabulate.", "لا صفحات لجدولتها.")}
              </div>
            ) : (
              <>
                <p className="px-4 py-1.5 text-[0.62rem] text-zinc-500">
                  {t(
                    "One row per page, in page order. A page with no fields keeps its row blank.",
                    "صف واحد لكل صفحة بترتيب الصفحات. الصفحة بلا حقول يبقى صفها فارغًا.",
                  )}
                </p>
                <table className="w-full border-collapse text-[0.72rem]">
                  <thead className="sticky top-0 z-10 bg-ink-900">
                    <tr className="text-[0.58rem] micro-label tracking-[0.14em] text-zinc-500">
                      <th scope="col" className="px-3 py-1.5 text-start font-medium">
                        {t("Page", "الصفحة")}
                      </th>
                      {batch.columns.map((column) => (
                        <th
                          key={column.key}
                          scope="col"
                          className="whitespace-nowrap px-3 py-1.5 text-start font-medium"
                          title={column.key}
                        >
                          {rtl && column.label_ar ? column.label_ar : column.label}
                        </th>
                      ))}
                      <th scope="col" className="px-3 py-1.5 text-end font-medium">
                        {t("Confidence", "الثقة")}
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {batch.rows.map((row) => (
                      <tr
                        key={row.page}
                        className={`border-t border-white/[0.05] ${
                          row.needs_review ? "bg-amber-400/[0.03]" : ""
                        }`}
                      >
                        <th
                          scope="row"
                          className="px-3 py-1.5 text-start font-mono text-[0.62rem] text-zinc-500"
                        >
                          {row.page + 1}
                        </th>
                        {row.values.map((value, index) => (
                          <td
                            key={index}
                            dir="auto"
                            className="whitespace-nowrap px-3 py-1.5 text-zinc-200"
                          >
                            {value.trim() === "" ? (
                              <span className="text-zinc-600">—</span>
                            ) : (
                              value
                            )}
                          </td>
                        ))}
                        <td className="px-3 py-1.5 text-end">
                          {confidenceChip(row.confidence, rtl)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </>
            )}
          </div>
        ) : tab === "organized" && showSource ? (
          <div className="flex min-h-0 flex-1 flex-col">
            <div className="flex items-center gap-2 border-b border-white/[0.05] px-4 py-1.5">
              <span className="text-[0.62rem] text-zinc-500">
                {t("Recognized text, untouched", "النص المستخرج كما هو")}
              </span>
              <button
                type="button"
                onClick={() => setShowSource(false)}
                className="ms-auto rounded-full border border-white/10 px-2 py-0.5 text-[0.6rem] text-zinc-300 transition hover:bg-white/10"
              >
                {t("Show organized", "عرض المنظّم")}
              </button>
            </div>
            <pre
              dir="auto"
              className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap px-4 py-3 font-sans text-[0.72rem] leading-relaxed text-zinc-300"
            >
              {sourceText || t("No recognized text.", "لا نص مستخرج.")}
            </pre>
          </div>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col">
            {tab === "organized" && (
              <div className="flex items-center gap-2 border-b border-white/[0.05] px-4 py-1.5">
                <span className="text-[0.62rem] text-zinc-500">
                  {t("Organized by rules, not rewritten", "منظّم بقواعد، بلا إعادة كتابة")}
                </span>
                <button
                  type="button"
                  onClick={() => setShowSource(true)}
                  className="ms-auto rounded-full border border-white/10 px-2 py-0.5 text-[0.6rem] text-zinc-300 transition hover:bg-white/10"
                >
                  {t("Show source", "عرض الأصل")}
                </button>
              </div>
            )}
            {previewBusy && preview === null ? (
              <div className="grid flex-1 place-items-center text-xs text-zinc-500">
                <span className="flex items-center gap-2">
                  <Icon name="refresh" size={13} className="animate-spin" />
                  {t("Rendering…", "يُجهّز…")}
                </span>
              </div>
            ) : (
              <pre
                dir={tab === "json" || tab === "csv" ? "ltr" : "auto"}
                className={`min-h-0 flex-1 overflow-auto whitespace-pre-wrap px-4 py-3 text-[0.7rem] leading-relaxed text-zinc-300 ${
                  tab === "json" || tab === "csv" ? "font-mono text-left" : "font-sans"
                }`}
              >
                {preview ?? t("Nothing to preview.", "لا شيء للمعاينة.")}
              </pre>
            )}
          </div>
        )}
      </div>

      <footer className="flex flex-wrap items-center gap-1.5 border-t border-white/[0.07] bg-black/20 px-4 py-2 text-[0.62rem] text-zinc-500">
        <span className="flex items-center gap-1.5">
          <Icon name="export" size={11} />
          {subject === "batch"
            ? t("Export batch", "تصدير الدفعة")
            : subject === "organized"
              ? t("Export organized", "تصدير المنظّم")
              : t("Export fields", "تصدير الحقول")}
        </span>
        {FORMATS.map((format) => (
          <button
            key={format.id}
            type="button"
            onClick={() => void exportAs(format.id)}
            disabled={busy || exporting !== null || !extraction}
            className="rounded-full border border-white/10 px-2 py-0.5 text-zinc-300 transition hover:border-accent-400/30 hover:bg-accent-400/10 hover:text-accent-200 disabled:opacity-40"
          >
            {exporting === format.id ? "…" : format.label}
          </button>
        ))}
        <button
          type="button"
          onClick={() => void copy()}
          disabled={busy}
          className="flex items-center gap-1 rounded-full border border-white/10 px-2 py-0.5 text-zinc-300 transition hover:bg-white/10 hover:text-white disabled:opacity-40"
        >
          <Icon name="clipboard" size={11} />
          {t("Copy", "نسخ")}
        </button>
        <span className="ms-auto flex items-center gap-1.5">
          {reviewItems > 0 ? (
            <span className="flex items-center gap-1 text-amber-300">
              <span className="h-1.5 w-1.5 rounded-full bg-amber-400" />
              {t(`${reviewItems} to review`, `${reviewItems} للمراجعة`)}
            </span>
          ) : (
            <span className="flex items-center gap-1 text-emerald-300">
              <Icon name="check" size={10} />
              {t("Nothing flagged", "لا شيء معلّم")}
            </span>
          )}
        </span>
      </footer>
    </div>
  );
}
