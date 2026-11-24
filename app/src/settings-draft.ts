import type { AppSettings, SettingsPatch } from "./api";

/**
 * The form's working copy. Numbers stay strings while they are being typed: a
 * `number` input bound to a number cannot represent "1." or an emptied field,
 * and turning those into 0 (or NaN) would save a value nobody asked for.
 */
export interface Draft {
  fileScanDepth: string;
  fileScanLimit: string;
  fileScanExtraRoots: string[];
  lazyIndexingEnabled: boolean;
  startMinimized: boolean;
  localizedAppNames: boolean;
  clipboardHistoryLimit: string;
  clipboardFilterSecrets: boolean;
  clipboardRetentionDays: string;
  webSearchEnabled: boolean;
  webSearchEngine: string;
}

export type FieldError = "required" | "integer" | "range";

export interface ChangeSet {
  patch: SettingsPatch;
  errors: Partial<Record<keyof Draft, FieldError>>;
  dirty: boolean;
}

export function toDraft(settings: AppSettings): Draft {
  return {
    fileScanDepth: String(settings.fileScanDepth),
    fileScanLimit: String(settings.fileScanLimit),
    fileScanExtraRoots: [...settings.fileScanExtraRoots],
    lazyIndexingEnabled: settings.lazyIndexingEnabled,
    startMinimized: settings.startMinimized,
    localizedAppNames: settings.localizedAppNames,
    clipboardHistoryLimit: String(settings.clipboardHistoryLimit),
    clipboardFilterSecrets: settings.clipboardFilterSecrets,
    clipboardRetentionDays: String(settings.clipboardRetentionDays),
    webSearchEnabled: settings.webSearchEnabled,
    webSearchEngine: settings.webSearchEngine,
  };
}

export function readInteger(
  raw: string,
  [min, max]: [number, number],
): { value: number } | { error: FieldError } {
  const trimmed = raw.trim();
  if (trimmed === "") return { error: "required" };
  const value = Number(trimmed);
  if (!Number.isInteger(value)) return { error: "integer" };
  if (value < min || value > max) return { error: "range" };
  return { value };
}

/**
 * What the user changed, and what is wrong with the rest. Client-side checks
 * exist so the error appears next to the field; the backend re-validates every
 * value against the same bounds and stays authoritative.
 *
 * A field the user never touched is never checked: a hand-edited config can
 * hold a value outside today's bounds, and reporting it as an error the moment
 * the panel opens would be a lie about the draft.
 */
export function diff(draft: Draft, settings: AppSettings): ChangeSet {
  const patch: SettingsPatch = {};
  const errors: Partial<Record<keyof Draft, FieldError>> = {};
  const limits = settings.limits;

  const number = (
    key: "fileScanDepth" | "fileScanLimit" | "clipboardHistoryLimit" | "clipboardRetentionDays",
    current: number,
    bounds: [number, number],
  ) => {
    if (draft[key] === String(current)) return;
    const parsed = readInteger(draft[key], bounds);
    if ("error" in parsed) {
      errors[key] = parsed.error;
      return;
    }
    if (parsed.value !== current) patch[key] = parsed.value;
  };

  number("fileScanDepth", settings.fileScanDepth, limits.fileScanDepth);
  number("fileScanLimit", settings.fileScanLimit, limits.fileScanLimit);
  number("clipboardHistoryLimit", settings.clipboardHistoryLimit, limits.clipboardHistoryLimit);
  number("clipboardRetentionDays", settings.clipboardRetentionDays, limits.clipboardRetentionDays);

  const rootsChanged =
    draft.fileScanExtraRoots.length !== settings.fileScanExtraRoots.length ||
    draft.fileScanExtraRoots.some((root, index) => root !== settings.fileScanExtraRoots[index]);
  if (rootsChanged) patch.fileScanExtraRoots = draft.fileScanExtraRoots;

  if (draft.lazyIndexingEnabled !== settings.lazyIndexingEnabled) {
    patch.lazyIndexingEnabled = draft.lazyIndexingEnabled;
  }
  if (draft.startMinimized !== settings.startMinimized) {
    patch.startMinimized = draft.startMinimized;
  }
  if (draft.localizedAppNames !== settings.localizedAppNames) {
    patch.localizedAppNames = draft.localizedAppNames;
  }
  if (draft.clipboardFilterSecrets !== settings.clipboardFilterSecrets) {
    patch.clipboardFilterSecrets = draft.clipboardFilterSecrets;
  }
  if (
    draft.webSearchEnabled !== settings.webSearchEnabled ||
    draft.webSearchEngine !== settings.webSearchEngine
  ) {
    patch.webSearch = {
      enabled: draft.webSearchEnabled,
      engine: draft.webSearchEngine,
    };
  }

  const dirty = Object.keys(patch).length > 0 || Object.keys(errors).length > 0;
  return { patch, errors, dirty };
}
