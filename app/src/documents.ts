/**
 * Which dropped or picked files the OCR Studio can actually read.
 *
 * The dialog filter and the drop handler share this list so a file that the
 * dialog offers is never refused after a drop, and a dropped file that would
 * fail deep inside the engine is refused at the edge with a readable message.
 */
export const SUPPORTED_DOCUMENT_EXTENSIONS = [
  "png",
  "jpg",
  "jpeg",
  "tif",
  "tiff",
  "bmp",
  "webp",
  "pdf",
  "docx",
  "xlsx",
  "odt",
  "txt",
  "md",
  "markdown",
  "csv",
  "tsv",
  "log",
] as const;

/**
 * Formats that already hold text, so the Studio extracts instead of runs OCR.
 * Kept beside the list above because the dialog filter and the run button must
 * agree: a format the dialog offers but the button cannot read is a dead end.
 */
export const TEXT_DOCUMENT_EXTENSIONS = [
  "docx",
  "xlsx",
  "odt",
  "txt",
  "md",
  "markdown",
  "csv",
  "tsv",
  "log",
] as const;

export function isTextDocument(path: string): boolean {
  const extension = documentExtension(path);
  return (
    extension !== null &&
    (TEXT_DOCUMENT_EXTENSIONS as readonly string[]).includes(extension)
  );
}

/** The `source` label the content index stores for a text document. */
export function textDocumentSource(path: string): "docx" | "xlsx" | "odt" | "text" {
  const extension = documentExtension(path);
  if (extension === "docx") return "docx";
  if (extension === "xlsx") return "xlsx";
  if (extension === "odt") return "odt";
  return "text";
}

/**
 * The extension of the last path segment, lowercased, or `null`.
 *
 * Splits on both separators so a Windows path is read on any host, and treats
 * a leading dot as part of the name rather than as the start of an extension,
 * which is what `.hidden` and `.config` mean on a filesystem.
 */
export function documentExtension(path: string): string | null {
  const name = path.split(/[\\/]/).pop() ?? "";
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || dot === name.length - 1) return null;
  return name.slice(dot + 1).toLowerCase();
}

export function isSupportedDocument(path: string): boolean {
  const extension = documentExtension(path);
  return (
    extension !== null &&
    (SUPPORTED_DOCUMENT_EXTENSIONS as readonly string[]).includes(extension)
  );
}
