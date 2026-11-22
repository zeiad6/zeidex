/**
 * Formatting for the OCR Models screen. Kept out of the components so the
 * numbers are computed once, in one place, and can be unit tested.
 */

/** Bytes to a short human figure: `18 MB`, `1.2 GB`. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 MB";
  if (bytes >= 1024 * 1024 * 1024) {
    const gb = bytes / (1024 * 1024 * 1024);
    return `${gb >= 10 ? gb.toFixed(0) : gb.toFixed(1)} GB`;
  }
  const mb = bytes / (1024 * 1024);
  return `${mb >= 10 ? Math.round(mb) : mb.toFixed(1)} MB`;
}

/** Bytes per second to `840 KB/s` or `3.2 MB/s`. */
export function formatSpeed(bytesPerSecond: number): string {
  if (!Number.isFinite(bytesPerSecond) || bytesPerSecond <= 0) return "—";
  if (bytesPerSecond >= 1024 * 1024) {
    return `${(bytesPerSecond / (1024 * 1024)).toFixed(1)} MB/s`;
  }
  return `${Math.max(1, Math.round(bytesPerSecond / 1024))} KB/s`;
}

/** 0–100, clamped, for a progress bar. */
export function percentOf(done: number, total: number): number {
  if (!Number.isFinite(done) || !Number.isFinite(total) || total <= 0) return 0;
  return Math.max(0, Math.min(100, Math.round((done / total) * 100)));
}

/** `0.042` → `4.2%`. */
export function formatCer(cer: number): string {
  if (!Number.isFinite(cer)) return "—";
  return `${(Math.max(0, Math.min(1, cer)) * 100).toFixed(1)}%`;
}
