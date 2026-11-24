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

/**
 * How the query matched a result's name, strongest first. Computed in Rust
 * (`core/matching::MatchKind`); the UI only labels it.
 */
export type MatchKind =
  | "exact"
  | "prefix"
  | "suffix"
  | "contains"
  | "fuzzy"
  | "approximate";

export interface LaunchResult {
  id: string;
  kind: string;
  title: string;
  subtitle: string | null;
  path: string;
  score: number;
  action: LaunchResultAction;
  icon?: string | null;
  match_kind?: MatchKind | null;
  /** File size in bytes, when the row has a file behind it. */
  size_bytes?: number | null;
  /** Filesystem modification time (Unix seconds), when there is one. */
  modified_unix_s?: number | null;
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

export interface DocumentIndexStatus {
  enabled: boolean;
  count: number;
}

export interface PageResult {
  page_index: number;
  width: number;
  height: number;
  layout: PageLayout;
  text: string;
}

/** A document profile the organizer knows. */
export type DocumentKind =
  | "id_card"
  | "passport"
  | "invoice"
  | "receipt"
  | "bank_statement"
  | "certificate"
  | "business_card"
  | "letter"
  | "form"
  | "table"
  | "unknown";

export type OrganizedBlockKind =
  | "title"
  | "heading"
  | "paragraph"
  | "list"
  | "key_value"
  | "table"
  | "contact"
  | "date"
  | "amount"
  | "figure"
  | "equation"
  | "header"
  | "footer"
  | "caption";

export interface OrganizedTable {
  header: string[];
  rows: string[][];
  columns: number;
  confidence: number;
  needs_review: boolean;
}

export interface OrganizedBlock {
  kind: OrganizedBlockKind;
  text: string;
  level?: number | null;
  items?: string[];
  key?: string | null;
  value?: string | null;
  table?: OrganizedTable | null;
  confidence: number;
  source_index: number;
  page: number;
}

export type OrganizerLanguage = "arabic" | "english" | "mixed" | "unknown";

export interface OrganizerWarning {
  code: string;
  message: string;
  block?: number | null;
}

export interface OrganizerStats {
  characters: number;
  words: number;
  arabic_ratio: number;
  blocks: number;
  tables: number;
  needs_review: number;
}

/** The organized view of the recognized pages. */
export interface OrganizedDocument {
  schema: number;
  pages: number;
  language: OrganizerLanguage;
  blocks: OrganizedBlock[];
  stats: OrganizerStats;
  warnings: OrganizerWarning[];
}

export interface ExtractionField {
  key: string;
  label: string;
  label_ar: string;
  value: string;
  confidence: number;
  needs_review: boolean;
  source_index: number;
  page: number;
}

export interface Extraction {
  schema: number;
  kind: DocumentKind;
  confidence: number;
  language: OrganizerLanguage;
  fields: ExtractionField[];
  warnings: OrganizerWarning[];
}

export interface BatchColumn {
  key: string;
  label: string;
  label_ar: string;
}

export interface BatchRow {
  /** Page this row came from. Row order is page order and is never renumbered. */
  page: number;
  values: string[];
  confidence: number;
  needs_review: boolean;
}

/** One row per page: a stack of ID cards cannot shift or interleave. */
export interface BatchExtraction {
  schema: number;
  kind: DocumentKind;
  confidence: number;
  language: OrganizerLanguage;
  columns: BatchColumn[];
  rows: BatchRow[];
  warnings: OrganizerWarning[];
}

export interface ModelsStatus {
  dir: string;
  complete: boolean;
  missing: string[];
  pack_id: string;
  pack_name: string;
  pack_name_ar: string;
  bundled: boolean;
  bundled_dir: string;
  models_root: string;
}

export type ModelTier = "compact" | "balanced" | "precision";
export type InstallState = "not_installed" | "partial" | "installed";

export interface ModelFileView {
  role: string;
  name: string;
  present: boolean;
  expected_bytes: number;
  actual_bytes: number;
}

/** One row of the OCR Models screen. */
export interface ModelPackView {
  id: string;
  tier: ModelTier;
  tier_label: string;
  tier_label_ar: string;
  name: string;
  name_ar: string;
  summary: string;
  summary_ar: string;
  ram_mb: number;
  download_bytes: number;
  installed_bytes: number;
  bundled: boolean;
  arabic: boolean;
  english: boolean;
  rec_ar_visual: boolean;
  source: string;
  license: string;
  state: InstallState;
  active: boolean;
  downloading: boolean;
  dir: string;
  files: ModelFileView[];
}

export interface DeviceProfile {
  cpu_model: string;
  physical_cores: number;
  logical_cores: number;
  total_ram_mb: number;
  available_ram_mb: number;
  free_disk_mb: number;
  os: string;
  arch: string;
  gpu: string | null;
  gpu_memory_mb: number | null;
  gpu_acceleration: boolean;
}

export type DeviceClass = "basic" | "standard" | "advanced" | "high_performance";

export interface Criterion {
  key: string;
  label: string;
  label_ar: string;
  met: boolean;
  points: number;
  detail: string;
}

export interface Recommendation {
  pack_id: string;
  device_class: DeviceClass;
  score: number;
  reason: string;
  reason_ar: string;
  downgraded: boolean;
  /** The recommended pack needs more RAM than the device can give it. */
  insufficient_memory: boolean;
  ocr_concurrency: number;
  batch_chunk: number;
  index_threads: number;
}

export interface AdvisorView {
  profile: DeviceProfile;
  class: DeviceClass;
  class_label: string;
  class_label_ar: string;
  score: number;
  max_score: number;
  criteria: Criterion[];
  recommendation: Recommendation;
  recommended_pack: ModelPackView;
  ram_used_percent: number;
}

export interface ModelVerifyView {
  pack_id: string;
  ok: boolean;
  problems: string[];
  checked_files: number;
  checked_bytes: number;
}

export interface ModelSampleView {
  name: string;
  milliseconds: number;
  characters: number;
  expected_characters: number;
  cer: number;
  text: string;
}

export interface ModelBenchmarkView {
  pack_id: string;
  pack_name: string;
  load_ms: number;
  total_ms: number;
  memory_mb: number;
  samples: ModelSampleView[];
}

export type DownloadPhase =
  | "downloading"
  | "verifying"
  | "installing"
  | "done"
  | "cancelled"
  | "error";

export interface DownloadProgress {
  pack_id: string;
  phase: DownloadPhase;
  file: string | null;
  file_index: number;
  file_count: number;
  downloaded: number;
  total: number;
  speed_bps: number;
  message: string | null;
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

export type BatchStatus = "exact" | "high" | "possible" | "not_found" | "error";
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
  errors: number;
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

export interface HotkeyStatus {
  accelerator: string;
  registered: boolean;
  error: string | null;
  default: string;
}

/** Launch-at-login state, read from the OS rather than a local flag. */
export interface AutostartStatus {
  supported: boolean;
  enabled: boolean;
  /** Human label for where the entry lives ("Windows registry", …). */
  mechanism: string;
  /** Registry key or file path, when there is one. */
  location: string | null;
  /** The executable the entry points at. */
  command: string;
  /** A debug build points at target/debug; enabling it is a trap. */
  development: boolean;
  error: string | null;
}

/** The four rule kinds the walker understands. */
export type ExclusionKind = "folder" | "file" | "extension" | "pattern";

/**
 * One saved exclusion. `added_at` is Unix seconds and may be `0` for a rule
 * written into the config file by hand.
 */
export interface ExclusionEntry {
  id: string;
  kind: ExclusionKind;
  value: string;
  enabled: boolean;
  added_at: number;
}

export interface PresetRule {
  kind: ExclusionKind;
  value: string;
}

export interface ExclusionPreset {
  id: string;
  name: string;
  nameAr: string;
  description: string;
  descriptionAr: string;
  rules: PresetRule[];
  /** How many of the preset's rules are already in the list. */
  alreadyApplied: number;
}

export interface ExclusionsSnapshot {
  entries: ExclusionEntry[];
  active: number;
  presets: ExclusionPreset[];
  configPath: string | null;
  indexPath: string;
}

/** What a mutation did to the list and to the index. */
export interface ExclusionsChange {
  entries: ExclusionEntry[];
  active: number;
  /** Rows deleted because the new list rejects them. */
  removed: number;
  /** True when a full walk was requested (extension/pattern removed). */
  reindexing: boolean;
}

/** A local volume, as the Drives screen shows it. */
export interface DriveInfo {
  root: string;
  label: string;
  removable: boolean;
  /** `ssd`, `hdd` or `unknown`. Presentation only. */
  kind: string;
  totalBytes: number;
  freeBytes: number;
  /** True when the drive root itself is a configured scan root. */
  scanRoot: boolean;
}

export interface DiagnosticsSnapshot {
  app: {
    name: string;
    version: string;
    build: string;
    tauri: string;
    developer: string;
    website: string;
    repository: string;
    license: string;
    copyright: string;
  };
  platform: { os: string; arch: string; family: string; executable: string };
  paths: {
    config: string | null;
    database: string;
    modelsBundled: string;
    modelsUser: string;
    onnxRuntime: string | null;
    pdfium: string | null;
    startupLog: string | null;
  };
  index: {
    candidates: number;
    documents: number;
    databaseBytes: number;
    scanRoots: string[];
    exclusionRules: number;
  };
}

/** The answer to "is there a newer release?", from the public GitHub API. */
export interface UpdateInfo {
  current: string;
  latest: string;
  available: boolean;
  url: string;
  notes: string;
  publishedAt: string;
  checkedAt: number;
  /** Why the check could not answer; `null` when it did. */
  error: string | null;
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
  startMinimized: boolean;
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
  startMinimized?: boolean;
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
  indexPause: () => invoke<boolean>("index_pause"),
  indexResume: () => invoke<boolean>("index_resume"),
  indexCancel: () => invoke<boolean>("index_cancel"),
  indexControlState: () => invoke<string>("index_control_state"),
  indexClear: () => invoke<number>("index_clear"),
  // Removes the launch history only; the index keeps answering searches.
  searchHistoryClear: () => invoke<number>("search_history_clear"),
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
  settingsPickFolder: (title?: string) =>
    invoke<string | null>("settings_pick_folder", { title }),
  // A real picker for "exclude this file". The dialog runs in Rust for the same
  // reason the folder picker does: the page never names a path it did not see
  // the user choose.
  settingsPickFile: (title?: string) =>
    invoke<string | null>("settings_pick_file", { title }),
  settingsUpdate: (patch: SettingsPatch) =>
    invoke<SettingsUpdate>("settings_update", { patch }),
  exclusionsSnapshot: () => invoke<ExclusionsSnapshot>("exclusions_snapshot"),
  exclusionsAdd: (kind: ExclusionKind, value: string) =>
    invoke<ExclusionsChange>("exclusions_add", { kind, value }),
  // Only the fields that changed; the backend re-validates the value against
  // the rule's own kind, so an edit cannot turn an extension into a path.
  exclusionsUpdate: (id: string, change: { value?: string; enabled?: boolean }) =>
    invoke<ExclusionsChange>("exclusions_update", { id, ...change }),
  exclusionsRemove: (id: string) =>
    invoke<ExclusionsChange>("exclusions_remove", { id }),
  exclusionsClear: () => invoke<ExclusionsChange>("exclusions_clear"),
  exclusionsApplyPreset: (preset: string) =>
    invoke<ExclusionsChange>("exclusions_apply_preset", { preset }),
  // How many indexed rows a candidate rule would reject right now.
  exclusionsPreview: (kind: ExclusionKind, value: string) =>
    invoke<number>("exclusions_preview", { kind, value }),
  systemDrives: () => invoke<DriveInfo[]>("system_drives"),
  diagnosticsSnapshot: () => invoke<DiagnosticsSnapshot>("diagnostics_snapshot"),
  checkForUpdates: () => invoke<UpdateInfo>("check_for_updates"),
  // The first-run guide is stored in the config file, not in webview storage:
  // the shell reads it before the window is shown.
  onboardingStatus: () => invoke<boolean>("onboarding_status"),
  onboardingComplete: () => invoke<boolean>("onboarding_complete"),
  hotkeyStatus: () => invoke<HotkeyStatus>("hotkey_status"),
  // `null` resets to the default. A combination the OS refuses comes back as a
  // rejected promise carrying the reason, so the panel can show it verbatim.
  hotkeySet: (accelerator: string | null) =>
    invoke<HotkeyStatus>("hotkey_set", { accelerator }),
  autostartStatus: () => invoke<AutostartStatus>("autostart_status"),
  autostartSet: (enabled: boolean) =>
    invoke<AutostartStatus>("autostart_set", { enabled }),
  translateText: (text: string, targetLang: string) =>
    invoke<TranslationOutcome>("translate_text", { text, targetLang }),
  ocrModelsStatus: () => invoke<ModelsStatus>("ocr_models_status"),
  // Rust opens the dialog and records the chosen path as readable, so the page
  // never decides what the next command touches. `null` = dialog dismissed.
  ocrPickFile: (title?: string, filterName?: string) =>
    invoke<string | null>("ocr_pick_file", { title, filterName }),
  ocrPagePreview: (path: string, page = 0, maxSide = 1400) =>
    invoke<PagePreview>("ocr_page_preview", { path, page, maxSide }),
  ocrProcessImage: (path: string) => invoke<PageResult>("ocr_process_image", { path }),
  documentExtractText: (path: string) =>
    invoke<PageResult[]>("document_extract_text", { path }),
  ocrProcessPdf: (path: string) => invoke<PageResult[]>("ocr_process_pdf", { path }),
  ocrCancel: () => invoke<boolean>("ocr_cancel"),
  ocrProcessPdfRange: (path: string, firstPage: number, lastPage: number) =>
    invoke<PageResult[]>("ocr_process_pdf_range", { path, firstPage, lastPage }),
  // Rust opens the save dialog and writes the file, so the page never chooses
  // the path it writes to. `null` means the dialog was dismissed.
  exportDocxPages: (
    pages: PageResult[],
    defaultName?: string,
    title?: string,
    filterName?: string,
  ) =>
    invoke<string | null>("export_docx_pages", {
      pages,
      defaultName,
      title,
      filterName,
    }),
  exportXlsxPages: (
    pages: PageResult[],
    defaultName?: string,
    title?: string,
    filterName?: string,
  ) =>
    invoke<string | null>("export_xlsx_pages", {
      pages,
      defaultName,
      title,
      filterName,
    }),
  // The organizer runs in Rust on the pages already in memory. Nothing is
  // written and nothing leaves the machine.
  organizePages: (pages: PageResult[]) =>
    invoke<OrganizedDocument>("organize_pages", { pages }),
  // `profile` is a manual override; `null` lets the detector decide.
  extractDocument: (pages: PageResult[], profile: DocumentKind | null = null) =>
    invoke<Extraction>("extract_document", { pages, profile }),
  extractDocumentBatch: (pages: PageResult[], profile: DocumentKind | null = null) =>
    invoke<BatchExtraction>("extract_document_batch", { pages, profile }),
  // Text formats go through the same Rust-owned save dialog as DOCX/XLSX, so
  // the page never chooses the path it writes to. `null` = dialog dismissed.
  exportOrganized: (
    document: OrganizedDocument,
    format: "txt" | "csv" | "json" | "md" | "html",
    defaultName?: string,
    title?: string,
    filterName?: string,
  ) =>
    invoke<string | null>("export_organized", {
      document,
      format,
      defaultName,
      title,
      filterName,
    }),
  // The field table is exported from the edited extraction, not re-derived, so
  // a corrected value is what reaches the file.
  exportExtraction: (
    extraction: Extraction,
    format: "txt" | "csv" | "json" | "md" | "html",
    defaultName?: string,
    title?: string,
    filterName?: string,
  ) =>
    invoke<string | null>("export_extraction", {
      extraction,
      format,
      defaultName,
      title,
      filterName,
    }),
  exportBatch: (
    batch: BatchExtraction,
    format: "txt" | "csv" | "json" | "md" | "html",
    defaultName?: string,
    title?: string,
    filterName?: string,
  ) =>
    invoke<string | null>("export_batch", {
      batch,
      format,
      defaultName,
      title,
      filterName,
    }),
  // Rust renders the preview with the same function the exporter uses, so what
  // the pane shows is exactly what the file will contain.
  previewExport: (
    payload:
      | { kind: "organized"; document: OrganizedDocument }
      | { kind: "extraction"; extraction: Extraction }
      | { kind: "batch"; batch: BatchExtraction },
    format: "txt" | "csv" | "json" | "md" | "html",
  ) => invoke<string>("preview_export", { payload, format }),
  documentIndexStatus: () => invoke<DocumentIndexStatus>("document_index_status"),
  documentSetIndexing: (enabled: boolean) =>
    invoke<boolean>("document_set_indexing", { enabled }),
  documentTextStore: (
    path: string,
    body: string,
    pageCount?: number,
    source?: string,
  ) =>
    invoke<number>("document_text_store", { path, body, pageCount, source }),
  documentTextRemove: (path: string) => invoke<boolean>("document_text_remove", { path }),
  documentTextClear: () => invoke<number>("document_text_clear"),
  batchParse: (text: string) => invoke<ParsedList>("batch_parse", { text }),
  // The picker is opened by the Rust command, so the page never chooses the
  // path it reads. `null` means the dialog was dismissed.
  batchImport: (title?: string, filterName?: string) =>
    invoke<ParsedList | null>("batch_import", { title, filterName }),
  batchSearch: (entries: string[], threshold: number) =>
    invoke<BatchReport>("batch_search", { entries, threshold }),
  // Likewise for the save dialog: Rust asks, writes, and returns the path it
  // actually wrote, or `null` when the dialog was dismissed.
  batchExport: (report: BatchReport, title?: string, filterName?: string) =>
    invoke<string | null>("batch_export", { report, title, filterName }),
  modelsCatalog: () => invoke<ModelPackView[]>("models_catalog"),
  modelsAdvisor: () => invoke<AdvisorView>("models_advisor"),
  modelsDownload: (packId: string) =>
    invoke<ModelPackView>("models_download", { packId }),
  modelsCancelDownload: () => invoke<boolean>("models_cancel_download"),
  modelsActivate: (packId: string) =>
    invoke<ModelPackView[]>("models_activate", { packId }),
  modelsRemove: (packId: string) =>
    invoke<ModelPackView[]>("models_remove", { packId }),
  modelsVerify: (packId: string) =>
    invoke<ModelVerifyView>("models_verify", { packId }),
  modelsBenchmark: (packId: string) =>
    invoke<ModelBenchmarkView>("models_benchmark", { packId }),
  onModelsProgress: (callback: (event: DownloadProgress) => void): Promise<() => void> =>
    listen<DownloadProgress>("models-progress", (event) => callback(event.payload)),
  onOcrProgress: (callback: (event: PipelineProgress) => void) =>
    listen<PipelineProgress>("ocr-progress", (event) => callback(event.payload)),
  onIndexStatus: (callback: (status: IndexStatus) => void): Promise<() => void> =>
    listen<unknown>("zeidex://index-status", (event) => {
      const status = parseIndexStatus(event.payload);
      if (status) callback(status);
    }),
  onMode: async (callback: (mode: string) => void): Promise<() => void> =>
    listen<string>("zeidex-mode", (event) => callback(event.payload)),
  // Emitted only when a newer release exists. Nothing about the user travels
  // with it; the payload is the public release record.
  onUpdate: (callback: (info: UpdateInfo) => void): Promise<() => void> =>
    listen<UpdateInfo>("zeidex://update", (event) => callback(event.payload)),
};
