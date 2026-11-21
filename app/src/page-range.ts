/**
 * The PDF page window typed into the Studio, parsed before any work starts.
 *
 * Empty inputs mean "every page", which is the only state that does not need a
 * number. Everything else is validated here rather than in the backend so the
 * user sees the mistake in the same panel they typed it, and a partially filled
 * pair never turns into a surprising full-document run.
 */
export type PageRange =
  | { kind: "all" }
  | { kind: "range"; first: number; last: number }
  | { kind: "invalid"; message: string };

export function parsePageRange(first: string, last: string): PageRange {
  const firstText = first.trim();
  const lastText = last.trim();
  if (firstText === "" && lastText === "") return { kind: "all" };
  if (firstText === "" || lastText === "") {
    return {
      kind: "invalid",
      message: "Enter both page numbers, or leave both empty to read every page.",
    };
  }
  const firstNumber = Number(firstText);
  const lastNumber = Number(lastText);
  if (
    !/^\d+$/.test(firstText) ||
    !/^\d+$/.test(lastText) ||
    !Number.isSafeInteger(firstNumber) ||
    !Number.isSafeInteger(lastNumber) ||
    firstNumber < 1 ||
    lastNumber < 1
  ) {
    return {
      kind: "invalid",
      message: "Page numbers are whole numbers starting at 1.",
    };
  }
  if (lastNumber < firstNumber) {
    return {
      kind: "invalid",
      message: "The last page cannot be before the first page.",
    };
  }
  return { kind: "range", first: firstNumber, last: lastNumber };
}
