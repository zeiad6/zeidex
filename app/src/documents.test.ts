import assert from "node:assert/strict";
import test from "node:test";
import { documentExtension, isSupportedDocument } from "./documents";

test("accepts what the dialog offers, in any case", () => {
  for (const path of [
    "/home/u/scan.pdf",
    "/home/u/scan.PDF",
    "/home/u/photo.JPG",
    "/home/u/page.tiff",
    "C:\\Users\\u\\scan.bmp",
    "/home/u/.hidden.pdf",
    "/home/u/a.b.c.webp",
  ]) {
    assert.equal(isSupportedDocument(path), true, path);
  }
});

test("rejects what the engine cannot open", () => {
  for (const path of [
    "/home/u/notes.txt",
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
