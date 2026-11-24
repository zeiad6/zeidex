import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type AdvisorView,
  type DownloadProgress,
  type ModelBenchmarkView,
  type ModelPackView,
  type ModelVerifyView,
} from "../api";
import { formatBytes } from "../model-format";
import AdvisorCard from "./AdvisorCard";
import { BrandMark } from "./Brand";
import { Icon } from "./Icons";
import WindowControls from "./WindowControls";
import ThemeToggle from "./ThemeToggle";
import ModelCard from "./ModelCard";

/** How the download area reports a finished job, so the message is not lost
 *  when the progress event has already been superseded by the reload. */
type Notice = { kind: "notice" | "error"; text: string };

export interface ModelsProps {
  rtl: boolean;
  onBack: () => void;
}

export default function Models({ rtl, onBack }: ModelsProps) {
  const [packs, setPacks] = useState<ModelPackView[]>([]);
  const [advisor, setAdvisor] = useState<AdvisorView | null>(null);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [benchmarks, setBenchmarks] = useState<Record<string, ModelBenchmarkView>>({});
  const [verifies, setVerifies] = useState<Record<string, ModelVerifyView>>({});
  const [details, setDetails] = useState<Set<string>>(new Set());
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [loading, setLoading] = useState(true);
  const [comparing, setComparing] = useState(false);
  const tierGrid = useRef<HTMLDivElement | null>(null);

  const t = useCallback((en: string, ar: string) => (rtl ? ar : en), [rtl]);

  const reload = useCallback(async () => {
    const [nextPacks, nextAdvisor] = await Promise.all([
      api.modelsCatalog(),
      api.modelsAdvisor(),
    ]);
    setPacks(nextPacks);
    setAdvisor(nextAdvisor);
  }, []);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        await reload();
      } catch (err) {
        if (active) setNotice({ kind: "error", text: String(err) });
      } finally {
        if (active) setLoading(false);
      }
    })();
    return () => {
      active = false;
    };
  }, [reload]);

  // One event stream carries every download; the card filters by pack id.
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    api
      .onModelsProgress((event) => {
        if (!active) return;
        setProgress(event);
        if (event.phase === "done" || event.phase === "cancelled") {
          void reload().catch(() => undefined);
        }
        if (event.phase === "error" && event.message) {
          setNotice({ kind: "error", text: event.message });
        }
      })
      .then((fn) => {
        if (active) unlisten = fn;
        else fn();
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [reload]);

  const downloadingAny =
    progress !== null &&
    (progress.phase === "downloading" ||
      progress.phase === "verifying" ||
      progress.phase === "installing");

  const download = useCallback(
    async (pack: ModelPackView) => {
      if (busy) return;
      setBusy(pack.id);
      setNotice(null);
      try {
        await api.modelsDownload(pack.id);
        await reload();
        setNotice({
          kind: "notice",
          text: t(`${pack.name} installed and verified.`, `تم تنزيل ${pack.name_ar} والتحقق منه.`),
        });
      } catch (err) {
        // A cancel is a user decision, not a failure worth a red banner.
        const text = String(err);
        if (!text.includes("cancelled")) {
          setNotice({ kind: "error", text });
        }
        await reload().catch(() => undefined);
      } finally {
        setBusy(null);
        setProgress(null);
      }
    },
    [busy, reload, t],
  );

  const cancel = useCallback(async () => {
    try {
      await api.modelsCancelDownload();
    } catch (err) {
      setNotice({ kind: "error", text: String(err) });
    }
  }, []);

  const activate = useCallback(
    async (pack: ModelPackView) => {
      if (busy) return;
      setBusy(pack.id);
      setNotice(null);
      try {
        const next = await api.modelsActivate(pack.id);
        setPacks(next);
        setNotice({
          kind: "notice",
          text: t(
            `${pack.name} is now the active OCR model.`,
            `${pack.name_ar} صار نموذج OCR المفعّل.`,
          ),
        });
      } catch (err) {
        setNotice({ kind: "error", text: String(err) });
      } finally {
        setBusy(null);
      }
    },
    [busy, t],
  );

  const remove = useCallback(
    async (pack: ModelPackView) => {
      if (busy) return;
      setBusy(pack.id);
      setNotice(null);
      setConfirmRemove(null);
      try {
        const next = await api.modelsRemove(pack.id);
        setPacks(next);
        setVerifies((current) => {
          const copy = { ...current };
          delete copy[pack.id];
          return copy;
        });
        setBenchmarks((current) => {
          const copy = { ...current };
          delete copy[pack.id];
          return copy;
        });
        setNotice({
          kind: "notice",
          text: t(`${pack.name} was removed.`, `أُزيل ${pack.name_ar}.`),
        });
      } catch (err) {
        setNotice({ kind: "error", text: String(err) });
      } finally {
        setBusy(null);
      }
    },
    [busy, t],
  );

  const verify = useCallback(
    async (pack: ModelPackView) => {
      if (busy) return;
      setBusy(pack.id);
      setNotice(null);
      try {
        const result = await api.modelsVerify(pack.id);
        setVerifies((current) => ({ ...current, [pack.id]: result }));
        setNotice({
          kind: result.ok ? "notice" : "error",
          text: result.ok
            ? t(
                `${pack.name}: ${result.checked_files} files verified.`,
                `${pack.name_ar}: تم التحقق من ${result.checked_files} ملفات.`,
              )
            : t(
                `${pack.name} has problems.`,
                `${pack.name_ar} به مشكلات.`,
              ),
        });
      } catch (err) {
        setNotice({ kind: "error", text: String(err) });
      } finally {
        setBusy(null);
      }
    },
    [busy, t],
  );

  const benchmark = useCallback(
    async (pack: ModelPackView) => {
      if (busy) return;
      setBusy(pack.id);
      setNotice(null);
      try {
        const result = await api.modelsBenchmark(pack.id);
        setBenchmarks((current) => ({ ...current, [pack.id]: result }));
        setDetails((current) => new Set(current).add(pack.id));
        setNotice({
          kind: "notice",
          text: t(
            `${pack.name}: two samples in ${result.total_ms} ms.`,
            `${pack.name_ar}: عيّنتان في ${result.total_ms} مللي ثانية.`,
          ),
        });
      } catch (err) {
        setNotice({ kind: "error", text: String(err) });
      } finally {
        setBusy(null);
      }
    },
    [busy, t],
  );

  /**
   * Benchmarks every ready pack, one after another, so the numbers are
   * comparable: running two ONNX sessions at once would make each look slower
   * and the memory figures would overlap. Only packs already on disk are run.
   */
  const compare = useCallback(async () => {
    if (busy || comparing) return;
    const ready = packs.filter((pack) => pack.bundled || pack.state === "installed");
    if (ready.length < 2) {
      setNotice({
        kind: "notice",
        text: t(
          "Install a second model to compare them.",
          "ثبّت نموذجاً ثانياً للمقارنة بينهما.",
        ),
      });
      return;
    }
    setComparing(true);
    setNotice(null);
    const measured: Record<string, ModelBenchmarkView> = {};
    try {
      for (const pack of ready) {
        const result = await api.modelsBenchmark(pack.id);
        measured[pack.id] = result;
        setBenchmarks((current) => ({ ...current, [pack.id]: result }));
        setDetails((current) => new Set(current).add(pack.id));
      }
      const lines = ready
        .map((pack) => `${pack.name}: ${measured[pack.id]?.total_ms ?? "—"} ms`)
        .join(" · ");
      setNotice({ kind: "notice", text: t(`Measured — ${lines}`, `القياس — ${lines}`) });
    } catch (err) {
      setNotice({ kind: "error", text: String(err) });
    } finally {
      setComparing(false);
    }
  }, [busy, comparing, packs, t]);

  const toggleDetails = useCallback((id: string) => {
    setDetails((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const recommended = advisor?.recommended_pack;
  const installedBytes = useMemo(
    () => packs.reduce((total, pack) => total + pack.installed_bytes, 0),
    [packs],
  );

  return (
    <div className="window-shell flex h-full items-center justify-center px-3 py-3">
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
              {t("OCR Models", "نماذج OCR")}
            </span>
            <span className="mt-1 text-[0.5rem] font-medium uppercase tracking-[0.3em] text-zinc-500">
              {t("Runs entirely on this device", "يعمل بالكامل على هذا الجهاز")}
            </span>
          </div>
          <span className="ms-2 rounded-full border border-white/10 bg-white/[0.03] px-2.5 py-1 font-mono text-[0.625rem] text-zinc-400">
            {formatBytes(installedBytes)}
          </span>
          <div className="ms-auto flex items-center gap-1.5">
            <button
              type="button"
              onClick={() => void compare()}
              disabled={comparing || busy !== null || downloadingAny}
              className="flex items-center gap-1.5 rounded-full border border-white/10 bg-white/[0.03] px-3 py-1.5 text-xs font-medium text-zinc-300 transition hover:border-iris-400/30 hover:bg-iris-400/10 hover:text-iris-400 disabled:cursor-not-allowed disabled:opacity-40"
            >
              <Icon name="scan" size={13} />
              {comparing
                ? t("Measuring…", "جارٍ القياس…")
                : t("Compare models", "مقارنة النماذج")}
            </button>
            <button
              type="button"
              onClick={() => {
                setNotice(null);
                void reload().catch((err: unknown) =>
                  setNotice({ kind: "error", text: String(err) }),
                );
              }}
              className="flex items-center gap-1.5 rounded-full border border-white/10 bg-white/[0.03] px-3 py-1.5 text-xs font-medium text-zinc-300 transition hover:border-accent-400/30 hover:bg-accent-400/10 hover:text-accent-200"
            >
              <Icon name="refresh" size={13} />
              {t("Refresh", "تحديث")}
            </button>
          <ThemeToggle rtl={rtl} />
          <WindowControls rtl={rtl} />
          </div>
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto px-4 pb-4">
          {notice && (
            <div
              role="status"
              className={`mb-3 flex items-start gap-2 rounded-xl border px-3 py-2 text-[0.68rem] ${
                notice.kind === "error"
                  ? "border-rose-400/30 bg-rose-400/[0.07] text-rose-200"
                  : "border-accent-400/25 bg-accent-400/[0.06] text-accent-200"
              }`}
            >
              <Icon name={notice.kind === "error" ? "alert" : "check"} size={13} />
              <span className="min-w-0 flex-1 break-words">{notice.text}</span>
              <button
                type="button"
                onClick={() => setNotice(null)}
                aria-label={t("Dismiss", "إغلاق")}
                className="shrink-0 text-current/70 transition hover:text-current"
              >
                <Icon name="x" size={12} />
              </button>
            </div>
          )}

          {loading ? (
            <div className="grid h-40 place-items-center text-[0.7rem] text-zinc-500">
              {t("Reading the model catalog…", "جارٍ قراءة كتالوج النماذج…")}
            </div>
          ) : (
            <div className="space-y-3">
              {advisor && (
                <AdvisorCard
                  advisor={advisor}
                  rtl={rtl}
                  t={t}
                  busy={busy !== null || downloadingAny}
                  onDownloadRecommended={() => {
                    if (recommended) void download(recommended);
                  }}
                  onTestRecommended={() => {
                    if (recommended) void benchmark(recommended);
                  }}
                  onChooseManually={() =>
                    tierGrid.current?.scrollIntoView({ behavior: "smooth", block: "start" })
                  }
                />
              )}

              <div ref={tierGrid} className="grid gap-3 lg:grid-cols-3">
                {packs.map((pack) => (
                  <ModelCard
                    key={pack.id}
                    pack={pack}
                    rtl={rtl}
                    t={t}
                    progress={progress}
                    downloadingAny={downloadingAny}
                    busy={busy !== null}
                    benchmark={benchmarks[pack.id]}
                    verify={verifies[pack.id]}
                    detailsOpen={details.has(pack.id)}
                    confirmingRemove={confirmRemove === pack.id}
                    recommended={advisor?.recommendation.pack_id === pack.id}
                    onDownload={() => void download(pack)}
                    onCancel={() => void cancel()}
                    onActivate={() => void activate(pack)}
                    onVerify={() => void verify(pack)}
                    onTest={() => void benchmark(pack)}
                    onRemove={() => void remove(pack)}
                    onConfirmRemove={(next) => setConfirmRemove(next ? pack.id : null)}
                    onToggleDetails={() => toggleDetails(pack.id)}
                  />
                ))}
              </div>

              <p className="px-1 pb-1 text-[0.58rem] leading-relaxed text-zinc-500">
                {t(
                  "Models are downloaded from their pinned upstream releases and checked against a published SHA-256 before they can be installed. Nothing about your documents leaves this machine, and the app sends no telemetry.",
                  "تُنزَّل النماذج من إصداراتها الأصلية المثبّتة ويُتحقق من بصمة SHA-256 قبل التثبيت. لا يخرج أي شيء عن مستنداتك من هذا الجهاز، ولا يرسل التطبيق أي بيانات.",
                )}
                {advisor ? (
                  <>
                    {" "}
                    <span className="font-mono">{advisor.profile.arch}</span>
                    {" · "}
                    <span className="font-mono">{advisor.profile.os}</span>
                  </>
                ) : null}
              </p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
