import { useId } from "react";

/**
 * The mark: a rounded aperture tile holding a "Z" cut, with a single cyan
 * node. It is pure geometry, so it stays crisp from 16 px in the tray to
 * 1024 px in the README.
 */
export function BrandMark({
  size = 28,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  const id = useId();
  const stroke = `stroke-${id}`;
  const fill = `fill-${id}`;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 32 32"
      className={className}
      role="img"
      aria-label="Zeidex Studio"
    >
      <defs>
        <linearGradient id={stroke} x1="4" y1="2" x2="29" y2="30" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#a5f3fc" />
          <stop offset="0.55" stopColor="#22d3ee" />
          <stop offset="1" stopColor="#a78bfa" />
        </linearGradient>
        <linearGradient id={fill} x1="4" y1="2" x2="28" y2="30" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#0e1520" />
          <stop offset="1" stopColor="#070a11" />
        </linearGradient>
      </defs>
      <rect x="1.25" y="1.25" width="29.5" height="29.5" rx="9.25" fill={`url(#${fill})`} />
      <rect
        x="1.25"
        y="1.25"
        width="29.5"
        height="29.5"
        rx="9.25"
        fill="none"
        stroke={`url(#${stroke})`}
        strokeOpacity="0.55"
        strokeWidth="1.1"
      />
      <path
        d="M10.6 10.4h10.9l-9.5 11.2h10.4"
        fill="none"
        stroke={`url(#${stroke})`}
        strokeWidth="2.3"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <circle cx="24.1" cy="8.2" r="1.9" fill="#67e8f9" />
    </svg>
  );
}

/** Brand lockup: mark, name, and the small "STUDIO" eyebrow. */
export function Wordmark({ compact = false }: { compact?: boolean }) {
  return (
    <span className="flex items-center gap-2.5">
      <BrandMark size={compact ? 24 : 30} />
      <span className="flex flex-col leading-none">
        <span className="text-[0.95rem] font-semibold tracking-[-0.015em] text-white">
          Zeidex
        </span>
        {!compact && (
          <span className="mt-1 text-[0.5rem] font-medium uppercase tracking-[0.34em] text-zinc-500">
            Studio
          </span>
        )}
      </span>
    </span>
  );
}
