import { fileFormatOf, type FileFormat } from "../file-format";

/**
 * A small, self-contained format badge: a coloured tile with a white page and
 * the format's own mark. It is drawn, not an image, so it inherits the theme,
 * stays sharp at every DPI and adds no asset to the bundle.
 *
 * `aria-hidden` by default: the file name beside it already says what the file
 * is. Pass `decorative={false}` where the badge stands alone.
 */
export default function FileGlyph({
  path,
  size = 22,
  className = "",
  label,
  decorative = true,
}: {
  path: string;
  size?: number;
  className?: string;
  label?: string;
  decorative?: boolean;
}) {
  const format = fileFormatOf(path);
  const caption = label ?? `${format.name} file`;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 32 32"
      className={className}
      role={decorative ? undefined : "img"}
      aria-hidden={decorative ? "true" : undefined}
      aria-label={decorative ? undefined : caption}
      focusable="false"
    >
      {!decorative && <title>{caption}</title>}
      <rect x="1" y="1" width="30" height="30" rx="8.5" fill={format.color} />
      <path
        d="M9.5 1h13A8.5 8.5 0 0 1 31 9.5V15H1V9.5A8.5 8.5 0 0 1 9.5 1Z"
        fill="#fff"
        opacity="0.14"
      />
      <rect
        x="1"
        y="1"
        width="30"
        height="30"
        rx="8.5"
        fill="none"
        stroke="#fff"
        strokeOpacity="0.18"
      />
      <path
        d="M9.4 6.1h8.9l4.9 4.9v13.7a1.6 1.6 0 0 1-1.6 1.6H9.4a1.6 1.6 0 0 1-1.6-1.6V7.7a1.6 1.6 0 0 1 1.6-1.6Z"
        fill="#fff"
        fillOpacity="0.97"
        stroke="#0b1220"
        strokeOpacity="0.08"
        strokeWidth="0.6"
      />
      <path d="M18.3 6.1 23.2 11h-4.9V6.1Z" fill={format.color} fillOpacity="0.32" />
      <Mark format={format} />
    </svg>
  );
}

function Mark({ format }: { format: FileFormat }) {
  const color = format.color;
  switch (format.id) {
    case "pdf":
      return (
        <text
          x="16"
          y="23.1"
          textAnchor="middle"
          fontFamily="ui-sans-serif, system-ui, sans-serif"
          fontSize="5.4"
          fontWeight="800"
          letterSpacing="0.15"
          fill={color}
        >
          PDF
        </text>
      );
    case "word":
      return (
        <text
          x="16"
          y="24.2"
          textAnchor="middle"
          fontFamily="ui-sans-serif, system-ui, sans-serif"
          fontSize="9"
          fontWeight="800"
          fill={color}
        >
          W
        </text>
      );
    case "sheet":
      return (
        <text
          x="16"
          y="24.2"
          textAnchor="middle"
          fontFamily="ui-sans-serif, system-ui, sans-serif"
          fontSize="9"
          fontWeight="800"
          fill={color}
        >
          X
        </text>
      );
    case "slides":
      return (
        <text
          x="16"
          y="24.2"
          textAnchor="middle"
          fontFamily="ui-sans-serif, system-ui, sans-serif"
          fontSize="9"
          fontWeight="800"
          fill={color}
        >
          P
        </text>
      );
    case "image":
      return (
        <g fill={color}>
          <circle cx="13.5" cy="19.2" r="1.35" />
          <path d="M10.8 25.2l3.7-4.6 2.5 3 1.9-2.2 2.4 3.8Z" />
        </g>
      );
    case "text":
      return (
        <g stroke={color} strokeWidth="1.15" strokeLinecap="round">
          <path d="M11.5 18h9M11.5 21h9M11.5 24h5.6" />
        </g>
      );
    case "code":
      return (
        <text
          x="16"
          y="23.4"
          textAnchor="middle"
          fontFamily="ui-monospace, SFMono-Regular, monospace"
          fontSize="5"
          fontWeight="700"
          fill={color}
        >
          {"</>"}
        </text>
      );
    case "archive":
      return (
        <g stroke={color} strokeWidth="1.1" strokeLinecap="round">
          <path d="M16 17v8" />
          <path d="M13.9 19.2h4.2M13.9 21.4h4.2M13.9 23.6h4.2" />
        </g>
      );
    case "audio":
      return (
        <g fill={color} stroke={color} strokeWidth="1.1">
          <path d="M14.4 23V18l5-1v5" fill="none" strokeLinecap="round" />
          <circle cx="12.9" cy="23.4" r="1.5" stroke="none" />
          <circle cx="17.9" cy="22.4" r="1.5" stroke="none" />
        </g>
      );
    case "video":
      return (
        <g>
          <rect
            x="10.8"
            y="17.2"
            width="10.4"
            height="8"
            rx="1.6"
            fill="none"
            stroke={color}
            strokeWidth="1.1"
          />
          <path d="M14.6 19.3 18.5 21.2l-3.9 1.9Z" fill={color} />
        </g>
      );
    default:
      return (
        <g stroke={color} strokeWidth="1.15" strokeLinecap="round">
          <path d="M11.5 18h9M11.5 21h9M11.5 24h6.2" />
        </g>
      );
  }
}
