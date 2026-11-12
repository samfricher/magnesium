// A file of the study folder, opened as a document: a PDF drawn page by page
// by the bundled reader, a Markdown or text file, an image. The core reads
// it, inside the study folder, and refuses an answer key.

import { useEffect, useRef, useState } from "react";

import { core } from "../bridge";
import type { Paper, PaperSitting, Source } from "../types";
import { drawPage, openPdf, type Pdf } from "../ui/pdf";
import { Rich } from "../ui/Rich";
import { fileName } from "../workspace";

function PdfPages({ path, page, onPage, active, onNote }: { path: string; page: number; onPage: (n: number) => void; active: boolean; onNote: (t: string) => void }) {
  const [pdf, setPdf] = useState<Pdf | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const holder = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const latest = useRef({ page, onPage, pdf });
  latest.current = { page, onPage, pdf };

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
      } catch (error) {
        if (alive) setFailure(String(error).replace(/^Error: /, ""));
      }
    })();
    return () => {
      alive = false;
      void opened?.destroy();
    };
  }, [path]);

  useEffect(() => {
    const el = holder.current;
    if (!el) return;
    const watch = new ResizeObserver(() => setWidth(Math.floor(el.clientWidth)));
    watch.observe(el);
    setWidth(Math.floor(el.clientWidth));
    return () => watch.disconnect();
  }, []);

  const shown = pdf ? Math.min(Math.max(page, 1), pdf.numPages) : page;
  useEffect(() => {
    if (!pdf || !canvas.current || width < 80) return;
    let current = true;
    drawPage(pdf, shown, canvas.current, Math.min(width, 860)).catch((error) => {
      if (current && !String(error).includes("Rendering cancelled")) onNote(`The page could not be drawn: ${error}`);
    });
    return () => {
      current = false;
    };
  }, [pdf, shown, width, onNote]);

  useEffect(() => {
    if (!active) return;
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.closest("input, textarea, [contenteditable]") || event.metaKey || event.ctrlKey) return;
      const { page, onPage, pdf } = latest.current;
      if (!pdf) return;
      if (event.key === "ArrowRight" || event.key === "PageDown" || event.key === "j") {
        event.preventDefault();
        onPage(Math.min(pdf.numPages, page + 1));
      } else if (event.key === "ArrowLeft" || event.key === "PageUp" || event.key === "k") {
        event.preventDefault();
        onPage(Math.max(1, page - 1));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active]);

  if (failure) return <p className="quiet-para">{failure}</p>;
  return (
    <div className="pdf" ref={holder}>
      <canvas ref={canvas} className="pdf-page" aria-label={`Page ${shown}`} data-role="pdf-page" />
      <p className="pdf-nav">
        <button type="button" className="text-button" disabled={!pdf || shown <= 1} onClick={() => onPage(shown - 1)}>
          Previous
        </button>
        <span data-role="pdf-page-number">{pdf ? `Page ${shown} of ${pdf.numPages}` : "Opening…"}</span>
        <button type="button" className="text-button" disabled={!pdf || shown >= pdf.numPages} onClick={() => onPage(shown + 1)}>
          Next
        </button>
      </p>
    </div>
  );
}

export type FileDocProps = {
  path: string;
  page: number;
  onPage: (page: number) => void;
  active: boolean;
  paper: Paper | null;
  sitting: PaperSitting | null;
  onTeach: (name: string) => void;
  onSit: (paper: Paper) => void;
  onOpenOutside: (path: string) => void;
  onNote: (text: string) => void;
};

export function FileDoc(props: FileDocProps) {
  const [source, setSource] = useState<Source | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const latest = useRef(props);
  latest.current = props;

  useEffect(() => {
    let alive = true;
    setSource(null);
    setFailure(null);
    void core()
      .then((api) => api.source(props.path))
      .then((got) => alive && setSource(got))
      .catch((error) => alive && setFailure(String(error).replace(/^Error: /, "")));
    return () => {
      alive = false;
    };
  }, [props.path]);

  // Return, when nothing is being written: start a lesson from this file, or sit this paper.
  useEffect(() => {
    if (!props.active) return;
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (event.key !== "Enter" || !(event.metaKey || event.ctrlKey) || target?.closest("input, textarea")) return;
      event.preventDefault();
      const { paper, sitting, onSit, onTeach, path } = latest.current;
      if (paper && !sitting) onSit(paper);
      else if (!paper) onTeach(fileName(path));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [props.active]);

  // A file the core refused is not named: an answer key's name stays out of sight.
  const name = failure ? "File" : (source?.name ?? fileName(props.path));
  const markdown = /\.(md|markdown)$/i.test(props.path);
  return (
    <article className="file" data-doc="file">
      <h1 className="doc-title">{name.replace(/\.[a-z0-9]+$/i, "")}</h1>
      <p className="doc-path">
        {failure ? "" : props.path}
        {!failure && (
          <>
            {" · "}
            <button type="button" className="text-button" onClick={() => props.onOpenOutside(props.path)}>
              Open in its own app
            </button>
          </>
        )}
      </p>
      {failure ? (
        <p className="quiet-para">{failure}</p>
      ) : !source ? (
        <p className="quiet-para">Opening…</p>
      ) : source.kind === "pdf" ? (
        <PdfPages path={props.path} page={props.page} onPage={props.onPage} active={props.active} onNote={props.onNote} />
      ) : source.kind === "image" && source.image ? (
        <img className="file-image" src={source.image} alt={source.name} />
      ) : markdown ? (
        <Rich text={source.text ?? ""} final onNote={props.onNote} />
      ) : (
        <pre className="file-text">{source.text}</pre>
      )}
      {source?.truncated && <p className="quiet-para">Only the start of this file is shown.</p>}
      {!failure && source && (
        <p className="file-foot">
          {props.paper ? (
            props.sitting ? (
              <span className="quiet">You are sitting {props.sitting.name}. Hand it in from ⌘K when you are done.</span>
            ) : (
              <button type="button" className="text-button" onClick={() => props.paper && props.onSit(props.paper)}>
                Sit this paper{props.paper.minutes ? `, ${props.paper.minutes} minutes` : ""} <kbd>⌘Return</kbd>
              </button>
            )
          ) : (
            <button type="button" className="text-button" onClick={() => props.onTeach(fileName(props.path))}>
              Start a lesson from this <kbd>⌘Return</kbd>
            </button>
          )}
        </p>
      )}
    </article>
  );
}
