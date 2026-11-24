import { Icon } from "./Icons";
import { useTheme } from "../theme-context";

/**
 * The one theme button, used in every screen header.
 *
 * It is a single icon button rather than a three-way control: the header is
 * narrow, and the common action is "give me the other theme". Settings keeps
 * the full Light / Dark / System selector for the deliberate choice.
 *
 * The glyph shows the theme you are in (sun on paper, moon in the dark), so the
 * button reads as a state before it is pressed; the tooltip and the accessible
 * name state the destination, so a screen reader hears what pressing it does.
 */
export default function ThemeToggle({ rtl }: { rtl: boolean }) {
  const { resolved, toggle } = useTheme();
  const nextIsLight = resolved === "dark";
  const label = rtl
    ? nextIsLight
      ? "التبديل إلى السمة الفاتحة"
      : "التبديل إلى السمة الداكنة"
    : nextIsLight
      ? "Switch to light theme"
      : "Switch to dark theme";

  return (
    <button
      type="button"
      onClick={toggle}
      aria-label={label}
      title={label}
      className="theme-toggle grid h-8 w-8 place-items-center rounded-full border border-white/10 text-zinc-400 transition hover:border-accent-400/30 hover:bg-white/10 hover:text-accent-200 active:scale-95"
    >
      <span key={resolved} className="theme-toggle-icon grid place-items-center">
        <Icon name={resolved === "light" ? "sun" : "moon"} size={15} />
      </span>
    </button>
  );
}
