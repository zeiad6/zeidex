import type { IndexStatus } from "./index-status";

/**
 * The index's health, derived rather than stored.
 *
 * The shell already reports the three facts that decide it — a running walk, a
 * parked walk, and an error — so a second backend field would be a second
 * source of truth that can disagree with the footer. A missing status is
 * "needs refresh": the first walk has not reported yet, and claiming "healthy"
 * before any row exists would be the one lie this card must not tell.
 */
export type IndexHealth = "healthy" | "updating" | "needs-refresh" | "partial" | "error";

export function indexHealth(status: IndexStatus | null): IndexHealth {
  if (!status) return "needs-refresh";
  // An error outranks the walk flags: a walk that failed while a previous
  // status still said "updating" must not keep showing progress.
  if (status.error) return "error";
  if (status.paused) return "partial";
  if (status.indexing) return "updating";
  if (status.count === 0) return "needs-refresh";
  return "healthy";
}
