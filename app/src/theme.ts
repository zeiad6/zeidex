/**
 * Theme selection.
 *
 * The interface is written against a dark surface, so "light" is not a second
 * design: it is the same tokens re-pointed (see the `data-theme="light"` block
 * in `styles.css`). That makes the choice a single attribute on the root
 * element, which is what this module owns.
 *
 * The choice outlives the window in local storage, like the interface
 * direction. `system` follows the OS and keeps following it while the app runs,
 * so a desktop that switches at sunset does not need a restart.
 */

export type ThemeChoice = "dark" | "light" | "system";
export type ResolvedTheme = "dark" | "light";

export const THEME_KEY = "zeidex.theme";

/**
 * Anything unrecognised means dark: it is the product's own identity, and a
 * corrupted or older value should not silently flip a user into a theme they
 * never chose.
 */
export function parseTheme(value: string | null | undefined): ThemeChoice {
  return value === "light" || value === "system" ? value : "dark";
}

/**
 * `system` is the only choice that needs the OS answer; the other two are
 * already resolved. Kept pure so the decision is testable without a DOM.
 */
export function resolveTheme(
  choice: ThemeChoice,
  systemPrefersLight: boolean,
): ResolvedTheme {
  if (choice === "system") return systemPrefersLight ? "light" : "dark";
  return choice;
}

/** The OS preference, or dark when the platform cannot answer. */
export function systemPrefersLight(): boolean {
  try {
    return window.matchMedia("(prefers-color-scheme: light)").matches;
  } catch {
    return false;
  }
}

/**
 * Apply the resolved theme. `color-scheme` goes on the root as well so native
 * widgets (scrollbars, form controls, the caret) follow the theme instead of
 * staying dark on a light surface.
 */
export function applyTheme(
  theme: ResolvedTheme,
  root: HTMLElement | null = typeof document === "undefined"
    ? null
    : document.documentElement,
): void {
  if (!root) return;
  root.dataset.theme = theme;
  root.style.colorScheme = theme;
}

/** Read the saved choice; a storage failure is a dark session, not a crash. */
export function storedTheme(): ThemeChoice {
  try {
    return parseTheme(window.localStorage.getItem(THEME_KEY));
  } catch {
    return "dark";
  }
}

/** Save the choice; the theme still applies for this run if storage refuses. */
export function saveTheme(choice: ThemeChoice): void {
  try {
    window.localStorage.setItem(THEME_KEY, choice);
  } catch {
    // Private mode or a wiped profile: the choice is session-only.
  }
}
