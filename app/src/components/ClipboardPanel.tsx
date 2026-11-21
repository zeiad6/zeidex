import { useEffect, useState } from "react";
import type { ClipEntry } from "../api";
import { api } from "../api";
import { Icon } from "./Icons";

function formatTime(unixSeconds: number): string {
  const date = new Date(unixSeconds * 1000);
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export default function ClipboardPanel({
  entries,
  rtl,
  paused,
  onDeleted,
  onToggled,
  onTogglePause,
  onClear,
  onError,
}: {
  entries: ClipEntry[];
  rtl: boolean;
  paused: boolean;
  onDeleted: (id: number) => void;
  onToggled: (entry: ClipEntry) => void;
  onTogglePause: () => void;
  onClear: () => void;
  onError: (message: string) => void;
}) {
  // Clearing is one click but cannot be undone, so the first click only arms
  // the button; the second one, within three seconds, wipes the history.
  const [confirmingClear, setConfirmingClear] = useState(false);
  useEffect(() => {
    if (!confirmingClear) return;
    const timer = window.setTimeout(() => setConfirmingClear(false), 3000);
    return () => window.clearTimeout(timer);
  }, [confirmingClear]);

  const copy = async (entry: ClipEntry) => {
    try {
      await navigator.clipboard.writeText(entry.content);
    } catch {
      onError(rtl ? "تعذّر النسخ" : "Copy failed");
    }
  };

  const togglePin = async (entry: ClipEntry) => {
    const pinned = !entry.pinned;
    try {
      await api.clipboardSetPinned(entry.id, pinned);
    } catch {
      // Nothing changed, so the row keeps the state it had; say why.
      onError(rtl ? "تعذّر تثبيت المقطع" : "Could not pin the clip");
      return;
    }
    onToggled({ ...entry, pinned });
  };

  const remove = async (entry: ClipEntry) => {
    try {
      await api.clipboardDelete(entry.id);
    } catch {
      // The row is still there; removing it from the list would be a lie.
      onError(rtl ? "تعذّر حذف المقطع" : "Could not delete the clip");
      return;
    }
    onDeleted(entry.id);
  };

  const clear = () => {
    if (!confirmingClear) {
      setConfirmingClear(true);
      return;
    }
    setConfirmingClear(false);
    onClear();
  };

  // Arabic is a joined script: letter-spacing opens the joins and there are no
  // capitals to transform. The Latin micro-label keeps its tracking; the Arabic
  // one is set at a size that reads without it.
  const labelClass = rtl
    ? "text-[0.68rem] font-semibold"
    : "text-[0.62rem] font-semibold uppercase tracking-[0.12em]";

  return (
    <div className="flex min-h-0 flex-col">
      <div className="flex items-center justify-between gap-2 px-4 pb-1.5 pt-0.5">
        <button
          type="button"
          onClick={onTogglePause}
          title={
            paused
              ? rtl
                ? "ابدأ تسجيل ما تنسخه من جديد"
                : "Start saving new clips again"
              : rtl
                ? "توقّف عن تسجيل ما تنسخه"
                : "Stop saving new clips"
          }
          aria-label={
            paused
              ? rtl
                ? "استئناف التسجيل"
                : "Resume recording"
              : rtl
                ? "إيقاف التسجيل"
                : "Pause recording"
          }
          className={`flex items-center gap-1.5 rounded-full px-2.5 py-1 ${labelClass} transition ${
            paused
              ? "bg-amber-400/10 text-amber-300 ring-1 ring-amber-400/25 hover:bg-amber-400/15"
              : "text-zinc-500 hover:bg-white/[0.06] hover:text-zinc-300"
          }`}
        >
          <Icon name={paused ? "play" : "pause"} size={11} />
          {paused ? (rtl ? "متوقف" : "Paused") : rtl ? "إيقاف التسجيل" : "Pause"}
        </button>
        <div className="flex items-center gap-2">
          <span className="text-[0.62rem] tabular-nums text-zinc-600">{entries.length}</span>
          <button
            type="button"
            onClick={clear}
            disabled={entries.length === 0}
            aria-live="polite"
            className={`rounded-full px-2.5 py-1 ${labelClass} transition disabled:opacity-40 ${
              confirmingClear
                ? "bg-red-500/15 text-red-300 ring-1 ring-red-400/25"
                : "text-zinc-500 hover:bg-red-500/15 hover:text-red-300 disabled:hover:bg-transparent disabled:hover:text-zinc-500"
            }`}
          >
            {confirmingClear
              ? rtl
                ? "تأكيد المسح"
                : "Confirm"
              : rtl
                ? "مسح الكل"
                : "Clear"}
          </button>
        </div>
      </div>

      {entries.length === 0 ? (
        <div className="flex flex-col items-center gap-2 px-6 py-10 text-center">
          <span className="grid h-10 w-10 place-items-center rounded-full bg-white/[0.04] text-zinc-600 ring-1 ring-white/10">
            <Icon name="clipboard" size={18} />
          </span>
          <p className="text-sm font-medium text-zinc-400">
            {paused
              ? rtl
                ? "التسجيل متوقف"
                : "Recording is paused"
              : rtl
                ? "الحافظة فارغة"
                : "No clips yet"}
          </p>
          <p className="max-w-[20rem] text-xs leading-relaxed text-zinc-600">
            {paused
              ? rtl
                ? "ما تنسخه الآن لا يُحفظ. اضغط «متوقف» للاستئناف."
                : "Anything you copy now is not saved. Press Paused to resume."
              : rtl
                ? "نص ما تنسخه يظهر هنا. المقاطع الضخمة تُتجاهَل، والمثبّت لا يحذفه التقادم."
                : "Text you copy shows up here. Very large clips are skipped, and pinned clips are never pruned."}
          </p>
        </div>
      ) : (
        <ul className="max-h-[43vh] space-y-px overflow-y-auto px-2 pb-1">
          {entries.map((entry) => (
            <li
              key={entry.id}
              className={`group flex items-start gap-3 rounded-xl px-3 py-2.5 transition-colors ${
                entry.pinned ? "bg-accent-500/[0.06]" : "hover:bg-white/[0.03]"
              }`}
            >
              <button
                type="button"
                onClick={() => copy(entry)}
                className="min-w-0 flex-1 text-start"
                title={rtl ? "نسخ" : "Copy"}
              >
                <span className="line-clamp-2 whitespace-pre-wrap break-words text-[0.85rem] leading-relaxed text-zinc-200">
                  <bdi>{entry.content}</bdi>
                </span>
                <span className="mt-1 flex items-center gap-1.5 text-[0.66rem] text-zinc-600">
                  <span dir="ltr" className="font-mono">
                    {formatTime(entry.copied_at_unix_s)} · ×{entry.copy_count}
                  </span>
                  {entry.pinned && (
                    <>
                      <span aria-hidden="true">·</span>
                      <span className="text-accent-400/80">{rtl ? "مثبّت" : "pinned"}</span>
                    </>
                  )}
                </span>
              </button>
              <span className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity focus-within:opacity-100 group-hover:opacity-100">
                <button
                  type="button"
                  onClick={() => togglePin(entry)}
                  aria-pressed={entry.pinned}
                  aria-label={
                    entry.pinned ? (rtl ? "إلغاء التثبيت" : "Unpin") : rtl ? "تثبيت" : "Pin"
                  }
                  className={`grid h-7 w-7 place-items-center rounded-lg transition ${
                    entry.pinned
                      ? "text-accent-300 hover:bg-white/10"
                      : "text-zinc-500 hover:bg-white/10 hover:text-zinc-200"
                  }`}
                >
                  <Icon name="pin" size={14} />
                </button>
                <button
                  type="button"
                  onClick={() => remove(entry)}
                  aria-label={rtl ? "حذف المقطع" : "Delete clip"}
                  className="grid h-7 w-7 place-items-center rounded-lg text-zinc-500 transition hover:bg-red-500/15 hover:text-red-300"
                >
                  <Icon name="trash" size={14} />
                </button>
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
