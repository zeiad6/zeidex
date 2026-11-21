import { useEffect, useRef } from "react";

import type { PageResult, Region } from "../../api";
import { Icon, type IconName } from "../Icons";
import TableEditor from "./TableEditor";

const KIND: Record<string, { label: string; labelAr: string; icon: IconName; tone: string }> = {
  text: { label: "Text", labelAr: "نص", icon: "text", tone: "text-sky-300 bg-sky-400/10" },
  title: { label: "Title", labelAr: "عنوان", icon: "text", tone: "text-violet-300 bg-violet-400/10" },
  list: { label: "List", labelAr: "قائمة", icon: "text", tone: "text-cyan-300 bg-cyan-400/10" },
  table: { label: "Table", labelAr: "جدول", icon: "table", tone: "text-emerald-300 bg-emerald-400/10" },
  figure: { label: "Figure", labelAr: "صورة", icon: "image", tone: "text-amber-300 bg-amber-400/10" },
  equation: { label: "Equation", labelAr: "معادلة", icon: "sparkle", tone: "text-pink-300 bg-pink-400/10" },
  header: { label: "Header", labelAr: "ترويسة", icon: "text", tone: "text-zinc-300 bg-white/5" },
  footer: { label: "Footer", labelAr: "تذييل", icon: "text", tone: "text-zinc-300 bg-white/5" },
  caption: { label: "Caption", labelAr: "تعليق", icon: "text", tone: "text-orange-300 bg-orange-400/10" },
};

function TextRegion({ region, rtl }: { region: Region; rtl: boolean }) {
  const paragraphs = (region.text ?? "").split(/\n+/).filter((line) => line.trim() !== "");
  return (
    <div className="space-y-1 px-1">
      {paragraphs.length === 0 ? (
        <span className="text-xs text-zinc-600">{rtl ? "لا نص" : "No text"}</span>
      ) : (
        paragraphs.map((line, index) => (
          <p
            key={index}
            dir="auto"
            className="text-[0.82rem] leading-relaxed text-zinc-200"
          >
            {line}
          </p>
        ))
      )}
    </div>
  );
}

export default function RegionEditor({
  page,
  activeRegion,
  rtl,
  onSelect,
  onRegionText,
  onCellText,
}: {
  page: PageResult;
  activeRegion: number | null;
  rtl: boolean;
  onSelect: (index: number) => void;
  onRegionText: (index: number, text: string) => void;
  onCellText: (regionIndex: number, cellIndex: number, text: string) => void;
}) {
  const regions = page.layout.regions;
  const cards = useRef<Array<HTMLDivElement | null>>([]);

  // Selecting a box on the page should bring its editor into view; otherwise a
  // click near the table looks like it did nothing.
  useEffect(() => {
    if (activeRegion === null) return;
    cards.current[activeRegion]?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [activeRegion]);

  if (regions.length === 0) {
    return (
      <p className="rounded-xl border border-dashed border-white/10 bg-white/[0.015] p-5 text-center text-sm text-zinc-500">
        {rtl ? "لا توجد مناطق مستخرجة" : "No regions extracted"}
      </p>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-1.5 overflow-y-auto pe-1">
      {regions.map((region, index) => {
        const meta = KIND[region.kind] ?? KIND.text;
        const isActive = activeRegion === index;
        const rows = Math.min(10, Math.max(2, Math.ceil((region.text?.length ?? 0) / 68)));
        return (
          <div
            key={`${region.kind}-${index}`}
            ref={(node) => {
              cards.current[index] = node;
            }}
            onFocus={() => onSelect(index)}
            onClick={() => onSelect(index)}
            className={`cursor-text rounded-xl border p-2.5 transition-colors ${
              isActive
                ? "border-accent-400/35 bg-accent-400/[0.05]"
                : "border-white/[0.07] bg-white/[0.015]"
            }`}
          >
            <div className="mb-1.5 flex items-center gap-2">
              <span className={`grid h-5 w-5 place-items-center rounded-md ${meta.tone}`}>
                <Icon name={meta.icon} size={11} />
              </span>
              <button
                type="button"
                onClick={() => onSelect(index)}
                className="text-[0.7rem] font-medium text-zinc-300 transition hover:text-white"
              >
                {rtl ? meta.labelAr : meta.label}
                <span className="ms-1.5 font-mono text-[0.6rem] text-zinc-600">
                  #{index + 1}
                </span>
              </button>

              <span className="ms-auto flex items-center gap-1.5" title="confidence">
                <span className="h-1 w-12 overflow-hidden rounded-full bg-white/10">
                  <span
                    className="block h-full rounded-full bg-gradient-to-r from-accent-400 to-iris-400"
                    style={{ width: `${Math.round(region.confidence * 100)}%` }}
                  />
                </span>
                <span className="font-mono text-[0.6rem] text-zinc-600">
                  {Math.round(region.confidence * 100)}%
                </span>
              </span>
            </div>

            {region.kind === "table" && region.table ? (
              <TableEditor
                table={region.table}
                rtl={rtl}
                active={isActive}
                onChange={(cellIndex, text) => onCellText(index, cellIndex, text)}
              />
            ) : region.kind === "figure" ? (
              <p className="flex items-center gap-1.5 px-1 py-1 text-xs text-zinc-600">
                <Icon name="image" size={12} />
                {rtl ? "صورة — لا نص" : "Figure — no text"}
              </p>
            ) : isActive ? (
              <textarea
                value={region.text ?? ""}
                rows={rows}
                dir="auto"
                onChange={(event) => onRegionText(index, event.target.value)}
                aria-label={`${rtl ? meta.labelAr : meta.label} ${index + 1}`}
                className="w-full resize-y rounded-lg bg-transparent px-1 py-0.5 text-[0.82rem] leading-relaxed text-zinc-100 outline-none placeholder:text-zinc-600"
              />
            ) : (
              <TextRegion region={region} rtl={rtl} />
            )}
          </div>
        );
      })}
    </div>
  );
}
