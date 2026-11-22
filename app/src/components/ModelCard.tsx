import type {
  DownloadPhase,
  DownloadProgress,
  ModelBenchmarkView,
  ModelPackView,
  ModelVerifyView,
} from "../api";
import { formatBytes, formatCer, formatSpeed, percentOf } from "../model-format";
import { Icon } from "./Icons";

const ACTIVE_PHASES: DownloadPhase[] = ["downloading", "verifying", "installing"];

export function isActivePhase(phase: DownloadPhase | undefined): boolean {
  return phase !== undefined && ACTIVE_PHASES.includes(phase);
}

const TIER_TONE: Record<string, string> = {
  compact: "border-white/15 bg-white/[0.04] text-zinc-300",
  balanced: "border-accent-400/30 bg-accent-400/10 text-accent-200",
  precision: "border-iris-400/30 bg-iris-400/10 text-iris-400",
};

const PHASE_LABEL: Record<DownloadPhase, { en: string; ar: string }> = {
  downloading: { en: "Downloading", ar: "جارٍ التنزيل" },
  verifying: { en: "Verifying", ar: "جارٍ التحقق" },
  installing: { en: "Installing", ar: "جارٍ التثبيت" },
  done: { en: "Done", ar: "تم" },
  cancelled: { en: "Cancelled", ar: "أُلغي" },
  error: { en: "Failed", ar: "فشل" },
};

function statusTone(pack: ModelPackView): string {
  if (pack.active) return "border-accent-400/40 bg-accent-400/10 text-accent-200";
  if (pack.state === "installed") {
    return "border-emerald-400/30 bg-emerald-400/10 text-emerald-300";
  }
  if (pack.state === "partial") {
    return "border-amber-400/30 bg-amber-400/10 text-amber-300";
  }
  return "border-white/10 bg-white/[0.03] text-zinc-400";
}

function statusLabel(pack: ModelPackView, t: (en: string, ar: string) => string): string {
  if (pack.active) return t("Active", "مفعّل");
  if (pack.state === "installed") return t("Installed", "مثبّت");
  if (pack.state === "partial") return t("Partial", "جزئي");
  return t("Not installed", "غير مثبّت");
}

export interface ModelCardProps {
  pack: ModelPackView;
  rtl: boolean;
  t: (en: string, ar: string) => string;
  progress: DownloadProgress | null;
  downloadingAny: boolean;
  busy: boolean;
  benchmark?: ModelBenchmarkView;
  verify?: ModelVerifyView;
  detailsOpen: boolean;
  confirmingRemove: boolean;
  /** The advisor's pick for this machine, shown on the card it chose. */
  recommended: boolean;
  onDownload: () => void;
  onCancel: () => void;
  onActivate: () => void;
  onVerify: () => void;
  onTest: () => void;
  onRemove: () => void;
  onConfirmRemove: (next: boolean) => void;
  onToggleDetails: () => void;
}

export default function ModelCard({
  pack,
  rtl,
  t,
  progress,
  downloadingAny,
  busy,
  benchmark,
  verify,
  detailsOpen,
  confirmingRemove,
  recommended,
  onDownload,
  onCancel,
  onActivate,
  onVerify,
  onTest,
  onRemove,
  onConfirmRemove,
  onToggleDetails,
}: ModelCardProps) {
  const mine = progress?.pack_id === pack.id ? progress : null;
  const downloading = mine !== null && isActivePhase(mine.phase);
  const installed = pack.bundled || pack.state === "installed";
  const percent = mine ? percentOf(mine.downloaded, mine.total) : 0;
  const name = rtl ? pack.name_ar : pack.name;
  const summary = rtl ? pack.summary_ar : pack.summary;
  const disabled = busy || (downloadingAny && !downloading);

  return (
    <section
      className={`flex flex-col rounded-2xl border p-4 transition ${
        pack.active
          ? "border-accent-400/35 bg-accent-400/[0.045]"
          : "border-white/[0.07] bg-white/[0.02]"
      }`}
      aria-label={name}
    >
      <div className="flex flex-wrap items-center gap-2">
        <span
          className={`rounded-full border px-2 py-0.5 text-[0.6rem] font-semibold uppercase tracking-[0.12em] ${
            TIER_TONE[pack.tier] ?? TIER_TONE.compact
          }`}
        >
          {rtl ? pack.tier_label_ar : pack.tier_label}
        </span>
        <h3 className="text-sm font-semibold text-white">{name}</h3>
        {recommended && (
          <span className="rounded-full border border-iris-400/30 bg-iris-400/10 px-2 py-0.5 text-[0.58rem] text-iris-400">
            {t("For this device", "لهذا الجهاز")}
          </span>
        )}
        <span
          className={`ms-auto rounded-full border px-2 py-0.5 text-[0.6rem] font-medium ${statusTone(pack)}`}
        >
          {statusLabel(pack, t)}
        </span>
      </div>

      <p className="mt-2 min-h-[2.5rem] text-[0.72rem] leading-relaxed text-zinc-400">{summary}</p>

      <dl className="mt-3 grid grid-cols-2 gap-x-3 gap-y-1.5 text-[0.65rem]">
        <div className="flex items-center justify-between gap-2">
          <dt className="text-zinc-500">{t("RAM", "الذاكرة")}</dt>
          <dd className="font-mono text-zinc-300">{pack.ram_mb} MB</dd>
        </div>
        <div className="flex items-center justify-between gap-2">
          <dt className="text-zinc-500">{t("Download", "التنزيل")}</dt>
          <dd className="font-mono text-zinc-300">{formatBytes(pack.download_bytes)}</dd>
        </div>
        <div className="flex items-center justify-between gap-2">
          <dt className="text-zinc-500">{t("On disk", "على القرص")}</dt>
          <dd className="font-mono text-zinc-300">
            {pack.installed_bytes > 0 ? formatBytes(pack.installed_bytes) : "—"}
          </dd>
        </div>
        <div className="flex items-center justify-between gap-2">
          <dt className="text-zinc-500">{t("Languages", "اللغات")}</dt>
          <dd className="text-zinc-300">
            {[pack.arabic ? t("Arabic", "العربية") : null, pack.english ? "English" : null]
              .filter(Boolean)
              .join(" · ")}
          </dd>
        </div>
        <div className="flex items-center justify-between gap-2">
          <dt className="text-zinc-500">{t("Runs on", "يعمل على")}</dt>
          <dd className="text-zinc-300">{t("CPU · GPU optional", "المعالج · GPU اختياري")}</dd>
        </div>
        <div className="flex items-center justify-between gap-2">
          <dt className="text-zinc-500">{t("Files", "الملفات")}</dt>
          <dd className="font-mono text-zinc-300">{pack.files.length}</dd>
        </div>
      </dl>

      {mine && (downloading || mine.phase === "error" || mine.phase === "cancelled") && (
        <div className="mt-3 rounded-xl border border-white/10 bg-ink-900/70 p-2.5">
          <div className="flex items-center justify-between text-[0.62rem]">
            <span
              className={
                mine.phase === "error"
                  ? "text-rose-300"
                  : mine.phase === "cancelled"
                    ? "text-amber-300"
                    : "text-zinc-300"
              }
            >
              {rtl ? PHASE_LABEL[mine.phase].ar : PHASE_LABEL[mine.phase].en}
              {mine.file ? ` · ${mine.file}` : ""}
            </span>
            <span className="font-mono text-zinc-400">{percent}%</span>
          </div>
          <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-white/10">
            <div
              className={`h-full rounded-full transition-[width] duration-200 ${
                mine.phase === "error" ? "bg-rose-400" : "bg-accent-400"
              }`}
              style={{ width: `${percent}%` }}
            />
          </div>
          <div className="mt-1 flex items-center justify-between font-mono text-[0.58rem] text-zinc-500">
            <span>
              {formatBytes(mine.downloaded)} / {formatBytes(mine.total)}
            </span>
            <span>
              {downloading
                ? formatSpeed(mine.speed_bps)
                : `${formatBytes(Math.max(0, mine.total - mine.downloaded))} ${t("left", "متبقٍ")}`}
            </span>
          </div>
          {mine.message && (
            <p className="mt-1.5 break-words text-[0.6rem] leading-relaxed text-rose-300/90">
              {mine.message}
            </p>
          )}
        </div>
      )}

      {verify && !verify.ok && (
        <ul className="mt-3 space-y-1 rounded-xl border border-amber-400/25 bg-amber-400/[0.06] p-2.5 text-[0.62rem] text-amber-200">
          {verify.problems.slice(0, 4).map((problem) => (
            <li key={problem} className="break-words">
              {problem}
            </li>
          ))}
          {verify.problems.length > 4 && <li>+{verify.problems.length - 4}</li>}
        </ul>
      )}

      {benchmark && benchmark.pack_id === pack.id && (
        <div className="mt-3 rounded-xl border border-white/10 bg-ink-900/70 p-2.5 text-[0.62rem]">
          <div className="flex items-center justify-between text-zinc-400">
            <span>{t("Measured on this machine", "قياس على هذا الجهاز")}</span>
            <span className="font-mono">{benchmark.total_ms} ms</span>
          </div>
          <table className="mt-1.5 w-full text-start">
            <thead className="text-zinc-500">
              <tr>
                <th className="text-start font-normal">{t("Sample", "العيّنة")}</th>
                <th className="text-end font-normal">{t("Time", "الزمن")}</th>
                <th className="text-end font-normal">CER</th>
              </tr>
            </thead>
            <tbody className="font-mono text-zinc-300">
              {benchmark.samples.map((sample) => (
                <tr key={sample.name}>
                  <td className="text-start">
                    {sample.name === "arabic" ? t("Arabic", "عربية") : t("English", "إنجليزية")}
                  </td>
                  <td className="text-end">{sample.milliseconds} ms</td>
                  <td className="text-end">{formatCer(sample.cer)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="mt-1 text-[0.58rem] text-zinc-500">
            {t("Load", "التحميل")} {benchmark.load_ms} ms
            {benchmark.memory_mb > 0
              ? ` · ${t("Memory", "الذاكرة")} ${benchmark.memory_mb} MB`
              : ""}
          </p>
        </div>
      )}

      <div className="mt-auto flex flex-wrap items-center gap-1.5 pt-3">
        {downloading ? (
          <Action
            label={t("Cancel", "إلغاء")}
            icon="x"
            onClick={onCancel}
            disabled={busy}
          />
        ) : !installed ? (
          <Action
            label={
              pack.state === "partial"
                ? t("Resume", "متابعة")
                : t("Download", "تنزيل")
            }
            icon="download"
            onClick={onDownload}
            disabled={disabled}
            primary
          />
        ) : pack.active ? (
          <span className="flex items-center gap-1.5 rounded-lg border border-accent-400/30 bg-accent-400/10 px-2.5 py-1.5 text-[0.65rem] font-medium text-accent-200">
            <Icon name="check" size={12} />
            {t("In use", "قيد الاستخدام")}
          </span>
        ) : (
          <Action
            label={t("Activate", "تفعيل")}
            icon="play"
            onClick={onActivate}
            disabled={disabled}
            primary
          />
        )}

        {installed && (
          <Action label={t("Test", "اختبار")} icon="scan" onClick={onTest} disabled={disabled} />
        )}
        {installed && (
          <Action
            label={t("Verify", "تحقق")}
            icon="refresh"
            onClick={onVerify}
            disabled={disabled}
          />
        )}
        {!pack.bundled && (
          confirmingRemove ? (
            <span className="flex items-center gap-1.5">
              <Action
                label={t("Confirm", "تأكيد")}
                icon="trash"
                onClick={onRemove}
                disabled={disabled}
              />
              <Action
                label={t("Keep", "تراجع")}
                icon="x"
                onClick={() => onConfirmRemove(false)}
              />
            </span>
          ) : (
            <Action
              label={t("Remove", "إزالة")}
              icon="trash"
              onClick={() => onConfirmRemove(true)}
              disabled={disabled}
            />
          )
        )}
        <Action
          label={detailsOpen ? t("Hide files", "إخفاء الملفات") : t("Files", "الملفات")}
          icon="info"
          onClick={onToggleDetails}
        />
      </div>

      {detailsOpen && (
        <div className="mt-2.5 rounded-xl border border-white/[0.07] bg-ink-950/50 p-2.5">
          <p className="break-all font-mono text-[0.58rem] text-zinc-500">{pack.dir}</p>
          <ul className="mt-1.5 space-y-0.5">
            {pack.files.map((file) => (
              <li
                key={file.name}
                className="flex items-center justify-between gap-2 font-mono text-[0.6rem]"
              >
                <span className={file.present ? "text-zinc-300" : "text-rose-300"}>
                  {file.name}
                </span>
                <span className="text-zinc-500">
                  {file.present ? formatBytes(file.actual_bytes) : t("missing", "مفقود")}
                </span>
              </li>
            ))}
          </ul>
          <p className="mt-1.5 text-[0.56rem] leading-relaxed text-zinc-600">
            {pack.source} · {pack.license}
            {pack.rec_ar_visual ? " · Arabic visual-order recognition" : ""}
          </p>
        </div>
      )}
    </section>
  );
}

function Action({
  label,
  icon,
  onClick,
  disabled,
  primary,
}: {
  label: string;
  icon: "download" | "play" | "check" | "refresh" | "trash" | "x" | "eye" | "info" | "scan";
  onClick: () => void;
  disabled?: boolean;
  primary?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={
        primary
          ? "flex items-center gap-1.5 rounded-lg bg-accent-400 px-2.5 py-1.5 text-[0.65rem] font-semibold text-ink-950 transition hover:bg-accent-300 disabled:cursor-not-allowed disabled:opacity-40"
          : "flex items-center gap-1.5 rounded-lg border border-white/10 px-2.5 py-1.5 text-[0.65rem] text-zinc-300 transition hover:bg-white/10 hover:text-white disabled:cursor-not-allowed disabled:opacity-40"
      }
    >
      <Icon name={icon} size={12} />
      {label}
    </button>
  );
}
