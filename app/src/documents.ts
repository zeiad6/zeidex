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
] as const;

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
