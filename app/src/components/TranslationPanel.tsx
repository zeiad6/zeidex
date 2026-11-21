import { useEffect, useState } from "react";
import { api, type TranslationOutcome } from "../api";
import { Icon } from "./Icons";

/** Target languages offered by `tr"`. A fixed set keeps the mode predictable;
 *  a settings picker can replace this constant later. */
const TARGETS = [
  { code: "ar", name: "العربية", dir: "rtl" },
  { code: "en", name: "English", dir: "ltr" },
  { code: "fr", name: "Français", dir: "ltr" },
  { code: "es", name: "Español", dir: "ltr" },
] as const;

type Entry =
  | { state: "loading" }
  | { state: "done"; text: string }
  | { state: "error"; message: string };

/**
 * `tr"` — the translations of the typed text, one row per target language.
 * Every row is an independent best-effort request: Google's `gtx` endpoint is
 * rate-limited, so one failure is shown in its own row instead of failing the
 * whole mode.
 */
export default function TranslationPanel({ text, rtl }: { text: string; rtl: boolean }) {
  const [entries, setEntries] = useState<Record<string, Entry>>({});
  const [copied, setCopied] = useState<string | null>(null);

  useEffect(() => {
    const source = text.trim();
    if (!source) {
      setEntries({});
      return;
    }

    let cancelled = false;
    // Wait for a typing pause: translating on every keystroke would send a
    // request per language per character into a rate-limited endpoint.
    const handle = window.setTimeout(() => {
      setEntries(
        Object.fromEntries(TARGETS.map((target) => [target.code, { state: "loading" }])),
      );
      for (const target of TARGETS) {
        api
          .translateText(source, target.code)
          .then((out: TranslationOutcome) => {
            if (cancelled) return;
            setEntries((prev) => ({
              ...prev,
              [target.code]: out.error
                ? { state: "error", message: out.error.message }
                : { state: "done", text: out.translated },
            }));
          })
          .catch((err: unknown) => {
            if (cancelled) return;
            setEntries((prev) => ({
              ...prev,
              [target.code]: { state: "error", message: String(err) },
            }));
          });
      }
    }, 200);

    return () => {
      cancelled = true;
      window.clearTimeout(handle);
    };
  }, [text]);

  if (!text.trim()) {
    return (
      <div className="flex flex-col items-center gap-2 px-6 py-12 text-center">
        <span className="grid h-10 w-10 place-items-center rounded-full bg-white/[0.04] text-zinc-600 ring-1 ring-white/10">
          <Icon name="translate" size={18} />
        </span>
        <p className="text-sm font-medium text-zinc-400">
          {rtl ? "وضع الترجمة" : "Translation mode"}
        </p>
        <p className="max-w-[20rem] text-xs leading-relaxed text-zinc-600">
          {rtl
            ? 'اكتب نصًا بعد tr" لترجمته إلى أربع لغات.'
            : 'Type text after tr" to translate it into four languages.'}
        </p>
      </div>
    );
  }

  return (
    <div className="max-h-[46vh] overflow-y-auto px-2 pb-1">
      <div className="flex items-center gap-1.5 px-3 pb-1 pt-2 text-[0.625rem] text-zinc-600">
        <Icon name="globe" size={11} />
        {rtl ? "الترجمة الوحيدة التي تحتاج إنترنت" : "The only feature that uses the network"}
      </div>
      <ul className="space-y-px">
        {TARGETS.map((target) => {
          const entry = entries[target.code];
          return (
            <li key={target.code} className="group rounded-xl px-3 py-2.5 hover:bg-white/[0.03]">
              <div className="mb-1 flex items-center gap-2">
                <span className="rounded-md border border-white/10 px-1.5 py-px font-mono text-[0.5625rem] uppercase tracking-[0.14em] text-zinc-500">
                  {target.code}
                </span>
                <span className="text-[0.7rem] font-medium text-zinc-500">{target.name}</span>
                {entry?.state === "done" && (
                  <button
                    type="button"
                    onClick={() => {
                      void navigator.clipboard
                        .writeText(entry.text)
                        .then(() => setCopied(target.code))
                        .catch(() => undefined);
                    }}
                    className="ms-auto flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[0.625rem] text-zinc-600 opacity-0 transition hover:bg-white/10 hover:text-zinc-200 focus-visible:opacity-100 group-hover:opacity-100"
                  >
                    <Icon name={copied === target.code ? "check" : "clipboard"} size={11} />
                    {copied === target.code
                      ? rtl
                        ? "نُسخ"
                        : "Copied"
                      : rtl
                        ? "نسخ"
                        : "Copy"}
                  </button>
                )}
              </div>
              <div dir={target.dir} className="text-[0.88rem] leading-relaxed text-zinc-100">
                {entry?.state === "done" ? (
                  entry.text
                ) : entry?.state === "error" ? (
                  <span className="text-red-300/90">{entry.message}</span>
                ) : (
                  <span className="mt-1 block space-y-1.5" aria-label={rtl ? "يترجم" : "Translating"}>
                    <span className="skeleton block h-3 w-3/4" />
                    <span className="skeleton block h-3 w-1/2" />
                  </span>
                )}
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
