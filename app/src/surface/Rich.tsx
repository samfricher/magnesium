// Text the tutor or the learner wrote, drawn with its mathematics.
//
// `Rich` draws a whole reply: Markdown, formulas, diagrams, local images.
// `Line` draws one line anywhere else on the page, with the same formulas.

import { memo, useEffect, useLayoutEffect, useMemo, useRef } from "react";
import "katex/dist/katex.min.css";

import { core } from "../bridge";
import { holdBack, markTask, renderInline, renderMarkdown, setSourcesApart } from "../render";
import { docTitle } from "../docs";
import { sourceRef, type SourceRef } from "../sources";

let diagrams = 0;

// A reply that is still streaming brings its images back with every chunk, and
// each read sends the whole file over the bridge. Until the reply is final,
// an image already read is reused. The final pass reads it afresh and lets go
// of the kept copy.
const pictures = new Map<string, Promise<string>>();

// A diagram drawn once is kept by its source. A reply that is still being
// written is put on the page again with every chunk; a diagram that has been
// drawn goes back in the same instant, so it never blinks back to its
// placeholder and what is below it never moves.
const drawn = new Map<string, string>();
const failed = new Set<string>();
const inflight = new Map<string, Promise<void>>();
/** The scheme on screen: the one set on the page, else the Mac's. */
export const darkNow = () => {
  const set = document.documentElement.dataset.scheme;
  return set ? set === "dark" : window.matchMedia("(prefers-color-scheme: dark)").matches;
};
const keyOf = (source: string) => `${darkNow() ? "dark" : "light"}:${source}`;

/**
 * A diagram is as big as it was drawn, so its words keep their size. It may be
 * narrowed to fit the column, but only to four fifths, which keeps 15 px words
 * at 12 px; past that it scrolls sideways in its own box. It is never scaled
 * to fit a height.
 */
function keepWordsReadable(holder: HTMLElement) {
  const svg = holder.querySelector("svg");
  if (!svg) return;
  const width = parseFloat(svg.getAttribute("width") ?? "") || Number(/^[\d.-]+ [\d.-]+ ([\d.]+)/.exec(svg.getAttribute("viewBox") ?? "")?.[1] ?? 0);
  if (width > 0) svg.style.minWidth = `${Math.round(width * 0.8)}px`;
  holder.classList.toggle("wide", holder.scrollWidth > holder.clientWidth + 1);
}

/** A diagram's boxes take a small radius, as the window's few boxes do. */
function roundNodes(svg: string): string {
  try {
    const doc = new DOMParser().parseFromString(svg, "image/svg+xml");
    if (doc.querySelector("parsererror")) return svg;
    doc.querySelectorAll(".node rect, .cluster rect, rect.actor, rect.note").forEach((rect) => {
      rect.setAttribute("rx", "4");
      rect.setAttribute("ry", "4");
    });
    return new XMLSerializer().serializeToString(doc.documentElement);
  } catch {
    return svg;
  }
}

/** Put each diagram already drawn in the place of its placeholder. Returns the ones still to draw. */
function reuseDiagrams(root: HTMLElement): HTMLElement[] {
  const waiting: HTMLElement[] = [];
  for (const block of Array.from(root.querySelectorAll<HTMLElement>("pre > code.language-mermaid"))) {
    const pre = block.parentElement;
    const source = block.textContent ?? "";
    const svg = drawn.get(keyOf(source));
    if (!pre) continue;
    if (svg !== undefined) {
      const holder = document.createElement("div");
      holder.className = "diagram";
      holder.innerHTML = svg;
      pre.replaceWith(holder);
      keepWordsReadable(holder);
    } else if (failed.has(keyOf(source))) {
      // A diagram that will not draw stays as the text the tutor wrote.
      pre.classList.add("undrawn");
    } else {
      waiting.push(block);
    }
  }
  return waiting;
}

async function drawOne(source: string, room: number) {
  try {
    // The diagram library is large, so it is fetched only when a diagram appears.
    const mermaid = (await import("mermaid")).default;
    const dark = darkNow();
    // The window's own colours, read from its tokens so a diagram follows the
    // scheme. The diagram library works colours out from these, so they are
    // resolved to plain values here.
    // A token can be a mix of others (color-mix), which the diagram library
    // cannot read: each is resolved to a plain hex colour first.
    const token = (name: string) => {
      const probe = document.createElement("span");
      probe.style.color = `var(${name})`;
      document.body.appendChild(probe);
      const computed = getComputedStyle(probe).color;
      probe.remove();
      const numbers = (computed.match(/[\d.]+/g) ?? []).map(Number);
      if (numbers.length < 3) return "gray";
      const scale = computed.startsWith("color(") ? 255 : 1;
      return `#${numbers
        .slice(0, 3)
        .map((n) => Math.round(Math.min(255, Math.max(0, n * scale))).toString(16).padStart(2, "0"))
        .join("")}`;
    };
    const ink = {
      box: token("--raised"),
      text: token("--text"),
      edge: token("--faint"),
      line: token("--quiet"),
      second: token("--tint"),
      third: token("--ground"),
    };
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      theme: "base",
      fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif',
      // A diagram fits the column; CSS scales it to the width it is given.
      flowchart: { useMaxWidth: false },
      sequence: { useMaxWidth: false },
      themeVariables: {
        darkMode: dark,
        background: "transparent",
        fontSize: "15px",
        primaryColor: ink.box,
        primaryTextColor: ink.text,
        primaryBorderColor: ink.edge,
        lineColor: ink.line,
        secondaryColor: ink.second,
        tertiaryColor: ink.third,
        textColor: ink.text,
        edgeLabelBackground: ink.third,
      },
    });
    let { svg } = await mermaid.render(`mg-diagram-${++diagrams}`, source);
    // A wide chart shrunk to the column is too small to read. Stood on its
    // end it is narrower, so the labels keep their size.
    const wide = Number(/viewBox="[\d.-]+ [\d.-]+ ([\d.]+)/.exec(svg)?.[1] ?? 0);
    if (wide > room * 1.4 && /^\s*(graph|flowchart)\s+LR\b/i.test(source)) {
      try {
        const tall = (await mermaid.render(`mg-diagram-${++diagrams}`, source.replace(/^(\s*(?:graph|flowchart)\s+)LR\b/i, "$1TB"))).svg;
        const tallWide = Number(/viewBox="[\d.-]+ [\d.-]+ ([\d.]+)/.exec(tall)?.[1] ?? 0);
        if (tallWide < wide) svg = tall;
      } catch {
        // The first drawing stands.
      }
    }
    drawn.set(keyOf(source), roundNodes(svg));
  } catch {
    failed.add(keyOf(source));
    document.querySelectorAll(`#dmg-diagram-${diagrams}`).forEach((leftover) => leftover.remove());
  }
}

/** Draw the diagrams in `blocks`, and put each in its place in `root` as it is done. */
async function drawDiagrams(root: HTMLElement, blocks: HTMLElement[]) {
  for (const block of blocks) {
    const source = block.textContent ?? "";
    let job = inflight.get(keyOf(source));
    if (!job) {
      job = drawOne(source, root.clientWidth - 32).finally(() => inflight.delete(keyOf(source)));
      inflight.set(keyOf(source), job);
    }
    await job;
    // The text may have been put on the page again while this was drawn.
    if (root.isConnected) reuseDiagrams(root);
  }
}

async function loadImages(root: HTMLElement, final: boolean) {
  const images = Array.from(root.querySelectorAll<HTMLImageElement>("img[data-src]"));
  if (images.length === 0) return;
  const api = await core();
  for (const image of images) {
    const src = image.getAttribute("data-src") ?? "";
    image.removeAttribute("data-src");
    const missing = (why: string) => {
      const note = document.createElement("span");
      note.className = "missing";
      note.textContent = `[${image.alt || "image"}: ${why}]`;
      image.replaceWith(note);
    };
    if (src.startsWith("data:image/")) {
      image.src = src;
    } else if (/^[a-z][a-z0-9+.-]*:/i.test(src) && !src.startsWith("file:")) {
      missing("images from the web are not loaded");
    } else {
      try {
        const path = decodeURI(src.replace(/^file:\/\//, ""));
        let picture = final ? undefined : pictures.get(path);
        if (final) {
          // The finished reply keeps its own copy, so nothing is held here.
          pictures.delete(path);
        }
        if (!picture) {
          picture = api.image(path);
          if (!final) {
            pictures.set(path, picture);
            picture.catch(() => pictures.delete(path));
          }
        }
        image.src = await picture;
      } catch (error) {
        missing(String(error));
      }
    }
  }
}

// A file's name as the tutor writes it in code: words, spaces, "#" and the like, then a dot and a known kind.
const FILE = /^(?:\.{0,2}\/)?[\w .()+#,'-]+(?:\/[\w .()+#,'-]+)*\.(?:pdf|png|jpe?g|gif|webp|md|tex|txt|csv|html)$/i;

/**
 * A file the tutor names in inline code reads as its title, as a link that
 * opens it beside the lesson; its path is kept on the link, never shown. A
 * kind of file the window does not show itself keeps its name and opens on
 * the Mac.
 */
function markPaths(root: HTMLElement, titled: boolean, linksOpen: boolean, opensNow?: (path: string) => boolean) {
  root.querySelectorAll<HTMLElement>("code").forEach((code) => {
    if (code.parentElement?.tagName === "PRE") return;
    // Marked once already: the task it was shut for may have been answered since.
    if (code.dataset.path) {
      if (code.classList.contains("titled")) {
        const open = linksOpen || Boolean(opensNow?.(code.dataset.path));
        code.classList.toggle("path", open);
        code.classList.toggle("shut", !open);
      }
      return;
    }
    const text = (code.textContent ?? "").trim();
    if (text.length >= 200 || !FILE.test(text)) return;
    const open = linksOpen || Boolean(opensNow?.(text));
    code.dataset.path = text;
    // In the reply that sets the open task, it reads as its title but opens only once the task is answered, as a link does.
    if (open || !titled) code.classList.add("path");
    // Every file named reads as its title; one the panel cannot show still opens on the Mac.
    if (titled) {
      code.classList.add("titled");
      if (!open) code.classList.add("shut");
      code.textContent = docTitle(text);
      code.title = docTitle(text);
    }
  });
  // A link whose words are its own path reads as the file's title too, open or shut. Any other words are the
  // tutor's ("Week 6 slides, p. 3") and are left as they are.
  root.querySelectorAll<HTMLElement>("a.source, .source-shut").forEach((link) => {
    const href = link.getAttribute("href") ?? link.dataset.href ?? "";
    const path = sourceRef(href)?.path;
    const words = (link.textContent ?? "").trim();
    if (!path || words === "") return;
    if (words === href || words === path || words === href.split("#")[0] || (FILE.test(words) && words.includes("/"))) link.textContent = docTitle(path);
  });
}

type Props = {
  text: string;
  /** False while the text is still being written. */
  final: boolean;
  onNote?: (note: string) => void;
  /**
   * Whether a link to a PDF or a text file in the study folder opens. False
   * while the learner has yet to answer the task in this reply, when it reads
   * as plain text.
   */
  linksOpen?: boolean;
  /** A file that opens even while links are shut: a past paper, which holds questions, not their answers. */
  opensNow?: (path: string) => boolean;
  /**
   * Called with a link that opens a source over the lesson. Without it such a
   * link opens in the system's viewer. `from` is the link that was pressed, so
   * closing the source can give the focus back to it.
   */
  onSource?: (source: SourceRef, from?: HTMLElement) => void;
  /** The reply was stopped part way: what it ends inside of is left out, as while it is written. */
  stopped?: boolean;
  /** The task this reply posed, as the engine read it: the part of the reply that holds it is set apart. */
  task?: string;
  /** The reply is being written: a caret stands after its last word. */
  caret?: boolean;
};

/** The writing caret, put after the last word of the last block. */
function withCaret(html: string): string {
  const caret = '<span class="cursor" aria-hidden="true"></span>';
  const holder = document.createElement("template");
  holder.innerHTML = html;
  let last: Element | null = holder.content.lastElementChild;
  while (last && last.lastElementChild && /^(DIV|UL|OL|BLOCKQUOTE)$/.test(last.tagName) && !last.classList.contains("katex-display")) last = last.lastElementChild;
  if (!last || /^(PRE|TABLE|HR|DIV)$/.test(last.tagName) || last.classList.contains("katex-display")) return `${html}<p>${caret}</p>`;
  last.insertAdjacentHTML("beforeend", caret);
  return holder.innerHTML;
}

export const Rich = memo(function Rich({ text, final, onNote, linksOpen = true, opensNow, onSource, stopped = false, task, caret = false }: Props) {
  // A reply still being written, or stopped, never shows the marks of a
  // formula, a bold span or a link it ends inside of. A display formula that
  // is held back keeps a line's room, so what follows does not jump when it closes.
  const held = useMemo(() => (final && !stopped ? { text, wait: false } : holdBack(text)), [text, final, stopped]);
  const html = useMemo(() => {
    const whole = renderMarkdown(held.text);
    const drawn = task && final ? markTask(whole, task) : whole;
    const linked = onSource ? setSourcesApart(drawn, linksOpen, opensNow) : drawn;
    return caret ? withCaret(linked) : linked;
  }, [held.text, linksOpen, onSource, opensNow, task, final, caret]);
  const ref = useRef<HTMLDivElement>(null);

  // A diagram already drawn goes back before the page is painted; one not yet
  // drawn is drawn as soon as its code is complete (the one a reply is still
  // inside of is not).
  useLayoutEffect(() => {
    const root = ref.current;
    if (!root) return;
    const waiting = reuseDiagrams(root);
    const open = !final && (held.text.match(/^ {0,3}(?:```|~~~)/gm)?.length ?? 0) % 2 === 1;
    void drawDiagrams(root, waiting.filter((block) => !(open && block.parentElement === root.lastElementChild)));
  }, [html, final]);

  useEffect(() => {
    const root = ref.current;
    if (!root) return;
    void loadImages(root, final);
    markPaths(root, Boolean(onSource), linksOpen, opensNow);
  }, [html, final, onSource, linksOpen, opensNow]);

  const onClick = async (event: React.MouseEvent<HTMLDivElement>) => {
    const path = (event.target as HTMLElement).closest("code.path");
    const link = (event.target as HTMLElement).closest("a");
    // Stop the webview following an anchor before anything is awaited.
    if (path || link) event.preventDefault();
    if (path) {
      // A file the window can show opens in the window; only another kind goes to the Mac.
      const named = (path as HTMLElement).dataset.path ?? (path.textContent ?? "").trim();
      const shown = onSource ? sourceRef(named) : null;
      if (shown) return onSource?.(shown, path as HTMLElement);
      try {
        await (await core()).openFile(named);
      } catch (error) {
        onNote?.(String(error));
      }
      return;
    }
    if (!link) return;
    const href = link.getAttribute("href") ?? "";
    if (href === "" || href.startsWith("#")) return;
    const source = onSource ? sourceRef(href) : null;
    if (source) {
      if (linksOpen || opensNow?.(source.path)) onSource?.(source, link as HTMLElement);
      return;
    }
    const api = await core();
    try {
      if (/^https?:\/\//i.test(href)) await api.openUrl(href);
      else if (!/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("file:")) {
        await api.openFile(decodeURI(href.replace(/^file:\/\//, "")));
      }
    } catch (error) {
      onNote?.(String(error));
    }
  };

  return (
    <>
      <div className="prose" ref={ref} onClick={onClick} dangerouslySetInnerHTML={{ __html: html }} />
      {held.wait && !final && <div className="math-wait" aria-hidden="true" />}
    </>
  );
});

/** One line of text with its mathematics. Used for goals, steps, names and the learner's own words. */
export const Line = memo(function Line({ text, className }: { text: string; className?: string }) {
  const html = useMemo(() => renderInline(text), [text]);
  return <span className={className ? `line ${className}` : "line"} dangerouslySetInnerHTML={{ __html: html }} />;
});
