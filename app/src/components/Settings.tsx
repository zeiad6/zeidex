import { useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type AppSettings,
  type AutostartStatus,
  type DocumentIndexStatus,
  type HotkeyStatus,
  type ModelsStatus,
} from "../api";
import type { IndexStatus } from "../index-status";
import {
  diff,
  toDraft,
  type ChangeSet,
  type Draft,
  type FieldError,
} from "../settings-draft";
import { BrandMark } from "./Brand";
import type { ThemeChoice } from "../theme";
import { Icon, type IconName } from "./Icons";
import WindowControls from "./WindowControls";

interface Notice {
  kind: "done" | "warning" | "error";
  message: string;
}

type Section = "search" | "indexing" | "startup" | "clipboard" | "appearance" | "about";

const SECTIONS: { id: Section; label: string; labelAr: string; icon: IconName }[] = [
  { id: "search", label: "Search", labelAr: "البحث", icon: "search" },
  { id: "indexing", label: "Indexing", labelAr: "الفهرسة", icon: "folder" },
  { id: "startup", label: "Startup", labelAr: "بدء التشغيل", icon: "power" },
  { id: "clipboard", label: "Clipboard", labelAr: "الحافظة", icon: "clipboard" },
  { id: "appearance", label: "Appearance", labelAr: "المظهر", icon: "eye" },
  { id: "about", label: "About & Diagnostics", labelAr: "حول والتشخيص", icon: "info" },
];

function Toggle({
  checked,
  onChange,
  title,
  hint,
  disabled = false,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  title: string;
  hint: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      disabled={disabled}
      className="flex w-full items-start gap-3 rounded-xl border border-white/[0.07] bg-white/[0.02] px-3.5 py-3 text-start transition hover:border-white/15 hover:bg-white/[0.04] disabled:cursor-not-allowed disabled:opacity-50"
    >
      <span
        aria-hidden="true"
        className={`mt-0.5 flex h-5 w-9 shrink-0 items-center rounded-full p-0.5 transition-colors ${
          checked ? "justify-end bg-accent-500/80" : "justify-start bg-ink-700"
        }`}
      >
        <span className="block h-4 w-4 rounded-full bg-[#fff] shadow-sm" />
      </span>
      <span className="min-w-0">
        <span className="block text-[0.82rem] font-medium text-zinc-100">{title}</span>
        <span className="mt-0.5 block text-[0.72rem] leading-relaxed text-zinc-500">{hint}</span>
      </span>
    </button>
  );
}

function NumberField({
  id,
  label,
  hint,
  value,
  bounds,
  error,
  onChange,
  errorText,
}: {
  id: string;
  label: string;
  hint: string;
  value: string;
  bounds: [number, number];
  error?: FieldError;
  onChange: (next: string) => void;
  errorText: (error: FieldError) => string;
}) {
  return (
    <div className="flex flex-wrap items-start gap-3">
      <label htmlFor={id} className="min-w-[11rem] flex-1">
        <span className="block text-[0.82rem] font-medium text-zinc-100">{label}</span>
        <span className="mt-0.5 block text-[0.72rem] leading-relaxed text-zinc-500">{hint}</span>
      </label>
      <div className="flex flex-col items-end gap-1">
        <input
          id={id}
          type="number"
          inputMode="numeric"
          dir="ltr"
          min={bounds[0]}
          max={bounds[1]}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          aria-invalid={error ? true : undefined}
          aria-describedby={error ? `${id}-error` : undefined}
          className={`w-28 rounded-lg border bg-ink-900/70 px-3 py-1.5 text-sm tabular-nums text-zinc-100 transition focus:bg-ink-900 ${
            error ? "border-red-400/50" : "border-white/10 focus:border-accent-400/40"
          }`}
        />
        <span id={`${id}-error`} className="h-3 text-[0.62rem] text-red-300">
          {error ? errorText(error) : ""}
        </span>
      </div>
    </div>
  );
}

function SectionHeading({
  icon,
  title,
  description,
}: {
  icon: IconName;
  title: string;
  description: string;
}) {
  return (
    <div className="mb-4">
      <h2 className="flex items-center gap-2 text-[0.95rem] font-semibold tracking-[-0.01em] text-white">
        <span className="text-accent-400/80">
          <Icon name={icon} size={15} />
        </span>
        {title}
      </h2>
      <p className="mt-1 text-[0.74rem] leading-relaxed text-zinc-500">{description}</p>
    </div>
  );
}

function Card({ children }: { children: React.ReactNode }) {
  return (
    <div className="space-y-4 rounded-2xl border border-white/[0.07] bg-white/[0.02] p-4">
      {children}
    </div>
  );
}

export default function Settings({
  rtl,
  indexStatus,
  onSetRtl,
  theme,
  onSetTheme,
  onBack,
}: {
  rtl: boolean;
  indexStatus: IndexStatus | null;
  onSetRtl: (next: boolean) => void;
  theme: ThemeChoice;
  onSetTheme: (next: ThemeChoice) => void;
  onBack: () => void;
}) {
  const t = (en: string, ar: string) => (rtl ? ar : en);

  const [section, setSection] = useState<Section>("search");
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [indexing, setIndexing] = useState(false);
  // The button's own request and the worker's status are both "busy"; the
  // pause controls belong to the worker's walk, not to this component's flag.
  const busy = indexing || (indexStatus?.indexing ?? false);
  const paused = indexStatus?.paused ?? false;
  // The worker is the source of truth for the count; a watcher refresh or a
  // settings-triggered rebuild updates this panel without a reload. The ref is
  // what stops a slower `settingsSnapshot` from writing an older count over a
  // status that arrived while it was in flight.
  const latestIndex = useRef<IndexStatus | null>(null);
  useEffect(() => {
    if (!indexStatus) return;
    latestIndex.current = indexStatus;
    setSettings((current) =>
      current ? { ...current, candidateCount: indexStatus.count } : current,
    );
  }, [indexStatus]);
  const [confirmingClear, setConfirmingClear] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [models, setModels] = useState<ModelsStatus | null>(null);
  const [docIndex, setDocIndex] = useState<DocumentIndexStatus | null>(null);
  // The launcher shortcut is the app's only way in when it starts hidden, so
  // its registration result is shown here rather than logged and forgotten.
  const [hotkey, setHotkey] = useState<HotkeyStatus | null>(null);
  const [hotkeyDraft, setHotkeyDraft] = useState("");
  const [hotkeyBusy, setHotkeyBusy] = useState(false);
  const [hotkeyNotice, setHotkeyNotice] = useState<string | null>(null);
  // Launch-at-login is an OS fact, not a value in the config file: the toggle
  // reads the registry/autostart entry back after every write.
  const [autostart, setAutostart] = useState<AutostartStatus | null>(null);
  const [autostartBusy, setAutostartBusy] = useState(false);

  useEffect(() => {
    let active = true;
    api
      .settingsSnapshot()
      .then((snapshot) => {
        if (!active) return;
        void api
          .hotkeyStatus()
          .then((status) => {
            if (!active) return;
            setHotkey(status);
            setHotkeyDraft(status.accelerator);
          })
          .catch(() => {
            // The browser demo has no shell to answer.
          });
        void api
          .autostartStatus()
          .then((status) => {
            if (active) setAutostart(status);
          })
          .catch(() => {
            // Older shells have no such command; the card says "unavailable".
          });
        const latest = latestIndex.current;
        setSettings(latest ? { ...snapshot, candidateCount: latest.count } : snapshot);
        setDraft(toDraft(snapshot));
      })
      .catch((err) => {
        if (active) setNotice({ kind: "error", message: String(err) });
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    api
      .ocrModelsStatus()
      .then((status) => {
        if (active) setModels(status);
      })
      .catch(() => {
        // Models are optional for the launcher; About simply omits the line.
      });
    api
      .documentIndexStatus()
      .then((status) => {
        if (active) setDocIndex(status);
      })
      .catch(() => {
        // The panel omits the row rather than showing a count it cannot trust.
      });
    return () => {
      active = false;
    };
  }, []);

  // Escape leaves the panel, like every other surface in the app. There is no
  // unsaved-changes trap: the footer shows the state, and closing discards the
  // draft the way closing a dialog does.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onBack();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onBack]);

  useEffect(() => {
    if (!confirmingClear) return;
    const timer = window.setTimeout(() => setConfirmingClear(false), 3000);
    return () => window.clearTimeout(timer);
  }, [confirmingClear]);

  const change = useMemo<ChangeSet>(() => {
    if (!draft || !settings) {
      return { patch: {}, errors: {}, dirty: false };
    }
    return diff(draft, settings);
  }, [draft, settings]);

  const update = <K extends keyof Draft>(key: K, value: Draft[K]) => {
    setDraft((current) => (current ? { ...current, [key]: value } : current));
  };

  const errorText = (error: FieldError) => {
    if (error === "required") return t("Enter a number", "أدخل رقماً");
    if (error === "integer") return t("Whole numbers only", "أعداد صحيحة فقط");
    return t("Out of range", "خارج النطاق");
  };

  const save = async () => {
    if (!settings || !draft) return;
    const { patch, dirty, errors } = diff(draft, settings);
    if (Object.keys(errors).length > 0) {
      setNotice({
        kind: "error",
        message: t("Fix the highlighted fields first.", "صحّح الحقول المعلّمة أولاً."),
      });
      return;
    }
    if (!dirty) return;

    setSaving(true);
    setNotice(null);
    // What was on screen when Save was pressed. If the user keeps typing while
    // the write is in flight, their newer text wins over the echoed snapshot.
    const submitted = draft;
    try {
      const result = await api.settingsUpdate(patch);
      setSettings(result.settings);
      setDraft((current) => (current === submitted ? toDraft(result.settings) : current));
      if (result.warning) {
        setNotice({ kind: "warning", message: result.warning });
      } else if (result.reindexRequired) {
        setNotice({
          kind: "warning",
          message: t(
            "Saved. Rebuilding the index with the new settings.",
            "حُفظت. تُعاد فهرسة الفهرس بالإعدادات الجديدة.",
          ),
        });
      } else {
        setNotice({ kind: "done", message: t("Settings saved.", "حُفظت الإعدادات.") });
      }
    } catch (err) {
      setNotice({ kind: "error", message: String(err) });
    } finally {
      setSaving(false);
    }
  };

  const reindex = async () => {
    setIndexing(true);
    setNotice(null);
    try {
      const total = await api.bootstrapIndex();
      setSettings((current) => (current ? { ...current, candidateCount: total } : current));
      setNotice({
        kind: "done",
        message: t(`Indexed ${total} items.`, `اكتملت فهرسة ${total} عنصراً.`),
      });
    } catch (err) {
      const message = String(err);
      // Cancelling is a choice, not a failure: the backend reports how far it
      // got, and the index kept every row it had.
      setNotice(
        message.startsWith("cancelled")
          ? { kind: "warning", message }
          : { kind: "error", message },
      );
    } finally {
      setIndexing(false);
    }
  };

  const pauseIndex = async () => {
    const paused = await api.indexPause();
    setNotice(
      paused
        ? {
            kind: "warning",
            message: t(
              "Indexing paused. It resumes from the same entry.",
              "توقفت الفهرسة مؤقتاً. تستأنف من الموضع نفسه.",
            ),
          }
        : {
            kind: "warning",
            message: t("No walk is running to pause.", "لا توجد فهرسة جارية لإيقافها."),
          },
    );
  };

  const resumeIndex = async () => {
    const resumed = await api.indexResume();
    setNotice({
      kind: "warning",
      message: resumed
        ? t("Indexing resumed.", "استُئنفت الفهرسة.")
        : t("The walk is not paused.", "الفهرسة ليست متوقفة مؤقتاً."),
    });
  };

  const toggleDocumentIndexing = async (next: boolean) => {
    try {
      const enabled = await api.documentSetIndexing(next);
      setDocIndex((current) => ({ enabled, count: current?.count ?? 0 }));
      setNotice({
        kind: "done",
        message: enabled
          ? t(
              "Recognized text will be searchable.",
              "سيصبح النص المتعرَّف عليه قابلاً للبحث.",
            )
          : t(
              "Recognized text will no longer be stored.",
              "لن يُخزَّن النص المتعرَّف عليه بعد الآن.",
            ),
      });
    } catch (err) {
      setNotice({ kind: "error", message: String(err) });
    }
  };

  const clearDocumentText = async () => {
    try {
      const removed = await api.documentTextClear();
      setDocIndex((current) => ({ enabled: current?.enabled ?? true, count: 0 }));
      setNotice({
        kind: "done",
        message: t(
          `Removed the stored text of ${removed} documents. Your files are untouched.`,
          `حُذف النص المخزَّن لـ ${removed} مستنداً. ملفاتك لم تُمس.`,
        ),
      });
    } catch (err) {
      setNotice({ kind: "error", message: String(err) });
    }
  };

  const cancelIndex = async () => {
    const cancelled = await api.indexCancel();
    setNotice({
      kind: "warning",
      message: cancelled
        ? t(
            "Stopping the walk. Everything already indexed stays.",
            "جارٍ إيقاف الفهرسة. كل ما فُهرس يبقى محفوظاً.",
          )
        : t("No walk is running to stop.", "لا توجد فهرسة جارية لإيقافها."),
    });
  };

  const togglePause = async () => {
    if (!settings) return;
    try {
      const paused = await api.clipboardSetPaused(!settings.clipboardPaused);
      // Functional update: a save that lands while this request is in flight
      // must not have its snapshot reverted by a stale copy of `settings`.
      setSettings((current) => (current ? { ...current, clipboardPaused: paused } : current));
    } catch {
      setNotice({
        kind: "error",
        message: t("Could not change recording.", "تعذّر تغيير حالة التسجيل."),
      });
    }
  };

  const clearHistory = async () => {
    if (!confirmingClear) {
      setConfirmingClear(true);
      return;
    }
    setConfirmingClear(false);
    try {
      const cleared = await api.clipboardClear();
      setNotice({
        kind: "done",
        message: t(`Cleared ${cleared} clips.`, `تم مسح ${cleared} مقطعاً.`),
      });
    } catch (err) {
      setNotice({ kind: "error", message: String(err) });
    }
  };

  const addRoot = async () => {
    if (!draft) return;
    try {
      // The dialog runs in Rust: the folder becomes readable because the user
      // chose it there, not because the page named it.
      const selected = await api.settingsPickFolder(
        t("Add a folder to index", "أضف مجلداً للفهرسة"),
      );
      if (!selected) return;
      if (draft.fileScanExtraRoots.includes(selected)) return;
      update("fileScanExtraRoots", [...draft.fileScanExtraRoots, selected]);
    } catch {
      // The dialog is unavailable outside the desktop shell.
      setNotice({
        kind: "warning",
        message: t(
          "The folder picker is not available here; edit the config file instead.",
          "منتقي المجلدات غير متاح هنا؛ عدّل ملف الإعدادات بدلاً من ذلك.",
        ),
      });
    }
  };

  const removeRoot = (root: string) => {
    if (!draft) return;
    update(
      "fileScanExtraRoots",
      draft.fileScanExtraRoots.filter((entry) => entry !== root),
    );
  };

  const revealConfig = async () => {
    if (!settings?.configPath) return;
    try {
      await api.revealTarget(settings.configPath);
    } catch (err) {
      setNotice({ kind: "error", message: String(err) });
    }
  };

  const copyDiagnostics = async () => {
    if (!settings) return;
    // Paths and versions only: enough to compare two machines, nothing that
    // describes what was indexed or copied.
    const report = [
      `Zeidex ${settings.version}`,
      `platform: ${settings.platform}`,
      `config: ${settings.configPath ?? "unresolved"}`,
      `database: ${settings.dbPath}`,
      `indexed: ${settings.candidateCount}`,
      `models: ${models ? (models.complete ? "complete" : `missing ${models.missing.join(", ")}`) : "unknown"}`,
    ].join("\n");
    try {
      await navigator.clipboard.writeText(report);
      setNotice({ kind: "done", message: t("Diagnostics copied.", "نُسخت بيانات التشخيص.") });
    } catch {
      setNotice({
        kind: "error",
        message: t("Could not copy the diagnostics.", "تعذّر نسخ بيانات التشخيص."),
      });
    }
  };

  const saveHotkey = async (accelerator: string | null) => {
    setHotkeyBusy(true);
    setHotkeyNotice(null);
    try {
      const status = await api.hotkeySet(accelerator);
      setHotkey(status);
      setHotkeyDraft(status.accelerator);
      setHotkeyNotice(status.error);
    } catch (err) {
      setHotkeyNotice(String(err));
    } finally {
      setHotkeyBusy(false);
    }
  };

  const toggleAutostart = async (next: boolean) => {
    setAutostartBusy(true);
    setNotice(null);
    try {
      const status = await api.autostartSet(next);
      setAutostart(status);
      if (status.enabled === next) {
        setNotice({
          kind: "done",
          message: next
            ? t(
                "Zeidex will start with the system and stay in the tray.",
                "سيبدأ Zeidex مع النظام ويبقى في شريط النظام.",
              )
            : t(
                "Zeidex will no longer start with the system.",
                "لن يبدأ Zeidex مع النظام بعد الآن.",
              ),
        });
      } else {
        setNotice({
          kind: "warning",
          message:
            status.error ??
            t(
              "The system did not keep the change.",
              "لم يحتفظ النظام بالتغيير.",
            ),
        });
      }
    } catch (err) {
      setNotice({ kind: "error", message: String(err) });
      // A failed write can still have changed the entry (for example, the
      // delete succeeded and the follow-up read failed). Show what is there.
      try {
        setAutostart(await api.autostartStatus());
      } catch {
        // Leave the last known value; the error notice already explains it.
      }
    } finally {
      setAutostartBusy(false);
    }
  };

  const renderSection = () => {
    if (!settings || !draft) return null;
    const limits = settings.limits;

    switch (section) {
      case "search":
        return (
          <div className="animate-rise">
            <SectionHeading
              icon="sparkle"
              title={t("Global shortcut", "الاختصار العام")}
              description={t(
                "Opens Zeidex from any application and puts the cursor in the search box. If another program already owns the combination, Zeidex falls back to the default and says so here.",
                "يفتح Zeidex من أي تطبيق ويضع المؤشر في مربع البحث. إذا كان الاختصار محجوزاً لبرنامج آخر، يعود Zeidex إلى الافتراضي ويوضح ذلك هنا.",
              )}
            />
            <Card>
              <div className="flex flex-wrap items-end gap-3">
                <label htmlFor="hotkey-accelerator" className="min-w-[13rem] flex-1">
                  <span className="block text-[0.82rem] font-medium text-zinc-100">
                    {t("Combination", "التركيبة")}
                  </span>
                  <span className="mt-0.5 block text-[0.72rem] leading-relaxed text-zinc-500">
                    {t(
                      "Modifiers plus a key, for example Alt+Space or Ctrl+Shift+Z.",
                      "معدِّلات مع مفتاح، مثل Alt+Space أو Ctrl+Shift+Z.",
                    )}
                  </span>
                </label>
                <input
                  id="hotkey-accelerator"
                  value={hotkeyDraft}
                  onChange={(event) => setHotkeyDraft(event.target.value)}
                  spellCheck={false}
                  dir="ltr"
                  placeholder={hotkey?.default ?? "Alt+Space"}
                  aria-describedby="hotkey-status"
                  className="w-48 rounded-lg border border-white/10 bg-ink-900/70 px-3 py-1.5 font-mono text-sm text-zinc-100 transition focus:border-accent-400/40 focus:bg-ink-900"
                />
                <button
                  type="button"
                  onClick={() => void saveHotkey(hotkeyDraft)}
                  disabled={hotkeyBusy || hotkeyDraft.trim().length === 0}
                  className="rounded-lg border border-accent-400/30 bg-accent-400/10 px-3 py-1.5 text-xs font-medium text-accent-200 transition hover:bg-accent-400/20 disabled:cursor-not-allowed disabled:opacity-40"
                >
                  {hotkeyBusy ? t("Saving…", "جارٍ الحفظ…") : t("Save", "حفظ")}
                </button>
                <button
                  type="button"
                  onClick={() => void saveHotkey(null)}
                  disabled={hotkeyBusy}
                  className="rounded-lg border border-white/10 px-3 py-1.5 text-xs font-medium text-zinc-300 transition hover:bg-white/10 disabled:cursor-not-allowed disabled:opacity-40"
                >
                  {t("Reset to default", "استعادة الافتراضي")}
                </button>
              </div>
              <p
                id="hotkey-status"
                role="status"
                aria-live="polite"
                className={`text-[0.72rem] leading-relaxed ${
                  hotkey && !hotkey.registered ? "text-red-300" : "text-zinc-500"
                }`}
              >
                {hotkeyNotice ??
                  (hotkey
                    ? hotkey.registered
                      ? t(
                          `Registered: ${hotkey.accelerator}. Closing the window keeps Zeidex in the tray.`,
                          `مسجَّل: ${hotkey.accelerator}. إغلاق النافذة يبقي Zeidex في شريط النظام.`,
                        )
                      : t(
                          `Not registered: ${hotkey.error ?? "the system refused the combination"}.`,
                          `غير مسجَّل: ${hotkey.error ?? "رفض النظام التركيبة"}.`,
                        )
                    : t("Reading the current shortcut…", "جارٍ قراءة الاختصار الحالي…"))}
              </p>
            </Card>
            <div className="mt-6">
              <SectionHeading
                icon="globe"
                title={t("Web search", "البحث على الويب")}
              description={t(
                "When web search is on, every query ends with a row that opens it in your browser. It is always last, so it can never outrank a local match.",
                "عند تشغيل البحث على الويب، ينتهي كل استعلام بصف يفتحه في المتصفح. يبقى دائماً في الأسفل، فلا يتقدّم على نتيجة محلية.",
              )}
            />
            <Card>
              <Toggle
                checked={draft.webSearchEnabled}
                onChange={(next) => update("webSearchEnabled", next)}
                title={t("Add a web search row to results", "إضافة صف بحث على الويب إلى النتائج")}
                hint={t(
                  "Off means the row disappears completely: nothing is sent anywhere either way until you open it.",
                  "عند الإيقاف يختفي الصف تماماً؛ لا يُرسل شيء إلى أي جهة في كل الأحوال حتى تفتحه بنفسك.",
                )}
              />
              <div className="flex flex-wrap items-center gap-3">
                <label htmlFor="search-engine" className="min-w-[11rem] flex-1">
                  <span className="block text-[0.82rem] font-medium text-zinc-100">
                    {t("Search engine", "محرّك البحث")}
                  </span>
                  <span className="mt-0.5 block text-[0.72rem] leading-relaxed text-zinc-500">
                    {t(
                      "Used to build the URL that opens in your default browser.",
                      "يُستخدم لبناء الرابط الذي يُفتح في متصفحك الافتراضي.",
                    )}
                  </span>
                </label>
                <select
                  id="search-engine"
                  value={draft.webSearchEngine}
                  onChange={(event) => update("webSearchEngine", event.target.value)}
                  disabled={!draft.webSearchEnabled}
                  className="rounded-lg border border-white/10 bg-ink-850 px-2.5 py-1.5 text-sm text-zinc-200 disabled:opacity-50"
                >
                  {settings.searchEngines.map((engine) => (
                    <option key={engine} value={engine}>
                      {engine === "duckduckgo"
                        ? "DuckDuckGo"
                        : engine === "bing"
                          ? "Bing"
                          : "Google"}
                    </option>
                  ))}
                </select>
              </div>
            </Card>
            </div>
          </div>
        );

      case "indexing":
        return (
          <div className="animate-rise space-y-4">
            <SectionHeading
              icon="folder"
              title={t("What Zeidex indexes", "ما يفهرسه Zeidex")}
              description={t(
                "The index is local. Depth and size limits bound the walk; they are saved to the config file and applied on the next reindex.",
                "الفهرس محلي. الحدود تحدّ من عمق المسح وحجمه، وتُحفظ في ملف الإعدادات وتُطبَّق عند إعادة الفهرسة.",
              )}
            />
            <Card>
              <NumberField
                id="scan-depth"
                label={t("Folder depth", "عمق المجلدات")}
                hint={t(
                  "How many levels below each root are walked.",
                  "عدد المستويات التي تُمسح تحت كل جذر.",
                )}
                value={draft.fileScanDepth}
                bounds={limits.fileScanDepth}
                error={change.errors.fileScanDepth}
                errorText={errorText}
                onChange={(next) => update("fileScanDepth", next)}
              />
              <NumberField
                id="scan-limit"
                label={t("Maximum indexed files", "الحد الأقصى للملفات المفهرسة")}
                hint={t(
                  "A cap, not a target: the walk stops adding rows past it.",
                  "سقف لا هدف: يتوقف المسح عن إضافة صفوف بعد تجاوزه.",
                )}
                value={draft.fileScanLimit}
                bounds={limits.fileScanLimit}
                error={change.errors.fileScanLimit}
                errorText={errorText}
                onChange={(next) => update("fileScanLimit", next)}
              />
              <Toggle
                checked={draft.lazyIndexingEnabled}
                onChange={(next) => update("lazyIndexingEnabled", next)}
                title={t("Lazy indexing", "فهرسة مؤجّلة")}
                hint={t(
                  "Index folders on first use instead of walking everything at startup.",
                  "فهرس المجلدات عند أول استخدام بدل مسح كل شيء عند بدء التشغيل.",
                )}
              />
              <div className="flex flex-wrap items-center gap-2 border-t border-white/[0.06] pt-4">
                <button
                  type="button"
                  onClick={() => void reindex()}
                  disabled={busy}
                  className="flex items-center gap-1.5 rounded-full bg-gradient-to-r from-accent-400 to-iris-400 px-3.5 py-1.5 text-xs font-semibold text-ink-950 shadow-[0_8px_24px_-10px_rgb(34_211_238/0.8)] transition hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-40"
                >
                  <Icon name="refresh" size={13} className={busy && !paused ? "animate-spin" : ""} />
                  {busy
                    ? paused
                      ? t("Paused", "متوقفة")
                      : t("Reindexing…", "يفهرس…")
                    : t("Reindex now", "إعادة الفهرسة الآن")}
                </button>
                {busy && (
                  <>
                    <button
                      type="button"
                      onClick={() => void (paused ? resumeIndex() : pauseIndex())}
                      className="flex items-center gap-1.5 rounded-full border border-white/10 bg-white/[0.03] px-3 py-1.5 text-xs font-medium text-zinc-300 transition hover:border-accent-400/30 hover:bg-accent-400/10 hover:text-accent-200"
                    >
                      <Icon name={paused ? "play" : "pause"} size={13} />
                      {paused ? t("Resume", "استئناف") : t("Pause", "إيقاف مؤقت")}
                    </button>
                    <button
                      type="button"
                      onClick={() => void cancelIndex()}
                      className="flex items-center gap-1.5 rounded-full border border-rose-400/20 bg-rose-400/[0.06] px-3 py-1.5 text-xs font-medium text-rose-200 transition hover:border-rose-400/40 hover:bg-rose-400/10"
                    >
                      <Icon name="x" size={13} />
                      {t("Cancel", "إلغاء")}
                    </button>
                  </>
                )}
                <span className="text-[0.72rem] text-zinc-500">
                  {t(
                    `${settings.candidateCount.toLocaleString()} items indexed`,
                    `${settings.candidateCount.toLocaleString()} عنصراً مفهرساً`,
                  )}
                </span>
              </div>
              {indexStatus?.error && (
                <div
                  role="status"
                  className="flex items-start gap-2 rounded-lg border border-amber-400/25 bg-amber-400/[0.06] px-3 py-2 text-[0.7rem] leading-relaxed text-amber-200/90"
                >
                  <Icon name="alert" size={13} className="mt-0.5 shrink-0" />
                  <span className="min-w-0 break-words">{indexStatus.error}</span>
                </div>
              )}

              {docIndex && (
                <div className="space-y-2 border-t border-white/[0.06] pt-4">
                  <Toggle
                    checked={docIndex.enabled}
                    onChange={(next) => void toggleDocumentIndexing(next)}
                    title={t("Index recognized text", "فهرسة النص المتعرَّف عليه")}
                    hint={t(
                      "Store the text OCR extracts so a file can be found by a phrase inside it. It stays on this device.",
                      "خزّن النص الذي يستخرجه OCR ليمكن العثور على الملف بعبارة داخله. يبقى على هذا الجهاز.",
                    )}
                  />
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-[0.72rem] text-zinc-500">
                      {t(
                        `${docIndex.count.toLocaleString()} documents searchable by content`,
                        `${docIndex.count.toLocaleString()} مستنداً قابلاً للبحث بالمحتوى`,
                      )}
                    </span>
                    <button
                      type="button"
                      onClick={() => void clearDocumentText()}
                      disabled={docIndex.count === 0}
                      className="ms-auto flex items-center gap-1.5 rounded-full border border-white/10 bg-white/[0.03] px-3 py-1.5 text-xs font-medium text-zinc-300 transition hover:border-rose-400/30 hover:bg-rose-400/10 hover:text-rose-200 disabled:cursor-not-allowed disabled:opacity-40"
                    >
                      <Icon name="trash" size={13} />
                      {t("Clear indexed text", "مسح النص المفهرس")}
                    </button>
                  </div>
                </div>
              )}
            </Card>

            <div>
              <h3 className="mb-2 text-[0.62rem] font-semibold micro-label tracking-[0.16em] text-zinc-500">
                {t("Extra folders", "مجلدات إضافية")}
              </h3>
              <p className="mb-2 text-[0.72rem] leading-relaxed text-zinc-500">
                {t(
                  "Added on top of the default roots, never instead of them. Removing a default folder is done with an exclusion pattern in the config file.",
                  "تُضاف إلى الجذور الافتراضية ولا تستبدلها. لإزالة مجلد افتراضي استخدم نمط استبعاد في ملف الإعدادات.",
                )}
              </p>
              <div className="space-y-1.5">
                {draft.fileScanExtraRoots.map((root) => (
                  <div
                    key={root}
                    className="flex items-center gap-2 rounded-lg border border-white/[0.07] bg-ink-900/50 px-2.5 py-1.5"
                  >
                    <span className="min-w-0 flex-1 truncate font-mono text-[0.7rem] text-zinc-300" dir="ltr">
                      {root}
                    </span>
                    <button
                      type="button"
                      onClick={() => removeRoot(root)}
                      aria-label={t(`Remove ${root}`, `إزالة ${root}`)}
                      className="grid h-6 w-6 shrink-0 place-items-center rounded-md text-zinc-500 transition hover:bg-red-500/15 hover:text-red-300"
                    >
                      <Icon name="trash" size={12} />
                    </button>
                  </div>
                ))}
                <button
                  type="button"
                  onClick={() => void addRoot()}
                  className="flex items-center gap-1.5 rounded-lg border border-dashed border-white/15 px-3 py-2 text-[0.72rem] text-zinc-400 transition hover:border-accent-400/40 hover:text-accent-200"
                >
                  <Icon name="folder" size={12} />
                  {t("Add a folder…", "إضافة مجلد…")}
                </button>
              </div>
            </div>

            {settings.platform === "macos" && (
              <Card>
                <Toggle
                  checked={draft.localizedAppNames}
                  onChange={(next) => update("localizedAppNames", next)}
                  title={t("Localized app names", "أسماء التطبيقات المترجمة")}
                  hint={t(
                    "Shows apps under the name macOS uses for them. Slows the first index pass; applied on the next reindex.",
                    "يعرض التطبيقات بالأسماء التي يستخدمها macOS. يُبطئ الفهرسة الأولى ويُطبَّق عند إعادة الفهرسة.",
                  )}
                />
              </Card>
            )}
          </div>
        );

      case "startup":
        return (
          <div className="animate-rise space-y-4">
            <SectionHeading
              icon="power"
              title={t("Startup", "بدء التشغيل")}
              description={t(
                "Launch Zeidex when you sign in to the system. It starts hidden in the tray, so the launcher is one shortcut away without a window in the way.",
                "شغّل Zeidex عند تسجيل الدخول إلى النظام. يبدأ مخفياً في شريط النظام، فيكون المشغّل على بُعد اختصار واحد دون نافذة تعترض طريقك.",
              )}
            />
            <Card>
              {autostart === null ? (
                <p className="text-[0.74rem] text-zinc-500">
                  {t("Reading the startup entry…", "جارٍ قراءة إعداد بدء التشغيل…")}
                </p>
              ) : !autostart.supported ? (
                <div
                  role="status"
                  className="flex items-start gap-2 rounded-xl border border-amber-400/20 bg-amber-400/[0.07] px-3.5 py-3 text-[0.74rem] leading-relaxed text-amber-200"
                >
                  <Icon name="alert" size={13} className="mt-0.5 shrink-0" />
                  <span>
                    {autostart.error ??
                      t(
                        "Launch at login is not supported on this platform.",
                        "بدء التشغيل مع النظام غير مدعوم على هذه المنصة.",
                      )}
                  </span>
                </div>
              ) : (
                <>
                  <Toggle
                    checked={autostart.enabled}
                    onChange={(next) => void toggleAutostart(next)}
                    disabled={autostartBusy}
                    title={t("Start Zeidex when you sign in", "تشغيل Zeidex عند تسجيل الدخول")}
                    hint={t(
                      "Adds one entry to the system's own startup list. Nothing else is installed and no service runs in the background.",
                      "يضيف مدخلاً واحداً إلى قائمة بدء التشغيل الخاصة بالنظام. لا يُثبَّت شيء آخر ولا تعمل خدمة في الخلفية.",
                    )}
                  />
                  <div className="rounded-xl border border-white/[0.07] bg-white/[0.02] px-3.5 py-3">
                    <div className="flex flex-wrap items-center gap-2 text-[0.74rem]">
                      <span className="font-medium text-zinc-200">{autostart.mechanism}</span>
                      <span className="text-zinc-600">·</span>
                      <span
                        className={
                          autostart.enabled ? "text-emerald-300" : "text-zinc-500"
                        }
                      >
                        {autostart.enabled
                          ? t("Active", "مفعّل")
                          : t("Inactive", "غير مفعّل")}
                      </span>
                      {autostartBusy && (
                        <span className="text-zinc-500">{t("Saving…", "يحفظ…")}</span>
                      )}
                    </div>
                    {autostart.location && (
                      <p
                        className="mt-1.5 break-all font-mono text-[0.62rem] text-zinc-500"
                        dir="ltr"
                      >
                        {autostart.location}
                      </p>
                    )}
                    {autostart.command && (
                      <p
                        className="mt-1 break-all font-mono text-[0.62rem] text-zinc-600"
                        dir="ltr"
                      >
                        {autostart.command}
                      </p>
                    )}
                  </div>
                  {autostart.development && (
                    <div
                      role="status"
                      className="flex items-start gap-2 rounded-xl border border-amber-400/20 bg-amber-400/[0.07] px-3.5 py-2.5 text-[0.72rem] leading-relaxed text-amber-200"
                    >
                      <Icon name="alert" size={13} className="mt-0.5 shrink-0" />
                      <span>
                        {t(
                          "Development build: the entry points at the debug executable. Enable this only for an installed copy.",
                          "نسخة تطوير: يشير المدخل إلى الملف التنفيذي للتطوير. فعّل هذا فقط لنسخة مثبّتة.",
                        )}
                      </span>
                    </div>
                  )}
                  {autostart.error && (
                    <p
                      role="status"
                      className="text-[0.72rem] leading-relaxed text-amber-200"
                    >
                      {autostart.error}
                    </p>
                  )}
                  <div className="flex items-start gap-2 border-t border-white/[0.06] pt-4 text-[0.72rem] leading-relaxed text-zinc-500">
                    <Icon name="info" size={13} className="mt-0.5 shrink-0 text-zinc-600" />
                    <span>
                      {t(
                        "Closing the window keeps Zeidex running in the tray. Quit from the tray menu stops the process completely.",
                        "إغلاق النافذة يبقي Zeidex يعمل في شريط النظام. أما «إنهاء» من قائمة الشريط فيوقف العملية تماماً.",
                      )}
                    </span>
                  </div>
                </>
              )}
            </Card>
          </div>
        );

      case "clipboard":
        return (
          <div className="animate-rise space-y-4">
            <SectionHeading
              icon="clipboard"
              title={t("Clipboard history", "سجلّ الحافظة")}
              description={t(
                "Clips are stored on this device and are searchable from the Clipboard tab. Nothing leaves the machine.",
                "تُحفظ المقاطع على هذا الجهاز ويمكن البحث فيها من تبويب الحافظة. لا شيء يغادر الجهاز.",
              )}
            />
            <Card>
              <Toggle
                checked={!settings.clipboardPaused}
                onChange={() => void togglePause()}
                title={t("Record new clips", "تسجيل المقاطع الجديدة")}
                hint={
                  settings.clipboardPaused
                    ? t(
                        "Paused: anything copied now is not saved. Resuming does not re-add what was copied during the pause.",
                        "متوقف: ما يُنسخ الآن لا يُحفظ. الاستئناف لا يعيد إضافة ما نُسخ أثناء الإيقاف.",
                      )
                    : t(
                        "On. Pause before copying something you would not want kept.",
                        "مفعّل. أوقف التسجيل قبل نسخ شيء لا تريد الاحتفاظ به.",
                      )
                }
              />
              <NumberField
                id="clipboard-limit"
                label={t("History size", "حجم السجلّ")}
                hint={t(
                  "Unpinned text clips kept. Lowering it prunes immediately; pinned clips are always kept.",
                  "عدد المقاطع النصية غير المثبّتة المحفوظة. خفضه يحذف فوراً؛ المثبّتة تبقى دائماً.",
                )}
                value={draft.clipboardHistoryLimit}
                bounds={limits.clipboardHistoryLimit}
                error={change.errors.clipboardHistoryLimit}
                errorText={errorText}
                onChange={(next) => update("clipboardHistoryLimit", next)}
              />
              <NumberField
                id="clipboard-retention"
                label={t("Delete clips older than (days)", "حذف المقاطع الأقدم من (أيام)")}
                hint={t(
                  "0 keeps clips until the size limit removes them. Pinned clips are never deleted by age.",
                  "0 يعني الاحتفاظ حتى يزيلها حدّ الحجم. لا تُحذف المقاطع المثبّتة بالتقادم.",
                )}
                value={draft.clipboardRetentionDays}
                bounds={limits.clipboardRetentionDays}
                error={change.errors.clipboardRetentionDays}
                errorText={errorText}
                onChange={(next) => update("clipboardRetentionDays", next)}
              />
              <Toggle
                checked={draft.clipboardFilterSecrets}
                onChange={(next) => update("clipboardFilterSecrets", next)}
                title={t("Skip credentials", "تجاهل بيانات الاعتماد")}
                hint={t(
                  "Refuses clips shaped like private keys, API tokens or JWTs instead of storing them in a searchable history.",
                  "يرفض المقاطع التي تشبه المفاتيح الخاصة أو رموز API أو JWT بدل تخزينها في سجلّ قابل للبحث.",
                )}
              />
              <div className="flex flex-wrap items-center gap-2 border-t border-white/[0.06] pt-4">
                <button
                  type="button"
                  onClick={() => void clearHistory()}
                  className={`flex items-center gap-1.5 rounded-full px-3 py-1.5 text-xs font-medium transition ${
                    confirmingClear
                      ? "bg-red-500/15 text-red-300 ring-1 ring-red-400/25"
                      : "border border-white/10 text-zinc-300 hover:border-red-400/30 hover:bg-red-500/10 hover:text-red-300"
                  }`}
                >
                  <Icon name="trash" size={12} />
                  {confirmingClear
                    ? t("Click again to clear", "اضغط مرة أخرى للمسح")
                    : t("Clear history", "مسح السجلّ")}
                </button>
                <span className="text-[0.68rem] text-zinc-600">
                  {t("This cannot be undone.", "لا يمكن التراجع عن هذا.")}
                </span>
              </div>
            </Card>
          </div>
        );

      case "appearance":
        return (
          <div className="animate-rise space-y-4">
            <SectionHeading
              icon="sun"
              title={t("Theme", "السمة")}
              description={t(
                "Dark is the product's own surface. Light keeps the same layout and type on paper-white. System follows the operating system and keeps following it while the window is open.",
                "الداكنة هي سمة المنتج الأساسية. الفاتحة تحافظ على التخطيط والخط نفسهين على سطح ورقي. النظام يتبع نظام التشغيل ويستمر في متابعته أثناء فتح النافذة.",
              )}
            />
            <Card>
              <div
                className="grid grid-cols-3 gap-2"
                role="group"
                aria-label={t("Theme", "السمة")}
              >
                {([
                  { value: "dark" as const, label: "Dark", labelAr: "داكنة" },
                  { value: "light" as const, label: "Light", labelAr: "فاتحة" },
                  { value: "system" as const, label: "System", labelAr: "النظام" },
                ]).map((option) => (
                  <button
                    key={option.value}
                    type="button"
                    onClick={() => onSetTheme(option.value)}
                    aria-pressed={theme === option.value}
                    className={`rounded-xl border px-3 py-3 text-sm transition ${
                      theme === option.value
                        ? "border-accent-400/30 bg-accent-400/10 text-accent-100"
                        : "border-white/10 bg-white/[0.02] text-zinc-300 hover:border-white/20 hover:bg-white/[0.05]"
                    }`}
                  >
                    {rtl ? option.labelAr : option.label}
                  </button>
                ))}
              </div>
              <p className="text-[0.72rem] leading-relaxed text-zinc-500">
                {t(
                  "The choice is remembered on this device. Nothing about it leaves the machine.",
                  "يُحفظ الاختيار على هذا الجهاز، ولا يغادره شيء منه.",
                )}
              </p>
            </Card>
            <SectionHeading
              icon="eye"
              title={t("Direction", "اتجاه الواجهة")}
              description={t(
                "Arabic switches the interface to right-to-left and uses the Arabic typeface. The choice is remembered on this device.",
                "العربية تحوّل الواجهة إلى الاتجاه من اليمين إلى اليسار وتستخدم الخط العربي. يُحفظ الاختيار على هذا الجهاز.",
              )}
            />
            <Card>
              <div className="grid grid-cols-2 gap-2" role="group" aria-label={t("Direction", "الاتجاه")}>
                {[
                  { value: false, label: "English · LTR", labelAr: "الإنجليزية · يسار" },
                  { value: true, label: "العربية · RTL", labelAr: "العربية · يمين" },
                ].map((option) => (
                  <button
                    key={String(option.value)}
                    type="button"
                    onClick={() => onSetRtl(option.value)}
                    aria-pressed={rtl === option.value}
                    className={`rounded-xl border px-3 py-3 text-sm transition ${
                      rtl === option.value
                        ? "border-accent-400/30 bg-accent-400/10 text-accent-100"
                        : "border-white/10 bg-white/[0.02] text-zinc-300 hover:border-white/20 hover:bg-white/[0.05]"
                    }`}
                  >
                    {rtl ? option.labelAr : option.label}
                  </button>
                ))}
              </div>
              <p className="text-[0.72rem] leading-relaxed text-zinc-500">
                {t(
                  "Content stays in its logical order either way; the interface never reverses text to fake a direction.",
                  "يبقى المحتوى بترتيبه المنطقي في الحالتين؛ الواجهة لا تعكس النص لتصطنع اتجاهاً.",
                )}
              </p>
            </Card>
          </div>
        );

      case "about":
        return (
          <div className="animate-rise space-y-4">
            <SectionHeading
              icon="alert"
              title={t("About & diagnostics", "حول والتشخيص")}
              description={t(
                "Where the app keeps its data, and what is installed. Nothing here is sent anywhere.",
                "أين يحفظ التطبيق بياناته، وما المثبّت. لا شيء هنا يُرسل إلى أي جهة.",
              )}
            />
            <Card>
              <dl className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                {[
                  { label: t("Version", "الإصدار"), value: settings.version, mono: true },
                  { label: t("Platform", "المنصّة"), value: settings.platform, mono: true },
                  {
                    label: t("Indexed items", "العناصر المفهرسة"),
                    value: settings.candidateCount.toLocaleString(),
                    mono: true,
                  },
                  {
                    label: t("OCR models", "نماذج OCR"),
                    value: models
                      ? models.complete
                        ? t("ready", "جاهزة")
                        : t(`missing: ${models.missing.join(", ")}`, `ناقصة: ${models.missing.join(", ")}`)
                      : "—",
                    mono: false,
                  },
                  {
                    label: t("Config file", "ملف الإعدادات"),
                    value: settings.configPath ?? t("unresolved", "غير محدد"),
                    mono: true,
                  },
                  {
                    label: t("Database", "قاعدة البيانات"),
                    value: settings.dbPath,
                    mono: true,
                  },
                ].map((row) => (
                  <div key={row.label} className="min-w-0">
                    <dt className="text-[0.62rem] font-semibold micro-label tracking-[0.14em] text-zinc-500">
                      {row.label}
                    </dt>
                    <dd
                      className={`mt-1 truncate text-[0.72rem] text-zinc-300 ${row.mono ? "font-mono" : ""}`}
                      dir="ltr"
                      title={row.value}
                    >
                      {row.value}
                    </dd>
                  </div>
                ))}
              </dl>
              <div className="flex flex-wrap items-center gap-2 border-t border-white/[0.06] pt-4">
                <button
                  type="button"
                  onClick={() => void revealConfig()}
                  disabled={!settings.configPath}
                  className="flex items-center gap-1.5 rounded-full border border-white/10 px-3 py-1.5 text-xs text-zinc-300 transition hover:bg-white/10 hover:text-white disabled:opacity-40"
                >
                  <Icon name="folder" size={12} />
                  {t("Show config", "إظهار الإعدادات")}
                </button>
                <button
                  type="button"
                  onClick={() => void copyDiagnostics()}
                  className="flex items-center gap-1.5 rounded-full border border-white/10 px-3 py-1.5 text-xs text-zinc-300 transition hover:bg-white/10 hover:text-white"
                >
                  <Icon name="clipboard" size={12} />
                  {t("Copy diagnostics", "نسخ التشخيص")}
                </button>
              </div>
            </Card>
          </div>
        );
    }
  };

  const dirty = change.dirty;
  const hasErrors = Object.keys(change.errors).length > 0;

  return (
    <div className="window-shell flex h-full items-center justify-center px-3 py-3">
      <div className="panel flex h-[94vh] w-[min(1120px,97vw)] flex-col">
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
              {t("Settings", "الإعدادات")}
            </span>
            <span className="mt-1 text-[0.5rem] font-medium micro-label tracking-[0.3em] text-zinc-500">
              Zeidex Search
            </span>
          </div>
          <div className="ms-auto flex items-center gap-2">
            <span
              className="rounded-full border border-white/10 bg-white/[0.03] px-2.5 py-1 font-mono text-[0.66rem] text-zinc-400"
              dir="ltr"
            >
              v{settings?.version ?? "—"}
            </span>
            <WindowControls rtl={rtl} />
          </div>
        </header>

        {notice && (
          <div
            role={notice.kind === "error" ? "alert" : "status"}
            className={`mx-5 mb-3 flex items-center gap-2 rounded-lg border px-3 py-2 text-xs ${
              notice.kind === "error"
                ? "border-red-400/20 bg-red-400/[0.07] text-red-200"
                : notice.kind === "warning"
                  ? "border-amber-400/20 bg-amber-400/[0.07] text-amber-200"
                  : "border-emerald-400/20 bg-emerald-400/[0.07] text-emerald-200"
            }`}
          >
            <Icon name={notice.kind === "done" ? "check" : "alert"} size={13} />
            <span className="min-w-0 flex-1">{notice.message}</span>
            <button
              type="button"
              onClick={() => setNotice(null)}
              aria-label={t("Dismiss", "إغلاق")}
              className="grid h-5 w-5 shrink-0 place-items-center rounded text-zinc-400 transition hover:bg-white/10 hover:text-zinc-200"
            >
              <Icon name="x" size={11} />
            </button>
          </div>
        )}

        <div className="grid min-h-0 flex-1 grid-cols-1 gap-px bg-white/[0.06] sm:grid-cols-[13rem_1fr]">
          <nav
            aria-label={t("Settings sections", "أقسام الإعدادات")}
            className="flex flex-row gap-1 overflow-x-auto bg-ink-950/60 p-3 sm:flex-col sm:overflow-visible"
          >
            {SECTIONS.map((entry) => (
              <button
                key={entry.id}
                type="button"
                onClick={() => setSection(entry.id)}
                aria-current={section === entry.id ? "page" : undefined}
                className={`flex shrink-0 items-center gap-2 rounded-lg px-3 py-2 text-start text-[0.78rem] transition ${
                  section === entry.id
                    ? "bg-white/[0.06] text-white"
                    : "text-zinc-400 hover:bg-white/[0.03] hover:text-zinc-200"
                }`}
              >
                <Icon name={entry.icon} size={13} />
                {rtl ? entry.labelAr : entry.label}
              </button>
            ))}
          </nav>

          <div className="min-h-0 overflow-y-auto bg-ink-950/40 p-5">
            {loading ? (
              <div className="space-y-3" aria-busy="true">
                <div className="skeleton h-4 w-40" />
                <div className="skeleton h-20 w-full" />
                <div className="skeleton h-20 w-full" />
              </div>
            ) : settings === null ? (
              <p className="text-sm text-zinc-400">
                {t(
                  "The settings could not be loaded.",
                  "تعذّر تحميل الإعدادات.",
                )}
              </p>
            ) : (
              renderSection()
            )}
          </div>
        </div>

        <footer className="flex flex-wrap items-center gap-3 border-t border-white/[0.07] bg-black/20 px-5 py-2.5 text-[0.7rem]">
          <span
            role="status"
            aria-live="polite"
            className={hasErrors ? "text-red-300" : dirty ? "text-amber-300" : "text-zinc-500"}
          >
            {saving
              ? t("Saving…", "يحفظ…")
              : hasErrors
                ? t("Check the highlighted fields", "راجع الحقول المعلّمة")
                : dirty
                  ? t("Unsaved changes", "تغييرات غير محفوظة")
                  : t("All changes saved", "كل التغييرات محفوظة")}
          </span>
          <span className="ms-auto flex items-center gap-2">
            <button
              type="button"
              onClick={onBack}
              className="rounded-full px-3 py-1.5 text-xs text-zinc-400 transition hover:bg-white/10 hover:text-zinc-200"
            >
              {t("Back to search", "رجوع إلى البحث")}
            </button>
            <button
              type="button"
              onClick={() => void save()}
              disabled={!dirty || saving}
              className="flex items-center gap-1.5 rounded-full bg-gradient-to-r from-accent-400 to-iris-400 px-3.5 py-1.5 text-xs font-semibold text-ink-950 shadow-[0_8px_24px_-10px_rgb(34_211_238/0.8)] transition hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-40"
            >
              <Icon name="check" size={12} />
              {saving ? t("Saving…", "يحفظ…") : t("Save changes", "حفظ التغييرات")}
            </button>
          </span>
        </footer>
      </div>
    </div>
  );
}
