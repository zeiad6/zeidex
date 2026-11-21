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
  onDeleted,
  onToggled,
}: {
  entries: ClipEntry[];
  rtl: boolean;
  onDeleted: (id: number) => void;
  onToggled: (entry: ClipEntry) => void;
}) {
  const copy = async (entry: ClipEntry) => {
    try {
      await navigator.clipboard.writeText(entry.content);
    } catch {
      // Clipboard write needs a user gesture and a focused webview; ignore.
    }
  };

  const togglePin = async (entry: ClipEntry) => {
    const pinned = !entry.pinned;
    await api.clipboardSetPinned(entry.id, pinned);
    onToggled({ ...entry, pinned });
  };

  const remove = async (entry: ClipEntry) => {
    await api.clipboardDelete(entry.id);
    onDeleted(entry.id);
  };

  if (entries.length === 0) {
    return (
      <div className="flex flex-col items-center gap-2 px-6 py-12 text-center">
        <span className="grid h-10 w-10 place-items-center rounded-full bg-white/[0.04] text-zinc-600 ring-1 ring-white/10">
          <Icon name="clipboard" size={18} />
        </span>
        <p className="text-sm font-medium text-zinc-400">
          {rtl ? "الحافظة فارغة" : "No clips yet"}
        </p>
        <p className="max-w-[20rem] text-xs leading-relaxed text-zinc-600">
          {rtl
            ? "كل نص تنسخه يظهر هنا. ثبّت ما تحتاجه ليصمد أمام المسح."
            : "Everything you copy shows up here. Pin the ones you keep coming back to."}
        </p>
      </div>
    );
  }

  return (
    <ul className="max-h-[46vh] space-y-px overflow-y-auto px-2 pb-1">
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
              aria-label={entry.pinned ? (rtl ? "إلغاء التثبيت" : "Unpin") : rtl ? "تثبيت" : "Pin"}
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
  );
}
