use std::path::Path;
use std::sync::{Arc, Mutex};

use image::DynamicImage;
pub use pdfium_render::prelude::Pdfium;
use pdfium_render::prelude::*;

use crate::error::{OcrError, OcrResult};

/// The process-wide pdfium handle, bound on first use.
///
/// `pdfium-render` keeps its library bindings in a process-global `OnceCell`:
/// `Pdfium::bind_to_library` succeeds exactly once and every later call returns
/// `PdfiumLibraryBindingsAlreadyInitialized`. Building a second `OcrEngine` -
/// which happens when the user activates a model pack, when the benchmark runs,
/// or after `AppState::reset_ocr_engine` - used to re-bind and fail, taking the
/// whole OCR engine (images included) down with it. Binding once and handing
/// out the same handle is the architecture the library requires.
static SHARED_PDFIUM: Mutex<Option<Arc<Pdfium>>> = Mutex::new(None);

/// Serialises every call into the pdfium C library.
///
/// Binding the library once is necessary but not sufficient: pdfium keeps
/// process-wide state that is not thread-safe, so two commands that both call
/// it at the same time - the Studio rendering a preview while a PDF OCR run is
/// paging through the same document, or two previews from a fast page switch -
/// can corrupt that state or crash the process. The pipeline already renders a
/// single document's pages sequentially; this lock extends the same guarantee
/// across documents and across commands.
///
/// The guard is held for one FFI call, never for a whole OCR run: recognition
/// is the expensive part and runs on rayon without the lock, so a preview only
/// waits for the page being rendered.
static PDFIUM_FFI: Mutex<()> = Mutex::new(());

fn ffi_guard() -> std::sync::MutexGuard<'static, ()> {
    PDFIUM_FFI
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A PDF document whose close is serialised with every other pdfium call.
///
/// `PdfDocument` releases its handle in `Drop`, and that release is an FFI call
/// like any other: closing a document while another thread is rendering it
/// frees the handle underneath the renderer. Owning the document through this
/// wrapper makes the lock part of the type, so no caller can forget it.
pub struct SharedDocument<'a> {
    inner: Option<PdfDocument<'a>>,
}

impl<'a> std::ops::Deref for SharedDocument<'a> {
    type Target = PdfDocument<'a>;

    fn deref(&self) -> &Self::Target {
        // `inner` is `Some` for the whole life of the value: only `Drop` takes
        // it, and no deref can run once `Drop` has started.
        match self.inner.as_ref() {
            Some(document) => document,
            None => unreachable!("a closed document was dereferenced"),
        }
    }
}

impl Drop for SharedDocument<'_> {
    fn drop(&mut self) {
        let _guard = ffi_guard();
        drop(self.inner.take());
    }
}

/// Opens a PDF under the process-wide pdfium lock.
pub fn load_document<'a>(pdfium: &'a Pdfium, path: &Path) -> OcrResult<SharedDocument<'a>> {
    let _guard = ffi_guard();
    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|err| OcrError::Pdf(err.to_string()))?;
    Ok(SharedDocument {
        inner: Some(document),
    })
}

/// Returns the shared pdfium handle, binding on first use.
///
/// `library_path` is only consulted for the first successful bind: pdfium can
/// be initialized once per process, so a later call cannot replace the library.
/// That is a property of pdfium-render, not a choice made here.
///
/// `Ok(None)` means "no pdfium available": no bundled path was configured and
/// the system loader has no `pdfium` library. PDF commands then report that
/// clearly; images and text documents keep working.
///
/// A failed bind is not cached, so installing the library and asking again can
/// succeed without restarting the process.
pub fn shared_pdfium(library_path: Option<&Path>) -> OcrResult<Option<Arc<Pdfium>>> {
    let mut guard = SHARED_PDFIUM
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(existing) = guard.as_ref() {
        return Ok(Some(Arc::clone(existing)));
    }

    // Loading the shared library and initialising it touches the same
    // process-wide state as a render, so it waits for any in-flight call. The
    // lock order here is SHARED_PDFIUM then PDFIUM_FFI; no other path takes
    // them in the opposite order, so this cannot deadlock.
    let _ffi = ffi_guard();
    let bindings = match library_path {
        Some(path) => match Pdfium::bind_to_library(path) {
            Ok(bindings) => bindings,
            // Another component in this process won the race or bound the
            // library first. `Pdfium::default()` is the crate's documented way
            // to reuse those bindings; it cannot panic on this branch because
            // the global cell is already initialized.
            Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => {
                let pdfium = Arc::new(Pdfium::default());
                *guard = Some(Arc::clone(&pdfium));
                return Ok(Some(pdfium));
            }
            Err(err) => return Err(OcrError::Pdf(err.to_string())),
        },
        None => match Pdfium::bind_to_system_library() {
            Ok(bindings) => bindings,
            Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => {
                let pdfium = Arc::new(Pdfium::default());
                *guard = Some(Arc::clone(&pdfium));
                return Ok(Some(pdfium));
            }
            // No system pdfium is a normal configuration: PDF support is
            // optional, and the error a PDF command gives is explicit.
            Err(PdfiumError::LoadLibraryError(_)) => return Ok(None),
            Err(err) => return Err(OcrError::Pdf(err.to_string())),
        },
    };

    let pdfium = Arc::new(Pdfium::new(bindings));
    *guard = Some(Arc::clone(&pdfium));
    Ok(Some(pdfium))
}

pub fn page_count(document: &PdfDocument<'_>) -> i32 {
    let _guard = ffi_guard();
    document.pages().len()
}

/// A page's own text layer plus its pixel size at the OCR resolution.
pub struct NativePage {
    pub text: String,
    pub width: u32,
    pub height: u32,
}

/// Reads the text a PDF already carries, without rendering or OCR.
///
/// A born-digital PDF - a report exported from Word, a downloaded invoice -
/// stores its text as text. Rendering such a page at 300 DPI and running
/// detection and recognition over it is slower, and strictly less accurate
/// than the characters the file already holds: OCR can only lose information
/// the PDF has. This returns whatever layer exists; the pipeline decides
/// whether it is enough to skip OCR for that page, because a scan with a
/// "Page 3" stamp has a text layer and still needs OCR for its body.
///
/// The page size is reported in pixels at `dpi` so a page read this way has
/// the same geometry as a rendered one and the preview can scale onto it.
pub fn native_page_text(
    document: &PdfDocument<'_>,
    page_index: i32,
    dpi: f32,
) -> OcrResult<NativePage> {
    let _guard = ffi_guard();
    let page = document
        .pages()
        .get(page_index)
        .map_err(|err| OcrError::Pdf(err.to_string()))?;
    let text = page
        .text()
        .map_err(|err| OcrError::Pdf(err.to_string()))?
        .all();
    let scale = dpi / 72.0;
    Ok(NativePage {
        text,
        width: (page.width().value * scale).round().max(1.0) as u32,
        height: (page.height().value * scale).round().max(1.0) as u32,
    })
}

/// Renders one page at `dpi` (pdfium's default is 72). 300 DPI is the
/// blueprint's target for OCR-quality scans.
pub fn render_document_page(
    document: &PdfDocument<'_>,
    page_index: i32,
    dpi: f32,
) -> OcrResult<DynamicImage> {
    let _guard = ffi_guard();
    let page = document
        .pages()
        .get(page_index)
        .map_err(|err| OcrError::Pdf(err.to_string()))?;
    let config = PdfRenderConfig::new()
        .scale_page_by_factor(dpi / 72.0)
        .set_format(PdfBitmapFormat::BGR);
    let bitmap = page
        .render_with_config(&config)
        .map_err(|err| OcrError::Pdf(err.to_string()))?;
    bitmap
        .as_image()
        .map_err(|err| OcrError::Pdf(err.to_string()))
}
