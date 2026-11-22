import assert from "node:assert/strict";
import test from "node:test";
import { SUPPORTED_DOCUMENT_EXTENSIONS } from "./documents";
import { fileFormatOf, isKnownFormat } from "./file-format";

test("maps the formats the Studio opens to a named badge", () => {
  const expected: Record<string, string> = {
    pdf: "pdf",
    docx: "word",
    odt: "word",
    xlsx: "sheet",
    csv: "sheet",
    txt: "text",
    md: "text",
    png: "image",
    jpg: "image",
    tiff: "image",
  };
  for (const [extension, id] of Object.entries(expected)) {
    assert.equal(fileFormatOf(`/docs/report.${extension}`).id, id, extension);
  }
});

test("every format the picker offers has a real badge, not the fallback", () => {
  for (const extension of SUPPORTED_DOCUMENT_EXTENSIONS) {
    const format = fileFormatOf(`/docs/file.${extension}`);
    assert.notEqual(format.id, "other", `${extension} fell through to the generic file`);
    assert.notEqual(format.short, "", `${extension} has no badge label`);
  }
});

test("reads the extension case-insensitively and after a dotted name", () => {
  assert.equal(fileFormatOf("/docs/INVOICE.PDF").id, "pdf");
  assert.equal(fileFormatOf("/docs/report.final.XLSX").id, "sheet");
  assert.equal(fileFormatOf("C:\\Users\\u\\scan.JpEg").id, "image");
});

test("a dotfile is a file without an extension, not a PDF", () => {
  assert.equal(fileFormatOf("/home/u/.pdf").id, "other");
  assert.equal(fileFormatOf("/home/u/.hidden.docx").id, "word");
});

test("an unknown or missing extension keeps the neutral file badge", () => {
  for (const path of [
    "/docs/README",
    "/docs/archive.tar.gz.exe",
    "/docs/report.",
    "/docs/v1.2/README",
    "",
  ]) {
    assert.equal(fileFormatOf(path).id, "other", path);
    assert.equal(isKnownFormat(path), false, path);
  }
});

test("Arabic file names keep their extension", () => {
  assert.equal(fileFormatOf("/docs/فاتورة-2024.pdf").id, "pdf");
  assert.equal(fileFormatOf("/docs/تقرير-سنوي.docx").id, "word");
});

test("each format carries the mark users already know", () => {
  const marks: Record<string, string> = {
    pdf: "PDF",
    word: "W",
    sheet: "X",
    slides: "P",
  };
  for (const [id, short] of Object.entries(marks)) {
    const sample: Record<string, string> = {
      pdf: "a.pdf",
      word: "a.docx",
      sheet: "a.xlsx",
      slides: "a.pptx",
    };
    assert.equal(fileFormatOf(sample[id]).short, short, id);
  }
});

test("every format is visually distinct: unique colour and label", () => {
  // One sample per format id, so the loop covers the whole palette.
  const samples = [
    "a.pdf",
    "a.docx",
    "a.xlsx",
    "a.pptx",
    "a.png",
    "a.txt",
    "a.ts",
    "a.zip",
    "a.mp3",
    "a.mp4",
    "a.unknownextension",
  ];
  const colors = new Map<string, string>();
  const labels = new Map<string, string>();
  for (const sample of samples) {
    const format = fileFormatOf(sample);
    const previous = colors.get(format.color);
    assert.equal(previous, undefined, `${format.id} reuses the colour of ${previous}`);
    colors.set(format.color, format.id);
    assert.notEqual(format.short, "", `${format.id} has no label`);
    assert.notEqual(format.name, "", `${format.id} has no name`);
    assert.notEqual(format.nameAr, "", `${format.id} has no Arabic name`);
    labels.set(format.id, format.short);
  }
  assert.equal(colors.size, samples.length, "every sample resolved to its own format");
  assert.equal(new Set(labels.keys()).size, samples.length);
});
