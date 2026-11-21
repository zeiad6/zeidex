/**
 * Tiny counted-noun helpers.
 *
 * Arabic does not pluralize with an `s`: 1, 2, 3–10 and 11+ each take a
 * different form. The panel shows counts constantly, so they are spelled
 * correctly instead of falling back to `{n} نتيجة` for every number.
 */
type Forms = {
  none: string;
  one: string;
  two: string;
  few: string;
  many: string;
};

const FORMS: Record<"results" | "pages" | "regions" | "cells" | "clips", Forms> = {
  results: {
    none: "لا نتائج",
    one: "نتيجة واحدة",
    two: "نتيجتان",
    few: "نتائج",
    many: "نتيجة",
  },
  pages: {
    none: "لا صفحات",
    one: "صفحة واحدة",
    two: "صفحتان",
    few: "صفحات",
    many: "صفحة",
  },
  regions: {
    none: "لا مناطق",
    one: "منطقة واحدة",
    two: "منطقتان",
    few: "مناطق",
    many: "منطقة",
  },
  cells: {
    none: "لا خلايا",
    one: "خلية واحدة",
    two: "خليتان",
    few: "خلايا",
    many: "خلية",
  },
  clips: {
    none: "لا مقاطع",
    one: "مقطع واحد",
    two: "مقطعان",
    few: "مقاطع",
    many: "مقطع",
  },
};

const EN: Record<"results" | "pages" | "regions" | "cells" | "clips", string> = {
  results: "result",
  pages: "page",
  regions: "region",
  cells: "cell",
  clips: "clip",
};

/** `3 نتائج`, `نتيجتان`, or `2 results`. */
export function count(
  noun: keyof typeof FORMS,
  value: number,
  rtl: boolean,
): string {
  if (!rtl) return `${value.toLocaleString()} ${EN[noun]}${value === 1 ? "" : "s"}`;
  const forms = FORMS[noun];
  if (value === 0) return forms.none;
  if (value === 1) return forms.one;
  if (value === 2) return forms.two;
  if (value <= 10) return `${value} ${forms.few}`;
  return `${value.toLocaleString("en-US")} ${forms.many}`;
}
