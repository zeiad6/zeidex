import type { SVGProps } from "react";

/**
 * A small, dependency-free icon set. Every glyph is a 24 px stroke path, so it
 * inherits `currentColor` and stays sharp at any size.
 */
const PATHS = {
  search: "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16Zm10 2-4.35-4.35",
  app: "M4 6.5A2.5 2.5 0 0 1 6.5 4h3l2 2h6A2.5 2.5 0 0 1 20 8.5v9A2.5 2.5 0 0 1 17.5 20h-11A2.5 2.5 0 0 1 4 17.5v-11Z",
  file: "M7 3h7l5 5v11a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2Zm7 1.5V9h4.5",
  folder: "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z",
  clipboard: "M9 4h6v3H9V4Zm-2 3h10a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1Zm2 5h6m-6 4h4",
  pin: "M9 4h6l-1 5 3 3v2H7v-2l3-3-1-5Zm3 10v6",
  trash: "M5 7h14M10 11v6m4-6v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2",
  refresh: "M20 11a8 8 0 1 0-2.3 5.7M20 5v6h-6",
  enter: "M20 5v7a3 3 0 0 1-3 3H5m0 0 4-4m-4 4 4 4",
  translate: "M4 6h9M8.5 4v2c0 4-2 7-5 8.5M6 9c1 2.5 3 4.5 6 5.5M13 20l4-10 4 10m-7-3h6",
  image: "M4 5h16v14H4V5Zm0 11 4.5-4.5 3.5 3.5L15 12l5 5M9 9.5a1 1 0 1 0 0-1",
  table: "M4 5h16v14H4V5Zm0 5h16M4 15h16M10 10v9",
  text: "M5 6h14M5 12h10M5 18h13",
  check: "m5 13 4 4L19 7",
  alert: "M12 4 3 20h18L12 4Zm0 5v5m0 3v.5",
  sparkle: "M12 3.5l1.8 4.7L18.5 10l-4.7 1.8L12 16.5l-1.8-4.7L5.5 10l4.7-1.8L12 3.5ZM18 16l.9 2.1L21 19l-2.1.9L18 22l-.9-2.1L15 19l2.1-.9L18 16Z",
  export: "M12 3v11m0 0 4-4m-4 4-4-4M4 17v3h16v-3",
  eye: "M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12Zm9.5 2.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5Z",
  scan: "M4 8V6a2 2 0 0 1 2-2h2m8 0h2a2 2 0 0 1 2 2v2m0 8v2a2 2 0 0 1-2 2h-2m-8 0H6a2 2 0 0 1-2-2v-2M8 12h8",
  lock: "M7 11V8a5 5 0 0 1 10 0v3M5 11h14v10H5V11Zm7 4v2",
  globe: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18Zm0 0c2.5-2.5 3.5-5.5 3.5-9S14.5 5.5 12 3c-2.5 2.5-3.5 5.5-3.5 9s1 6.5 3.5 9ZM3.5 9h17m-17 6h17",
  x: "m6 6 12 12M18 6 6 18",
  chevron: "m9 6 6 6-6 6",
  panel: "M4 5h16v14H4V5Zm6 0v14",
} as const;

export type IconName = keyof typeof PATHS;

export function Icon({
  name,
  size = 16,
  className = "",
  ...rest
}: { name: IconName; size?: number; className?: string } & Omit<
  SVGProps<SVGSVGElement>,
  "name" | "size"
>) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.7"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
      focusable="false"
      {...rest}
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
