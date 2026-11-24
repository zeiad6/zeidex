import type { AdvisorView } from "../api";
import { formatBytes } from "../model-format";
import { Icon } from "./Icons";

export interface AdvisorCardProps {
  advisor: AdvisorView;
  rtl: boolean;
  t: (en: string, ar: string) => string;
  busy: boolean;
  onDownloadRecommended: () => void;
  onTestRecommended: () => void;
  onChooseManually: () => void;
}

/** One label/value line in the hardware summary. */
function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-3 py-0.5">
      <dt className="shrink-0 text-[0.65rem] text-zinc-500">{label}</dt>
      <dd className="truncate text-end font-mono text-[0.65rem] text-zinc-300" title={value}>
        {value}
      </dd>
    </div>
  );
}

export default function AdvisorCard({
  advisor,
  rtl,
  t,
  busy,
  onDownloadRecommended,
  onTestRecommended,
  onChooseManually,
}: AdvisorCardProps) {
  const { profile, recommendation, recommended_pack: pack } = advisor;
  const classLabel = rtl ? advisor.class_label_ar : advisor.class_label;
  const reason = rtl ? recommendation.reason_ar : recommendation.reason;
  const gpu = profile.gpu
    ? `${profile.gpu}${profile.gpu_memory_mb ? ` · ${formatBytes(profile.gpu_memory_mb * 1024 * 1024)}` : ""}`
    : t("Not detected", "لم يُكتشف");
  const ram = `${formatBytes(profile.available_ram_mb * 1024 * 1024)} ${t("free of", "متاح من")} ${formatBytes(profile.total_ram_mb * 1024 * 1024)}`;
  const ready = pack.bundled || pack.state === "installed";
  const downloadLabel =
    pack.state === "partial" ? t("Resume download", "متابعة التنزيل") : t("Download model", "تنزيل النموذج");

  return (
    <section className="rounded-2xl border border-white/[0.07] bg-surface p-4">
      <div className="flex flex-wrap items-center gap-2">
        <Icon name="cpu" size={15} />
        <h2 className="text-sm font-semibold text-white">
          {t("Hardware advisor", "مستشار العتاد")}
        </h2>
        <span className="rounded-full border border-iris-400/30 bg-iris-400/10 px-2 py-0.5 text-[0.6rem] font-semibold text-iris-400">
          {classLabel}
        </span>
        <span className="ms-auto font-mono text-[0.6rem] text-zinc-500">
          {advisor.score}/{advisor.max_score}
        </span>
      </div>

      <div className="mt-3 grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
        <dl className="min-w-0">
          <Row
            label={t("CPU", "المعالج")}
            value={
              profile.cpu_model ||
              t("Unknown", "غير معروف")
            }
          />
          <Row
            label={t("Cores", "الأنوية")}
            value={`${profile.physical_cores} ${t("physical", "فيزيائي")} · ${profile.logical_cores} ${t("logical", "منطقي")}`}
          />
          <Row label={t("Memory", "الذاكرة")} value={ram} />
          <Row label="GPU" value={gpu} />
          <Row
            label={t("Free disk", "المساحة الحرة")}
            value={formatBytes(profile.free_disk_mb * 1024 * 1024)}
          />
          <Row label={t("System", "النظام")} value={`${profile.os} · ${profile.arch}`} />
        </dl>

        <div className="min-w-0">
          <p className="text-[0.65rem] font-medium text-zinc-400">
            {t("How the class was measured", "كيف صُنّف الجهاز")}
          </p>
          <ul className="mt-1 space-y-0.5">
            {advisor.criteria.map((criterion) => (
              <li
                key={criterion.key}
                className="flex items-center gap-1.5 text-[0.62rem] leading-5"
              >
                <span
                  className={`grid size-3.5 shrink-0 place-items-center rounded-full border ${
                    criterion.met
                      ? "border-accent-400/40 bg-accent-400/15 text-accent-300"
                      : "border-white/10 text-zinc-600"
                  }`}
                  aria-hidden
                >
                  {criterion.met && <Icon name="check" size={8} />}
                </span>
                <span className={criterion.met ? "text-zinc-300" : "text-zinc-500"}>
                  {rtl ? criterion.label_ar : criterion.label}
                </span>
                <span className="ms-auto truncate font-mono text-[0.58rem] text-zinc-500">
                  {criterion.detail}
                </span>
              </li>
            ))}
          </ul>
        </div>
      </div>

      <div className="mt-3 rounded-xl border border-accent-400/20 bg-accent-400/[0.05] p-3">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-[0.62rem] uppercase tracking-[0.1em] text-accent-300/80">
            {t("Recommended", "الموصى به")}
          </span>
          <span className="text-sm font-semibold text-white">
            {rtl ? pack.name_ar : pack.name}
          </span>
          <span className="rounded-full border border-white/10 px-2 py-0.5 text-[0.58rem] text-zinc-400">
            {rtl ? pack.tier_label_ar : pack.tier_label}
          </span>
          {recommendation.downgraded && (
            <span className="rounded-full border border-amber-400/30 bg-amber-400/10 px-2 py-0.5 text-[0.58rem] text-amber-300">
              {t("Stepped down for this device", "خُفّض لهذا الجهاز")}
            </span>
          )}
        </div>
        <p className="mt-1.5 text-[0.68rem] leading-relaxed text-zinc-300">{reason}</p>
        <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 font-mono text-[0.58rem] text-zinc-500">
          <span>
            {t("OCR workers", "عوامل OCR")}: {recommendation.ocr_concurrency}
          </span>
          <span>
            {t("Batch chunk", "دفعة البحث")}: {recommendation.batch_chunk}
          </span>
          <span>
            {t("Index threads", "خيوط الفهرسة")}: {recommendation.index_threads}
          </span>
          <span>
            {t("Model RAM", "ذاكرة النموذج")}: {pack.ram_mb} MB
          </span>
        </div>
        <div className="mt-2.5 flex flex-wrap gap-1.5">
          {ready ? (
            <button
              type="button"
              onClick={onTestRecommended}
              disabled={busy}
              className="flex items-center gap-1.5 rounded-lg bg-accent-400 px-3 py-1.5 text-[0.65rem] font-semibold text-ink-950 transition hover:bg-accent-300 disabled:cursor-not-allowed disabled:opacity-40"
            >
              <Icon name="scan" size={12} />
              {t("Test recommended model", "اختبار النموذج الموصى به")}
            </button>
          ) : (
            <button
              type="button"
              onClick={onDownloadRecommended}
              disabled={busy || pack.downloading}
              className="flex items-center gap-1.5 rounded-lg bg-accent-400 px-3 py-1.5 text-[0.65rem] font-semibold text-ink-950 transition hover:bg-accent-300 disabled:cursor-not-allowed disabled:opacity-40"
            >
              <Icon name="download" size={12} />
              {downloadLabel}
            </button>
          )}
          <button
            type="button"
            onClick={onChooseManually}
            className="flex items-center gap-1.5 rounded-lg border border-white/10 px-3 py-1.5 text-[0.65rem] text-zinc-300 transition hover:bg-white/10 hover:text-white"
          >
            <Icon name="panel" size={12} />
            {t("Choose manually", "اختيار يدوي")}
          </button>
        </div>
        <p className="mt-2 text-[0.56rem] leading-relaxed text-zinc-500">
          {t(
            "Speed and accuracy are not claimed until a model has been measured on this machine. The test button runs two small samples locally.",
            "لا تُدّعى السرعة أو الدقة قبل قياس النموذج على هذا الجهاز. زر الاختبار يشغّل عيّنتين صغيرتين محلياً.",
          )}
        </p>
      </div>
    </section>
  );
}
