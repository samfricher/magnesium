import DOMPurify from "dompurify";
import katex from "katex";
import { marked } from "marked";

import { sourceRef } from "./sources";

type Math = { tex: string; display: boolean };

const token = (index: number) => `MGMATH${index}ENDMG`;

function closingFence(src: string, from: number, fence: string): number {
  const mark = fence[0];
  let at = from;
  while (at < src.length) {
    const lineEnd = src.indexOf("\n", at);
    const line = src.slice(at, lineEnd === -1 ? src.length : lineEnd);
    const trimmed = line.trimStart();
    if (trimmed.startsWith(fence) && trimmed.replace(new RegExp(`^\\${mark}+`), "").trim() === "") {
      return lineEnd === -1 ? src.length : lineEnd + 1;
    }
    if (lineEnd === -1) break;
    at = lineEnd + 1;
  }
  return src.length;
}

const ENVIRONMENT = /^\\begin\{(equation\*?|align\*?|aligned|gather\*?|gathered|cases|array|[pbvBV]?matrix|split|multline\*?)\}/;

const INLINE_LIMIT = 200;

export function liftMath(src: string): { text: string; math: Math[]; open: number | null; hold: number | null } {
  const math: Math[] = [];
  let out = "";
  let i = 0;
  let open: number | null = null;
  let boldAt: number | null = null;
  let linkAt: number | null = null;
  let inUrl = false;
  const unclosed = (at: number) => {
    open ??= at;
  };
  const push = (tex: string, display: boolean) => {
    math.push({ tex: tex.trim(), display });
    out += display ? `\n\n${token(math.length - 1)}\n\n` : token(math.length - 1);
  };
  while (i < src.length) {
    const atLineStart = i === 0 || src[i - 1] === "\n";
    if (atLineStart) {
      const fence = /^ {0,3}(`{3,}|~{3,})/.exec(src.slice(i, i + 12));
      if (fence) {
        const firstLineEnd = src.indexOf("\n", i);
        const end = firstLineEnd === -1 ? src.length : closingFence(src, firstLineEnd + 1, fence[1]);
        out += src.slice(i, end);
        i = end;
        continue;
      }
    }
    const c = src[i];
    if (c === "`") {
      let run = 1;
      while (src[i + run] === "`") run++;
      const ticks = "`".repeat(run);
      const close = src.indexOf(ticks, i + run);
      const end = close === -1 ? i + run : close + run;
      out += src.slice(i, end);
      i = end;
      continue;
    }
    if (c === "\\" && src[i + 1] === "$") {
      out += "\\$";
      i += 2;
      continue;
    }
    if (c === "$" && src[i + 1] === "$") {
      const close = src.indexOf("$$", i + 2);
      if (close !== -1) {
        push(src.slice(i + 2, close), true);
        i = close + 2;
        continue;
      }
      unclosed(i);
    }
    if (c === "\\" && (src[i + 1] === "[" || src[i + 1] === "(")) {
      const display = src[i + 1] === "[";
      const close = src.indexOf(display ? "\\]" : "\\)", i + 2);
      if (close !== -1) {
        push(src.slice(i + 2, close), display);
        i = close + 2;
        continue;
      }
      unclosed(i);
    }
    if (c === "\\" && src[i + 1] === "b") {
      const begin = ENVIRONMENT.exec(src.slice(i, i + 24));
      if (begin) {
        const closer = `\\end{${begin[1]}}`;
        const close = src.indexOf(closer, i);
        if (close !== -1) {
          push(src.slice(i, close + closer.length), true);
          i = close + closer.length;
          continue;
        }
        unclosed(i);
      }
    }
    if (c === "$" && src[i + 1] !== undefined && !/\s/.test(src[i + 1])) {
      let close = -1;
      let ranOut = true;
      for (let j = i + 1; j < src.length; j++) {
        if (src[j] === "\n" && src[j + 1] === "\n") {
          ranOut = false;
          break;
        }
        if (src[j] === "\\") {
          j++;
          continue;
        }
        if (src[j] === "$") {
          if (!/\s/.test(src[j - 1]) && !/\d/.test(src[j + 1] ?? "")) close = j;
          ranOut = false;
          break;
        }
      }
      if (close > i + 1) {
        push(src.slice(i + 1, close), false);
        i = close + 1;
        continue;
      }
      if (ranOut && src.length - i <= INLINE_LIMIT) unclosed(i);
    }
    if (c === "*" && src[i + 1] === "*") {
      boldAt = boldAt === null ? i : null;
      out += "**";
      i += 2;
      continue;
    }
    if (c === "[" && linkAt === null) {
      linkAt = src[i - 1] === "!" ? i - 1 : i;
      inUrl = false;
    } else if (c === "]" && linkAt !== null && !inUrl) {
      if (src[i + 1] === "(") {
        inUrl = true;
      } else if (i + 1 < src.length) {
        linkAt = null;
      }
    } else if (c === ")" && inUrl) {
      linkAt = null;
      inUrl = false;
    } else if (c === "\n" && src[i + 1] === "\n") {
      boldAt = null;
      linkAt = null;
      inUrl = false;
    }
    out += c;
    i++;
  }
  const near = (at: number | null) => (at !== null && src.length - at <= HOLD_LIMIT ? at : null);
  const holds = [near(boldAt), near(linkAt)].filter((at): at is number => at !== null);
  return { text: out, math, open, hold: holds.length > 0 ? Math.min(...holds) : null };
}

const HOLD_LIMIT = 400;

export function withoutOpenMath(src: string): string {
  const { open } = liftMath(src);
  return open === null ? src : src.slice(0, open).trimEnd();
}

export function holdBack(src: string): { text: string; wait: boolean } {
  const { open, hold } = liftMath(src);
  const cuts = [open, hold].filter((at): at is number => at !== null);
  if (cuts.length === 0) return { text: src, wait: false };
  const at = Math.min(...cuts);
  const wait = open !== null && /^(?:\$\$|\\\[|\\begin)/.test(src.slice(open, open + 8));
  return { text: src.slice(0, at).trimEnd(), wait };
}

const rendered = new Map<string, string>();

const escape = (text: string) => text.replace(/[&<>]/g, (ch) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[ch] ?? ch);

const WORDS = "max|min|eq|tot|avg|obs|exp|crit|cat|sys|rel|net|in|out|ref";
const BRACED = new RegExp(`(^|[^\\\\])_\\{\\s*(${WORDS})\\s*\\}`, "g");
const BARE = new RegExp(`(^|[^\\\\])_(${WORDS})(?![A-Za-z])`, "g");

export function uprightWords(tex: string): string {
  if (!tex.includes("_")) return tex;
  const once = (text: string) => text.replace(BRACED, "$1_{\\text{$2}}").replace(BARE, "$1_{\\text{$2}}");
  return once(once(tex));
}

function renderMath({ tex: source, display }: Math): string {
  const tex = uprightWords(source);
  const key = (display ? "D" : "I") + tex;
  let html = rendered.get(key);
  if (html === undefined) {
    try {
      html = katex.renderToString(tex, {
        displayMode: display,
        throwOnError: false,
        strict: "ignore",
        trust: false,
        errorColor: "currentColor",
      });
    } catch {
      html = `<code>${escape(tex)}</code>`;
    }
    if (rendered.size > 2000) rendered.clear();
    rendered.set(key, html);
  }
  return html;
}

let hooked = false;

let lifted: Math[] = [];

function hook(): void {
  if (hooked) return;
  hooked = true;
  DOMPurify.addHook("uponSanitizeAttribute", (_node, data) => {
    data.attrValue = data.attrValue.replace(/MGMATH(\d+)ENDMG/g, (whole, index: string) => lifted[Number(index)]?.tex ?? whole);
  });
  DOMPurify.addHook("afterSanitizeAttributes", (node) => {
    if (node.nodeName === "IMG") {
      const src = node.getAttribute("src") ?? "";
      node.removeAttribute("src");
      node.removeAttribute("srcset");
      node.setAttribute("data-src", src);
    }
  });
}

function clean(html: string, math: Math[]): string {
  lifted = math;
  hook();
  return DOMPurify.sanitize(html, {
    FORBID_TAGS: ["style", "form", "input", "button", "iframe", "video", "audio", "object", "embed", "svg", "math", "map", "area"],
    FORBID_ATTR: ["style", "srcset", "usemap"],
  });
}

function putMathBack(html: string, math: Math[], display: boolean): string {
  if (math.length === 0) return html;
  return html.replace(/MGMATH(\d+)ENDMG/g, (whole, index: string) => {
    const item = math[Number(index)];
    return item ? renderMath(display ? item : { ...item, display: false }) : whole;
  });
}

marked.setOptions({ gfm: true, breaks: false });

const TASK = /<p>(<strong>(?:task|check|quick check|question|your turn|try it|try this|redo|paper redo|now you)\b[^<]{0,40}<\/strong>)/gi;

export function renderMarkdown(src: string): string {
  const { text, math } = liftMath(src);
  let html = clean(marked.parse(text, { async: false }) as string, math);
  html = putMathBack(html, math, true);
  if (math.length > 0) {
    html = html.replace(/<p>(<span class="katex-display">[\s\S]*?<\/span>)<\/p>/g, "$1");
  }
  html = html.replace(/<table>/g, '<div class="table"><table>').replace(/<\/table>/g, "</table></div>");
  html = html.replace(/<pre><code class="language-mermaid">/g, '<pre class="diagram-wait"><code class="language-mermaid">');
  return quietEarlier(setTaskApart(html.replace(TASK, (_whole, label: string) => `<p class="task">${label.replace(/\s*[.:]\s*<\/strong>$/, "</strong>")}`)));
}

function setTaskApart(html: string): string {
  if (!html.includes('<p class="task">')) return html;
  const holder = document.createElement("template");
  holder.innerHTML = html;
  const blocks = Array.from(holder.content.children);
  const start = blocks.map((block) => block.matches("p.task")).lastIndexOf(true);
  if (start === -1) return html;
  const box = document.createElement("div");
  box.className = "task";
  holder.content.insertBefore(box, blocks[start]);
  let node: ChildNode | null = box.nextSibling;
  while (node) {
    const next: ChildNode | null = node.nextSibling;
    box.appendChild(node);
    node = next;
  }
  return holder.innerHTML;
}

const inline = new Map<string, string>();

export function renderInline(src: string): string {
  let html = inline.get(src);
  if (html !== undefined) return html;
  const { text, math } = liftMath(src.replace(/\s*\n+\s*/g, " "));
  lifted = math;
  hook();
  html = DOMPurify.sanitize(marked.parseInline(text, { async: false }) as string, {
    ALLOWED_TAGS: ["strong", "em", "b", "i", "code", "del", "sub", "sup"],
    ALLOWED_ATTR: [],
  });
  html = putMathBack(html, math, false);
  if (inline.size > 2000) inline.clear();
  inline.set(src, html);
  return html;
}

export function renderPlain(src: string): string {
  const { text, math } = liftMath(src);
  const lines = text.replace(/\n\n(MGMATH\d+ENDMG)\n\n/g, " $1 ").trim();
  return putMathBack(escape(lines).replace(/\n/g, "<br>"), math, false);
}

export function hasMath(src: string): boolean {
  return liftMath(src).math.length > 0;
}

const tasks = new Map<string, boolean>();

export function hasTask(src: string): boolean {
  let found = tasks.get(src);
  if (found === undefined) {
    found = renderMarkdown(src).includes('class="task"');
    if (tasks.size > 400) tasks.clear();
    tasks.set(src, found);
  }
  return found;
}

export function sourceLinks(src: string): { text: string; href: string }[] {
  const holder = document.createElement("template");
  holder.innerHTML = renderMarkdown(src);
  return Array.from(holder.content.querySelectorAll("a[href]"))
    .map((link) => ({ text: (link.textContent ?? "").trim(), href: link.getAttribute("href") ?? "" }))
    .filter((link) => link.text !== "" && sourceRef(link.href) !== null);
}

export function setSourcesApart(html: string, open: boolean, opensNow?: (path: string) => boolean): string {
  if (!html.includes("<a ")) return html;
  const holder = document.createElement("template");
  holder.innerHTML = html;
  const links = Array.from(holder.content.querySelectorAll("a[href]")).filter((a) => sourceRef(a.getAttribute("href") ?? "") !== null);
  if (links.length === 0) return html;
  for (const link of links) {
    const path = sourceRef(link.getAttribute("href") ?? "")?.path ?? "";
    if (open || opensNow?.(path)) {
      link.classList.add("source");
      continue;
    }
    const plain = document.createElement("span");
    plain.className = "source-shut";
    plain.dataset.href = link.getAttribute("href") ?? "";
    plain.replaceChildren(...Array.from(link.childNodes));
    link.replaceWith(plain);
  }
  return holder.innerHTML;
}

export function taskWords(text: string): string[] {
  const plain = text
    .replace(/\$\$[\s\S]*?\$\$|\$[^$]*\$|\\\([\s\S]*?\\\)|\\\[[\s\S]*?\\\]/g, " ")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, " ");
  return plain.split(" ").filter((word) => word.length >= 3);
}

const wordSets = new Map<string, Set<string>>();

function wordsOf(text: string): Set<string> {
  let found = wordSets.get(text);
  if (found === undefined) {
    found = new Set(taskWords(text));
    if (wordSets.size > 2000) wordSets.clear();
    wordSets.set(text, found);
  }
  return found;
}

export function coverage(text: string, task: string): number {
  const wanted = wordsOf(task);
  if (wanted.size === 0) return 0;
  const have = wordsOf(text);
  let found = 0;
  wanted.forEach((word) => have.has(word) && (found += 1));
  return found / wanted.size;
}

export function poses(reply: string, task: string): boolean {
  return reply.trim() !== "" && coverage(reply, task) >= 0.8;
}

export function heldAnswerGoes(
  held: { question: string; id: string | null },
  after: { posed: string | null | undefined; task: string | null; taskUnseen: boolean; reply: string },
): "answer" | "unmarked" | "line" {
  const byRecord = Boolean(held.id) && after.posed !== undefined;
  if (byRecord && after.posed !== held.id) return "line";
  const bare = (text: string) => text.replace(/\s+/g, "");
  const holds = (text: string) => bare(held.question) !== "" && bare(text).includes(bare(held.question));
  if (after.task !== null) return holds(after.task) ? "answer" : "line";
  if (!holds(after.reply)) return "line";
  return byRecord ? "answer" : after.taskUnseen ? "unmarked" : "line";
}

export function quietEarlier(html: string): string {
  return html.replace(
    /(<p[^>]*>|<li>)\s*(?:<strong>)?(?:Quick check|Paper redo|Repair)\s+\d+(?:\s+of\s+\d+)?\s*[:.]?\s*(?:<\/strong>)?\s*[:.]?/g,
    (_whole, open: string) => `${open}<span class="earlier">From earlier</span> `,
  );
}
