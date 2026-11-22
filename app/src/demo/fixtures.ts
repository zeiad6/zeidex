import type {
  AdvisorView,
  ClipEntry,
  LaunchResult,
  ModelPackView,
  ModelsStatus,
  PageResult,
  TranslationOutcome,
} from "../api";
import samplePageUrl from "./sample-page.png";

/**
 * Demo data for `?demo=` — the real React tree rendered against a mocked Tauri
 * bridge, so the screenshots in the README and the visual review runs show the
 * actual components instead of a mockup. Regenerate the page and this object
 * together with `python3 tools/make-demo-page.py`.
 */
export const SAMPLE_PAGE: PageResult = {
  "page_index": 0,
  "width": 1240,
  "height": 1600,
  "text": "AL NOOR TRADING CO.\nINVOICE 2026-014\nTotal due 3,030.00 SAR",
  "layout": {
    "width": 1240,
    "height": 1600,
    "regions": [
      {
        "kind": "title",
        "bbox": {
          "x0": 90.0,
          "y0": 112.0,
          "x1": 820.0,
          "y1": 206.0
        },
        "confidence": 0.98,
        "text": "AL NOOR TRADING CO.",
        "table": null
      },
      {
        "kind": "text",
        "bbox": {
          "x0": 90.0,
          "y0": 262.0,
          "x1": 620.0,
          "y1": 422.0
        },
        "confidence": 0.96,
        "text": "Najd Logistics\nOlaya District, Building 12\nRiyadh 12211, Saudi Arabia\nVAT 3100 4421 9900 003",
        "table": null
      },
      {
        "kind": "table",
        "bbox": {
          "x0": 90.0,
          "y0": 548.0,
          "x1": 1150.0,
          "y1": 838.0
        },
        "confidence": 0.94,
        "text": null,
        "table": {
          "bbox": {
            "x0": 90.0,
            "y0": 548.0,
            "x1": 1150.0,
            "y1": 838.0
          },
          "rows": 5,
          "cols": 3,
          "cells": [
            {
              "row": 0,
              "col": 0,
              "row_span": 1,
              "col_span": 1,
              "bbox": {
                "x0": 90.0,
                "y0": 548.0,
                "x1": 700.0,
                "y1": 606.0
              },
              "text": "Description"
            },
            {
              "row": 0,
              "col": 1,
              "row_span": 1,
              "col_span": 1,
              "bbox": {
                "x0": 700.0,
                "y0": 548.0,
                "x1": 850.0,
                "y1": 606.0
              },
              "text": "Qty"
            },
            {
              "row": 0,
              "col": 2,
              "row_span": 1,
              "col_span": 1,
              "bbox": {
                "x0": 850.0,
                "y0": 548.0,
                "x1": 1150.0,
                "y1": 606.0
              },
              "text": "Amount"
            },
            {
              "row": 1,
              "col": 0,
              "row_span": 1,
              "col_span": 1,
              "bbox": {
                "x0": 90.0,
                "y0": 606.0,
                "x1": 700.0,
                "y1": 664.0
              },
              "text": "Document scanning setup"
            },
            {
              "row": 1,
              "col": 1,
              "row_span": 1,
              "col_span": 1,
              "bbox": {
                "x0": 700.0,
                "y0": 606.0,
                "x1": 850.0,
                "y1": 664.0
              },
              "text": "1"
            },
            {
              "row": 1,
              "col": 2,
              "row_span": 1,
              "col_span": 1,
              "bbox": {
                "x0": 850.0,
                "y0": 606.0,
                "x1": 1150.0,
                "y1": 664.0
              },
              "text": "1,450.00"
            },
            {
              "row": 2,
              "col": 0,
              "row_span": 1,
              "col_span": 1,
              "bbox": {
                "x0": 90.0,
                "y0": 664.0,
                "x1": 700.0,
                "y1": 722.0
              },
              "text": "Arabic OCR licence"
            },
            {
              "row": 2,
              "col": 1,
              "row_span": 1,
              "col_span": 1,
              "bbox": {
                "x0": 700.0,
                "y0": 664.0,
                "x1": 850.0,
                "y1": 722.0
              },
              "text": "2"
            },
            {
              "row": 2,
              "col": 2,
              "row_span": 1,
              "col_span": 1,
              "bbox": {
                "x0": 850.0,
                "y0": 664.0,
                "x1": 1150.0,
                "y1": 722.0
              },
              "text": "780.00"
            },
            {
              "row": 3,
              "col": 0,
              "row_span": 1,
              "col_span": 1,
              "bbox": {
                "x0": 90.0,
                "y0": 722.0,
                "x1": 700.0,
                "y1": 780.0
              },
              "text": "On-site training (hours)"
            },
            {
              "row": 3,
              "col": 1,
              "row_span": 1,
              "col_span": 1,
              "bbox": {
                "x0": 700.0,
                "y0": 722.0,
                "x1": 850.0,
                "y1": 780.0
              },
              "text": "6"
            },
            {
              "row": 3,
              "col": 2,
              "row_span": 1,
              "col_span": 1,
              "bbox": {
                "x0": 850.0,
                "y0": 722.0,
                "x1": 1150.0,
                "y1": 780.0
              },
              "text": "540.00"
            },
            {
              "row": 4,
              "col": 0,
              "row_span": 1,
              "col_span": 1,
              "bbox": {
                "x0": 90.0,
                "y0": 780.0,
                "x1": 700.0,
                "y1": 838.0
              },
              "text": "Priority support"
            },
            {
              "row": 4,
              "col": 1,
              "row_span": 1,
              "col_span": 1,
              "bbox": {
                "x0": 700.0,
                "y0": 780.0,
                "x1": 850.0,
                "y1": 838.0
              },
              "text": "1"
            },
            {
              "row": 4,
              "col": 2,
              "row_span": 1,
              "col_span": 1,
              "bbox": {
                "x0": 850.0,
                "y0": 780.0,
                "x1": 1150.0,
                "y1": 838.0
              },
              "text": "260.00"
            }
          ]
        }
      },
      {
        "kind": "text",
        "bbox": {
          "x0": 820.0,
          "y0": 848.0,
          "x1": 1150.0,
          "y1": 924.0
        },
        "confidence": 0.97,
        "text": "Total due 3,030.00 SAR",
        "table": null
      },
      {
        "kind": "footer",
        "bbox": {
          "x0": 90.0,
          "y0": 1442.0,
          "x1": 1150.0,
          "y1": 1496.0
        },
        "confidence": 0.9,
        "text": "Thank you for your business · billing@alnoor.example",
        "table": null
      }
    ]
  }
};

export const SAMPLE_PREVIEW = {
  data_url: samplePageUrl,
  width: SAMPLE_PAGE.width,
  height: SAMPLE_PAGE.height,
};

export const MODELS: ModelsStatus = {
  dir: "~/.local/share/zeidex/models",
  complete: true,
  missing: [],
  pack_id: "ppocrv4-mobile",
  pack_name: "PP-OCRv4 Mobile",
  pack_name_ar: "PP-OCRv4 المحمول",
  bundled: true,
  bundled_dir: "~/.local/share/zeidex/models",
  models_root: "~/.local/share/zeidex/models",
};

const NOW = Math.floor(Date.now() / 1000);

export const RESULTS: LaunchResult[] = [
  {
    id: "app:code",
    kind: "app",
    title: "Visual Studio Code",
    subtitle: "/usr/share/applications/code.desktop",
    path: "/usr/share/applications/code.desktop",
    score: 0.98,
    action: { type: "open", path: "/usr/share/applications/code.desktop" },
    icon: null,
  },
  {
    id: "file:invoice",
    kind: "file",
    title: "invoice-2026-014.pdf",
    subtitle: "~/Documents/Invoices · 1.2 MB",
    path: "/home/you/Documents/Invoices/invoice-2026-014.pdf",
    score: 0.93,
    action: { type: "open", path: "/home/you/Documents/Invoices/invoice-2026-014.pdf" },
    icon: null,
  },
  {
    id: "folder:invoices",
    kind: "folder",
    title: "Invoices",
    subtitle: "~/Documents/Invoices · 42 items",
    path: "/home/you/Documents/Invoices",
    score: 0.95,
    action: { type: "open_folder", path: "/home/you/Documents/Invoices" },
    icon: null,
  },
  {
    id: "file:inventory",
    kind: "file",
    title: "inventory-2026.xlsx",
    subtitle: "~/Documents/Finance · 84 KB",
    path: "/home/you/Documents/Finance/inventory-2026.xlsx",
    score: 0.91,
    action: { type: "open", path: "/home/you/Documents/Finance/inventory-2026.xlsx" },
    icon: null,
  },
  {
    id: "file:report",
    kind: "file",
    title: "report-2026.docx",
    subtitle: "~/Documents/Finance · 96 KB",
    path: "/home/you/Documents/Finance/report-2026.docx",
    score: 0.9,
    action: { type: "open", path: "/home/you/Documents/Finance/report-2026.docx" },
    icon: null,
  },
  {
    id: "file:annual-report",
    kind: "file",
    title: "annual-report.pdf",
    subtitle: "~/Documents/Finance · 2.4 MB",
    path: "/home/you/Documents/Finance/annual-report.pdf",
    score: 0.88,
    action: { type: "open", path: "/home/you/Documents/Finance/annual-report.pdf" },
    icon: null,
  },
  {
    id: "file:budget-report",
    kind: "file",
    title: "budget-report-2025.xlsx",
    subtitle: "~/Documents/Finance · 148 KB",
    path: "/home/you/Documents/Finance/budget-report-2025.xlsx",
    score: 0.86,
    action: { type: "open", path: "/home/you/Documents/Finance/budget-report-2025.xlsx" },
    icon: null,
  },
  {
    id: "file:reprot",
    kind: "file",
    title: "reprot-final.docx",
    subtitle: "~/Documents/Drafts · 41 KB",
    path: "/home/you/Documents/Drafts/reprot-final.docx",
    score: 0.6,
    action: { type: "open", path: "/home/you/Documents/Drafts/reprot-final.docx" },
    icon: null,
  },
  {
    id: "folder:projects",
    kind: "folder",
    title: "Projects",
    subtitle: "~/Projects",
    path: "/home/you/Projects",
    score: 0.9,
    action: { type: "open_folder", path: "/home/you/Projects" },
    icon: null,
  },
  {
    id: "file:keyboard",
    kind: "file",
    title: "keyboard-shortcuts.md",
    subtitle: "~/Notes · 6 KB",
    path: "/home/you/Notes/keyboard-shortcuts.md",
    score: 0.84,
    action: { type: "open", path: "/home/you/Notes/keyboard-shortcuts.md" },
    icon: null,
  },
  {
    id: "setting:display",
    kind: "setting",
    title: "Display settings",
    subtitle: "System settings · monitors, scale, night light",
    path: "settings://display",
    score: 0.8,
    action: { type: "open", path: "settings://display" },
    icon: null,
  },
];

export const CLIPS: ClipEntry[] = [
  {
    id: 1,
    content: "npm run tauri build -- --bundles deb,appimage",
    kind: "text",
    copied_at_unix_s: NOW - 120,
    copy_count: 4,
    pinned: true,
  },
  {
    id: 2,
    content: "Total due 3,030.00 SAR — invoice 2026-014",
    kind: "text",
    copied_at_unix_s: NOW - 900,
    copy_count: 2,
    pinned: false,
  },
  {
    id: 3,
    content: "SELECT title, score FROM candidates WHERE kind = 'file' ORDER BY score DESC LIMIT 20;",
    kind: "text",
    copied_at_unix_s: NOW - 2400,
    copy_count: 1,
    pinned: false,
  },
  {
    id: 4,
    content: "https://github.com/zeiad6/zeidex-search",
    kind: "text",
    copied_at_unix_s: NOW - 7200,
    copy_count: 7,
    pinned: false,
  },
];

export const TRANSLATIONS: Record<string, string> = {
  ar: "مستنداتك تبقى على جهازك.",
  en: "Your documents stay on your machine.",
  fr: "Vos documents restent sur votre machine.",
  es: "Tus documentos se quedan en tu equipo.",
};

export const TRANSLATION_FAILURE: TranslationOutcome = {
  original: "",
  translated: "",
  error: { code: "unavailable", message: "Demo mode: translations are canned." },
};

/**
 * The three model tiers as the Models screen renders them. Sizes and RAM
 * figures mirror the real catalog; nothing here claims a measurement, so the
 * demo shows the same "not benchmarked" state a fresh install does.
 */
export const MODEL_PACKS: ModelPackView[] = [
  {
    id: "ppocrv4-mobile",
    tier: "compact",
    tier_label: "Lightweight · Fast",
    tier_label_ar: "خفيف · سريع",
    name: "PP-OCRv4 Mobile",
    name_ar: "PP-OCRv4 المحمول",
    summary:
      "Bundled with the app. Fast on CPU-only machines and small enough for 4 GB laptops.",
    summary_ar: "مضمّن مع التطبيق. سريع على الأجهزة بلا كرت رسوميات وصغير ليناسب حاسوباً بذاكرة 4 غ.ب.",
    ram_mb: 320,
    download_bytes: 20_400_000,
    installed_bytes: 20_400_000,
    bundled: true,
    arabic: true,
    english: true,
    rec_ar_visual: false,
    source: "PaddleOCR PP-OCRv4 mobile",
    license: "Apache-2.0",
    state: "installed",
    active: true,
    downloading: false,
    dir: "/home/you/.local/share/zeidex/models",
    files: [
      { role: "det", name: "ppocrv4_det.onnx", present: true, expected_bytes: 4_700_000, actual_bytes: 4_700_000 },
      { role: "rec_en", name: "ppocrv4_rec_en.onnx", present: true, expected_bytes: 10_500_000, actual_bytes: 10_500_000 },
      { role: "rec_ar", name: "ppocrv4_rec_ar.onnx", present: true, expected_bytes: 5_200_000, actual_bytes: 5_200_000 },
    ],
  },
  {
    id: "ppocrv4-server-det",
    tier: "balanced",
    tier_label: "Balanced · Recommended",
    tier_label_ar: "متوازن · موصى به",
    name: "PP-OCRv4 Server",
    name_ar: "PP-OCRv4 الخادم",
    summary:
      "The recommended tier: a larger detector finds text in scans the mobile model misses.",
    summary_ar: "الطبقة الموصى بها: كاشف أكبر يجد النص في المسح الضوئي الذي يفوته النموذج المحمول.",
    ram_mb: 900,
    download_bytes: 129_000_000,
    installed_bytes: 0,
    bundled: false,
    arabic: true,
    english: true,
    rec_ar_visual: false,
    source: "PaddleOCR PP-OCRv4 server detector",
    license: "Apache-2.0",
    state: "not_installed",
    active: false,
    downloading: false,
    dir: "/home/you/.local/share/zeidex/models/packs/ppocrv4-server-det",
    files: [
      { role: "det", name: "ppocrv4_server_det.onnx", present: false, expected_bytes: 113_350_183, actual_bytes: 0 },
      { role: "rec_en", name: "ppocrv4_rec_en.onnx", present: false, expected_bytes: 10_500_000, actual_bytes: 0 },
      { role: "rec_ar", name: "ppocrv4_rec_ar.onnx", present: false, expected_bytes: 5_200_000, actual_bytes: 0 },
    ],
  },
  {
    id: "ppocrv5-server",
    tier: "precision",
    tier_label: "Advanced · High accuracy",
    tier_label_ar: "متقدم · دقة عالية",
    name: "PP-OCRv5 Server",
    name_ar: "PP-OCRv5 الخادم",
    summary:
      "The most accurate tier. Best for difficult Arabic scans, noise and low-resolution pages.",
    summary_ar: "الطبقة الأدق. الأفضل للمسح العربي الصعب والصور المشوّشة منخفضة الدقة.",
    ram_mb: 1600,
    download_bytes: 255_000_000,
    installed_bytes: 0,
    bundled: false,
    arabic: true,
    english: true,
    rec_ar_visual: true,
    source: "PaddleOCR PP-OCRv5 server",
    license: "Apache-2.0",
    state: "not_installed",
    active: false,
    downloading: false,
    dir: "/home/you/.local/share/zeidex/models/packs/ppocrv5-server",
    files: [
      { role: "det", name: "ppocrv5_server_det.onnx", present: false, expected_bytes: 88_118_768, actual_bytes: 0 },
      { role: "rec_en", name: "ppocrv5_server_rec.onnx", present: false, expected_bytes: 84_577_022, actual_bytes: 0 },
      { role: "rec_ar", name: "ppocrv5_rec_ar.onnx", present: false, expected_bytes: 8_025_038, actual_bytes: 0 },
    ],
  },
];

export const ADVISOR: AdvisorView = {
  profile: {
    cpu_model: "AMD Ryzen 7 5800H with Radeon Graphics",
    physical_cores: 8,
    logical_cores: 16,
    total_ram_mb: 16_384,
    available_ram_mb: 9_216,
    free_disk_mb: 214_000,
    os: "Linux 6.12",
    arch: "x86_64",
    gpu: null,
    gpu_memory_mb: null,
    gpu_acceleration: false,
  },
  class: "advanced",
  class_label: "Advanced",
  class_label_ar: "متقدم",
  score: 5,
  max_score: 7,
  criteria: [
    { key: "cores4", label: "4+ logical cores", label_ar: "4+ نواة منطقية", met: true, points: 1, detail: "16" },
    { key: "cores8", label: "8+ logical cores", label_ar: "8+ نواة منطقية", met: true, points: 1, detail: "16" },
    { key: "cores16", label: "16+ logical cores", label_ar: "16+ نواة منطقية", met: true, points: 1, detail: "16" },
    { key: "ram8", label: "8 GB RAM or more", label_ar: "8 غ.ب ذاكرة أو أكثر", met: true, points: 1, detail: "16 GB" },
    { key: "ram16", label: "16 GB RAM or more", label_ar: "16 غ.ب ذاكرة أو أكثر", met: true, points: 1, detail: "16 GB" },
    { key: "ram32", label: "32 GB RAM or more", label_ar: "32 غ.ب ذاكرة أو أكثر", met: false, points: 0, detail: "16 GB" },
    { key: "gpu", label: "GPU acceleration", label_ar: "تسريع كرت الرسوميات", met: false, points: 0, detail: "no usable GPU detected" },
  ],
  recommendation: {
    pack_id: "ppocrv5-server",
    device_class: "advanced",
    score: 5,
    reason: "PP-OCRv5 Server matches this advanced machine: 16 logical cores, 16 GB RAM.",
    reason_ar: "PP-OCRv5 الخادم يناسب جهازاً بمستوى متقدم: 16 نواة منطقية و16 غ.ب ذاكرة.",
    downgraded: false,
    insufficient_memory: false,
    ocr_concurrency: 2,
    batch_chunk: 1_024,
    index_threads: 8,
  },
  recommended_pack: MODEL_PACKS[2],
  ram_used_percent: 44,
};
