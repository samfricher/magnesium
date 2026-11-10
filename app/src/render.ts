// The tutor writes Markdown with mathematics. This turns it into safe HTML.
//
// Order matters. Mathematics is lifted out first, so Markdown cannot mangle
// it. The Markdown is rendered and cleaned. Then the mathematics, rendered
// here by KaTeX, is put back. Nothing the tutor wrote reaches the page
// without passing the cleaner.

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

/** Lift mathematics out of Markdown, leaving a token in its place. */
export function liftMath(src: string): { text: string; math: Math[] } {
  const math: Math[] = [];
  let out = "";
  let i = 0;
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
    }
    if (c === "\\" && (src[i + 1] === "[" || src[i + 1] === "(")) {
      const display = src[i + 1] === "[";
      const close = src.indexOf(display ? "\\]" : "\\)", i + 2);
      if (close !== -1) {
        push(src.slice(i + 2, close), display);
        i = close + 2;
        continue;
      }
    }
    if (c === "$" && src[i + 1] !== undefined && !/\s/.test(src[i + 1])) {
      // Inline mathematics: the closing sign has no space before it and no
      // digit after it, which keeps prices such as $5 and $10 as text.
      let close = -1;
      for (let j = i + 1; j < src.length; j++) {
        if (src[j] === "\n" && src[j + 1] === "\n") break;
        if (src[j] === "\\") {
          j++;
          continue;
        }
        if (src[j] === "$") {
          if (!/\s/.test(src[j - 1]) && !/\d/.test(src[j + 1] ?? "")) close = j;
          break;
        }
      }
      if (close > i + 1) {
        push(src.slice(i + 1, close), false);
        i = close + 1;
        continue;
      }
    }
    out += c;
    i++;
  }
  return { text: out, math };
}

const rendered = new Map<string, string>();

function renderMath({ tex, display }: Math): string {
  const key = (display ? "D" : "I") + tex;
  let html = rendered.get(key);
  if (html === undefined) {
    try {
      html = katex.renderToString(tex, { displayMode: display, throwOnError: false, strict: "ignore", trust: false });
    } catch {
      html = `<code>${tex.replace(/[&<>]/g, (ch) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[ch] ?? ch)}</code>`;
    }
    if (rendered.size > 2000) rendered.clear();
    rendered.set(key, html);
  }
  return html;
}

let hooked = false;

function clean(html: string): string {
  if (!hooked) {
    hooked = true;
    // An image is never loaded by the page itself. A local one is fetched
    // through the core, which keeps it inside the study folder. A remote one
    // is not fetched at all: the window makes no network request of its own.
    DOMPurify.addHook("afterSanitizeAttributes", (node) => {
      if (node.nodeName === "IMG") {
        const src = node.getAttribute("src") ?? "";
        node.removeAttribute("src");
        node.removeAttribute("srcset");
        node.setAttribute("data-src", src);
      }
    });
  }
  return DOMPurify.sanitize(html, {
    FORBID_TAGS: ["style", "form", "input", "button", "iframe", "video", "audio", "object", "embed", "svg", "math"],
    FORBID_ATTR: ["style", "srcset"],
  });
}

marked.setOptions({ gfm: true, breaks: false });

export function renderMarkdown(src: string): string {
  const { text, math } = liftMath(src);
  let html = clean(marked.parse(text, { async: false }) as string);
  if (math.length > 0) {
    html = html.replace(/MGMATH(\d+)ENDMG/g, (whole, index: string) => {
      const item = math[Number(index)];
      return item ? renderMath(item) : whole;
    });
    // A display formula alone in a paragraph should not carry the paragraph's margins twice.
    html = html.replace(/<p>(<span class="katex-display">[\s\S]*?<\/span>)<\/p>/g, "$1");
  }
  return html;
}
