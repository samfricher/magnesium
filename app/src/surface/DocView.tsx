// One document, read beside the lesson: a PDF as a column of pages whose
// words can be selected, a note drawn as prose, text, or a picture. It opens
// where it was left (its page and zoom, or how far down a note was read) and
// says where it is as it moves, so the session keeps it. Nothing here reads a
// file: the core does, inside the study folder, and refuses an answer key.

import { forwardRef, useCallback, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState } from "react";

import { core } from "../bridge";
import { stepZoom, type OpenDoc, type Zoom } from "../docs";
import { openPdf, pdfLib, type Pdf } from "../pdf";
import { renderMarkdown } from "../render";
import type { Source } from "../types";

/** What the panel can ask of the document in front. */
export type DocHandle = {
  zoom: (by: 1 | -1 | 0) => void;
  focus: () => void;
};

type Props = {
  doc: OpenDoc;
  folder: string;
  /** Bumped when a link asks for the document's page again, so it goes there even if it is the page in view. */
  goTo: number;
  onPlace: (change: Partial<OpenDoc>) => void;
  onNote: (note: string) => void;
  /** What the document is, once read: whether it has pages and how many, for the panel's head. */
  onKind: (kind: { pages: number | null; zoom: number | null; refused: boolean }) => void;
};

// 100% is a page at its printed size: a PDF point is 1/72 inch, a CSS pixel 1/96.
const CSS_PER_POINT = 96 / 72;
const PAD = 16;

type Size = { width: number; height: number };

const PdfDoc = forwardRef<DocHandle, Props & { pdf: Pdf }>(function PdfDoc({ pdf, doc, goTo, onPlace, onNote, onKind }, ref) {
  const box = useRef<HTMLDivElement>(null);
  const [sizes, setSizes] = useState<Size[] | null>(null);
  const [room, setRoom] = useState(0);
  const pages = useRef<(HTMLDivElement | null)[]>([]);
  const drawn = useRef(new Map<number, number>());
  const placed = useRef(false);
  const pageNow = useRef(doc.page);

  // Each page's own size, in points, so the column has its height before any page is drawn.
  useEffect(() => {
    let alive = true;
    void (async () => {
      const found: Size[] = [];
      for (let n = 1; n <= pdf.numPages; n++) {
        const page = await pdf.getPage(n);
        const view = page.getViewport({ scale: 1 });
        found.push({ width: view.width, height: view.height });
        if (!alive) return;
      }
      setSizes(found);
    })();
    return () => {
      alive = false;
    };
  }, [pdf]);

  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    const watch = new ResizeObserver(() => setRoom(el.clientWidth));
    watch.observe(el);
    setRoom(el.clientWidth);
    return () => watch.disconnect();
  }, []);

  const widest = sizes ? Math.max(...sizes.map((size) => size.width)) : 0;
  const fit = widest > 0 && room > 0 ? Math.max(0.2, (room - 2 * PAD) / (widest * CSS_PER_POINT)) : 1;
  const zoom = doc.zoom === "fit" ? fit : doc.zoom;
  const scale = zoom * CSS_PER_POINT;

  useEffect(() => onKind({ pages: pdf.numPages, zoom, refused: false }), [pdf, zoom, onKind]);

  useImperativeHandle(ref, () => ({
    zoom: (by) => onPlace({ zoom: by === 0 ? "fit" : (stepZoom(doc.zoom, fit, by) as Zoom) }),
    focus: () => box.current?.focus({ preventScroll: true }),
  }));

  /** Draw a page, with its words as a text layer over it, at the scale now. */
  const draw = useCallback(
    async (n: number, holder: HTMLDivElement) => {
      if (drawn.current.get(n) === scale) return;
      drawn.current.set(n, scale);
      try {
        const lib = await pdfLib();
        const page = await pdf.getPage(n);
        if (drawn.current.get(n) !== scale) return;
        const view = page.getViewport({ scale });
        const ratio = Math.min(window.devicePixelRatio || 1, 3);
        const canvas = document.createElement("canvas");
        canvas.width = Math.floor(view.width * ratio);
        canvas.height = Math.floor(view.height * ratio);
        canvas.style.width = `${view.width}px`;
        canvas.style.height = `${view.height}px`;
        const context = canvas.getContext("2d");
        if (!context) return;
        await page.render({ canvasContext: context, viewport: page.getViewport({ scale: scale * ratio }) }).promise;
        if (drawn.current.get(n) !== scale) return;
        const words = document.createElement("div");
        words.className = "textLayer";
        words.style.setProperty("--scale-factor", String(scale));
        const layer = new lib.TextLayer({ textContentSource: page.streamTextContent(), container: words, viewport: view });
        await layer.render();
        if (drawn.current.get(n) !== scale) return;
        holder.replaceChildren(canvas, words);
      } catch (error) {
        drawn.current.delete(n);
        if (!String(error).includes("cancelled")) onNote(`A page could not be drawn: ${error}`);
      }
    },
    [pdf, scale, onNote],
  );

  // Pages are drawn as they come near the view, and drawn again at a new zoom.
  useEffect(() => {
    const root = box.current;
    if (!root || !sizes) return;
    const watch = new IntersectionObserver(
      (seen) => {
        for (const entry of seen) {
          if (!entry.isIntersecting) continue;
          const n = Number((entry.target as HTMLElement).dataset.page);
          void draw(n, entry.target as HTMLDivElement);
        }
      },
      { root, rootMargin: "100% 0px" },
    );
    pages.current.forEach((page) => page && watch.observe(page));
    return () => watch.disconnect();
  }, [sizes, draw]);

  const scrollToPage = useCallback((n: number) => {
    const page = pages.current[n - 1];
    const root = box.current;
    if (!page || !root) return;
    root.scrollTop = page.offsetTop - PAD;
  }, []);

  // Opened where it was left, once the column has its height.
  useLayoutEffect(() => {
    if (!sizes || room === 0 || placed.current) return;
    placed.current = true;
    scrollToPage(Math.min(Math.max(1, doc.page), sizes.length));
  }, [sizes, room, doc.page, scrollToPage]);

  // A link that names a page goes to it.
  const lastGo = useRef(goTo);
  useEffect(() => {
    if (goTo === lastGo.current || !sizes) return;
    lastGo.current = goTo;
    scrollToPage(Math.min(Math.max(1, doc.page), sizes.length));
  }, [goTo, doc.page, sizes, scrollToPage]);

  // At a new zoom, the page in view stays in view.
  const lastScale = useRef(scale);
  useLayoutEffect(() => {
    if (lastScale.current === scale || !placed.current) return;
    lastScale.current = scale;
    scrollToPage(pageNow.current);
  }, [scale, scrollToPage]);

  const onScroll = () => {
    const root = box.current;
    if (!root) return;
    const middle = root.scrollTop + root.clientHeight / 3;
    let n = 1;
    pages.current.forEach((page, at) => {
      if (page && page.offsetTop <= middle) n = at + 1;
    });
    if (n !== pageNow.current) {
      pageNow.current = n;
      onPlace({ page: n });
    }
  };

  // ← and → (and Page Up and Down) turn the page while the document has the focus; never while the line does.
  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.metaKey || event.altKey || event.ctrlKey || !sizes) return;
    const by = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (by === 0) return;
    event.preventDefault();
    const next = Math.min(sizes.length, Math.max(1, pageNow.current + by));
    scrollToPage(next);
  };

  return (
    <div className="doc-scroll doc-pdf" ref={box} tabIndex={0} onScroll={onScroll} onKeyDown={onKeyDown} data-role="doc-pages" aria-label="Pages">
      {sizes?.map((size, at) => (
        <div
          key={at}
          ref={(el) => {
            pages.current[at] = el;
          }}
          className="doc-page"
          data-page={at + 1}
          style={{ width: `${Math.floor(size.width * scale)}px`, height: `${Math.floor(size.height * scale)}px` }}
          aria-label={`Page ${at + 1}`}
        />
      ))}
    </div>
  );
});

/** A note, drawn as prose; text as it is; a picture. Where it was read to is kept. */
function FlowDoc({ shown, doc, onPlace }: { shown: Source; doc: OpenDoc; onPlace: (change: Partial<OpenDoc>) => void }) {
  const box = useRef<HTMLDivElement>(null);
  const markdown = /\.(md|markdown)$/i.test(doc.path);
  useLayoutEffect(() => {
    const el = box.current;
    if (el) el.scrollTop = doc.scroll * Math.max(0, el.scrollHeight - el.clientHeight);
    // Only when it opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const timer = useRef<number | null>(null);
  useEffect(() => () => {
    if (timer.current !== null) window.clearTimeout(timer.current);
  }, []);
  const onScroll = () => {
    const el = box.current;
    if (!el) return;
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      const range = el.scrollHeight - el.clientHeight;
      onPlace({ scroll: range > 0 ? Math.min(1, Math.max(0, el.scrollTop / range)) : 0 });
    }, 200);
  };
  // A link inside a note stays in the window: a web link goes to the browser, nothing else is followed.
  const onLink = (event: React.MouseEvent<HTMLElement>) => {
    const link = (event.target as HTMLElement).closest("a");
    if (!link) return;
    event.preventDefault();
    const href = link.getAttribute("href") ?? "";
    if (/^https?:\/\//i.test(href)) void core().then((api) => api.openUrl(href));
  };
  return (
    <div className="doc-scroll doc-flow" ref={box} tabIndex={0} onScroll={onScroll} data-role="doc-flow">
      {shown.kind === "image" && shown.image ? (
        <img className="doc-image" src={shown.image} alt="" />
      ) : markdown ? (
        // Notes are drawn as prose, as the tutor's words are: their Markdown is never shown.
        <div className="prose doc-prose" onClick={onLink} dangerouslySetInnerHTML={{ __html: renderMarkdown(shown.text ?? "") }} />
      ) : (
        <pre className="doc-text">{shown.text}</pre>
      )}
      {shown.truncated && <p className="doc-note">Only the start of this file is shown.</p>}
    </div>
  );
}

export const DocView = forwardRef<DocHandle, Props>(function DocView(props, ref) {
  const { doc, folder, onKind } = props;
  const [shown, setShown] = useState<Source | null>(null);
  const [pdf, setPdf] = useState<Pdf | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const flow = useRef<HTMLDivElement>(null);
  const pages = useRef<DocHandle>(null);

  useEffect(() => {
    let alive = true;
    let opened: Pdf | null = null;
    setShown(null);
    setPdf(null);
    setFailure(null);
    void (async () => {
      try {
        const api = await core();
        const found = await api.source(doc.path, folder);
        if (!alive) return;
        if (found.kind === "pdf") {
          opened = await openPdf(await api.sourcePdf(doc.path, folder));
          if (!alive) return void opened.destroy();
          setPdf(opened);
        } else onKind({ pages: null, zoom: null, refused: false });
        setShown(found);
      } catch (error) {
        if (!alive) return;
        const said = String(error).replace(/^Error: /, "");
        setFailure(said);
        onKind({ pages: null, zoom: null, refused: /answer key/i.test(said) });
      }
    })();
    return () => {
      alive = false;
      void opened?.destroy();
    };
  }, [doc.path, folder, onKind]);

  useImperativeHandle(
    ref,
    () => ({
      zoom: (by) => pages.current?.zoom(by),
      focus: () => (pages.current ? pages.current.focus() : flow.current?.querySelector<HTMLElement>(".doc-scroll")?.focus({ preventScroll: true })),
    }),
    [],
  );

  if (failure) return <p className="doc-note">{failure}</p>;
  if (!shown) return <p className="doc-note">Opening…</p>;
  if (pdf) return <PdfDoc ref={pages} pdf={pdf} {...props} />;
  return (
    <div className="doc-holder" ref={flow}>
      <FlowDoc shown={shown} doc={doc} onPlace={props.onPlace} />
    </div>
  );
});
