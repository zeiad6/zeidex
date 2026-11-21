import type { LaunchResult, LaunchResultAction } from "../api";
import { Icon, type IconName } from "./Icons";

const KIND: Record<string, { icon: IconName; label: string; labelAr: string; tone: string }> = {
  app: {
    icon: "app",
    label: "App",
    labelAr: "تطبيق",
    tone: "text-cyan-300 bg-cyan-400/10 ring-cyan-400/20",
  },
  file: {
    icon: "file",
    label: "File",
    labelAr: "ملف",
    tone: "text-indigo-300 bg-indigo-400/10 ring-indigo-400/20",
  },
  folder: {
    icon: "folder",
    label: "Folder",
    labelAr: "مجلد",
    tone: "text-amber-300 bg-amber-400/10 ring-amber-400/20",
  },
  setting: {
    icon: "panel",
    label: "Setting",
    labelAr: "إعداد",
    tone: "text-violet-300 bg-violet-400/10 ring-violet-400/20",
  },
  block: {
    icon: "sparkle",
    label: "Action",
    labelAr: "إجراء",
    tone: "text-fuchsia-300 bg-fuchsia-400/10 ring-fuchsia-400/20",
  },
};

function hint(action: LaunchResultAction, rtl: boolean): string {
  switch (action.type) {
    case "reveal":
      return rtl ? "كشف" : "Reveal";
    case "open_folder":
      return rtl ? "فتح" : "Open";
    case "web_search":
      return rtl ? "بحث" : "Web";
    case "translate":
      return rtl ? "ترجمة" : "Translate";
    case "perform_block":
      return rtl ? "تشغيل" : "Run";
    default:
      return rtl ? "فتح" : "Open";
  }
}

export default function ResultList({
  results,
  selected,
  onHover,
  onRun,
  actionLabel,
  rtl,
}: {
  results: LaunchResult[];
  selected: number;
  onHover: (index: number) => void;
  onRun: (result: LaunchResult) => void;
  actionLabel?: Record<string, string>;
  rtl: boolean;
}) {
  if (results.length === 0) {
    return (
      <div className="flex flex-col items-center gap-2 px-6 py-12 text-center">
        <span className="grid h-10 w-10 place-items-center rounded-full bg-white/[0.04] text-zinc-600 ring-1 ring-white/10">
          <Icon name="search" size={18} />
        </span>
        <p className="text-sm font-medium text-zinc-400">
          {rtl ? "لا توجد نتائج مطابقة" : "No matches"}
        </p>
        <p className="max-w-[22rem] text-xs leading-relaxed text-zinc-600">
          {rtl
            ? "جرّب كلمة أقصر، أو بدّل المرشّح من الأعلى، أو ابدأ البحث في الويب."
            : "Try a shorter word, switch the filter above, or search the web from here."}
        </p>
      </div>
    );
  }

  return (
    <ul
      id="zeidex-results"
      role="listbox"
      aria-label={rtl ? "النتائج" : "Results"}
      className="max-h-[46vh] overflow-y-auto px-2 pb-1"
    >
      {results.map((result, index) => {
        const meta = KIND[result.kind] ?? KIND.file;
        const active = index === selected;
        return (
          <li key={result.id} role="none">
            <div
              id={`zeidex-option-${index}`}
              role="option"
              aria-selected={active}
              tabIndex={-1}
              onMouseEnter={() => onHover(index)}
              onClick={() => onRun(result)}
              className={`group relative flex w-full cursor-default items-center gap-3 rounded-xl px-3 py-2.5 text-start transition-colors duration-150 ${
                active ? "bg-white/[0.06]" : "hover:bg-white/[0.03]"
              }`}
            >
              <span
                aria-hidden="true"
                className={`absolute inset-y-2 start-0 w-[2px] rounded-full bg-gradient-to-b from-accent-300 to-iris-400 transition-opacity ${
                  active ? "opacity-100" : "opacity-0"
                }`}
              />
              <span
                className={`grid h-9 w-9 shrink-0 place-items-center rounded-lg ring-1 ring-inset ${meta.tone}`}
              >
                <Icon name={meta.icon} size={17} />
              </span>

              <span className="min-w-0 flex-1">
                <span className="flex items-center gap-2">
                  <span className="truncate text-[0.9rem] font-medium text-zinc-100">
                    {result.title}
                  </span>
                  <span className="shrink-0 rounded-md border border-white/10 px-1.5 py-px text-[0.5625rem] font-medium uppercase tracking-[0.12em] text-zinc-500">
                    {rtl ? meta.labelAr : meta.label}
                  </span>
                </span>
                {result.subtitle && (
                  <span className="mt-0.5 block truncate text-[0.72rem] text-zinc-500">
                    {/* `bdi` isolates a Latin path from the RTL line: the order
                        stays LTR while the block keeps the row's alignment. */}
                    <bdi>{result.subtitle}</bdi>
                  </span>
                )}
              </span>

              <span
                className={`shrink-0 text-[0.625rem] font-medium uppercase tracking-[0.14em] transition-colors ${
                  active ? "text-accent-300" : "text-zinc-600"
                }`}
              >
                {actionLabel?.[result.action.type] ?? hint(result.action, rtl)}
              </span>
            </div>
          </li>
        );
      })}
    </ul>
  );
}
