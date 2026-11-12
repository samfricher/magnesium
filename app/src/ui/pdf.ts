// PDFs are drawn by the reader that ships inside the app, so a page opens
// offline and starts no server. The core reads the file, inside the study
// folder, and never an answer key.

type Pdf = import("pdfjs-dist/legacy/build/pdf.mjs").PDFDocumentProxy;
export type { Pdf };

let reader: Promise<typeof import("pdfjs-dist/legacy/build/pdf.mjs")> | null = null;

/** The PDF reader and its worker, fetched from the app's own files the first time a PDF opens. */
function pdfReader() {
  reader ??= (async () => {
    const lib = await import("pdfjs-dist/legacy/build/pdf.mjs");
    const worker = await import("pdfjs-dist/legacy/build/pdf.worker.min.mjs?url");
    lib.GlobalWorkerOptions.workerSrc = worker.default;
    return lib;
  })();
  return reader;
}

/** Open a PDF from its bytes. */
export async function openPdf(bytes: ArrayBuffer | Uint8Array): Promise<Pdf> {
  const lib = await pdfReader();
  // The reader takes the bytes over, so it gets a copy.
  return lib.getDocument({ data: new Uint8Array(bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes)).slice() }).promise;
}

/** Draw one page into a canvas, as wide as `width` CSS pixels. Resolves to the page's size in CSS pixels. */
export async function drawPage(pdf: Pdf, number: number, canvas: HTMLCanvasElement, width: number): Promise<{ width: number; height: number }> {
  const page = await pdf.getPage(number);
  const ratio = Math.min(window.devicePixelRatio || 1, 3);
  const natural = page.getViewport({ scale: 1 });
  const scale = Math.max(0.2, width / natural.width);
  const view = page.getViewport({ scale: scale * ratio });
  canvas.width = Math.floor(view.width);
  canvas.height = Math.floor(view.height);
  canvas.style.width = `${view.width / ratio}px`;
  canvas.style.height = `${view.height / ratio}px`;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("The page could not be drawn.");
  await page.render({ canvasContext: context, viewport: view }).promise;
  return { width: view.width / ratio, height: view.height / ratio };
}
