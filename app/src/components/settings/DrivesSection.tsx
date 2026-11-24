import { useEffect, useState } from "react";
import { api, type DriveInfo } from "../../api";
import { Icon } from "../Icons";

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

function normalized(root: string): string {
  const trimmed = root.replace(/[\\/]+$/, "");
  return trimmed === "" ? root : trimmed;
}

function sameRoot(left: string, right: string): boolean {
  const a = normalized(left);
  const b = normalized(right);
  // Windows drive letters are case-insensitive; POSIX paths are not. The
  // browser demo is always Windows, so the demo toggle behaves like Windows.
  return /^[A-Za-z]:/.test(a) || /^[A-Za-z]:/.test(b)
    ? a.toLowerCase() === b.toLowerCase()
    : a === b;
}

/**
 * Local volumes and whether the walker covers them.
 *
 * `scanRoot` comes from the backend, which compares the drive root with the
 * configured roots; the toggle edits `fileScanExtraRoots`, the same list the
 * Indexing screen edits. There is one source of truth, so the two screens can
 * never disagree.
 */
export default function DrivesSection({
  rtl,
  roots,
  onChangeRoots,
}: {
  rtl: boolean;
  roots: string[];
  onChangeRoots: (next: string[]) => void;
}) {
  const t = (en: string, ar: string) => (rtl ? ar : en);
  const [drives, setDrives] = useState<DriveInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    api
      .systemDrives()
      .then((list) => {
        if (active) setDrives(list);
      })
      .catch((err) => {
        if (active) setError(String(err));
      });
    return () => {
      active = false;
    };
  }, []);

  const toggle = (drive: DriveInfo) => {
    const covered = roots.some((root) => sameRoot(root, drive.root));
    if (covered) {
      onChangeRoots(roots.filter((root) => !sameRoot(root, drive.root)));
    } else if (!roots.some((root) => sameRoot(root, drive.root))) {
      onChangeRoots([...roots, drive.root]);
    }
  };

  return (
    <div className="animate-rise space-y-4">
      <div>
        <h2 className="flex items-center gap-2 text-[0.95rem] font-semibold tracking-[-0.01em] text-white">
          <span className="text-accent-400/80">
            <Icon name="drive" size={15} />
          </span>
          {t("Drives", "الأقراص")}
        </h2>
        <p className="mt-1 text-[0.74rem] leading-relaxed text-zinc-500">
          {t(
            "The list is read from the operating system on every visit. Turning a drive on adds its root to the scan folders; turning it off removes it. Changes are saved with the rest of the settings.",
            "تُقرأ القائمة من نظام التشغيل في كل زيارة. تشغيل قرص يضيف جذره إلى مجلدات المسح، وإيقافه يزيله. تُحفظ التغييرات مع بقية الإعدادات.",
          )}
        </p>
      </div>

      {error && (
        <p role="alert" className="rounded-lg border border-red-400/20 bg-red-400/[0.07] px-3 py-2 text-[0.72rem] text-red-200">
          {error}
        </p>
      )}

      {drives === null ? (
        <div className="space-y-2" aria-busy="true">
          <div className="skeleton h-16 w-full" />
          <div className="skeleton h-16 w-full" />
        </div>
      ) : drives.length === 0 ? (
        <p className="rounded-xl border border-dashed border-white/10 px-3 py-6 text-center text-[0.74rem] text-zinc-500">
          {t(
            "The platform did not report any volumes. Use “Add a folder” on the Indexing screen instead.",
            "لم يُبلّغ النظام عن أي وحدات تخزين. استخدم «إضافة مجلد» في شاشة الفهرسة بدلاً من ذلك.",
          )}
        </p>
      ) : (
        <ul className="space-y-2">
          {drives.map((drive) => {
            const covered = roots.some((root) => sameRoot(root, drive.root));
            const used = Math.max(0, drive.totalBytes - drive.freeBytes);
            const percent =
              drive.totalBytes > 0 ? Math.min(100, Math.round((used / drive.totalBytes) * 100)) : 0;
            return (
              <li
                key={drive.root}
                className="rounded-xl border border-white/[0.07] bg-surface-2 p-3"
              >
                <div className="flex flex-wrap items-center gap-3">
                  <span className="grid h-8 w-8 shrink-0 place-items-center rounded-lg bg-white/[0.04] text-zinc-400 ring-1 ring-inset ring-white/10">
                    <Icon name="drive" size={15} />
                  </span>
                  <div className="min-w-0 flex-1">
                    <p className="flex items-center gap-2 text-[0.8rem] font-medium text-zinc-100">
                      <span className="truncate">{drive.label}</span>
                      <span className="rounded-md border border-white/10 px-1.5 py-px font-mono text-[0.58rem] text-zinc-500" dir="ltr">
                        {drive.root}
                      </span>
                      {drive.removable && (
                        <span className="rounded-md border border-amber-400/20 bg-amber-400/[0.07] px-1.5 py-px text-[0.58rem] text-amber-200">
                          {t("Removable", "قابل للإزالة")}
                        </span>
                      )}
                    </p>
                    <p className="mt-0.5 text-[0.66rem] text-zinc-500">
                      {t(
                        `${formatBytes(used)} used of ${formatBytes(drive.totalBytes)} · ${drive.kind.toUpperCase()}`,
                        `${formatBytes(used)} مستخدم من ${formatBytes(drive.totalBytes)} · ${drive.kind.toUpperCase()}`,
                      )}
                    </p>
                  </div>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={covered}
                    onClick={() => toggle(drive)}
                    aria-label={
                      covered
                        ? t(`Stop indexing ${drive.root}`, `إيقاف فهرسة ${drive.root}`)
                        : t(`Index ${drive.root}`, `فهرسة ${drive.root}`)
                    }
                    className={`flex items-center gap-2 rounded-full border px-2.5 py-1.5 text-[0.68rem] font-medium transition ${
                      covered
                        ? "border-accent-400/30 bg-accent-400/10 text-accent-200"
                        : "border-white/10 text-zinc-400 hover:border-white/20 hover:text-zinc-200"
                    }`}
                  >
                    <span
                      aria-hidden="true"
                      className={`flex h-4 w-7 items-center rounded-full p-0.5 ${
                        covered ? "justify-end bg-accent-500/80" : "justify-start bg-ink-700"
                      }`}
                    >
                      <span className="block h-3 w-3 rounded-full bg-[#fff]" />
                    </span>
                    {covered ? t("Indexed", "مفهرس") : t("Index this drive", "فهرسة هذا القرص")}
                  </button>
                </div>
                <div
                  aria-hidden="true"
                  className="mt-2.5 h-1 overflow-hidden rounded-full bg-white/[0.06]"
                >
                  <div
                    className="h-full rounded-full bg-gradient-to-r from-accent-500 to-iris-500"
                    style={{ width: `${percent}%` }}
                  />
                </div>
              </li>
            );
          })}
        </ul>
      )}

      <p className="text-[0.68rem] leading-relaxed text-zinc-600">
        {t(
          "A drive is only indexed when its root is a scan root. Your home folders are always walked on the system drive; indexing the whole drive is a separate, larger choice.",
          "لا يُفهرس القرص إلا إذا كان جذره ضمن جذور المسح. مجلداتك الشخصية تُمسح دائماً على قرص النظام؛ أما فهرسة القرص كاملاً فهي خيار منفصل وأكبر.",
        )}
      </p>
    </div>
  );
}
