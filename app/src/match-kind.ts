import type { MatchKind } from "./api";

/**
 * How a row is labelled: the name starts with the query, ends with it, or is
 * only close to it. The tier itself is decided in Rust
 * (`core/matching::MatchKind`); this module only owns the presentation and a
 * small mirror used by the demo bridge, so the screenshots show the same
 * labels the real app does.
 */
export interface MatchKindInfo {
  label: string;
  labelAr: string;
  hint: string;
  hintAr: string;
  /** Chip classes: border, background and text for one tier. */
  tone: string;
}

export const MATCH_KINDS: Record<MatchKind, MatchKindInfo> = {
  exact: {
    label: "Exact",
    labelAr: "مطابق",
    hint: "The name is exactly what you typed",
    hintAr: "الاسم مطابق تماماً لما كتبته",
    tone: "border-emerald-400/25 bg-emerald-400/[0.07] text-emerald-300/90",
  },
  prefix: {
    label: "Start",
    labelAr: "بداية",
    hint: "The name starts with what you typed",
    hintAr: "الاسم يبدأ بما كتبته",
    tone: "border-cyan-400/25 bg-cyan-400/[0.07] text-cyan-300/90",
  },
  suffix: {
    label: "End",
    labelAr: "نهاية",
    hint: "The name ends with what you typed",
    hintAr: "الاسم ينتهي بما كتبته",
    tone: "border-amber-400/25 bg-amber-400/[0.07] text-amber-300/90",
  },
  contains: {
    label: "Inside",
    labelAr: "داخل",
    hint: "The name contains what you typed",
    hintAr: "الاسم يحتوي على ما كتبته",
    tone: "border-white/10 bg-white/[0.03] text-zinc-400",
  },
  fuzzy: {
    label: "Partial",
    labelAr: "جزئي",
    hint: "The letters match in order, with gaps",
    hintAr: "الحروف متطابقة بالترتيب مع فجوات",
    tone: "border-violet-400/25 bg-violet-400/[0.07] text-violet-300/90",
  },
  approximate: {
    label: "Close",
    labelAr: "مقارب",
    hint: "The name is close to what you typed (typo tolerant)",
    hintAr: "الاسم قريب مما كتبته (يتحمّل الأخطاء المطبعية)",
    tone: "border-rose-400/25 bg-rose-400/[0.07] text-rose-300/90",
  },
};

export function isMatchKind(value: string): value is MatchKind {
  return Object.prototype.hasOwnProperty.call(MATCH_KINDS, value);
}

/**
 * The demo bridge's mirror of the Rust tiers, used so `?demo=` screenshots
 * show real labels. The real ranking is the Rust one; this only has to be
 * close enough for fixture data.
 */
export function demoMatchKind(query: string, title: string): MatchKind | null {
  const q = query.trim().toLowerCase();
  const t = title.toLowerCase();
  if (!q || !t) {
    return null;
  }
  if (t === q) {
    return "exact";
  }
  if (t.startsWith(q)) {
    return "prefix";
  }
  if (q.length >= 2 && endsWithQuery(t, q)) {
    return "suffix";
  }
  if (t.includes(q)) {
    return "contains";
  }
  const words = q.split(/\s+/).filter(Boolean);
  if (words.length > 1 && words.every((word) => t.includes(word))) {
    return "contains";
  }
  if (isSubsequence(q, t)) {
    return "fuzzy";
  }
  return approximateScore(q, t) === null ? null : "approximate";
}

function endsWithQuery(title: string, query: string): boolean {
  const dot = title.lastIndexOf(".");
  const hasExtension = dot > 0 && dot + 1 < title.length;
  // A query that is only the extension is a format filter, not an ending.
  if (hasExtension && title.slice(dot + 1) === query) {
    return false;
  }
  if (title.endsWith(query)) {
    return true;
  }
  const stem = hasExtension ? title.slice(0, dot) : title;
  return stem.endsWith(query);
}

function isSubsequence(query: string, title: string): boolean {
  let index = 0;
  for (const char of title) {
    if (index < query.length && char === query[index]) {
      index += 1;
    }
  }
  return index === query.length;
}

/** `null` when the title is not within the query's typo budget. */
function approximateScore(query: string, title: string): number | null {
  const tokens = query.split(/[^\p{L}\p{N}]+/u).filter(Boolean);
  if (tokens.length === 0 || tokens.length > 4) {
    return null;
  }
  const words = title.split(/[^\p{L}\p{N}]+/u).filter(Boolean);
  let total = 0;
  for (const token of tokens) {
    const chars = Array.from(token);
    if (chars.length < 4) {
      return null;
    }
    const budget = Math.min(Math.max(Math.floor(chars.length / 4), 1), 3);
    let best = Number.POSITIVE_INFINITY;
    for (const word of words) {
      const wordChars = Array.from(word);
      best = Math.min(best, osaDistance(chars, wordChars, budget));
      if (best === 0) {
        break;
      }
      if (wordChars.length > chars.length) {
        best = Math.min(best, osaDistance(chars, wordChars.slice(0, chars.length), budget));
      }
    }
    if (!Number.isFinite(best)) {
      return null;
    }
    total += best;
  }
  return total;
}

function osaDistance(a: string[], b: string[], max: number): number {
  if (Math.abs(a.length - b.length) > max) {
    return Number.POSITIVE_INFINITY;
  }
  const rows: number[][] = [];
  for (let i = 0; i <= a.length; i += 1) {
    rows.push(new Array<number>(b.length + 1).fill(Number.POSITIVE_INFINITY));
  }
  for (let i = 0; i <= a.length; i += 1) {
    rows[i][0] = i;
  }
  for (let j = 0; j <= b.length; j += 1) {
    rows[0][j] = j;
  }
  for (let i = 1; i <= a.length; i += 1) {
    for (let j = 1; j <= b.length; j += 1) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      let value = Math.min(rows[i - 1][j] + 1, rows[i][j - 1] + 1, rows[i - 1][j - 1] + cost);
      if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) {
        value = Math.min(value, rows[i - 2][j - 2] + 1);
      }
      rows[i][j] = value;
    }
  }
  const distance = rows[a.length][b.length];
  return distance <= max ? distance : Number.POSITIVE_INFINITY;
}
