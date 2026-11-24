import type { LaunchResult, LaunchResultAction } from "../api";
import { fileFormatOf } from "../file-format";
import FileGlyph from "./FileGlyph";
import { Icon, type IconName } from "./Icons";
import { MATCH_KINDS } from "../match-kind";
import { formatBytes } from "../batch-view";

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
  // A file found by the text inside it (OCR), not by its name. It opens like
  // any other file, so the row differs only in the icon and the excerpt.
  document: {
    icon: "text",
    label: "Document",
    labelAr: "مستند",
    tone: "text-emerald-300 bg-emerald-400/10 ring-emerald-400/20",
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
  // Composed by the shell, not indexed: the row that hands the query to the
  // browser. Without an entry it would borrow the file icon and label.
  web: {
    icon: "globe",
    label: "Web",
    labelAr: "ويب",
    tone: "text-sky-300 bg-sky-400/10 ring-sky-400/20",
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

/**
 * The drive a path lives on, for the metadata line.
 *
 * Only Windows-style paths answer: `C:\Users\me\a.txt` → `C:`. On POSIX the
 * first component is a directory (`home`, `usr`), not a volume, and printing
 * `/` on every row would be noise.
 */
export function driveOf(path: string): string | null {
  const match = /^([A-Za-z]):[\\/]/.exec(path);
  return match ? `${match[1].toUpperCase()}:` : null;
}

/**
 * A short, locale-aware date. The full timestamp is in the title attribute, so
 * a row stays scannable without hiding the exact value.
 */
export function formatModified(unixSeconds: number | null | undefined, rtl: boolean): string | null {
  if (unixSeconds === null || unixSeconds === undefined || unixSeconds <= 0) return null;
  const date = new Date(unixSeconds * 1000);
  if (Number.isNaN(date.getTime())) return null;
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

export default function ResultList({
  results,
  selected,
  onHover,
  onRun,
  onMenu,
  actionLabel,
  rtl,
}: {
  results: LaunchResult[];
  selected: number;
  onHover: (index: number) => void;
  onRun: (result: LaunchResult) => void;
  onMenu: (index: number, x: number, y: number) => void;
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
        // A file row says what the file is: the format badge and its name
        // replace the generic page and the word "File". Folders, apps and
        // actions keep the kind icon.
        const format =
          result.kind === "file" || result.kind === "document"
            ? fileFormatOf(result.path || result.title)
            : null;
        const match = result.match_kind ? MATCH_KINDS[result.match_kind] : undefined;
        const active = index === selected;
        const drive = result.path ? driveOf(result.path) : null;
        const modified = formatModified(result.modified_unix_s, rtl);
        const size =
          result.size_bytes !== null && result.size_bytes !== undefined
            ? formatBytes(result.size_bytes)
            : null;
        // Date, size and drive share one quiet line. A row with none of them
        // keeps only the path, so nothing reserves space for an em dash.
        const facts = [modified, size, drive].filter(Boolean) as string[];
        return (
          <li key={result.id} role="none">
            <div
              id={`zeidex-option-${index}`}
              role="option"
              aria-selected={active}
              tabIndex={-1}
              onMouseEnter={() => onHover(index)}
              onClick={() => onRun(result)}
              onContextMenu={(event) => {
                event.preventDefault();
                onMenu(index, event.clientX, event.clientY);
              }}
              className={`group relative flex w-full cursor-default items-center gap-3 rounded-xl border px-3 py-2.5 text-start transition-[background-color,border-color,box-shadow] duration-150 ${
                active
                  ? "border-white/10 bg-surface-3 shadow-[var(--shadow-card)]"
                  : "border-transparent hover:border-white/[0.06] hover:bg-surface-2"
              }`}
            >
              <span
                aria-hidden="true"
                className={`absolute inset-y-2 start-0 w-[2px] rounded-full bg-gradient-to-b from-accent-300 to-iris-400 transition-opacity ${
                  active ? "opacity-100" : "opacity-0"
                }`}
              />
              <span
                className={
                  format
                    ? "grid h-9 w-9 shrink-0 place-items-center"
                    : `grid h-9 w-9 shrink-0 place-items-center rounded-lg ring-1 ring-inset ${meta.tone}`
                }
              >
                {format ? (
                  <FileGlyph path={result.path || result.title} size={30} />
                ) : (
                  <Icon name={meta.icon} size={17} />
                )}
              </span>

              <span className="min-w-0 flex-1">
                <span className="flex items-center gap-2">
                  <span className="truncate text-[0.9rem] font-medium text-zinc-100" dir="auto">
                    {result.title}
                  </span>
                  <span className="shrink-0 rounded-md border border-white/10 px-1.5 py-px text-[0.5625rem] font-medium micro-label tracking-[0.12em] text-zinc-500">
                    {format
                      ? rtl
                        ? format.nameAr
                        : format.name
                      : rtl
                        ? meta.labelAr
                        : meta.label}
                  </span>
                  {match && (
                    <span
                      title={rtl ? match.hintAr : match.hint}
                      className={`shrink-0 rounded-md border px-1.5 py-px text-[0.5625rem] font-medium micro-label tracking-[0.12em] ${match.tone}`}
                    >
                      {rtl ? match.labelAr : match.label}
                    </span>
                  )}
                </span>
                {result.subtitle && (
                  <span className="mt-0.5 block truncate text-[0.72rem] text-zinc-500">
                    {/* `bdi` isolates a Latin path from the RTL line: the order
                        stays LTR while the block keeps the row's alignment. */}
                    <bdi>{result.subtitle}</bdi>
                  </span>
                )}
                {facts.length > 0 && (
                  <span className="mt-1 flex items-center gap-1.5 text-[0.64rem] text-zinc-600">
                    {modified && (
                      <span className="inline-flex items-center gap-1">
                        <Icon name="calendar" size={10} className="shrink-0" />
                        <span dir="ltr">{modified}</span>
                      </span>
                    )}
                    {modified && (size || drive) && <span aria-hidden="true">·</span>}
                    {size && (
                      <span className="inline-flex items-center gap-1">
                        <Icon name="size" size={10} className="shrink-0" />
                        <span dir="ltr">{size}</span>
                      </span>
                    )}
                    {size && drive && <span aria-hidden="true">·</span>}
                    {drive && (
                      <span className="inline-flex items-center gap-1">
                        <Icon name="drive" size={10} className="shrink-0" />
                        <span dir="ltr">{drive}</span>
                      </span>
                    )}
                  </span>
                )}
              </span>

              <span
                className={`shrink-0 text-[0.625rem] font-medium micro-label tracking-[0.14em] transition-colors ${
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
