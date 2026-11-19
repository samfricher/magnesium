export type Pdf = import("pdfjs-dist/legacy/build/pdf.mjs").PDFDocumentProxy;

let reader: Promise<typeof import("pdfjs-dist/legacy/build/pdf.mjs")> | null = null;

function pdfReader() {
  reader ??= (async () => {
    const lib = await import("pdfjs-dist/legacy/build/pdf.mjs");
    const worker = await import("pdfjs-dist/legacy/build/pdf.worker.min.mjs?url");
    lib.GlobalWorkerOptions.workerSrc = worker.default;
    return lib;
  })();
  return reader;
}

export const pdfLib = () => pdfReader();

export async function openPdf(bytes: ArrayBuffer | Uint8Array): Promise<Pdf> {
  const lib = await pdfReader();
  return lib.getDocument({ data: new Uint8Array(bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes)).slice() }).promise;
}

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
