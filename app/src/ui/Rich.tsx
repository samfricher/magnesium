// Text the tutor or the learner wrote, drawn with its mathematics.
//
// `Rich` draws a whole reply: Markdown, formulas, diagrams, local images.
// `Line` draws one line anywhere else on the page, with the same formulas.

import { memo, useEffect, useMemo, useRef } from "react";
import "katex/dist/katex.min.css";

import { core } from "../bridge";
import { renderInline, renderMarkdown, setSourcesApart, withoutOpenMath } from "../render";
import { sourceRef, type SourceRef } from "../sources";

let diagrams = 0;

// A reply that is still streaming brings its images back with every chunk, and
// each read sends the whole file over the bridge. Until the reply is final,
// an image already read is reused. The final pass reads it afresh and lets go
// of the kept copy.
const pictures = new Map<string, Promise<string>>();

async function drawDiagrams(root: HTMLElement) {
  const blocks = Array.from(root.querySelectorAll<HTMLElement>("pre > code.language-mermaid"));
  if (blocks.length === 0) return;
  // The diagram library is large, so it is fetched only when a diagram appears.
  const mermaid = (await import("mermaid")).default;
  const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  // The window's own colours, so a diagram reads as part of the page. The
  // diagram library works colours out from these, so they are plain hex.
  const ink = dark
    ? { box: "#1e2025", text: "#edebe6", edge: "#55585f", line: "#a3a4aa", second: "#25282e", third: "#17181c" }
    : { box: "#ffffff", text: "#1d1c19", edge: "#c4c0b5", line: "#65625b", second: "#ece9e1", third: "#fdfcf8" };
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: "strict",
    theme: "base",
    fontFamily: "-apple-system, BlinkMacSystemFont, sans-serif",
    // A diagram keeps its own size, so its labels stay readable. A wide one
    // scrolls inside its box.
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
  for (const block of blocks) {
    const pre = block.parentElement;
    if (!pre || !pre.isConnected) continue;
    const id = `mg-diagram-${++diagrams}`;
    try {
      const { svg } = await mermaid.render(id, block.textContent ?? "");
      const holder = document.createElement("div");
      holder.className = "diagram";
      holder.innerHTML = svg;
      pre.replaceWith(holder);
    } catch {
      // A diagram that will not draw stays as the text the tutor wrote.
      document.querySelectorAll(`#d${id}`).forEach((leftover) => leftover.remove());
    }
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

const FILE = /^(?:\.{0,2}\/)?[\w .()+-]+(?:\/[\w .()+-]+)*.(?:pdf|png|jpe?g|gif|webp|md|tex|txt|csv|html)$/i;

/** Mark inline code that names a file, so a click opens it. */
function markPaths(root: HTMLElement) {
  root.querySelectorAll<HTMLElement>("code").forEach((code) => {
    if (code.parentElement?.tagName === "PRE") return;
    const text = code.textContent ?? "";
    if (text.length < 200 && FILE.test(text.trim())) code.classList.add("path");
  });
}

type Props = {
  text: string;
  /** False while the text is still being written. Diagrams wait for the end. */
  final: boolean;
  onNote?: (note: string) => void;
  /**
   * Whether a link to a PDF or a text file in the study folder opens. False
   * while the learner has yet to answer the task in this reply, when it reads
   * as plain text.
   */
  linksOpen?: boolean;
  /** Called with a link that opens a source beside the lesson. Without it such a link opens in the system's viewer. */
  onSource?: (source: SourceRef) => void;
};

export const Rich = memo(function Rich({ text, final, onNote, linksOpen = true, onSource }: Props) {
  const html = useMemo(
    () => (onSource ? setSourcesApart(renderMarkdown(final ? text : withoutOpenMath(text)), linksOpen) : renderMarkdown(final ? text : withoutOpenMath(text))),
    [text, final, linksOpen, onSource],
  );
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const root = ref.current;
    if (!root) return;
    void loadImages(root, final);
    markPaths(root);
    if (final) void drawDiagrams(root);
  }, [html, final]);

  const onClick = async (event: React.MouseEvent<HTMLDivElement>) => {
    const path = (event.target as HTMLElement).closest("code.path");
    const link = (event.target as HTMLElement).closest("a");
    // Stop the webview following an anchor before anything is awaited.
    if (path || link) event.preventDefault();
    if (path) {
      try {
        await (await core()).openFile((path.textContent ?? "").trim());
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
      if (linksOpen) onSource?.(source);
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

  return <div className="prose" ref={ref} onClick={onClick} dangerouslySetInnerHTML={{ __html: html }} />;
});

/** One line of text with its mathematics. Used for goals, steps, names and the learner's own words. */
export const Line = memo(function Line({ text, className }: { text: string; className?: string }) {
  const html = useMemo(() => renderInline(text), [text]);
  return <span className={className ? `line ${className}` : "line"} dangerouslySetInnerHTML={{ __html: html }} />;
});
