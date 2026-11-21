export type WindowSize = { width: number; height: number };

/**
 * Below these the two-pane layouts start clipping their controls, so the
 * clamped window never shrinks past them unless the screen itself is smaller.
 */
export const MIN_WINDOW: WindowSize = { width: 720, height: 480 };

/**
 * Tauri reports monitor bounds, not the desktop work area, and the window
 * manager adds decorations outside the client size. Reserving this much covers
 * title bars plus a taskbar, dock or panel on Linux, Windows and macOS.
 */
export const SCREEN_MARGIN: WindowSize = { width: 48, height: 112 };

/**
 * Fit a requested client size inside the monitor that will host the window.
 *
 * A `null` screen means the platform could not answer (browser demo, a
 * monitorless session): the requested size is returned untouched so the
 * phrasing "no monitor information" never turns into a zero-sized window.
 */
export function clampToScreen(
  requested: WindowSize,
  screen: WindowSize | null,
): WindowSize {
  if (!screen) return requested;
  const maxWidth = Math.floor(screen.width - SCREEN_MARGIN.width);
  const maxHeight = Math.floor(screen.height - SCREEN_MARGIN.height);
  if (maxWidth <= 0 || maxHeight <= 0) return requested;

  const width = Math.max(
    Math.min(MIN_WINDOW.width, maxWidth),
    Math.min(requested.width, maxWidth),
  );
  const height = Math.max(
    Math.min(MIN_WINDOW.height, maxHeight),
    Math.min(requested.height, maxHeight),
  );
  return { width, height };
}

/**
 * Convert the monitor report into logical pixels, the unit `LogicalSize`
 * expects. A non-positive scale factor is a bogus report, not a screen of
 * infinite size.
 */
export function logicalScreen(
  monitor: { size: { width: number; height: number }; scaleFactor: number } | null,
): WindowSize | null {
  if (!monitor || !(monitor.scaleFactor > 0)) return null;
  const width = monitor.size.width / monitor.scaleFactor;
  const height = monitor.size.height / monitor.scaleFactor;
  if (!(width > 0) || !(height > 0)) return null;
  return { width, height };
}
