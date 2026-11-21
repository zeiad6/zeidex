/**
 * The index-status event payload, parsed defensively.
 *
 * The backend is the only producer, but an event listener receives `unknown`
 * and a malformed payload (a version skew, a test double) must not put
 * `undefined` in the footer or, worse, `NaN` in a count. A rejected payload is
 * dropped; the previous count stays.
 */
export interface IndexStatus {
  indexing: boolean;
  count: number;
  error: string | null;
}

export function parseIndexStatus(payload: unknown): IndexStatus | null {
  if (typeof payload !== "object" || payload === null) return null;
  const record = payload as Record<string, unknown>;
  if (typeof record.indexing !== "boolean") return null;
  if (typeof record.count !== "number" || !Number.isFinite(record.count) || record.count < 0) {
    return null;
  }
  if (record.error !== null && record.error !== undefined && typeof record.error !== "string") {
    return null;
  }
  return {
    indexing: record.indexing,
    count: Math.trunc(record.count),
    error: typeof record.error === "string" ? record.error : null,
  };
}
