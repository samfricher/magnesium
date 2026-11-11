import { memo, useEffect, useMemo, useRef } from "react";
import "katex/dist/katex.min.css";

import { core } from "./bridge";
import { renderMarkdown } from "./render";

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
  mermaid.initialize({ startOnLoad: false, securityLevel: "strict", theme: dark ? "dark" : "neutral" });
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
};

export const Markdown = memo(function Markdown({ text, final, onNote }: Props) {
  const html = useMemo(() => renderMarkdown(text), [text]);
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

  return <div className="md" ref={ref} onClick={onClick} dangerouslySetInnerHTML={{ __html: html }} />;
});
