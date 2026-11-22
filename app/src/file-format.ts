/**
 * File-format identity for the small badges shown beside file names.
 *
 * The launcher and the Studio deal with a fixed, local set of formats. A
 * generic page icon tells the user nothing: a PDF, an Excel sheet and a scan
 * all looked the same. This module maps an extension to one stable format id,
 * a colour and a short label, and `FileGlyph` draws it. The mapping is pure so
 * the node tests can cover it without a DOM.
 */
import { documentExtension } from "./documents";

export type FileFormatId =
  | "pdf"
  | "word"
  | "sheet"
  | "slides"
  | "image"
  | "text"
  | "code"
  | "archive"
  | "audio"
  | "video"
  | "other";

export interface FileFormat {
  id: FileFormatId;
  /** Human name, English. */
  name: string;
  /** Human name, Arabic. */
  nameAr: string;
  /** Letters drawn inside the badge; kept short so they stay legible. */
  short: string;
  /** The badge's solid fill. Chosen for contrast against white page art. */
  color: string;
}

const FORMATS: Record<FileFormatId, FileFormat> = {
  pdf: { id: "pdf", name: "PDF", nameAr: "PDF", short: "PDF", color: "#e0281f" },
  word: { id: "word", name: "Word", nameAr: "وورد", short: "W", color: "#2b579a" },
  sheet: { id: "sheet", name: "Excel", nameAr: "إكسل", short: "X", color: "#217346" },
  slides: {
    id: "slides",
    name: "PowerPoint",
    nameAr: "بوربوينت",
    short: "P",
    color: "#d24726",
  },
  image: { id: "image", name: "Image", nameAr: "صورة", short: "IMG", color: "#0d9488" },
  text: { id: "text", name: "Text", nameAr: "نص", short: "TXT", color: "#64748b" },
  code: { id: "code", name: "Code", nameAr: "شيفرة", short: "</>", color: "#4f46e5" },
  archive: { id: "archive", name: "Archive", nameAr: "أرشيف", short: "ZIP", color: "#b45309" },
  audio: { id: "audio", name: "Audio", nameAr: "صوت", short: "AUD", color: "#db2777" },
  video: { id: "video", name: "Video", nameAr: "فيديو", short: "VID", color: "#7c3aed" },
  other: { id: "other", name: "File", nameAr: "ملف", short: "FILE", color: "#475569" },
};

/**
 * Extension to format. Extensions are lowercased before lookup, so a `PDF`
 * saved by a Windows tool is the same badge as a `pdf`.
 */
const BY_EXTENSION: Record<string, FileFormatId> = {
  pdf: "pdf",

  doc: "word",
  docx: "word",
  docm: "word",
  dot: "word",
  dotx: "word",
  odt: "word",
  ott: "word",
  rtf: "word",
  pages: "word",

  xls: "sheet",
  xlsx: "sheet",
  xlsm: "sheet",
  xlsb: "sheet",
  xlt: "sheet",
  xltx: "sheet",
  ods: "sheet",
  ots: "sheet",
  csv: "sheet",
  tsv: "sheet",

  ppt: "slides",
  pptx: "slides",
  pptm: "slides",
  pps: "slides",
  ppsx: "slides",
  odp: "slides",
  otp: "slides",
  key: "slides",

  png: "image",
  jpg: "image",
  jpeg: "image",
  jpe: "image",
  jfif: "image",
  gif: "image",
  bmp: "image",
  dib: "image",
  tif: "image",
  tiff: "image",
  webp: "image",
  svg: "image",
  ico: "image",
  heic: "image",
  heif: "image",
  avif: "image",

  txt: "text",
  md: "text",
  markdown: "text",
  mdx: "text",
  log: "text",
  rst: "text",
  tex: "text",

  json: "code",
  jsonc: "code",
  js: "code",
  mjs: "code",
  cjs: "code",
  jsx: "code",
  ts: "code",
  tsx: "code",
  py: "code",
  rs: "code",
  go: "code",
  java: "code",
  kt: "code",
  kts: "code",
  c: "code",
  h: "code",
  cc: "code",
  cpp: "code",
  hpp: "code",
  cs: "code",
  rb: "code",
  php: "code",
  swift: "code",
  sh: "code",
  bash: "code",
  zsh: "code",
  fish: "code",
  ps1: "code",
  bat: "code",
  cmd: "code",
  yml: "code",
  yaml: "code",
  toml: "code",
  ini: "code",
  conf: "code",
  xml: "code",
  html: "code",
  htm: "code",
  css: "code",
  scss: "code",
  sass: "code",
  less: "code",
  sql: "code",
  vue: "code",
  svelte: "code",
  lua: "code",
  r: "code",
  dart: "code",
  ex: "code",
  exs: "code",
  erl: "code",
  hs: "code",
  clj: "code",
  scala: "code",
  gradle: "code",

  zip: "archive",
  rar: "archive",
  "7z": "archive",
  tar: "archive",
  gz: "archive",
  tgz: "archive",
  bz2: "archive",
  xz: "archive",
  zst: "archive",
  lz: "archive",
  cab: "archive",
  iso: "archive",

  mp3: "audio",
  wav: "audio",
  flac: "audio",
  ogg: "audio",
  oga: "audio",
  m4a: "audio",
  aac: "audio",
  opus: "audio",
  wma: "audio",
  aiff: "audio",

  mp4: "video",
  mkv: "video",
  mov: "video",
  avi: "video",
  webm: "video",
  wmv: "video",
  flv: "video",
  m4v: "video",
  mpeg: "video",
  mpg: "video",
};

export function fileFormatOf(path: string): FileFormat {
  const extension = documentExtension(path);
  if (extension === null) return FORMATS.other;
  const id = BY_EXTENSION[extension];
  return id === undefined ? FORMATS.other : FORMATS[id];
}

/** True when the file's format is one the Studio can open. */
export function isKnownFormat(path: string): boolean {
  return fileFormatOf(path).id !== "other";
}
