import type { Region } from "../../api";
import { count } from "../../i18n";
import { Icon, type IconName } from "../Icons";
import { regionStyle } from "./edits";

const KIND_STYLE: Record<string, string> = {
  text: "border-sky-400/70 bg-sky-400/10",
  title: "border-violet-400/80 bg-violet-400/10",
  list: "border-cyan-400/70 bg-cyan-400/10",
  table: "border-emerald-400/80 bg-emerald-400/10",
  figure: "border-amber-400/70 bg-amber-400/10",
  equation: "border-pink-400/70 bg-pink-400/10",
  header: "border-white/40 bg-white/5",
  footer: "border-white/40 bg-white/5",
  caption: "border-orange-400/70 bg-orange-400/10",
};

const KIND_ICON: Record<string, IconName> = {
  text: "text",
  title: "text",
  list: "text",
  table: "table",
  figure: "image",
  equation: "sparkle",
  header: "text",
  footer: "text",
  caption: "text",
};

export default function PagePreview({
  dataUrl,
  page,
  regions,
  activeRegion,
  showBoxes,
  onSelect,
  rtl,
  emptyMessage,
}: {
  dataUrl: string | null;
  page: { width: number; height: number } | null;
  regions: Region[];
  activeRegion: number | null;
  showBoxes: boolean;
  onSelect: (index: number) => void;
  rtl: boolean;
  /** Shown when there is no page image; a text document has none by nature. */
  emptyMessage?: string;
}) {
  if (!dataUrl || !page) {
    return (
      <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-white/10 bg-white/[0.015] px-6 text-center">
        <span className="grid h-10 w-10 place-items-center rounded-xl bg-white/[0.04] text-zinc-600 ring-1 ring-white/10">
          <Icon name="image" size={18} />
        </span>
        <p className="text-xs text-zinc-500">
          {emptyMessage ??
            (rtl ? "شغّل التعرف الضوئي لعرض الصفحة" : "Run OCR to render the page")}
        </p>
      </div>
    );
  }

  const active = activeRegion !== null ? regions[activeRegion] : null;

  return (
    <div className="relative flex min-h-0 flex-1 items-center justify-center overflow-auto rounded-xl border border-white/10 bg-[radial-gradient(120%_120%_at_50%_0%,rgb(255_255_255/0.04),transparent_60%)] p-3">
      {regions.length > 0 && (
        <span className="pointer-events-none absolute bottom-2 end-3 z-10 rounded-full bg-black/60 px-2 py-0.5 font-mono text-[0.6rem] text-zinc-400 backdrop-blur">
          {count("regions", regions.length, rtl)}
          {active ? ` · ${active.kind}` : ""}
        </span>
      )}
      <div className="relative h-full w-fit max-w-full shadow-[0_18px_50px_-20px_rgb(0_0_0/0.9)]">
        <img
          src={dataUrl}
          alt={rtl ? "معاينة الصفحة" : "Page preview"}
          className="block h-full w-auto max-w-full select-none rounded-sm bg-[#fff] object-contain"
          draggable={false}
        />
        {showBoxes &&
          regions.map((region, index) => (
            <button
              key={`${region.kind}-${index}`}
              type="button"
              title={`${region.kind} · ${Math.round(region.confidence * 100)}%`}
              aria-label={`${KIND_ICON[region.kind] ? region.kind : "region"} ${index + 1}`}
              onClick={() => onSelect(index)}
              style={regionStyle(region, page)}
              className={`absolute cursor-pointer border transition ${
                KIND_STYLE[region.kind] ?? "border-white/40 bg-white/5"
              } ${
                activeRegion === index
                  ? "outline outline-2 outline-offset-1 outline-accent-300"
                  : "hover:bg-white/15"
              }`}
            />
          ))}
      </div>
    </div>
  );
}
