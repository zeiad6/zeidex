import type {
  ClipEntry,
  LaunchResult,
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
    content: "https://github.com/cavaldos/zeidex",
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
