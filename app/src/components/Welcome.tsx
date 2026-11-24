import { useEffect, useRef } from "react";
import { Icon } from "./Icons";

/**
 * The first-run guide.
 *
 * Shown once per installation, on top of whatever screen the window opened
 * with. It is the only place the global shortcut is explained before the user
 * has found Settings, so it names the real, currently registered accelerator
 * instead of the default.
 *
 * It is a modal: focus moves into it, Tab stays inside, Escape dismisses, and
 * dismissing is what records "seen" in the config file.
 */
export default function Welcome({
  rtl,
  hotkey,
  version,
  onDismiss,
  onOpenSettings,
}: {
  rtl: boolean;
  hotkey: string;
  version: string;
  onDismiss: () => void;
  onOpenSettings: () => void;
}) {
  const t = (en: string, ar: string) => (rtl ? ar : en);
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const primaryRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    primaryRef.current?.focus();
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        // The screen behind the dialog also listens for Escape (back to the
        // launcher, close a panel). A modal owns the key: dismissing the guide
        // must not also navigate.
        event.stopPropagation();
        onDismiss();
        return;
      }
      if (event.key !== "Tab") return;
      event.stopPropagation();
      // Keep Tab inside the dialog: the launcher behind it is still mounted,
      // and tabbing into a hidden input would look like the dialog lost focus.
      const dialog = dialogRef.current;
      if (!dialog) return;
      const focusable = Array.from(
        dialog.querySelectorAll<HTMLElement>(
          'button:not([disabled]), [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
        ),
      ).filter((element) => element.offsetParent !== null);
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (event.shiftKey && (active === first || !dialog.contains(active))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown, true);
    return () => document.removeEventListener("keydown", onKeyDown, true);
  }, [onDismiss]);

  const keys = hotkey.split("+").map((key) => key.trim()).filter(Boolean);

  const rows: { icon: Parameters<typeof Icon>[0]["name"]; title: string; body: string }[] = [
    {
      icon: "power",
      title: t("Open it from anywhere", "افتحه من أي مكان"),
      body: t(
        `Press ${hotkey} and the launcher comes forward, even while another app has focus.`,
        `اضغط ${hotkey} فيظهر المشغّل حتى لو كان تطبيق آخر في المقدمة.`,
      ),
    },
    {
      icon: "search",
      title: t("Type, then move", "اكتب ثم تنقّل"),
      body: t(
        "Results appear as you type. ↑ ↓ move, Enter opens, Esc hides the window.",
        "تظهر النتائج أثناء الكتابة. ↑ ↓ للتنقل، Enter للفتح، Esc لإخفاء النافذة.",
      ),
    },
    {
      icon: "copy",
      title: t("Right-click a result", "انقر بالزر الأيمن على نتيجة"),
      body: t(
        "Open location, copy the path, or reveal the file in Explorer.",
        "افتح موقع الملف، أو انسخ المسار، أو أظهره في مستكشف الملفات.",
      ),
    },
    {
      icon: "panel",
      title: t("The tray keeps it running", "شريط النظام يبقيه يعمل"),
      body: t(
        "Show window, Open search, Settings and Exit are one click away. Closing the window only hides it.",
        "إظهار النافذة، فتح البحث، الإعدادات، والخروج — كلها بنقرة واحدة. إغلاق النافذة يخفيها فقط.",
      ),
    },
    {
      icon: "shield",
      title: t("Everything stays here", "كل شيء يبقى على جهازك"),
      body: t(
        "Search and indexing are local. Exclusions, startup and the theme live in Settings.",
        "البحث والفهرسة محليان. الاستثناءات وبدء التشغيل والثيم في الإعدادات.",
      ),
    },
  ];

  return (
    <div
      className="fixed inset-0 z-[60] grid place-items-center bg-black/55 p-4"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onDismiss();
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="zeidex-welcome-title"
        dir={rtl ? "rtl" : "ltr"}
        className="animate-rise max-h-full w-full max-w-[30rem] overflow-y-auto rounded-2xl border border-white/[0.08] bg-float shadow-[var(--shadow-pop)]"
      >
        <header className="flex items-start gap-3 border-b border-white/[0.06] px-5 py-4">
          <span className="grid h-9 w-9 shrink-0 place-items-center rounded-xl bg-accent-400/12 text-accent-300">
            <Icon name="sparkle" size={17} />
          </span>
          <div className="min-w-0 flex-1">
            <h2 id="zeidex-welcome-title" className="text-[0.95rem] font-semibold text-zinc-100">
              {t("Welcome to Zeidex", "أهلاً بك في Zeidex")}
            </h2>
            <p className="mt-0.5 text-[0.72rem] text-zinc-500">
              {t(
                `Version ${version} · a local launcher for apps, files and folders.`,
                `الإصدار ${version} · مشغّل محلي للتطبيقات والملفات والمجلدات.`,
              )}
            </p>
          </div>
          <button
            type="button"
            onClick={onDismiss}
            aria-label={t("Close the guide", "إغلاق الدليل")}
            className="grid h-7 w-7 place-items-center rounded-lg text-zinc-500 transition hover:bg-white/10 hover:text-zinc-200"
          >
            <Icon name="x" size={13} />
          </button>
        </header>

        <div className="space-y-3.5 px-5 py-4">
          {rows.map((row) => (
            <div key={row.title} className="flex items-start gap-3">
              <span className="mt-0.5 grid h-7 w-7 shrink-0 place-items-center rounded-lg border border-white/[0.06] bg-surface-2 text-zinc-400">
                <Icon name={row.icon} size={13} />
              </span>
              <div className="min-w-0">
                <p className="text-[0.76rem] font-medium text-zinc-200">{row.title}</p>
                <p className="mt-0.5 text-[0.72rem] leading-relaxed text-zinc-500">{row.body}</p>
                {row.icon === "power" && keys.length > 0 && (
                  <p className="mt-1.5">
                    {/* The chord reads in press order even in an Arabic UI, so
                        the chips keep their own LTR flow while the line stays
                        aligned with the paragraph above it. */}
                    <span
                      dir="ltr"
                      className="inline-flex flex-wrap items-center gap-1"
                      aria-hidden="true"
                    >
                      {keys.map((key, index) => (
                        <span key={`${key}-${index}`} className="flex items-center gap-1">
                          {index > 0 && <span className="text-[0.62rem] text-zinc-600">+</span>}
                          <kbd className="rounded-md border border-white/[0.09] bg-surface-2 px-1.5 py-0.5 font-mono text-[0.66rem] text-zinc-300">
                            {key}
                          </kbd>
                        </span>
                      ))}
                    </span>
                  </p>
                )}
              </div>
            </div>
          ))}
        </div>

        <footer className="flex flex-wrap items-center justify-end gap-2 border-t border-white/[0.06] bg-sunken px-5 py-3">
          <button
            type="button"
            onClick={onOpenSettings}
            className="rounded-lg px-3 py-1.5 text-[0.74rem] text-zinc-400 transition hover:bg-white/[0.06] hover:text-zinc-200"
          >
            {t("Open settings", "افتح الإعدادات")}
          </button>
          <button
            ref={primaryRef}
            type="button"
            onClick={onDismiss}
            className="rounded-lg bg-accent-400/15 px-3.5 py-1.5 text-[0.74rem] font-medium text-accent-200 ring-1 ring-accent-400/30 transition hover:bg-accent-400/25"
          >
            {t("Start searching", "ابدأ البحث")}
          </button>
        </footer>
      </div>
    </div>
  );
}
