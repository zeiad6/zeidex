/**
 * Where a fixed-position context menu can sit without leaving the window.
 *
 * A right-click near the bottom-right corner would otherwise put the menu half
 * outside the webview, where the last item is unreachable. The margin keeps a
 * sliver of the panel visible so it does not look glued to the edge.
 */
export function menuPosition(
  x: number,
  y: number,
  width: number,
  height: number,
  viewportWidth: number,
  viewportHeight: number,
  margin = 8,
): { left: number; top: number } {
  return {
    left: Math.max(margin, Math.min(x, viewportWidth - width - margin)),
    top: Math.max(margin, Math.min(y, viewportHeight - height - margin)),
  };
}
