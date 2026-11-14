// Two larger layers: a source the tutor cited, open over the lesson at its
// page, and text being pasted in as course material. Both go with Esc.

import { useEffect, useRef, useState } from "react";

import { core } from "../bridge";
import { drawPage, openPdf, type Pdf } from "../pdf";
import { renderMarkdown } from "../render";
import { materialTitle } from "../words";
import type { SourceRef } from "../sources";
import type { Source } from "../types";

function PdfView({ path, folder, start, onNote }: { path: string; folder: string; start: number | null; onNote: (note: string) => void }) {
  const [pdf, setPdf] = useState<Pdf | null>(null);
  const [page, setPage] = useState(start ?? 1);
  const [failure, setFailure] = useState<string | null>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const holder = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);

  useEffect(() => {
    let alive = true;
    let opened: Pdf | null = null;
    void (async () => {
      try {
        opened = await openPdf(await (await core()).sourcePdf(path, folder));
        if (!alive) return void opened.destroy();
        setPdf(opened);
        // A page past the end shows the last page, and the foot says which.
        setPage(Math.min(Math.max(start ?? 1, 1), opened.numPages));
      } catch (error) {
        if (alive) setFailure(String(error).replace(/^Error: /, ""));
      }
    })();
    return () => {
      alive = false;
      void opened?.destroy();
    };
  }, [path, folder, start]);

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
    drawPage(pdf, page, canvas.current, width - 32).catch((error) => {
      if (current && !String(error).includes("Rendering cancelled")) onNote(`The page could not be drawn: ${error}`);
    });
    holder.current?.scrollTo({ top: 0 });
    return () => {
      current = false;
    };
  }, [pdf, page, width, onNote]);

  // ← and → turn the page.
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (!pdf || event.metaKey || event.altKey) return;
      if (event.key === "ArrowLeft") setPage((n) => Math.max(1, n - 1));
      else if (event.key === "ArrowRight") setPage((n) => Math.min(pdf.numPages, n + 1));
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [pdf]);

  if (failure) return <p className="sheet-note">{failure}</p>;
  return (
    <>
      <div className="sheet-body" ref={holder}>
        <canvas ref={canvas} className="source-page" aria-label={`Page ${page}`} />
      </div>
      <p className="layer-keys" data-role="source-page">
        {pdf ? `Page ${page} of ${pdf.numPages}   ← → turn   esc close` : "Opening…"}
      </p>
    </>
  );
}

export function SourceSheet(props: { source: SourceRef; folder: string; onClose: () => void; onNote: (note: string) => void }) {
  const { source } = props;
  const [shown, setShown] = useState<Source | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let alive = true;
    void core()
      .then((api) => api.source(source.path, props.folder))
      .then((got) => alive && setShown(got))
      .catch((error) => alive && setFailure(String(error).replace(/^Error: /, "")));
    return () => {
      alive = false;
    };
  }, [source.path, props.folder]);

  useEffect(() => box.current?.focus(), []);

  // A file the window refused is not named: an answer key's name stays out of sight.
  // Otherwise it goes by a title: its name, with no folder and no extension.
  const name = failure ? "Source" : materialTitle(shown?.name ?? source.path.split("/").pop() ?? source.path);
  const markdown = /\.(md|markdown)$/i.test(shown?.name ?? source.path);
  // A link inside a note stays in the window: a web link goes to the browser, nothing else is followed.
  const onLink = (event: React.MouseEvent<HTMLElement>) => {
    const link = (event.target as HTMLElement).closest("a");
    if (!link) return;
    event.preventDefault();
    const href = link.getAttribute("href") ?? "";
    if (/^https?:\/\//i.test(href)) void core().then((api) => api.openUrl(href));
  };
  return (
    <>
      <div className="scrim" onMouseDown={props.onClose} />
      <div
        className="layer sheet"
        role="dialog"
        aria-modal="true"
        aria-label={`Source: ${name}`}
        data-role="source-panel"
        tabIndex={-1}
        ref={box}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            event.stopPropagation();
            props.onClose();
          }
        }}
      >
        {/* A note that opens with its own heading names itself. */}
        {!(markdown && /^\s*#\s/.test(shown?.text ?? "")) && <p className="sheet-name">{name}</p>}
        {failure ? (
          <p className="sheet-note">{failure}</p>
        ) : !shown ? (
          <p className="sheet-note">Opening…</p>
        ) : shown.kind === "pdf" ? (
          <PdfView path={source.path} folder={props.folder} start={source.page} onNote={props.onNote} />
        ) : shown.kind === "image" && shown.image ? (
          <div className="sheet-body">
            <img className="source-image" src={shown.image} alt={shown.name} />
          </div>
        ) : markdown ? (
          // Notes are drawn as prose, as the tutor's words are: their Markdown is never shown.
          <div className="sheet-body prose-body">
            <div className="prose source-prose" onClick={onLink} dangerouslySetInnerHTML={{ __html: renderMarkdown(shown.text ?? "") }} />
            {shown.truncated && <p className="sheet-note">Only the start of this file is shown.</p>}
          </div>
        ) : (
          <div className="sheet-body">
            <pre className="source-text">{shown.text}</pre>
            {shown.truncated && <p className="sheet-note">Only the start of this file is shown.</p>}
          </div>
        )}

      </div>
    </>
  );
}

export function PasteSheet(props: { initial: string; saving: boolean; onSave: (text: string) => void; onClose: () => void }) {
  const [text, setText] = useState(props.initial);
  const field = useRef<HTMLTextAreaElement>(null);
  useEffect(() => field.current?.focus(), []);
  return (
    <>
      <div className="scrim" onMouseDown={props.onClose} />
      <div className="layer sheet paste" role="dialog" aria-modal="true" aria-label="Paste text as course material">
        <p className="sheet-name">Text to keep with the course material</p>
        <textarea
          ref={field}
          className="paste-field"
          value={text}
          placeholder="Paste here"
          onChange={(event) => setText(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              props.onClose();
            } else if (event.key === "Enter" && event.metaKey && text.trim() !== "" && !props.saving) {
              event.preventDefault();
              props.onSave(text);
            }
          }}
        />
        <p className="layer-keys">{props.saving ? "Saving…" : "⌘↩ keep it   esc close"}</p>
      </div>
    </>
  );
}
