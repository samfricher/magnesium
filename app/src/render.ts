// The tutor writes Markdown with mathematics. This turns it into safe HTML.
//
// Order matters. Mathematics is lifted out first, so Markdown cannot mangle
// it. The Markdown is rendered and cleaned. Then the mathematics, rendered
// here by KaTeX, is put back. Nothing the tutor wrote reaches the page
// without passing the cleaner.
//
// Two entry points. `renderMarkdown` is for a whole reply. `renderInline` is
// for one line of text shown anywhere else: a goal, a step, a skill's name,
// the learner's own answer. Both draw the same mathematics, so a formula
// reads the same wherever it appears.

import DOMPurify from "dompurify";
import katex from "katex";
import { marked } from "marked";

type Math = { tex: string; display: boolean };

const token = (index: number) => `MGMATH${index}ENDMG`;

function closingFence(src: string, from: number, fence: string): number {
  // The end of the line that closes a fenced block, or the end of the text.
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

/** Environments that are display mathematics even with no `$$` around them. */
const ENVIRONMENT = /^\\begin\{(equation\*?|align\*?|aligned|gather\*?|gathered|cases|array|[pbvBV]?matrix|split|multline\*?)\}/;

/** How long a formula in a line of text may run before it is taken for something else, such as a price. */
const INLINE_LIMIT = 200;

/**
 * Lift mathematics out of Markdown, leaving a token in its place. `open` is
 * where a formula starts that the text ends inside of: a reply that is still
 * being written stops in the middle of one.
 */
export function liftMath(src: string): { text: string; math: Math[]; open: number | null } {
  const math: Math[] = [];
  let out = "";
  let i = 0;
  let open: number | null = null;
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
      // An environment written with no dollar signs around it.
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
      // Inline mathematics: the closing sign has no space before it and no
      // digit after it, which keeps prices such as $5 and $10 as text.
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
      // The text ends before this formula does. A long run is more likely a
      // price than a formula, and is left as it is.
      if (ranOut && src.length - i <= INLINE_LIMIT) unclosed(i);
    }
    out += c;
    i++;
  }
  return { text: out, math, open };
}

/**
 * While a reply is still being written, its last formula can be half there.
 * Cut the text before it, so the formula appears whole, in one step, and its
 * source is never on screen.
 */
export function withoutOpenMath(src: string): string {
  const { open } = liftMath(src);
  return open === null ? src : src.slice(0, open).trimEnd();
}

const rendered = new Map<string, string>();

const escape = (text: string) => text.replace(/[&<>]/g, (ch) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[ch] ?? ch);

function renderMath({ tex, display }: Math): string {
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

// The mathematics of the text being cleaned. A token inside an attribute value,
// such as an image's alt text or a link's title, cannot hold markup, so it goes
// back as the TeX source. It does so before the cleaner checks the value.
let lifted: Math[] = [];

function hook(): void {
  if (hooked) return;
  hooked = true;
  // An image is never loaded by the page itself. A local one is fetched
  // through the core, which keeps it inside the study folder. A remote one
  // is not fetched at all: the window makes no network request of its own.
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

/**
 * The words a tutor opens a task with. A paragraph that starts with one of
 * them in bold is the thing to do next, and the page sets it apart.
 */
const TASK = /<p>(<strong>(?:task|check|quick check|question|your turn|try it|try this|redo|now you)\b[^<]{0,40}<\/strong>)/gi;

export function renderMarkdown(src: string): string {
  const { text, math } = liftMath(src);
  let html = clean(marked.parse(text, { async: false }) as string, math);
  html = putMathBack(html, math, true);
  if (math.length > 0) {
    // A display formula alone in a paragraph should not carry the paragraph's margins twice.
    html = html.replace(/<p>(<span class="katex-display">[\s\S]*?<\/span>)<\/p>/g, "$1");
  }
  // Tables scroll inside their own box, so a wide one never widens the page.
  html = html.replace(/<table>/g, '<div class="table"><table>').replace(/<\/table>/g, "</table></div>");
  // The label loses the full stop or colon the tutor closed it with.
  return setTaskApart(html.replace(TASK, (_whole, label: string) => `<p class="task">${label.replace(/\s*[.:]\s*<\/strong>$/, "</strong>")}`));
}

/**
 * A task is the last thing in a reply, and it can run over several blocks: a
 * sentence, a formula, a list. Everything from the last task paragraph to the
 * end goes in one box.
 */
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

/** One line of text with mathematics, bold, italics and code. No links, images or blocks. */
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

/**
 * The learner's own words: every character as typed, with line breaks kept
 * and only the mathematics drawn. Nothing else is read as markup, so `2*x*3`
 * stays `2*x*3`.
 */
export function renderPlain(src: string): string {
  const { text, math } = liftMath(src);
  // Lifting sets a display formula on its own paragraph. Here it stays in the line it was typed in.
  const lines = text.replace(/\n\n(MGMATH\d+ENDMG)\n\n/g, " $1 ").trim();
  return putMathBack(escape(lines).replace(/\n/g, "<br>"), math, false);
}

/** Whether the text holds mathematics the page would draw. */
export function hasMath(src: string): boolean {
  return liftMath(src).math.length > 0;
}
