import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type BatchEntryResult, type BatchReport, type ParsedList } from "../api";
import {
  allPaths,
  bestMatch,
  bestScore,
  filterEntries,
  folderOf,
  formatBytes,
  matchKindLabel,
  refineEntries,
  sortEntries,
  statusLabel,
  thresholdLabel,
  type BatchFilter,
  type BatchSort,
} from "../batch-view";
import { BrandMark } from "./Brand";
import { Icon } from "./Icons";

/** How many entries are rendered at once. A 2,000-name paste must not put
 *  2,000 rows in the DOM before the user has read the first one. */
const PAGE = 150;

/** One template for the header and every row, so the columns line up. */
const GRID =
  "grid grid-cols-[22px_minmax(0,1.05fr)_96px_minmax(0,1.15fr)_52px_76px_70px_112px] items-center gap-2";

const FILTERS: { id: BatchFilter; label: string; rtl: string }[] = [
  { id: "all", label: "All", rtl: "الكل" },
  { id: "found", label: "Found", rtl: "موجود" },
  { id: "missing", label: "Missing", rtl: "مفقود" },
  { id: "exact", label: "Exact", rtl: "تام" },
  { id: "approximate", label: "Approximate", rtl: "تقريبي" },
  { id: "multiple", label: "Multiple", rtl: "متعدد" },
  { id: "error", label: "Errors", rtl: "أخطاء" },
];

const SORTS: { id: BatchSort; label: string; rtl: string }[] = [
  { id: "input", label: "Input order", rtl: "ترتيب الإدخال" },
  { id: "status", label: "Status", rtl: "الحالة" },
  { id: "score", label: "Match %", rtl: "نسبة التطابق" },
  { id: "name", label: "Name", rtl: "الاسم" },
  { id: "size", label: "Size", rtl: "الحجم" },
  { id: "type", label: "File type", rtl: "نوع الملف" },
  { id: "path", label: "Path", rtl: "المسار" },
];

type Message = { kind: "notice" | "error"; text: string } | null;

export default function BatchSearch({ rtl, onBack }: { rtl: boolean; onBack: () => void }) {
  const [text, setText] = useState("");
  const [parsed, setParsed] = useState<ParsedList | null>(null);
  const [threshold, setThreshold] = useState(70);
  const [report, setReport] = useState<BatchReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [message, setMessage] = useState<Message>(null);
  const [filter, setFilter] = useState<BatchFilter>("all");
  const [sort, setSort] = useState<BatchSort>("input");
  /** A second, display-only floor: the search threshold is what matched. */
  const [minScore, setMinScore] = useState(0);
  const [extension, setExtension] = useState("");
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  /** Requested names the user has cleared from the table without re-running. */
  const [hidden, setHidden] = useState<ReadonlySet<string>>(new Set());
  const [limit, setLimit] = useState(PAGE);
  const textarea = useRef<HTMLTextAreaElement | null>(null);

  const t = useCallback(
    (en: string, ar: string) => (rtl ? ar : en),
    [rtl],
  );

  // The review count is computed by the same parser the search uses, so what
  // the user sees before pressing the button is what will be searched. Debounced
  // because it runs on every keystroke of a long paste.
  useEffect(() => {
    let active = true;
    const handle = window.setTimeout(() => {
      api
        .batchParse(text)
        .then((next) => {
          if (active) setParsed(next);
        })
        .catch((err: unknown) => {
          if (active) setMessage({ kind: "error", text: String(err) });
        });
    }, 160);
    return () => {
      active = false;
      window.clearTimeout(handle);
    };
  }, [text]);

  useEffect(() => {
    setLimit(PAGE);
  }, [report, filter, sort]);

  const entries = parsed?.entries ?? [];
  const visible = useMemo(() => {
    if (!report) return [];
    return sortEntries(
      refineEntries(
        filterEntries(report.entries, filter).filter((entry) => !hidden.has(entry.requested)),
        minScore,
        extension,
      ),
      sort,
    );
  }, [report, filter, sort, hidden, minScore, extension]);

  /** Every extension present in the report, for the type filter. */
  const extensions = useMemo(() => {
    if (!report) return [];
    const found = new Set<string>();
    for (const entry of report.entries) {
      for (const match of entry.matches) {
        const ext = match.extension?.toLowerCase();
        if (ext) found.add(ext);
      }
    }
    return [...found].sort();
  }, [report]);

  const run = useCallback(async () => {
    if (busy || entries.length === 0) return;
    setBusy(true);
    setMessage(null);
    try {
      const next = await api.batchSearch(entries, threshold);
      setReport(next);
      setExpanded(new Set());
      setSelected(new Set());
      setHidden(new Set());
      setFilter("all");
      setMinScore(0);
      setExtension("");
      setMessage(
        next.summary.found === next.summary.total
          ? { kind: "notice", text: t("Every requested name was found.", "تم العثور على كل الأسماء المطلوبة.") }
          : null,
      );
    } catch (err) {
      setMessage({ kind: "error", text: String(err) });
    } finally {
      setBusy(false);
    }
  }, [busy, entries, threshold, t]);

  const importList = useCallback(async () => {
    try {
      const next = await api.batchImport(
        t("Import a list of file names", "استيراد قائمة أسماء ملفات"),
        t("Text list", "قائمة نصية"),
      );
      if (!next) return;
      // Going through the textarea keeps one source of truth: the box, the
      // review count and the search all show the same list.
      setText(next.entries.join("\n"));
      setMessage({
        kind: "notice",
        text: t(`Imported ${next.entries.length} names.`, `تم استيراد ${next.entries.length} اسماً.`),
      });
    } catch (err) {
      setMessage({ kind: "error", text: String(err) });
    }
  }, [t]);

  const pasteClipboard = useCallback(async () => {
    try {
      const clip = await navigator.clipboard.readText();
      if (!clip.trim()) {
        setMessage({ kind: "notice", text: t("The clipboard is empty.", "الحافظة فارغة.") });
        return;
      }
      setText((current) => (current.trim() ? `${current.trimEnd()}\n${clip}` : clip));
    } catch {
      // Reading the clipboard needs a user gesture and a permission that some
      // webviews refuse; typing the shortcut always works.
      textarea.current?.focus();
      setMessage({
        kind: "notice",
        text: t("Press Ctrl+V inside the box to paste.", "اضغط Ctrl+V داخل المربع للصق."),
      });
    }
  }, [t]);

  const exportReport = useCallback(async () => {
    if (!report || exporting) return;
    setExporting(true);
    try {
      const path = await api.batchExport(
        report,
        t("Save the batch report", "حفظ تقرير الدفعة"),
        t("Excel workbook", "مصنف Excel"),
      );
      if (!path) return;
      setMessage({ kind: "notice", text: t(`Report saved to ${path}`, `حُفظ التقرير في ${path}`) });
    } catch (err) {
      setMessage({ kind: "error", text: String(err) });
    } finally {
      setExporting(false);
    }
  }, [report, exporting, t]);

  const copy = useCallback(
    async (value: string, label: string) => {
      try {
        await navigator.clipboard.writeText(value);
        setMessage({ kind: "notice", text: label });
      } catch {
        setMessage({ kind: "error", text: t("Could not write to the clipboard.", "تعذّرت الكتابة إلى الحافظة.") });
      }
    },
    [t],
  );

  const toggleExpanded = (requested: string) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(requested)) next.delete(requested);
      else next.add(requested);
      return next;
    });
  };

  const toggleSelected = (path: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  /** Hides the entries that own any of these paths, and clears the selection. */
  const hidePaths = useCallback(
    (paths: ReadonlySet<string>) => {
      if (!report) return;
      const names = report.entries
        .filter((entry) => entry.matches.some((match) => paths.has(match.path)))
        .map((entry) => entry.requested);
      setHidden((current) => {
        const next = new Set(current);
        for (const name of names) next.add(name);
        return next;
      });
      setSelected(new Set());
    },
    [report],
  );

  const openPaths = useCallback(async (paths: string[]) => {
    for (const path of paths) {
      try {
        await api.openTarget(path);
      } catch (err) {
        setMessage({ kind: "error", text: String(err) });
        return;
      }
    }
  }, []);

  /** Filenames (not paths) of every selected match, for pasting into a mail. */
  const selectedNames = useMemo(() => {
    if (!report) return [];
    return report.entries
      .flatMap((entry) => entry.matches)
      .filter((match) => selected.has(match.path))
      .map((match) => match.name);
  }, [report, selected]);

  const summary = report?.summary;
  const progress = summary && summary.total > 0 ? summary.success_percent : 0;

  return (
    <div className="flex h-full items-center justify-center px-3 py-3">
      <div className="panel flex h-[94vh] w-[min(1460px,97vw)] flex-col">
        <header className="relative flex items-center gap-3 px-5 pb-3 pt-4" data-tauri-drag-region>
          <span className="pointer-events-none absolute inset-x-8 top-0 h-px bg-gradient-to-r from-transparent via-accent-400/40 to-transparent" />
          <button
            type="button"
            onClick={onBack}
            aria-label={t("Back", "رجوع")}
            className="grid h-8 w-8 place-items-center rounded-full border border-white/10 text-zinc-400 transition hover:bg-white/10 hover:text-white"
          >
            <Icon name="chevron" size={15} className={rtl ? "" : "rotate-180"} />
          </button>
          <BrandMark size={26} />
          <div className="flex flex-col leading-none">
            <span className="text-[0.9rem] font-semibold tracking-[-0.01em] text-white">
              {t("Batch Search", "البحث الدفعي")}
            </span>
            <span className="mt-1 text-[0.5rem] font-medium uppercase tracking-[0.3em] text-zinc-500">
              {t("Find many names at once", "ابحث عن أسماء متعددة دفعة واحدة")}
            </span>
          </div>
          {report && (
            <span className="ms-2 rounded-full border border-white/10 bg-white/[0.03] px-2.5 py-1 text-[0.625rem] text-zinc-400">
              {t(
                `${report.indexed_files.toLocaleString()} indexed files`,
                `${report.indexed_files.toLocaleString()} ملف مفهرس`,
              )}
            </span>
          )}

          <div className="ms-auto flex items-center gap-1.5">
            <button
              type="button"
              onClick={() => void exportReport()}
              disabled={!report || exporting}
              className="flex items-center gap-1.5 rounded-full border border-white/10 bg-white/[0.03] px-3 py-1.5 text-xs font-medium text-zinc-300 transition hover:border-accent-400/30 hover:bg-accent-400/10 hover:text-accent-200 disabled:cursor-not-allowed disabled:opacity-40"
            >
              <Icon name="export" size={13} />
              {exporting ? t("Writing…", "جارٍ الكتابة…") : t("Export Excel", "تصدير Excel")}
            </button>
          </div>
        </header>

        <div className="grid min-h-0 flex-1 gap-3 px-4 pb-4 lg:grid-cols-[minmax(0,1fr)_300px]">
          {/* ------------------------------------------------ input + results */}
          <div className="flex min-h-0 flex-col gap-3">
            <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-3">
              <div className="mb-2 flex items-center gap-2">
                <span className="text-[0.7rem] font-semibold text-zinc-200">
                  {t("Names to find", "الأسماء المطلوبة")}
                </span>
                <span className="text-[0.65rem] text-zinc-500">
                  {t("one per line", "اسم في كل سطر")}
                </span>
                <div className="ms-auto flex items-center gap-1.5">
                  <button
                    type="button"
                    onClick={() => void pasteClipboard()}
                    className="flex items-center gap-1 rounded-lg border border-white/10 px-2 py-1 text-[0.65rem] text-zinc-300 transition hover:bg-white/10 hover:text-white"
                  >
                    <Icon name="clipboard" size={12} />
                    {t("Paste", "لصق")}
                  </button>
                  <button
                    type="button"
                    onClick={() => void importList()}
                    className="flex items-center gap-1 rounded-lg border border-white/10 px-2 py-1 text-[0.65rem] text-zinc-300 transition hover:bg-white/10 hover:text-white"
                  >
                    <Icon name="folder" size={12} />
                    {t("Import .txt", "استيراد .txt")}
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      setText("");
                      setReport(null);
                      setMessage(null);
                    }}
                    disabled={text.length === 0}
                    className="flex items-center gap-1 rounded-lg border border-white/10 px-2 py-1 text-[0.65rem] text-zinc-300 transition hover:bg-white/10 hover:text-white disabled:opacity-40"
                  >
                    <Icon name="trash" size={12} />
                    {t("Clear", "مسح")}
                  </button>
                </div>
              </div>

              <textarea
                ref={textarea}
                value={text}
                onChange={(event) => setText(event.target.value)}
                onKeyDown={(event) => {
                  if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
                    event.preventDefault();
                    void run();
                  }
                }}
                spellCheck={false}
                dir="auto"
                placeholder={t(
                  "report.pdf\ninvoice_2024.xlsx\nتقرير الاجتماع.docx",
                  "تقرير.pdf\nفاتورة_2024.xlsx\nmeeting-notes.docx",
                )}
                className="h-24 w-full resize-none rounded-xl border border-white/10 bg-ink-950/70 px-3 py-2 font-mono text-[0.72rem] leading-relaxed text-zinc-200 outline-none transition placeholder:text-zinc-600 focus:border-accent-400/40"
              />

              <div className="mt-2 flex flex-wrap items-center gap-3 text-[0.65rem] text-zinc-500">
                <span className="text-zinc-300">
                  {t(
                    `${entries.length.toLocaleString()} ${entries.length === 1 ? "name" : "names"} ready`,
                    // A count with a counted noun needs one of four Arabic
                    // forms; a label and a number is correct for every count
                    // and is how Arabic interfaces usually show totals.
                    `الأسماء الجاهزة: ${entries.length.toLocaleString()}`,
                  )}
                </span>
                {parsed && parsed.duplicates > 0 && (
                  <span>
                    {t(
                      `${parsed.duplicates} ${parsed.duplicates === 1 ? "duplicate" : "duplicates"} folded`,
                      `مكررات مدموجة: ${parsed.duplicates}`,
                    )}
                  </span>
                )}
                {parsed && parsed.dropped_empty > 0 && (
                  <span>
                    {t(
                      `${parsed.dropped_empty} blank ${parsed.dropped_empty === 1 ? "line" : "lines"}`,
                      `أسطر فارغة: ${parsed.dropped_empty}`,
                    )}
                  </span>
                )}
                {parsed && parsed.overflow > 0 && (
                  <span className="text-amber-300">
                    {t(`${parsed.overflow} over the limit`, `فوق الحد: ${parsed.overflow}`)}
                  </span>
                )}
              </div>
            </section>

            {/* ------------------------------------------------------ results */}
            <section className="flex min-h-0 flex-1 flex-col rounded-2xl border border-white/[0.07] bg-white/[0.02]">
              <div className="flex flex-wrap items-center gap-2 border-b border-white/[0.06] px-3 py-2">
                <div className="segment">
                  {FILTERS.map((item) => (
                    <button
                      key={item.id}
                      type="button"
                      onClick={() => setFilter(item.id)}
                      aria-pressed={filter === item.id}
                    >
                      {rtl ? item.rtl : item.label}
                    </button>
                  ))}
                </div>

                <label className="ms-auto flex items-center gap-1.5 text-[0.65rem] text-zinc-500">
                  {t("Type", "النوع")}
                  <select
                    value={extension}
                    onChange={(event) => setExtension(event.target.value)}
                    className="rounded-lg border border-white/10 bg-ink-900 px-2 py-1 text-[0.65rem] text-zinc-200 outline-none focus:border-accent-400/40"
                  >
                    <option value="">{t("All types", "كل الأنواع")}</option>
                    {extensions.map((ext) => (
                      <option key={ext} value={ext}>
                        {ext}
                      </option>
                    ))}
                  </select>
                </label>

                <label className="flex items-center gap-1.5 text-[0.65rem] text-zinc-500">
                  {t("Show", "إظهار")}
                  <select
                    value={minScore}
                    onChange={(event) => setMinScore(Number(event.target.value))}
                    className="rounded-lg border border-white/10 bg-ink-900 px-2 py-1 text-[0.65rem] text-zinc-200 outline-none focus:border-accent-400/40"
                  >
                    {[0, 50, 60, 70, 80, 90, 100].map((value) => (
                      <option key={value} value={value}>
                        {value === 0 ? t("Any %", "أي نسبة") : `${value}%+`}
                      </option>
                    ))}
                  </select>
                </label>

                <label className="flex items-center gap-1.5 text-[0.65rem] text-zinc-500">
                  {t("Sort", "الترتيب")}
                  <select
                    value={sort}
                    onChange={(event) => setSort(event.target.value as BatchSort)}
                    className="rounded-lg border border-white/10 bg-ink-900 px-2 py-1 text-[0.65rem] text-zinc-200 outline-none focus:border-accent-400/40"
                  >
                    {SORTS.map((item) => (
                      <option key={item.id} value={item.id}>
                        {rtl ? item.rtl : item.label}
                      </option>
                    ))}
                  </select>
                </label>

                {selected.size > 0 && (
                  <>
                    <button
                      type="button"
                      onClick={() =>
                        void copy(
                          [...selected].join("\n"),
                          t(`Copied ${selected.size} paths.`, `نُسخ ${selected.size} مساراً.`),
                        )
                      }
                      className="flex items-center gap-1 rounded-lg border border-accent-400/30 bg-accent-400/10 px-2 py-1 text-[0.65rem] text-accent-200"
                    >
                      <Icon name="check" size={12} />
                      {t(`Copy ${selected.size}`, `نسخ ${selected.size}`)}
                    </button>
                    <button
                      type="button"
                      onClick={() =>
                        void copy(
                          selectedNames.join("\n"),
                          t(`Copied ${selectedNames.length} names.`, `نُسخ ${selectedNames.length} اسماً.`),
                        )
                      }
                      className="flex items-center gap-1 rounded-lg border border-white/10 px-2 py-1 text-[0.65rem] text-zinc-300 transition hover:bg-white/10 hover:text-white"
                    >
                      <Icon name="text" size={12} />
                      {t("Copy names", "نسخ الأسماء")}
                    </button>
                    <button
                      type="button"
                      onClick={() => void openPaths([...selected])}
                      className="flex items-center gap-1 rounded-lg border border-white/10 px-2 py-1 text-[0.65rem] text-zinc-300 transition hover:bg-white/10 hover:text-white"
                    >
                      <Icon name="file" size={12} />
                      {t(`Open ${selected.size}`, `فتح ${selected.size}`)}
                    </button>
                    <button
                      type="button"
                      onClick={() => hidePaths(selected)}
                      className="flex items-center gap-1 rounded-lg border border-white/10 px-2 py-1 text-[0.65rem] text-zinc-300 transition hover:bg-white/10 hover:text-white"
                    >
                      <Icon name="x" size={12} />
                      {t("Hide", "إخفاء")}
                    </button>
                  </>
                )}
                {hidden.size > 0 && (
                  <button
                    type="button"
                    onClick={() => setHidden(new Set())}
                    className="flex items-center gap-1 rounded-lg border border-white/10 px-2 py-1 text-[0.65rem] text-zinc-400 transition hover:bg-white/10 hover:text-white"
                  >
                    <Icon name="refresh" size={12} />
                    {t(`Restore ${hidden.size} hidden`, `إرجاع ${hidden.size} مخفياً`)}
                  </button>
                )}
              </div>

              {message && (
                <div
                  role={message.kind === "error" ? "alert" : "status"}
                  className={`mx-3 mt-2 flex items-start gap-2 rounded-xl border px-2.5 py-1.5 text-[0.68rem] ${
                    message.kind === "error"
                      ? "border-rose-400/30 bg-rose-500/10 text-rose-200"
                      : "border-emerald-400/25 bg-emerald-500/10 text-emerald-200"
                  }`}
                >
                  <Icon name={message.kind === "error" ? "alert" : "check"} size={13} className="mt-0.5 shrink-0" />
                  <span className="min-w-0 break-words">{message.text}</span>
                  <button
                    type="button"
                    onClick={() => setMessage(null)}
                    className="ms-auto shrink-0 opacity-60 transition hover:opacity-100"
                    aria-label={t("Dismiss", "إغلاق")}
                  >
                    <Icon name="x" size={12} />
                  </button>
                </div>
              )}

              {!report ? (
                <EmptyState
                  text={t(
                    "Paste a list of names and press Search. Nothing leaves this device.",
                    "الصق قائمة أسماء واضغط بحث. لا شيء يغادر هذا الجهاز.",
                  )}
                />
              ) : visible.length === 0 ? (
                <EmptyState
                  text={t("No entry matches this filter.", "لا يوجد إدخال يطابق هذا المرشح.")}
                />
              ) : (
                <>
                  <div
                    className={`${GRID} sticky top-0 z-10 border-b border-white/[0.06] bg-ink-900/90 px-3 py-1.5 text-[0.6rem] font-medium uppercase tracking-[0.12em] text-zinc-500 backdrop-blur`}
                  >
                    <span aria-hidden="true" />
                    <span>{t("Requested", "المطلوب")}</span>
                    <span>{t("Status", "الحالة")}</span>
                    <span>{t("Best match", "أفضل تطابق")}</span>
                    <span className="text-end">%</span>
                    <span>{t("Match", "التطابق")}</span>
                    <span className="text-end">{t("Size", "الحجم")}</span>
                    <span className="text-end">{t("Actions", "إجراءات")}</span>
                  </div>

                  <div className="min-h-0 flex-1 overflow-y-auto px-3 pb-3">
                    {visible.slice(0, limit).map((entry) => (
                      <ResultRow
                        key={entry.requested}
                        entry={entry}
                        rtl={rtl}
                        expanded={expanded.has(entry.requested)}
                        selected={selected}
                        onToggleExpanded={() => toggleExpanded(entry.requested)}
                        onToggleSelected={toggleSelected}
                        onOpen={(path) => void api.openTarget(path).catch((err: unknown) => setMessage({ kind: "error", text: String(err) }))}
                        onReveal={(path) => void api.revealTarget(path).catch((err: unknown) => setMessage({ kind: "error", text: String(err) }))}
                        onCopy={(path) => void copy(path, t("Path copied.", "نُسخ المسار."))}
                        onCopyName={(name) => void copy(name, t("Name copied.", "نُسخ الاسم."))}
                        onHide={() => hidePaths(new Set([bestPathOf(entry)]))}
                      />
                    ))}
                    {visible.length > limit && (
                      <button
                        type="button"
                        onClick={() => setLimit((current) => current + PAGE)}
                        className="mt-2 w-full rounded-xl border border-white/10 py-2 text-[0.68rem] text-zinc-300 transition hover:bg-white/[0.06]"
                      >
                        {t(
                          `Show more (${visible.length - limit} left)`,
                          `عرض المزيد (بقي ${visible.length - limit})`,
                        )}
                      </button>
                    )}
                  </div>
                </>
              )}
            </section>
          </div>

          {/* ------------------------------------------------------- settings */}
          <aside className="flex min-h-0 flex-col gap-3 overflow-y-auto">
            <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-3">
              <div className="mb-1 flex items-baseline justify-between">
                <span className="text-[0.7rem] font-semibold text-zinc-200">
                  {t("Minimum match", "أدنى نسبة تطابق")}
                </span>
                <span className="font-mono text-sm font-semibold text-accent-300">{threshold}%</span>
              </div>
              <p className="mb-2 text-[0.62rem] text-zinc-500">{thresholdLabel(threshold, rtl)}</p>
              <input
                type="range"
                min={50}
                max={100}
                step={1}
                value={threshold}
                onChange={(event) => setThreshold(Number(event.target.value))}
                aria-label={t("Minimum match percentage", "أدنى نسبة تطابق")}
                className="w-full accent-accent-400"
              />
              <div className="mt-1 flex justify-between text-[0.55rem] text-zinc-600">
                <span>50%</span>
                <span>70%</span>
                <span>85%</span>
                <span>100%</span>
              </div>
              <p className="mt-2 text-[0.6rem] leading-relaxed text-zinc-500">
                {t(
                  "100% accepts only the same name — or the same name without its extension — after case, diacritic and Arabic letter-variant folding. Below that, contains, all-words and up-to-four-typo matches are added.",
                  "100% لا يقبل إلا الاسم نفسه — أو الاسم نفسه بلا امتداده — بعد توحيد حالة الأحرف والتشكيل واختلافات الحروف العربية. أقل من ذلك يُضاف التطابق الجزئي وكل الكلمات وحتى أربعة أخطاء إملائية.",
                )}
              </p>
              <button
                type="button"
                onClick={() => void run()}
                disabled={busy || entries.length === 0}
                className="mt-3 flex w-full items-center justify-center gap-2 rounded-xl border border-accent-400/30 bg-gradient-to-b from-accent-400/20 to-accent-500/10 py-2 text-xs font-semibold text-accent-100 transition hover:from-accent-400/30 hover:to-accent-500/20 disabled:cursor-not-allowed disabled:opacity-40"
              >
                <Icon name={busy ? "refresh" : "search"} size={14} className={busy ? "animate-spin" : ""} />
                {busy
                  ? t("Searching…", "جارٍ البحث…")
                  : t("Search", "بحث")}
              </button>
              <p className="mt-1.5 text-center text-[0.55rem] text-zinc-600">
                {t("Ctrl+Enter", "Ctrl+Enter")}
              </p>
            </section>

            {report && summary && (
              <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-3">
                <div className="mb-2 flex items-center justify-between">
                  <span className="text-[0.7rem] font-semibold text-zinc-200">
                    {t("Summary", "الملخص")}
                  </span>
                  <span className="font-mono text-[0.6rem] text-zinc-500">
                    {summary.duration_ms} ms
                  </span>
                </div>

                <div className="mb-2 h-1.5 overflow-hidden rounded-full bg-white/[0.06]">
                  <div
                    className="h-full rounded-full bg-gradient-to-r from-emerald-400/80 to-accent-400"
                    style={{ width: `${Math.min(100, Math.max(0, progress))}%` }}
                  />
                </div>
                <p className="mb-2 text-[0.62rem] text-zinc-500">
                  {t(
                    `${summary.success_percent}% of names found`,
                    `${summary.success_percent}% من الأسماء موجودة`,
                  )}
                </p>

                <dl className="grid grid-cols-2 gap-x-3 gap-y-1 text-[0.65rem]">
                  <Stat label={t("Found", "موجود")} value={summary.found} tone="good" />
                  <Stat label={t("Missing", "مفقود")} value={summary.not_found} tone="bad" />
                  <Stat label={t("Exact", "تام")} value={summary.exact} />
                  <Stat label={t("Approximate", "تقريبي")} value={summary.approximate} />
                  <Stat label={t("Multiple", "متعدد")} value={summary.multiple} />
                  <Stat
                    label={t("Errors", "أخطاء")}
                    value={summary.errors}
                    tone={summary.errors > 0 ? "bad" : undefined}
                  />
                  <Stat label={t("Total", "الإجمالي")} value={summary.total} />
                </dl>

                <button
                  type="button"
                  onClick={() =>
                    void copy(
                      allPaths(report.entries).join("\n"),
                      t("All paths copied.", "نُسخت كل المسارات."),
                    )
                  }
                  className="mt-3 w-full rounded-xl border border-white/10 py-1.5 text-[0.65rem] text-zinc-300 transition hover:bg-white/[0.06]"
                >
                  {t("Copy all paths", "نسخ كل المسارات")}
                </button>
              </section>
            )}

            <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-3 text-[0.6rem] leading-relaxed text-zinc-500">
              <div className="mb-1 flex items-center gap-1.5 text-[0.65rem] font-semibold text-zinc-300">
                <Icon name="lock" size={12} />
                {t("Local only", "محلي فقط")}
              </div>
              {t(
                "Names are matched against the index on this device. Nothing is uploaded, and the report is written wherever you choose.",
                "تُطابق الأسماء مع الفهرس على هذا الجهاز. لا يُرفع شيء، ويُكتب التقرير حيث تختار.",
              )}
            </section>
          </aside>
        </div>
      </div>
    </div>
  );
}

function Stat({
  label,
  value,
  tone,
}: {
  label: string;
  value: number;
  tone?: "good" | "bad";
}) {
  const color = tone === "good" ? "text-emerald-300" : tone === "bad" ? "text-rose-300" : "text-zinc-200";
  return (
    <>
      <dt className="text-zinc-500">{label}</dt>
      <dd className={`text-end font-mono tabular-nums ${color}`}>{value.toLocaleString()}</dd>
    </>
  );
}

function EmptyState({ text }: { text: string }) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-2 px-6 text-center">
      <span className="grid h-10 w-10 place-items-center rounded-full border border-white/10 text-zinc-500">
        <Icon name="search" size={18} />
      </span>
      <p className="max-w-[36ch] text-[0.7rem] leading-relaxed text-zinc-500">{text}</p>
    </div>
  );
}

/** The path the row's actions act on: its best candidate, or empty when none. */
function bestPathOf(entry: BatchEntryResult): string {
  return entry.matches[0]?.path ?? "";
}

function ResultRow({
  entry,
  rtl,
  expanded,
  selected,
  onToggleExpanded,
  onToggleSelected,
  onOpen,
  onReveal,
  onCopy,
  onCopyName,
  onHide,
}: {
  entry: BatchEntryResult;
  rtl: boolean;
  expanded: boolean;
  selected: ReadonlySet<string>;
  onToggleExpanded: () => void;
  onToggleSelected: (path: string) => void;
  onOpen: (path: string) => void;
  onReveal: (path: string) => void;
  onCopy: (path: string) => void;
  onCopyName: (name: string) => void;
  onHide: () => void;
}) {
  const best = bestMatch(entry);
  const score = bestScore(entry);
  const found = best !== null;
  const dot = found
    ? entry.status === "exact"
      ? "bg-emerald-400"
      : entry.status === "high"
        ? "bg-amber-400"
        : "bg-accent-400"
    : "bg-rose-400";
  const statusColor = found
    ? entry.status === "exact"
      ? "text-emerald-300"
      : entry.status === "high"
        ? "text-amber-300"
        : "text-accent-300"
    : "text-rose-300";

  return (
    <div className="border-b border-white/[0.05] last:border-b-0">
      <div className={`${GRID} py-1.5 text-[0.68rem]`}>
        <input
          type="checkbox"
          checked={best ? selected.has(best.path) : false}
          disabled={!best}
          onChange={() => best && onToggleSelected(best.path)}
          aria-label={rtl ? `تحديد ${entry.requested}` : `Select ${entry.requested}`}
          className="h-3 w-3 accent-accent-500 disabled:opacity-30"
        />
        <span className="flex min-w-0 items-center gap-1.5">
          <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${dot}`} />
          <span className="truncate text-zinc-100" title={entry.requested}>
            {entry.requested}
          </span>
        </span>
        <span className={`truncate ${statusColor}`}>{statusLabel(entry.status, rtl)}</span>
        <span className="min-w-0">
          {best ? (
            <span className="flex flex-col leading-tight">
              <span className="truncate text-zinc-300" title={best.path}>
                {best.name}
              </span>
              <span className="truncate text-[0.58rem] text-zinc-600" title={folderOf(best.path)}>
                {folderOf(best.path)}
              </span>
            </span>
          ) : (
            <span className="text-zinc-600">—</span>
          )}
        </span>
        <span className="text-end font-mono tabular-nums text-zinc-300">{found ? `${score}%` : "—"}</span>
        <span className="truncate text-[0.6rem] text-zinc-500">
          {best ? matchKindLabel(best.match_type, rtl) : "—"}
        </span>
        <span className="text-end font-mono text-[0.6rem] tabular-nums text-zinc-500">
          {formatBytes(best?.size_bytes ?? null)}
        </span>
        <span className="flex items-center justify-end gap-0.5">
          {best && (
            <>
              <IconButton label={rtl ? "فتح" : "Open"} icon="file" onClick={() => onOpen(best.path)} />
              <IconButton label={rtl ? "إظهار المجلد" : "Show folder"} icon="folder" onClick={() => onReveal(best.path)} />
              <IconButton label={rtl ? "نسخ المسار" : "Copy path"} icon="clipboard" onClick={() => onCopy(best.path)} />
              <IconButton label={rtl ? "نسخ الاسم" : "Copy name"} icon="text" onClick={() => onCopyName(best.name)} />
              <IconButton label={rtl ? "إخفاء من العرض" : "Remove from view"} icon="x" onClick={onHide} />
            </>
          )}
          {entry.total_matches > 1 && (
            <button
              type="button"
              onClick={onToggleExpanded}
              aria-expanded={expanded}
              aria-label={rtl ? "عرض كل التطابقات" : "Show every match"}
              className="grid h-5 w-5 place-items-center rounded text-zinc-500 transition hover:bg-white/10 hover:text-white"
            >
              <Icon
                name="chevron"
                size={12}
                className={
                  expanded
                    ? rtl
                      ? "-rotate-90"
                      : "rotate-90"
                    : rtl
                      ? "rotate-180"
                      : ""
                }
              />
            </button>
          )}
        </span>
      </div>

      {expanded &&
        entry.matches.slice(1).map((match) => (
          <div
            key={match.path}
            className={`${GRID} bg-white/[0.015] py-1 text-[0.64rem] text-zinc-400`}
          >
            <input
              type="checkbox"
              checked={selected.has(match.path)}
              onChange={() => onToggleSelected(match.path)}
              aria-label={rtl ? `تحديد ${match.name}` : `Select ${match.name}`}
              className="h-3 w-3 accent-accent-500"
            />
            <span className="truncate ps-3 text-[0.58rem] text-zinc-600">
              {rtl ? "بديل" : "candidate"}
            </span>
            <span />
            <span className="min-w-0">
              <span className="flex flex-col leading-tight">
                <span className="truncate text-zinc-300" title={match.path}>
                  {match.name}
                </span>
                <span className="truncate text-[0.58rem] text-zinc-600" title={match.path}>
                  {folderOf(match.path)}
                </span>
              </span>
            </span>
            <span className="text-end font-mono tabular-nums">{match.score}%</span>
            <span className="truncate text-[0.58rem] text-zinc-500">
              {matchKindLabel(match.match_type, rtl)}
            </span>
            <span className="text-end font-mono text-[0.58rem] tabular-nums">
              {formatBytes(match.size_bytes)}
            </span>
            <span className="flex items-center justify-end gap-0.5">
              <IconButton label={rtl ? "فتح" : "Open"} icon="file" onClick={() => onOpen(match.path)} />
              <IconButton label={rtl ? "إظهار المجلد" : "Show folder"} icon="folder" onClick={() => onReveal(match.path)} />
              <IconButton label={rtl ? "نسخ المسار" : "Copy path"} icon="clipboard" onClick={() => onCopy(match.path)} />
              <IconButton label={rtl ? "نسخ الاسم" : "Copy name"} icon="text" onClick={() => onCopyName(match.name)} />
            </span>
          </div>
        ))}

      {expanded && entry.matches.length > 1 && (
        <p className="px-3 pb-1.5 text-[0.58rem] text-zinc-600">
          {rtl
            ? `${entry.total_matches} مرشحاً عند أو فوق الحد · ${entry.duration_ms} ms`
            : `${entry.total_matches} candidates at or above the threshold · ${entry.duration_ms} ms`}
        </p>
      )}
    </div>
  );
}

function IconButton({
  label,
  icon,
  onClick,
}: {
  label: string;
  icon: "file" | "folder" | "clipboard" | "text" | "x";
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={label}
      aria-label={label}
      className="grid h-5 w-5 place-items-center rounded text-zinc-500 transition hover:bg-white/10 hover:text-white"
    >
      <Icon name={icon} size={12} />
    </button>
  );
}
