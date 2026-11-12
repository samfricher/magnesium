// A passage the tutor cited, open beside the lesson. A PDF opens at the cited
// page, drawn here by a PDF reader that ships inside the app, so it works
// offline and starts no server. A text file shows as text. The core reads the
// file, inside the study folder, and never an answer key.

import { useEffect, useRef, useState } from "react";

import { core } from "../bridge";
import type { SourceRef } from "../sources";
import type { Source } from "../types";
import { Icon } from "./icons";

type Pdf = import("pdfjs-dist/legacy/build/pdf.mjs").PDFDocumentProxy;

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

function PdfView({ path, start, onNote }: { path: string; start: number | null; onNote: (note: string) => void }) {
  const [pdf, setPdf] = useState<Pdf | null>(null);
  const [page, setPage] = useState(start ?? 1);
  const [failure, setFailure] = useState<string | null>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const holder = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);

  useEffect(() => {
    let alive = true;
    let opened: Pdf | null = null;
    setPdf(null);
    setFailure(null);
    void (async () => {
      try {
        opened = await openPdf(await (await core()).sourcePdf(path));
        if (!alive) return void opened.destroy();
        setPdf(opened);
        // A page past the end shows the last page, and the header says which.
        setPage(Math.min(Math.max(start ?? 1, 1), opened.numPages));
      } catch (error) {
        if (alive) setFailure(String(error).replace(/^Error: /, ""));
      }
    })();
    return () => {
      alive = false;
      void opened?.destroy();
    };
  }, [path, start]);

  useEffect(() => {
    const el = holder.current;
    if (!el) return;
    const watch = new ResizeObserver(() => setWidth(Math.floor(el.clientWidth)));
    watch.observe(el);
    setWidth(Math.floor(el.clientWidth));
    return () => watch.disconnect();
  }, []);

  useEffect(() => {
    if (!pdf || !canvas.current || width < 80) return;
    let current = true;
    drawPage(pdf, page, canvas.current, width - 26).catch((error) => {
      if (current && !String(error).includes("Rendering cancelled")) onNote(`The page could not be drawn: ${error}`);
    });
    holder.current?.scrollTo({ top: 0 });
    return () => {
      current = false;
    };
  }, [pdf, page, width, onNote]);

  if (failure) return <p className="source-note selectable">{failure}</p>;
  return (
    <>
      <div className="source-nav">
        <button className="icon-btn" aria-label="Previous page" disabled={!pdf || page <= 1} onClick={() => setPage((n) => Math.max(1, n - 1))}>
          <Icon name="back" size={15} />
        </button>
        <span data-role="source-page">{pdf ? `Page ${page} of ${pdf.numPages}` : "Opening…"}</span>
        <button className="icon-btn" aria-label="Next page" disabled={!pdf || page >= pdf.numPages} onClick={() => setPage((n) => (pdf ? Math.min(pdf.numPages, n + 1) : n))}>
          <Icon name="right" size={15} />
        </button>
      </div>
      <div className="source-body" ref={holder}>
        <canvas ref={canvas} className="source-page" aria-label={`Page ${page}`} />
      </div>
    </>
  );
}

export function SourcePanel(props: {
  source: SourceRef;
  /** The window is too narrow for it beside the lesson: it is a sheet over the lesson, and takes the keyboard. */
  sheet?: boolean;
  onClose: () => void;
  onNote: (note: string) => void;
}) {
  const { source } = props;
  const close = useRef<HTMLButtonElement>(null);
  const [shown, setShown] = useState<Source | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    setShown(null);
    setFailure(null);
    void core()
      .then((api) => api.source(source.path))
      .then((got) => alive && setShown(got))
      .catch((error) => {
        if (!alive) return;
        // The reason is the core's own words, such as "That looks like an answer key".
        if (alive) setFailure(String(error).replace(/^Error: /, ""));
      });
    return () => {
      alive = false;
    };
  }, [source.path]);

  useEffect(() => {
    if (props.sheet) close.current?.focus();
  }, [props.sheet, source.path]);

  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !event.defaultPrevented) {
        event.preventDefault();
        props.onClose();
      }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [props]);

  // A file the window refused is not named: an answer key's name stays out of sight.
  const name = failure ? "Source" : (shown?.name ?? source.path.split("/").pop() ?? source.path);
  return (
    <aside
      className={props.sheet ? "source-panel as-sheet" : "source-panel"}
      data-role="source-panel"
      aria-label={`Source: ${name}`}
      {...(props.sheet ? { role: "dialog", "aria-modal": true } : {})}
    >
      <header>
        <Icon name="paper" size={15} />
        <span className="source-name selectable" title={source.path}>
          {name}
        </span>
        <button className="btn ghost small" ref={close} aria-label="Close the source" title="Close (Esc)" onClick={props.onClose} data-role="source-close">
          <Icon name="close" size={14} />
          Close
        </button>
      </header>
      {failure ? (
        <p className="source-note selectable">{failure}</p>
      ) : !shown ? (
        <p className="source-note">Opening…</p>
      ) : shown.kind === "pdf" ? (
        <PdfView path={source.path} start={source.page} onNote={props.onNote} />
      ) : shown.kind === "image" && shown.image ? (
        <div className="source-body">
          <img className="source-image" src={shown.image} alt={shown.name} />
        </div>
      ) : (
        <div className="source-body">
          <pre className="source-text selectable">{shown.text}</pre>
          {shown.truncated && <p className="source-note">Only the start of this file is shown.</p>}
        </div>
      )}
    </aside>
  );
}
