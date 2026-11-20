import { memo, useEffect, useLayoutEffect, useMemo, useRef } from "react";
import "temml/dist/Temml-Local.css";

import { core } from "../bridge";
import { holdBack, renderInline, renderMarkdown, setSourcesApart } from "../render";
import { drawTaskRule } from "../taskRule";
import { docTitle } from "../docs";
import { whenShown } from "../shown";
import { sourceRef, type SourceRef } from "../sources";

let diagrams = 0;

const pictures = new Map<string, Promise<string>>();

const drawn = new Map<string, string>();
const failed = new Set<string>();
const inflight = new Map<string, Promise<void>>();
export const darkNow = () => {
  const set = document.documentElement.dataset.scheme;
  return set ? set === "dark" : window.matchMedia("(prefers-color-scheme: dark)").matches;
};
const keyOf = (source: string) => `${darkNow() ? "dark" : "light"}:${source}`;

function keepWordsReadable(holder: HTMLElement): () => void {
  const svg = holder.querySelector("svg");
  if (!svg) return () => {};
  const width = parseFloat(svg.getAttribute("width") ?? "") || Number(/^[\d.-]+ [\d.-]+ ([\d.]+)/.exec(svg.getAttribute("viewBox") ?? "")?.[1] ?? 0);
  if (width > 0) svg.style.minWidth = `${Math.round(width * 0.8)}px`;
  return whenShown(holder, () => holder.classList.toggle("wide", holder.scrollWidth > holder.clientWidth + 1));
}

const awaited = new WeakMap<HTMLElement, Map<HTMLElement, () => void>>();

function stopAwaiting(root: HTMLElement, all: boolean) {
  const stops = awaited.get(root);
  if (!stops) return;
  for (const [holder, stop] of stops) {
    if (!all && root.contains(holder)) continue;
    stop();
    stops.delete(holder);
  }
}

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

function reuseDiagrams(root: HTMLElement): HTMLElement[] {
  const waiting: HTMLElement[] = [];
  stopAwaiting(root, false);
  const stops = awaited.get(root) ?? new Map<HTMLElement, () => void>();
  awaited.set(root, stops);
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
      stops.set(holder, keepWordsReadable(holder));
    } else if (failed.has(keyOf(source))) {
      pre.classList.add("undrawn");
    } else {
      waiting.push(block);
    }
  }
  return waiting;
}

async function drawOne(source: string, room: number) {
  try {
    const mermaid = (await import("mermaid")).default;
    const dark = darkNow();
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
    const wide = Number(/viewBox="[\d.-]+ [\d.-]+ ([\d.]+)/.exec(svg)?.[1] ?? 0);
    if (wide > room * 1.4 && /^\s*(graph|flowchart)\s+LR\b/i.test(source)) {
      try {
        const tall = (await mermaid.render(`mg-diagram-${++diagrams}`, source.replace(/^(\s*(?:graph|flowchart)\s+)LR\b/i, "$1TB"))).svg;
        const tallWide = Number(/viewBox="[\d.-]+ [\d.-]+ ([\d.]+)/.exec(tall)?.[1] ?? 0);
        if (tallWide < wide) svg = tall;
      } catch {
      }
    }
    drawn.set(keyOf(source), roundNodes(svg));
  } catch {
    failed.add(keyOf(source));
    document.querySelectorAll(`#dmg-diagram-${diagrams}`).forEach((leftover) => leftover.remove());
  }
}

let roomSeen = 0;

function roomIn(root: HTMLElement): number {
  if (roomSeen === 0 || root.checkVisibility?.() !== false) roomSeen = root.clientWidth - 32;
  return roomSeen;
}

async function drawDiagrams(root: HTMLElement, blocks: HTMLElement[]) {
  for (const block of blocks) {
    const source = block.textContent ?? "";
    let job = inflight.get(keyOf(source));
    if (!job) {
      job = drawOne(source, roomIn(root)).finally(() => inflight.delete(keyOf(source)));
      inflight.set(keyOf(source), job);
    }
    await job;
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

const FILE = /^(?:\.{0,2}\/)?[\w .()+#,'-]+(?:\/[\w .()+#,'-]+)*\.(?:pdf|png|jpe?g|gif|webp|md|tex|txt|csv|html)$/i;

function markPaths(root: HTMLElement, titled: boolean, linksOpen: boolean, opensNow?: (path: string) => boolean) {
  root.querySelectorAll<HTMLElement>("code").forEach((code) => {
    if (code.parentElement?.tagName === "PRE") return;
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
    if (open || !titled) code.classList.add("path");
    if (titled) {
      code.classList.add("titled");
      if (!open) code.classList.add("shut");
      code.textContent = docTitle(text);
      code.title = docTitle(text);
    }
  });
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
  final: boolean;
  onNote?: (note: string) => void;
  linksOpen?: boolean;
  opensNow?: (path: string) => boolean;
  onSource?: (source: SourceRef, from?: HTMLElement) => void;
  stopped?: boolean;
  task?: string;
  caret?: boolean;
};

function withCaret(html: string): string {
  const caret = '<span class="cursor" aria-hidden="true"></span>';
  const holder = document.createElement("template");
  holder.innerHTML = html;
  let last: Element | null = holder.content.lastElementChild;
  while (last && last.lastElementChild && /^(DIV|UL|OL|BLOCKQUOTE)$/.test(last.tagName) && !last.classList.contains("formula-display")) last = last.lastElementChild;
  if (!last || /^(PRE|TABLE|HR|DIV)$/.test(last.tagName) || last.classList.contains("formula-display")) return `${html}<p>${caret}</p>`;
  last.insertAdjacentHTML("beforeend", caret);
  return holder.innerHTML;
}

export const Rich = memo(function Rich({ text, final, onNote, linksOpen = true, opensNow, onSource, stopped = false, task, caret = false }: Props) {
  const held = useMemo(() => (final && !stopped ? { text, wait: false } : holdBack(text)), [text, final, stopped]);
  const whole = useMemo(() => renderMarkdown(held.text), [held.text]);
  const html = useMemo(() => {
    const linked = onSource ? setSourcesApart(whole, linksOpen, opensNow) : whole;
    return caret ? withCaret(linked) : linked;
  }, [whole, linksOpen, onSource, opensNow, caret]);
  const ref = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const root = ref.current;
    return () => {
      if (root) stopAwaiting(root, true);
    };
  }, []);

  useLayoutEffect(() => {
    const root = ref.current;
    if (!root) return;
    const waiting = reuseDiagrams(root);
    const open = !final && (held.text.match(/^ {0,3}(?:```|~~~)/gm)?.length ?? 0) % 2 === 1;
    void drawDiagrams(root, waiting.filter((block) => !(open && block.parentElement === root.lastElementChild)));
  }, [html, final]);

  useLayoutEffect(() => {
    const root = ref.current;
    if (!root || !task || !final || root.querySelector("div.task")) return;
    return drawTaskRule(root, task);
  }, [html, task, final]);

  useEffect(() => {
    const root = ref.current;
    if (!root) return;
    void loadImages(root, final);
    markPaths(root, Boolean(onSource), linksOpen, opensNow);
  }, [html, final, onSource, linksOpen, opensNow]);

  const onClick = async (event: React.MouseEvent<HTMLDivElement>) => {
    const path = (event.target as HTMLElement).closest("code.path");
    const link = (event.target as HTMLElement).closest("a");
    if (path || link) event.preventDefault();
    if (path) {
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

export const Line = memo(function Line({ text, className }: { text: string; className?: string }) {
  const html = useMemo(() => renderInline(text), [text]);
  return <span className={className ? `line ${className}` : "line"} dangerouslySetInnerHTML={{ __html: html }} />;
});
