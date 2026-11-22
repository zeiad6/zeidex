import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
  SUPPORTED_DOCUMENT_EXTENSIONS,
  documentExtension,
  isSupportedDocument,
  isTextDocument,
  textDocumentSource,
} from "./documents";

test("accepts what the dialog offers, in any case", () => {
  for (const path of [
    "/home/u/scan.pdf",
    "/home/u/scan.PDF",
    "/home/u/photo.JPG",
    "/home/u/page.tiff",
    "/home/u/report.docx",
    "/home/u/sheet.XLSX",
    "/home/u/report.odt",
    "/home/u/notes.txt",
    "/home/u/README.md",
    "/home/u/rows.csv",
    "C:\\Users\\u\\scan.bmp",
    "/home/u/.hidden.pdf",
    "/home/u/a.b.c.webp",
  ]) {
    assert.equal(isSupportedDocument(path), true, path);
  }
});

test("rejects what the engine cannot open", () => {
  for (const path of [
    "/home/u/legacy.doc",
    "/home/u/scan.pdf.exe",
    "/home/u/archive.tar.gz",
    "/home/u/photo",
    "/home/u/report.",
    "/home/u/.config",
    "/home/u/folder.v2",
    "",
  ]) {
    assert.equal(isSupportedDocument(path), false, path);
  }
});

test("text formats extract instead of running OCR, and say where they came from", () => {
  for (const path of [
    "/home/u/report.docx",
    "/home/u/sheet.xlsx",
    "/home/u/report.odt",
    "/home/u/notes.txt",
    "/home/u/README.md",
    "/home/u/rows.csv",
    "C:\\Users\\u\\log.LOG",
  ]) {
    assert.equal(isTextDocument(path), true, path);
  }
  assert.equal(isTextDocument("/home/u/scan.pdf"), false);
  assert.equal(isTextDocument("/home/u/photo.png"), false);
  assert.equal(isTextDocument(""), false);

  assert.equal(textDocumentSource("/home/u/report.docx"), "docx");
  assert.equal(textDocumentSource("/home/u/sheet.xlsx"), "xlsx");
  assert.equal(textDocumentSource("/home/u/report.odt"), "odt");
  assert.equal(textDocumentSource("/home/u/notes.txt"), "text");
});

test("reads the extension after a dot that is part of the name", () => {
  assert.equal(documentExtension("/home/u/.hidden.pdf"), "pdf");
  assert.equal(documentExtension("/home/u/scan.PDF"), "pdf");
  assert.equal(documentExtension("/home/u/no-dot"), null);
  assert.equal(documentExtension("/home/u/trailing."), null);
  assert.equal(documentExtension(""), null);
});

test("a dotted directory is not mistaken for a file extension", () => {
  assert.equal(isSupportedDocument("/home/u/v1.2/README"), false);
  assert.equal(isSupportedDocument("C:\\Users\\u.v2\\scan.png"), true);
});

/**
 * The dialog's filter is built in Rust and the drop check is built here, so the
 * two lists are a contract across languages. A format the dialog offers but the
 * drop handler refuses is a file the user cannot open; the reverse is a drop
 * that fails after the dialog would have accepted it.
 */
test("the frontend extension list matches the Rust picker's", () => {
  const source = readFileSync("../src-tauri/src/commands/ocr.rs", "utf8");
  const start = source.indexOf("SUPPORTED_DOCUMENT_EXTENSIONS");
  assert.notEqual(start, -1, "the Rust picker must declare its extension list");
  const open = source.indexOf("&[", start);
  const close = source.indexOf("];", open);
  assert.ok(open !== -1 && close !== -1, "the list must be a slice literal");
  const rust = [...source.slice(open + 2, close).matchAll(/"([a-z0-9]+)"/g)].map(
    (match) => match[1],
  );
  assert.ok(rust.length > 0, "the scan must find extensions; an empty list proves nothing");
  assert.deepEqual([...SUPPORTED_DOCUMENT_EXTENSIONS].sort(), [...rust].sort());
});
