import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { parseIndexStatus, type IndexStatus } from "./index-status";

export type LaunchResultAction =
  | { type: "open"; path: string }
  | { type: "open_folder"; path: string }
  | { type: "reveal"; path: string }
  | { type: "web_search"; query: string }
  | { type: "translate"; text: string; target_lang: string }
  | { type: "perform_block"; candidate_id: string };

export interface LaunchResult {
  id: string;
  kind: string;
  title: string;
  subtitle: string | null;
  path: string;
  score: number;
  action: LaunchResultAction;
  icon?: string | null;
}

export interface ClipEntry {
  id: number;
  content: string;
  kind: string;
  copied_at_unix_s: number;
  copy_count: number;
  pinned: boolean;
}

export type Tab = "all" | "apps" | "files" | "clipboard";

export interface BBox {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

export interface TableCell {
  row: number;
  col: number;
  row_span: number;
  col_span: number;
  bbox: BBox;
  text: string;
}

export interface TableStructure {
  bbox: BBox;
  rows: number;
  cols: number;
  cells: TableCell[];
}

export interface Region {
  kind: "text" | "title" | "list" | "table" | "figure" | "equation" | "header" | "footer" | "caption";
  bbox: BBox;
  confidence: number;
  text?: string | null;
  table?: TableStructure | null;
}

export interface PageLayout {
  width: number;
  height: number;
  regions: Region[];
}

export interface PageResult {
  page_index: number;
  width: number;
  height: number;
  layout: PageLayout;
  text: string;
}

export interface ModelsStatus {
  dir: string;
  complete: boolean;
  missing: string[];
}

export interface PagePreview {
  data_url: string;
  width: number;
  height: number;
}

export interface Calculation {
  display: string;
  raw: string;
  value: number;
}

export interface TranslationFailure {
  code: string;
  message: string;
}

export interface TranslationOutcome {
  original: string;
  translated: string;
  error: TranslationFailure | null;
}

export type PipelineProgress =
  | { type: "started"; pages: number }
  | { type: "page_done"; page: number; pages: number }
  | { type: "finished" };

export type BatchStatus = "exact" | "high" | "possible" | "not_found";
export type BatchMatchKind = "exact" | "stem" | "containment" | "tokens" | "fuzzy";
export type BatchFileKind = "file" | "folder";

/** What the review step shows before a search runs. */
export interface ParsedList {
  entries: string[];
  /** Lines that were only whitespace or quotes. */
  dropped_empty: number;
  /** Entries folded into an earlier one because they normalize the same. */
  duplicates: number;
  /** Entries past the backend's ceiling that were not accepted. */
  overflow: number;
}

export interface BatchMatch {
  path: string;
  name: string;
  extension: string | null;
  size_bytes: number | null;
  modified_unix_s: number | null;
  kind: BatchFileKind;
  /** Integer percentage, the same number the threshold is compared against. */
  score: number;
  match_type: BatchMatchKind;
}

export interface BatchEntryResult {
  requested: string;
  status: BatchStatus;
  matches: BatchMatch[];
  total_matches: number;
  duration_ms: number;
}

export interface BatchSummary {
  total: number;
  found: number;
  not_found: number;
  exact: number;
  approximate: number;
  multiple: number;
  success_percent: number;
  duration_ms: number;
}

export interface BatchReport {
  threshold: number;
  indexed_files: number;
  entries: BatchEntryResult[];
  summary: BatchSummary;
}

/**
 * Bounds for the numeric settings. They travel from the backend, which enforces
 * the same numbers, so the two cannot drift apart in a copy.
 */
export interface SettingLimits {
  fileScanDepth: [number, number];
  fileScanLimit: [number, number];
  clipboardHistoryLimit: [number, number];
  clipboardRetentionDays: [number, number];
}

export interface AppSettings {
  version: string;
  platform: string;
  configPath: string | null;
  dbPath: string;
  candidateCount: number;
  fileScanDepth: number;
  fileScanLimit: number;
  fileScanExtraRoots: string[];
  lazyIndexingEnabled: boolean;
  localizedAppNames: boolean;
  clipboardHistoryLimit: number;
  clipboardFilterSecrets: boolean;
  clipboardRetentionDays: number;
  clipboardPaused: boolean;
  webSearchEnabled: boolean;
  webSearchEngine: string;
  searchEngines: string[];
  limits: SettingLimits;
}

/** Only the fields the user changed; omitted keys are left untouched. */
export interface SettingsPatch {
  fileScanDepth?: number;
  fileScanLimit?: number;
  fileScanExtraRoots?: string[];
  lazyIndexingEnabled?: boolean;
  localizedAppNames?: boolean;
  clipboardHistoryLimit?: number;
  clipboardFilterSecrets?: boolean;
  clipboardRetentionDays?: number;
  webSearch?: { enabled: boolean; engine: string };
}

export interface SettingsUpdate {
  settings: AppSettings;
  reindexRequired: boolean;
  warning: string | null;
}

export const api = {
  search: (query: string, limit = 10) =>
    invoke<LaunchResult[]>("search", { query, limit }),
  calcEval: (query: string) => invoke<Calculation | null>("calc_eval", { query }),
  bootstrapIndex: () => invoke<number>("bootstrap_index"),
  candidateCount: () => invoke<number>("candidate_count"),
  recordUsage: (candidateId: string, action: string) =>
    invoke<void>("record_usage", { candidateId, action }),
  clipboardList: (kind: string, query: string, limit = 100) =>
    invoke<ClipEntry[]>("clipboard_list", { kind, query, limit }),
  clipboardPoll: () => invoke<null>("clipboard_poll"),
  clipboardPaused: () => invoke<boolean>("clipboard_paused"),
  clipboardSetPaused: (paused: boolean) =>
    invoke<boolean>("clipboard_set_paused", { paused }),
  clipboardDelete: (id: number) => invoke<boolean>("clipboard_delete", { id }),
  clipboardSetPinned: (id: number, pinned: boolean) =>
    invoke<boolean>("clipboard_set_pinned", { id, pinned }),
  clipboardClear: () => invoke<number>("clipboard_clear"),
  openTarget: (path: string) => invoke<void>("open_target", { path }),
  revealTarget: (path: string) => invoke<void>("reveal_target", { path }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  webSearchUrl: (query: string) => invoke<string>("web_search_url", { query }),
  dbPath: () => invoke<string>("db_path"),
  configPath: () => invoke<string>("config_path"),
  appVersion: () => invoke<string>("app_version"),
  settingsSnapshot: () => invoke<AppSettings>("settings_snapshot"),
  settingsUpdate: (patch: SettingsPatch) =>
    invoke<SettingsUpdate>("settings_update", { patch }),
  translateText: (text: string, targetLang: string) =>
    invoke<TranslationOutcome>("translate_text", { text, targetLang }),
  ocrModelsStatus: () => invoke<ModelsStatus>("ocr_models_status"),
  ocrPagePreview: (path: string, page = 0, maxSide = 1400) =>
    invoke<PagePreview>("ocr_page_preview", { path, page, maxSide }),
  ocrProcessImage: (path: string) => invoke<PageResult>("ocr_process_image", { path }),
  ocrProcessPdf: (path: string) => invoke<PageResult[]>("ocr_process_pdf", { path }),
  ocrCancel: () => invoke<boolean>("ocr_cancel"),
  ocrProcessPdfRange: (path: string, firstPage: number, lastPage: number) =>
    invoke<PageResult[]>("ocr_process_pdf_range", { path, firstPage, lastPage }),
  exportDocx: (page: PageResult, path: string) =>
    invoke<void>("export_docx", { page, path }),
  exportDocxPages: (pages: PageResult[], path: string) =>
    invoke<void>("export_docx_pages", { pages, path }),
  exportXlsx: (page: PageResult, path: string) =>
    invoke<void>("export_xlsx", { page, path }),
  exportXlsxPages: (pages: PageResult[], path: string) =>
    invoke<void>("export_xlsx_pages", { pages, path }),
  batchParse: (text: string) => invoke<ParsedList>("batch_parse", { text }),
  batchImport: (path: string) => invoke<ParsedList>("batch_import", { path }),
  batchSearch: (entries: string[], threshold: number) =>
    invoke<BatchReport>("batch_search", { entries, threshold }),
  batchExport: (report: BatchReport, path: string) =>
    invoke<void>("batch_export", { report, path }),
  onOcrProgress: (callback: (event: PipelineProgress) => void) =>
    listen<PipelineProgress>("ocr-progress", (event) => callback(event.payload)),
  onIndexStatus: (callback: (status: IndexStatus) => void): Promise<() => void> =>
    listen<unknown>("zeidex://index-status", (event) => {
      const status = parseIndexStatus(event.payload);
      if (status) callback(status);
    }),
  onMode: async (callback: (mode: string) => void): Promise<() => void> =>
    listen<string>("zeidex-mode", (event) => callback(event.payload)),
};
