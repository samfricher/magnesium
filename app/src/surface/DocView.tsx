import { forwardRef, useCallback, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState } from "react";

import { core } from "../bridge";
import { stepZoom, type OpenDoc, type Zoom } from "../docs";
import { openPdf, pdfLib, type Pdf } from "../pdf";
import { renderMarkdown } from "../render";
import type { Source } from "../types";

export type DocHandle = {
  zoom: (by: 1 | -1 | 0) => void;
  focus: () => void;
};

type Props = {
  doc: OpenDoc;
  folder: string;
  goTo: number;
  onPlace: (change: Partial<OpenDoc>) => void;
  onNote: (note: string) => void;
  onKind: (kind: { pages: number | null; zoom: number | null; refused: boolean }) => void;
};

const CSS_PER_POINT = 96 / 72;
const PAD = 16;
const INK_MARGIN = 10;
const INK_PAGES = 60;
const INK_SCALE = 0.5;
const INK_BELOW = 235;
const SETTLE_MS = 180;

type Size = { width: number; height: number };
type Across = { left: number; right: number };
type PdfPage = Awaited<ReturnType<Pdf["getPage"]>>;

async function inkAcross(page: PdfPage): Promise<Across | null> {
  const view = page.getViewport({ scale: INK_SCALE });
  const canvas = document.createElement("canvas");
  const w = (canvas.width = Math.max(1, Math.ceil(view.width)));
  const h = (canvas.height = Math.max(1, Math.ceil(view.height)));
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (!context) return null;
  try {
    await (page.render({ canvasContext: context, viewport: view }) as unknown as RenderTask).promise;
    const data = context.getImageData(0, 0, w, h).data;
    const ink = (x: number) => {
      for (let y = 0; y < h; y++) {
        const i = (y * w + x) * 4;
        if (data[i] < INK_BELOW || data[i + 1] < INK_BELOW || data[i + 2] < INK_BELOW) return true;
      }
      return false;
    };
    let left = 0;
    while (left < w && !ink(left)) left++;
    if (left === w) return null;
    let right = w - 1;
    while (right > left && !ink(right)) right--;
    return { left: left / INK_SCALE, right: (right + 1) / INK_SCALE };
  } finally {
    canvas.width = 0;
    canvas.height = 0;
  }
}

async function wordsAcross(page: PdfPage): Promise<Across | null> {
  const content = await page.getTextContent().catch(() => null);
  const x0 = page.view[0] ?? 0;
  let left = Infinity;
  let right = -Infinity;
  for (const item of content?.items ?? []) {
    if (!("str" in item) || item.str.trim() === "") continue;
    const x = item.transform[4] - x0;
    left = Math.min(left, x);
    right = Math.max(right, x + item.width);
  }
  return Number.isFinite(left) ? { left, right } : null;
}
type RenderTask = { cancel: () => void; promise: Promise<unknown> };
type Drawn = { scale: number; task: RenderTask | null; layer: { cancel: () => void } | null; done: boolean };

const PdfDoc = forwardRef<DocHandle, Props & { pdf: Pdf }>(function PdfDoc({ pdf, doc, goTo, onPlace, onNote, onKind }, ref) {
  const box = useRef<HTMLDivElement>(null);
  const [sizes, setSizes] = useState<Size[] | null>(null);
  const [across, setAcross] = useState<Across | null>(null);
  const [room, setRoom] = useState(0);
  const pages = useRef<(HTMLDivElement | null)[]>([]);
  const drawn = useRef(new Map<number, Drawn>());
  const place = useRef({ page: Math.max(1, doc.page), within: doc.scroll ?? 0 });
  const placed = useRef(false);

  useEffect(() => {
    let alive = true;
    void (async () => {
      const found: Size[] = [];
      for (let n = 1; n <= pdf.numPages; n++) {
        const view = (await pdf.getPage(n)).getViewport({ scale: 1 });
        found.push({ width: view.width, height: view.height });
        if (!alive) return;
      }
      const widest = Math.max(...found.map((size) => size.width));
      let left = Infinity;
      let right = -Infinity;
      const read = async (n: number) => {
        const page = await pdf.getPage(n);
        const seen = n <= INK_PAGES ? await inkAcross(page).catch(() => wordsAcross(page)) : await wordsAcross(page);
        if (!seen) return;
        left = Math.min(left, seen.left);
        right = Math.max(right, seen.right);
      };
      const drawnAcross = (): Across | null =>
        Number.isFinite(left) && right - left > 40 ? { left: Math.max(0, left - INK_MARGIN), right: Math.min(widest, right + INK_MARGIN) } : null;
      const began = Math.round(performance.now());
      const mark = (n: number) => box.current?.setAttribute("data-ink-read", `${n} ${pdf.numPages} ${began} ${Math.round(performance.now())}`);
      const first = Math.min(3, pdf.numPages);
      for (let n = 1; n <= first; n++) await read(n);
      mark(first);
      if (!alive) return;
      setSizes(found);
      const shown = drawnAcross();
      setAcross(shown);
      for (let n = first + 1; n <= pdf.numPages; n++) {
        await read(n);
        if (!alive) return;
        mark(n);
      }
      const all = drawnAcross();
      if (all && (!shown || all.left < shown.left - 0.5 || all.right > shown.right + 0.5)) setAcross(all);
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
  const fitted = across ? across.right - across.left : widest;
  const lastFit = useRef(1);
  if (fitted > 0 && room > 0) lastFit.current = Math.min(4, Math.max(0.2, (room - 2 * PAD) / (fitted * CSS_PER_POINT)));
  const fit = lastFit.current;
  const zoom = doc.zoom === "fit" ? fit : doc.zoom;
  const scale = zoom * CSS_PER_POINT;
  const [drawScale, setDrawScale] = useState(scale);
  useEffect(() => {
    if (drawScale === scale) return;
    const timer = window.setTimeout(() => setDrawScale(scale), SETTLE_MS);
    return () => window.clearTimeout(timer);
  }, [scale, drawScale]);

  useEffect(() => onKind({ pages: pdf.numPages, zoom, refused: false }), [pdf, zoom, onKind]);

  useImperativeHandle(ref, () => ({
    zoom: (by) => onPlace({ zoom: by === 0 ? "fit" : (stepZoom(doc.zoom, fit, by) as Zoom) }),
    focus: () => box.current?.focus({ preventScroll: true }),
  }));

  const release = useCallback((n: number) => {
    const was = drawn.current.get(n);
    was?.task?.cancel();
    was?.layer?.cancel();
    drawn.current.delete(n);
    pages.current[n - 1]?.replaceChildren();
  }, []);

  const draw = useCallback(
    async (n: number) => {
      const holder = pages.current[n - 1];
      if (!holder) return;
      const was = drawn.current.get(n);
      if (was && was.scale === drawScale && (was.done || was.task)) return;
      was?.task?.cancel();
      was?.layer?.cancel();
      const mine: Drawn = { scale: drawScale, task: null, layer: null, done: false };
      drawn.current.set(n, mine);
      const current = () => drawn.current.get(n) === mine;
      try {
        const lib = await pdfLib();
        const page = await pdf.getPage(n);
        if (!current()) return;
        const view = page.getViewport({ scale: drawScale });
        const ratio = Math.min(window.devicePixelRatio || 1, 3);
        const canvas = document.createElement("canvas");
        canvas.width = Math.floor(view.width * ratio);
        canvas.height = Math.floor(view.height * ratio);
        const context = canvas.getContext("2d");
        if (!context) return;
        const task = page.render({ canvasContext: context, viewport: page.getViewport({ scale: drawScale * ratio }) }) as unknown as RenderTask;
        mine.task = task;
        await task.promise;
        if (!current()) return;
        const words = document.createElement("div");
        words.className = "textLayer";
        words.style.setProperty("--scale-factor", String(drawScale));
        const layer = new lib.TextLayer({ textContentSource: page.streamTextContent(), container: words, viewport: view });
        mine.layer = layer;
        await layer.render();
        if (!current()) return;
        mine.task = null;
        mine.done = true;
        holder.replaceChildren(canvas, words);
      } catch (error) {
        if (current()) drawn.current.delete(n);
        if (!/cancel/i.test(String(error))) onNote(`A page could not be drawn: ${error}`);
      }
    },
    [pdf, drawScale, onNote],
  );

  const near = useRef(new Set<number>());
  useEffect(() => {
    const root = box.current;
    if (!root || !sizes) return;
    const drawNear = new IntersectionObserver(
      (seen) => {
        for (const entry of seen) {
          const n = Number((entry.target as HTMLElement).dataset.page);
          if (entry.isIntersecting) {
            near.current.add(n);
            void draw(n);
          } else near.current.delete(n);
        }
      },
      { root, rootMargin: "100% 0px" },
    );
    const keepFar = new IntersectionObserver(
      (seen) => {
        for (const entry of seen) if (!entry.isIntersecting) release(Number((entry.target as HTMLElement).dataset.page));
      },
      { root, rootMargin: "400% 0px" },
    );
    pages.current.forEach((page) => {
      if (!page) return;
      drawNear.observe(page);
      keepFar.observe(page);
    });
    return () => {
      drawNear.disconnect();
      keepFar.disconnect();
    };
  }, [sizes, draw, release]);

  useEffect(() => {
    near.current.forEach((n) => void draw(n));
  }, [draw]);

  useEffect(() => {
    const again = () => near.current.forEach((n) => (drawn.current.get(n)?.done ? undefined : void draw(n)));
    document.addEventListener("visibilitychange", again);
    window.addEventListener("focus", again);
    return () => {
      document.removeEventListener("visibilitychange", again);
      window.removeEventListener("focus", again);
    };
  }, [draw]);

  const fitting = doc.zoom === "fit" && across !== null;
  const alignAcross = useCallback(() => {
    const root = box.current;
    if (!root || !across || fitting) return;
    const drawnWidth = (across.right - across.left) * scale;
    root.scrollLeft = Math.max(0, across.left * scale - PAD - Math.max(0, (root.clientWidth - 2 * PAD - drawnWidth) / 2));
  }, [across, scale, fitting]);

  const goToPlace = useCallback(
    (page: number, within: number) => {
      const holder = pages.current[page - 1];
      const root = box.current;
      if (!holder || !root) return;
      root.scrollTop = holder.offsetTop + within * holder.offsetHeight - (fitting ? 0 : PAD);
      place.current = { page, within };
      alignAcross();
    },
    [alignAcross, fitting],
  );

  useLayoutEffect(() => {
    if (!sizes || room === 0 || placed.current) return;
    placed.current = true;
    goToPlace(Math.min(Math.max(1, place.current.page), sizes.length), place.current.within);
  }, [sizes, room, goToPlace]);

  const lastGo = useRef(goTo);
  useEffect(() => {
    if (goTo === lastGo.current || !sizes) return;
    lastGo.current = goTo;
    goToPlace(Math.min(Math.max(1, doc.page), sizes.length), 0);
  }, [goTo, doc.page, sizes, goToPlace]);

  const lastLayout = useRef({ scale, room });
  useLayoutEffect(() => {
    const was = lastLayout.current;
    lastLayout.current = { scale, room };
    if (!placed.current || room === 0 || (was.scale === scale && was.room === room)) return;
    goToPlace(place.current.page, place.current.within);
  }, [scale, room, goToPlace]);

  const saveTimer = useRef<number | null>(null);
  useEffect(
    () => () => {
      if (saveTimer.current !== null) window.clearTimeout(saveTimer.current);
    },
    [],
  );
  const onScroll = () => {
    const root = box.current;
    if (!root || root.clientWidth === 0) return;
    const top = root.scrollTop + (fitting ? 0 : PAD);
    let n = 1;
    pages.current.forEach((page, at) => {
      if (page && page.offsetTop <= top + 1) n = at + 1;
    });
    const holder = pages.current[n - 1];
    const within = holder ? Math.min(1, Math.max(0, (top - holder.offsetTop) / Math.max(1, holder.offsetHeight))) : 0;
    const moved = n !== place.current.page;
    place.current = { page: n, within };
    if (saveTimer.current !== null) window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => onPlace({ page: place.current.page, scroll: place.current.within }), moved ? 0 : 300);
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.metaKey || event.altKey || event.ctrlKey || event.shiftKey || !sizes) return;
    const by = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (by === 0) return;
    event.preventDefault();
    const { page, within } = place.current;
    const next = by < 0 && within > 0.02 ? page : Math.min(sizes.length, Math.max(1, page + by));
    goToPlace(next, 0);
  };

  return (
    <div className={`doc-scroll doc-pdf${fitting ? " fitted" : ""}`} ref={box} tabIndex={0} onScroll={onScroll} onKeyDown={onKeyDown} data-role="doc-pages" data-drive="doc-pages" aria-label="Pages">
      {sizes?.map((size, at) => (
        <div
          key={at}
          ref={(el) => {
            pages.current[at] = el;
          }}
          className="doc-page"
          data-page={at + 1}
          style={{
            width: `${Math.floor(size.width * scale)}px`,
            height: `${Math.floor(size.height * scale)}px`,
            ...(fitting && across ? { marginLeft: `${PAD - across.left * scale}px`, marginRight: 0 } : {}),
          }}
          aria-label={`Page ${at + 1}`}
        />
      ))}
    </div>
  );
});

function FlowDoc({ shown, doc, onPlace }: { shown: Source; doc: OpenDoc; onPlace: (change: Partial<OpenDoc>) => void }) {
  const box = useRef<HTMLDivElement>(null);
  const markdown = /\.(md|markdown)$/i.test(doc.path);
  const page = useMemo(() => (markdown ? renderMarkdown(shown.text ?? "") : ""), [markdown, shown.text]);
  useLayoutEffect(() => {
    const el = box.current;
    if (el) el.scrollTop = doc.scroll * Math.max(0, el.scrollHeight - el.clientHeight);
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
  const onLink = (event: React.MouseEvent<HTMLElement>) => {
    const link = (event.target as HTMLElement).closest("a");
    if (!link) return;
    event.preventDefault();
    const href = link.getAttribute("href") ?? "";
    if (/^https?:\/\//i.test(href)) void core().then((api) => api.openUrl(href));
  };
  return (
    <div className="doc-scroll doc-flow" ref={box} tabIndex={0} onScroll={onScroll} data-role="doc-flow" data-drive="doc-flow">
      {shown.kind === "image" && shown.image ? (
        <img className="doc-image" src={shown.image} alt="" />
      ) : markdown ? (
        <div className="prose doc-prose" onClick={onLink} dangerouslySetInnerHTML={{ __html: page }} />
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
