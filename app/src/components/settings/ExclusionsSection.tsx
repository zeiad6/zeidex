import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type ExclusionEntry,
  type ExclusionKind,
  type ExclusionsSnapshot,
} from "../../api";
import { Icon, type IconName } from "../Icons";

const KINDS: {
  id: ExclusionKind;
  label: string;
  labelAr: string;
  icon: IconName;
  placeholder: string;
  hint: string;
  hintAr: string;
}[] = [
  {
    id: "folder",
    label: "Folder",
    labelAr: "مجلد",
    icon: "folder",
    placeholder: "",
    hint: "A folder and everything below it.",
    hintAr: "مجلد وكل ما تحته.",
  },
  {
    id: "file",
    label: "File",
    labelAr: "ملف",
    icon: "file",
    placeholder: "",
    hint: "One exact file.",
    hintAr: "ملف واحد بمساره.",
  },
  {
    id: "extension",
    label: "Extension",
    labelAr: "امتداد",
    icon: "file",
    placeholder: "tmp",
    hint: "Every file with this suffix, for example tmp or log.",
    hintAr: "كل ملف بهذا الامتداد، مثل tmp أو log.",
  },
  {
    id: "pattern",
    label: "Pattern",
    labelAr: "نمط",
    icon: "filter",
    placeholder: "*.tmp or node_modules",
    hint: "A safe glob: a bare name matches at any depth, a path pattern matches the whole path.",
    hintAr: "نمط glob آمن: الاسم المجرّد يطابق في أي عمق، ونمط المسار يطابق المسار كاملاً.",
  },
];

function kindMeta(kind: ExclusionKind) {
  return KINDS.find((entry) => entry.id === kind) ?? KINDS[0];
}

function formatAdded(unix: number, rtl: boolean): string {
  if (!unix) return rtl ? "تاريخ غير معروف" : "date unknown";
  const date = new Date(unix * 1000);
  if (Number.isNaN(date.getTime())) return rtl ? "تاريخ غير معروف" : "date unknown";
  try {
    return new Intl.DateTimeFormat(rtl ? "ar" : "en", {
      day: "numeric",
      month: "short",
      year: "numeric",
    }).format(date);
  } catch {
    return date.toISOString().slice(0, 10);
  }
}

function Switch({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className={`flex h-5 w-9 shrink-0 items-center rounded-full p-0.5 transition-colors ${
        checked ? "justify-end bg-accent-500/80" : "justify-start bg-ink-700"
      }`}
    >
      <span className="block h-4 w-4 rounded-full bg-[#fff] shadow-sm" />
    </button>
  );
}

/**
 * Index Exclusions.
 *
 * The rules themselves live in the engine and are enforced by the walker; this
 * screen is the list, the pickers and the consequences. Every mutation returns
 * what it deleted from the index, so the status line reports a number instead
 * of a promise.
 */
export default function ExclusionsSection({ rtl }: { rtl: boolean }) {
  const t = (en: string, ar: string) => (rtl ? ar : en);
  const [snapshot, setSnapshot] = useState<ExclusionsSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [kind, setKind] = useState<ExclusionKind>("folder");
  const [value, setValue] = useState("");
  const [preview, setPreview] = useState<number | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const [filter, setFilter] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingValue, setEditingValue] = useState("");
  const [confirmingClear, setConfirmingClear] = useState(false);
  const [notice, setNotice] = useState<{ kind: "done" | "warning" | "error"; text: string } | null>(
    null,
  );
  const previewSeq = useRef(0);

  const refresh = useCallback(async () => {
    try {
      const next = await api.exclusionsSnapshot();
      setSnapshot(next);
    } catch (err) {
      setNotice({ kind: "error", text: String(err) });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!confirmingClear) return;
    const timer = window.setTimeout(() => setConfirmingClear(false), 3000);
    return () => window.clearTimeout(timer);
  }, [confirmingClear]);

  // The count is a preview of the current index, not a promise: a path that is
  // not indexed yet reports 0 and is still excluded from the next walk. The
  // sequence number drops a slow answer for an older value.
  useEffect(() => {
    const trimmed = value.trim();
    if (!trimmed || kind === "folder" || kind === "file") {
      // A path is previewed after the picker fills it in, by `previewPath`.
      if (!trimmed) setPreview(null);
      return;
    }
    const seq = ++previewSeq.current;
    setPreviewing(true);
    const timer = window.setTimeout(() => {
      api
        .exclusionsPreview(kind, trimmed)
        .then((count) => {
          if (previewSeq.current === seq) setPreview(count);
        })
        .catch(() => {
          if (previewSeq.current === seq) setPreview(null);
        })
        .finally(() => {
          if (previewSeq.current === seq) setPreviewing(false);
        });
    }, 320);
    return () => window.clearTimeout(timer);
  }, [kind, value]);

  const previewPath = async (nextKind: ExclusionKind, path: string) => {
    setValue(path);
    setPreviewing(true);
    try {
      setPreview(await api.exclusionsPreview(nextKind, path));
    } catch {
      setPreview(null);
    } finally {
      setPreviewing(false);
    }
  };

  const pick = async (nextKind: ExclusionKind) => {
    setNotice(null);
    try {
      const selected =
        nextKind === "folder"
          ? await api.settingsPickFolder(
              t("Choose a folder to exclude", "اختر مجلداً لاستبعاده"),
            )
          : await api.settingsPickFile(t("Choose a file to exclude", "اختر ملفاً لاستبعاده"));
      if (!selected) return;
      await previewPath(nextKind, selected);
    } catch (err) {
      setNotice({ kind: "error", text: String(err) });
    }
  };

  const applyChange = (result: {
    entries: ExclusionEntry[];
    active: number;
    removed: number;
    reindexing: boolean;
  }) => {
    setSnapshot((current) => (current ? { ...current, ...result } : current));
    if (result.reindexing) {
      setNotice({
        kind: "warning",
        text: t(
          "The rule was removed. Re-indexing the scope it covered in the background.",
          "أُزيل الاستثناء. تجري إعادة فهرسة النطاق الذي كان يغطيه في الخلفية.",
        ),
      });
    } else if (result.removed > 0) {
      setNotice({
        kind: "done",
        text: t(
          `Removed ${result.removed.toLocaleString()} matching items from the index. Future walks skip them entirely.`,
          `حُذف ${result.removed.toLocaleString()} عنصراً مطابقاً من الفهرس. الفهرسة القادمة تتخطاها تماماً.`,
        ),
      });
    } else {
      setNotice({
        kind: "done",
        text: t(
          "Saved. The walker applies it from the next traversal.",
          "حُفظ. يطبّقه محرك الفهرسة من المسح التالي.",
        ),
      });
    }
  };

  const add = async () => {
    const trimmed = value.trim();
    if (!trimmed) return;
    setBusy(true);
    setNotice(null);
    try {
      const result = await api.exclusionsAdd(kind, trimmed);
      applyChange(result);
      setValue("");
      setPreview(null);
    } catch (err) {
      setNotice({ kind: "error", text: String(err) });
    } finally {
      setBusy(false);
    }
  };

  const toggle = async (entry: ExclusionEntry, enabled: boolean) => {
    setBusy(true);
    try {
      applyChange(await api.exclusionsUpdate(entry.id, { enabled }));
    } catch (err) {
      setNotice({ kind: "error", text: String(err) });
    } finally {
      setBusy(false);
    }
  };

  const saveEdit = async (entry: ExclusionEntry) => {
    const trimmed = editingValue.trim();
    if (!trimmed || trimmed === entry.value) {
      setEditingId(null);
      return;
    }
    setBusy(true);
    try {
      applyChange(await api.exclusionsUpdate(entry.id, { value: trimmed }));
      setEditingId(null);
    } catch (err) {
      setNotice({ kind: "error", text: String(err) });
    } finally {
      setBusy(false);
    }
  };

  const remove = async (entry: ExclusionEntry) => {
    setBusy(true);
    try {
      applyChange(await api.exclusionsRemove(entry.id));
    } catch (err) {
      setNotice({ kind: "error", text: String(err) });
    } finally {
      setBusy(false);
    }
  };

  const clearAll = async () => {
    if (!confirmingClear) {
      setConfirmingClear(true);
      return;
    }
    setConfirmingClear(false);
    setBusy(true);
    try {
      applyChange(await api.exclusionsClear());
    } catch (err) {
      setNotice({ kind: "error", text: String(err) });
    } finally {
      setBusy(false);
    }
  };

  const applyPreset = async (presetId: string) => {
    setBusy(true);
    setNotice(null);
    try {
      applyChange(await api.exclusionsApplyPreset(presetId));
    } catch (err) {
      setNotice({ kind: "error", text: String(err) });
    } finally {
      setBusy(false);
    }
  };

  const visible = useMemo(() => {
    const entries = snapshot?.entries ?? [];
    const needle = filter.trim().toLowerCase();
    if (!needle) return entries;
    return entries.filter(
      (entry) =>
        entry.value.toLowerCase().includes(needle) || entry.kind.includes(needle),
    );
  }, [snapshot, filter]);

  const active = snapshot?.active ?? 0;
  const meta = kindMeta(kind);
  const presetApplied = (rules: { kind: ExclusionKind; value: string }[]) =>
    rules.filter((rule) =>
      (snapshot?.entries ?? []).some(
        (entry) => entry.kind === rule.kind && entry.value.toLowerCase() === rule.value.toLowerCase(),
      ),
    ).length;

  if (loading) {
    return (
      <div className="space-y-3" aria-busy="true">
        <div className="skeleton h-4 w-40" />
        <div className="skeleton h-20 w-full" />
        <div className="skeleton h-32 w-full" />
      </div>
    );
  }

  return (
    <div className="animate-rise space-y-4">
      <div>
        <h2 className="flex items-center gap-2 text-[0.95rem] font-semibold tracking-[-0.01em] text-white">
          <span className="text-accent-400/80">
            <Icon name="ban" size={15} />
          </span>
          {t("Index exclusions", "استثناءات الفهرسة")}
        </h2>
        <p className="mt-1 text-[0.74rem] leading-relaxed text-zinc-500">
          {t(
            "Rules the walker checks before it reads an entry: excluded folders are never entered, excluded files are never stat'ed, and nothing excluded reaches the database, the content index or OCR.",
            "قواعد يفحصها المسح قبل قراءة أي عنصر: المجلدات المستبعدة لا تُدخل، والملفات المستبعدة لا تُقرأ بياناتها، ولا يصل شيء مستبعد إلى قاعدة البيانات أو فهرس المحتوى أو OCR.",
          )}
        </p>
      </div>

      {/* ------------------------------------------------------------ add */}
      <div className="space-y-3 rounded-2xl border border-white/[0.07] bg-surface p-4 shadow-[var(--shadow-card)]">
        <div className="segment flex-wrap" role="group" aria-label={t("Rule type", "نوع القاعدة")}>
          {KINDS.map((entry) => (
            <button
              key={entry.id}
              type="button"
              aria-pressed={kind === entry.id}
              onClick={() => {
                setKind(entry.id);
                setValue("");
                setPreview(null);
              }}
              className="flex items-center gap-1.5"
            >
              <Icon name={entry.icon} size={12} />
              {rtl ? entry.labelAr : entry.label}
            </button>
          ))}
        </div>

        <p className="text-[0.72rem] leading-relaxed text-zinc-500">
          {rtl ? meta.hintAr : meta.hint}
        </p>

        <div className="flex flex-wrap items-center gap-2">
          {kind === "folder" || kind === "file" ? (
            <button
              type="button"
              onClick={() => void pick(kind)}
              className="flex items-center gap-1.5 rounded-lg border border-dashed border-white/15 px-3 py-2 text-[0.74rem] text-zinc-300 transition hover:border-accent-400/40 hover:text-accent-200"
            >
              <Icon name={kind === "folder" ? "folder" : "file"} size={13} />
              {kind === "folder"
                ? t("Choose a folder…", "اختيار مجلد…")
                : t("Choose a file…", "اختيار ملف…")}
            </button>
          ) : (
            <input
              value={value}
              onChange={(event) => setValue(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") void add();
              }}
              placeholder={meta.placeholder}
              spellCheck={false}
              dir="ltr"
              aria-label={rtl ? meta.labelAr : meta.label}
              className="min-w-[12rem] flex-1 rounded-lg border border-white/10 bg-ink-900/70 px-3 py-2 font-mono text-[0.74rem] text-zinc-100 transition focus:border-accent-400/40 focus:bg-ink-900"
            />
          )}
          {value && (
            <button
              type="button"
              onClick={() => void add()}
              disabled={busy || previewing}
              className="flex items-center gap-1.5 rounded-lg border border-accent-400/30 bg-accent-400/10 px-3 py-2 text-[0.74rem] font-medium text-accent-200 transition hover:bg-accent-400/20 disabled:cursor-not-allowed disabled:opacity-40"
            >
              <Icon name="check" size={12} />
              {t("Add exclusion", "إضافة الاستثناء")}
            </button>
          )}
        </div>

        {value && (
          <p className="break-all font-mono text-[0.7rem] text-zinc-400" dir="ltr">
            {value}
          </p>
        )}

        {value && (
          <p className="text-[0.72rem] leading-relaxed text-zinc-500" role="status" aria-live="polite">
            {previewing
              ? t("Counting what this would remove…", "جارٍ عدّ ما سيزيله هذا الاستثناء…")
              : preview === null
                ? t(
                    "This rule will be applied to the next walk.",
                    "ستُطبَّق هذه القاعدة على المسح التالي.",
                  )
                : kind === "folder" || kind === "file"
                  ? t(
                      `${preview.toLocaleString()} indexed items are inside this path and will be removed. Everything below it is excluded, now and in future walks.`,
                      `${preview.toLocaleString()} عنصراً مفهرساً داخل هذا المسار وسيُحذف. كل ما تحته مستبعد الآن وفي المسحات القادمة.`,
                    )
                  : t(
                      `${preview.toLocaleString()} indexed items match this rule right now. Future walks never see them.`,
                      `${preview.toLocaleString()} عنصراً مفهرساً يطابق هذه القاعدة الآن. المسحات القادمة لا تراها.`,
                    )}
          </p>
        )}

        {notice && (
          <div
            role={notice.kind === "error" ? "alert" : "status"}
            className={`flex items-start gap-2 rounded-lg border px-3 py-2 text-[0.72rem] leading-relaxed ${
              notice.kind === "error"
                ? "border-red-400/20 bg-red-400/[0.07] text-red-200"
                : notice.kind === "warning"
                  ? "border-amber-400/20 bg-amber-400/[0.07] text-amber-200"
                  : "border-emerald-400/20 bg-emerald-400/[0.07] text-emerald-200"
            }`}
          >
            <Icon name={notice.kind === "done" ? "check" : "alert"} size={12} className="mt-0.5 shrink-0" />
            <span className="min-w-0 flex-1">{notice.text}</span>
          </div>
        )}
      </div>

      {/* ----------------------------------------------------------- list */}
      <div className="rounded-2xl border border-white/[0.07] bg-surface p-4 shadow-[var(--shadow-card)]">
        <div className="mb-3 flex flex-wrap items-center gap-2">
          <span className="text-[0.78rem] font-medium text-zinc-200">
            {t(
              `${snapshot?.entries.length ?? 0} rules · ${active} active`,
              `${snapshot?.entries.length ?? 0} قاعدة · ${active} مفعّلة`,
            )}
          </span>
          <div className="ms-auto flex items-center gap-2">
            <label className="relative">
              <span className="visually-hidden">{t("Search exclusions", "بحث في الاستثناءات")}</span>
              <span className="pointer-events-none absolute inset-y-0 start-2 grid place-items-center text-zinc-600">
                <Icon name="search" size={12} />
              </span>
              <input
                value={filter}
                onChange={(event) => setFilter(event.target.value)}
                placeholder={t("Search…", "بحث…")}
                className="w-36 rounded-lg border border-white/10 bg-ink-900/70 py-1.5 pe-2 ps-7 text-[0.72rem] text-zinc-200 transition focus:border-accent-400/40"
              />
            </label>
            <button
              type="button"
              onClick={() => void clearAll()}
              disabled={busy || (snapshot?.entries.length ?? 0) === 0}
              className={`flex items-center gap-1.5 rounded-full px-2.5 py-1.5 text-[0.68rem] font-medium transition disabled:cursor-not-allowed disabled:opacity-40 ${
                confirmingClear
                  ? "bg-red-500/15 text-red-300 ring-1 ring-red-400/25"
                  : "border border-white/10 text-zinc-400 hover:border-red-400/30 hover:bg-red-500/10 hover:text-red-300"
              }`}
            >
              <Icon name="trash" size={11} />
              {confirmingClear
                ? t("Click again to clear", "اضغط مرة أخرى للمسح")
                : t("Clear all", "مسح الكل")}
            </button>
          </div>
        </div>

        {visible.length === 0 ? (
          <p className="rounded-lg border border-dashed border-white/10 px-3 py-4 text-center text-[0.72rem] text-zinc-500">
            {snapshot?.entries.length === 0
              ? t(
                  "No exclusions yet. Nothing is being skipped.",
                  "لا توجد استثناءات بعد. لا شيء يُتخطّى حالياً.",
                )
              : t("No rule matches this search.", "لا توجد قاعدة تطابق هذا البحث.")}
          </p>
        ) : (
          <ul className="space-y-1.5">
            {visible.map((entry) => {
              const entryMeta = kindMeta(entry.kind);
              return (
                <li
                  key={entry.id}
                  className="flex flex-wrap items-center gap-2 rounded-lg border border-white/[0.07] bg-ink-900/50 px-2.5 py-2"
                >
                  <span
                    className={`grid h-6 w-6 shrink-0 place-items-center rounded-md ${
                      entry.enabled ? "bg-accent-400/10 text-accent-300" : "bg-white/[0.04] text-zinc-600"
                    }`}
                  >
                    <Icon name={entryMeta.icon} size={12} />
                  </span>
                  <span className="min-w-0 flex-1">
                    {editingId === entry.id ? (
                      <input
                        autoFocus
                        value={editingValue}
                        onChange={(event) => setEditingValue(event.target.value)}
                        onKeyDown={(event) => {
                          if (event.key === "Enter") void saveEdit(entry);
                          if (event.key === "Escape") setEditingId(null);
                        }}
                        dir="ltr"
                        aria-label={t("Edit exclusion", "تعديل الاستثناء")}
                        className="w-full rounded-md border border-accent-400/30 bg-ink-950 px-2 py-1 font-mono text-[0.72rem] text-zinc-100"
                      />
                    ) : (
                      <span
                        className={`block break-all font-mono text-[0.72rem] ${
                          entry.enabled ? "text-zinc-200" : "text-zinc-600 line-through"
                        }`}
                        dir="ltr"
                      >
                        {entry.value}
                      </span>
                    )}
                    <span className="mt-0.5 block text-[0.62rem] text-zinc-600">
                      {rtl ? entryMeta.labelAr : entryMeta.label}
                      {" · "}
                      {formatAdded(entry.added_at, rtl)}
                    </span>
                  </span>
                  <Switch
                    checked={entry.enabled}
                    onChange={(next) => void toggle(entry, next)}
                    label={
                      entry.enabled
                        ? t("Disable this rule", "تعطيل هذه القاعدة")
                        : t("Enable this rule", "تفعيل هذه القاعدة")
                    }
                  />
                  {editingId === entry.id ? (
                    <>
                      <button
                        type="button"
                        onClick={() => void saveEdit(entry)}
                        disabled={busy}
                        aria-label={t("Save", "حفظ")}
                        className="grid h-6 w-6 place-items-center rounded-md text-emerald-300 transition hover:bg-emerald-400/10 disabled:opacity-40"
                      >
                        <Icon name="check" size={12} />
                      </button>
                      <button
                        type="button"
                        onClick={() => setEditingId(null)}
                        aria-label={t("Cancel", "إلغاء")}
                        className="grid h-6 w-6 place-items-center rounded-md text-zinc-500 transition hover:bg-white/10"
                      >
                        <Icon name="x" size={12} />
                      </button>
                    </>
                  ) : (
                    <button
                      type="button"
                      onClick={() => {
                        setEditingId(entry.id);
                        setEditingValue(entry.value);
                      }}
                      aria-label={t("Edit", "تعديل")}
                      className="grid h-6 w-6 place-items-center rounded-md text-zinc-500 transition hover:bg-white/10 hover:text-zinc-200"
                    >
                      <Icon name="settings" size={12} />
                    </button>
                  )}
                  <button
                    type="button"
                    onClick={() => void remove(entry)}
                    disabled={busy}
                    aria-label={t(`Remove ${entry.value}`, `إزالة ${entry.value}`)}
                    className="grid h-6 w-6 place-items-center rounded-md text-zinc-500 transition hover:bg-red-500/15 hover:text-red-300 disabled:opacity-40"
                  >
                    <Icon name="trash" size={12} />
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>

      {/* --------------------------------------------------------- presets */}
      <div>
        <h3 className="mb-2 text-[0.62rem] font-semibold micro-label tracking-[0.16em] text-zinc-500">
          {t("Smart presets", "استثناءات ذكية جاهزة")}
        </h3>
        <p className="mb-2 text-[0.72rem] leading-relaxed text-zinc-500">
          {t(
            "Nothing here is on by default. Applying a preset adds ordinary rules you can edit, disable or delete one by one.",
            "لا شيء هنا مفعّل افتراضياً. تطبيق أي مجموعة يضيف قواعد عادية يمكنك تعديلها أو تعطيلها أو حذفها فرداً فرداً.",
          )}
        </p>
        <div className="grid gap-2 sm:grid-cols-3">
          {(snapshot?.presets ?? []).map((preset) => {
            const applied = presetApplied(preset.rules);
            const complete = applied >= preset.rules.length;
            return (
              <div
                key={preset.id}
                className="flex flex-col gap-2 rounded-xl border border-white/[0.07] bg-surface-2 p-3"
              >
                <div>
                  <p className="text-[0.76rem] font-medium text-zinc-200">
                    {rtl ? preset.nameAr : preset.name}
                  </p>
                  <p className="mt-1 text-[0.68rem] leading-relaxed text-zinc-500">
                    {rtl ? preset.descriptionAr : preset.description}
                  </p>
                </div>
                <p className="text-[0.64rem] text-zinc-600">
                  {t(
                    `${preset.rules.length} rules${applied > 0 ? ` · ${applied} already added` : ""}`,
                    `${preset.rules.length} قاعدة${applied > 0 ? ` · ${applied} مضافة سابقاً` : ""}`,
                  )}
                </p>
                <button
                  type="button"
                  onClick={() => void applyPreset(preset.id)}
                  disabled={busy || complete}
                  className="mt-auto rounded-full border border-accent-400/30 bg-accent-400/10 px-2.5 py-1.5 text-[0.68rem] font-medium text-accent-200 transition hover:bg-accent-400/20 disabled:cursor-not-allowed disabled:opacity-40"
                >
                  {complete
                    ? t("Applied", "مطبّقة")
                    : t("Apply preset", "تطبيق المجموعة")}
                </button>
              </div>
            );
          })}
        </div>
      </div>

      <p className="text-[0.68rem] leading-relaxed text-zinc-600">
        {t(
          "Rules are saved in the config file and apply to every drive, the manual walk and the watcher. Adding a rule purges what the index already holds; removing one re-walks only the scope it covered (a full walk for an extension or pattern).",
          "تُحفظ القواعد في ملف الإعدادات وتُطبَّق على كل الأقراص والفهرسة اليدوية والمراقب. إضافة قاعدة تحذف ما يحمله الفهرس؛ وإزالتها تعيد مسح النطاق الذي كانت تغطيه فقط (مسح كامل للامتدادات والأنماط).",
        )}
      </p>
    </div>
  );
}
