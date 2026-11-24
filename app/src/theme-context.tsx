import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  applyTheme,
  resolveTheme,
  saveTheme,
  storedTheme,
  systemPrefersLight,
  type ResolvedTheme,
  type ThemeChoice,
} from "./theme";

/**
 * The one theme state for the whole application.
 *
 * Before this provider, `App` owned the choice and passed it down to Settings
 * while every other screen had no way to change it. The provider makes the
 * choice global: any header can render `ThemeToggle`, every screen reads the
 * same resolved value, and the CSS attribute is written once per change.
 */
interface ThemeContextValue {
  /** What the user chose, including `system`. */
  choice: ThemeChoice;
  /** What that resolves to right now. */
  resolved: ResolvedTheme;
  setChoice: (next: ThemeChoice) => void;
  /** Light <-> dark. `system` becomes the opposite of what it resolves to. */
  toggle: () => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [choice, setChoiceState] = useState<ThemeChoice>(storedTheme);
  const [osPrefersLight, setOsPrefersLight] = useState(systemPrefersLight);
  const resolved = resolveTheme(choice, osPrefersLight);

  // `system` must keep following the OS while the window is open, so the media
  // query is a subscription rather than a value read once at boot.
  useEffect(() => {
    let query: MediaQueryList;
    try {
      query = window.matchMedia("(prefers-color-scheme: light)");
    } catch {
      return;
    }
    const onChange = () => setOsPrefersLight(query.matches);
    setOsPrefersLight(query.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);

  // Paint the resolved theme, and mark the document for the length of a change
  // so the surfaces cross-fade instead of snapping. The mark is temporary: see
  // the `theme-switching` rules in `styles.css`.
  const painted = useRef<ResolvedTheme | null>(null);
  useEffect(() => {
    applyTheme(resolved);
    const previous = painted.current;
    painted.current = resolved;
    if (previous === null || previous === resolved) return;
    const root = document.documentElement;
    root.classList.add("theme-switching");
    const timer = window.setTimeout(() => root.classList.remove("theme-switching"), 260);
    return () => {
      window.clearTimeout(timer);
      root.classList.remove("theme-switching");
    };
  }, [resolved]);

  const setChoice = useCallback((next: ThemeChoice) => {
    setChoiceState(next);
    saveTheme(next);
  }, []);

  const toggle = useCallback(() => {
    setChoice(resolved === "dark" ? "light" : "dark");
  }, [resolved, setChoice]);

  return (
    <ThemeContext.Provider value={{ choice, resolved, setChoice, toggle }}>
      {children}
    </ThemeContext.Provider>
  );
}

/** The provider is mounted in `main.tsx`; a missing one is a wiring bug. */
export function useTheme(): ThemeContextValue {
  const value = useContext(ThemeContext);
  if (!value) {
    throw new Error("useTheme must be used inside ThemeProvider");
  }
  return value;
}
