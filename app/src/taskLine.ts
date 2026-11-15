/**
 * Find a task's words in a reply as the page drew it, so the line can be put
 * beside them without moving any text. The task comes from the record in the
 * tutor's markdown; the page has turned that into paragraphs, bold, links and
 * typeset formulas. Both are brought to one plain form: the words as read,
 * with each formula as a key of its TeX, which KaTeX keeps beside what it
 * draws.
 *
 * No imports, so `scripts/test-task-line.mjs` can run it under node.
 */

/** A run of the drawn reply in order: a text node's text, or a whole formula (`atomic`) as its TeX. */
export type Piece = { text: string; atomic: boolean };

/** Where the task's words begin and end among the pieces: the piece, and the character in it (none for a formula). */
export type Found = { start: { piece: number; at: number }; end: { piece: number; at: number } };

const ENVIRONMENT = /^\\begin\{([A-Za-z]+\*?)\}/;

/**
 * A formula's TeX as it is compared: no spaces, and `\text{…}` as its words.
 * The page sets some subscripts upright before typesetting (V_{max} becomes
 * V_{\text{max}}), so the TeX KaTeX keeps can differ from the tutor's in that.
 */
export function texKey(tex: string): string {
  return tex.replace(/\\text\{([^{}]*)\}/g, "$1").replace(/\s+/g, "");
}

/**
 * The markdown with each formula and code span swapped for `keep(content)`,
 * found by the rules the page typesets by (render.ts): $$…$$, \[…\], \(…\),
 * a bare \begin{…}…\end{…}, and $…$ with no space inside either sign and no
 * digit after the closing one, within a paragraph.
 */
function setAside(src: string, keep: (text: string, tex: boolean) => string): string {
  let out = "";
  let i = 0;
  while (i < src.length) {
    const c = src[i];
    const next = src[i + 1];
    if (c === "`") {
      let run = 1;
      while (src[i + run] === "`") run++;
      const close = src.indexOf("`".repeat(run), i + run);
      if (close !== -1) {
        out += keep(src.slice(i + run, close), false);
        i = close + run;
        continue;
      }
    }
    if (c === "\\" && next === "$") {
      out += "\\$";
      i += 2;
      continue;
    }
    const pairs: [string, string][] = [["$$", "$$"], ["\\[", "\\]"], ["\\(", "\\)"]];
    const pair = pairs.find(([open]) => src.startsWith(open, i));
    if (pair) {
      const close = src.indexOf(pair[1], i + 2);
      if (close !== -1) {
        out += keep(src.slice(i + 2, close), true);
        i = close + 2;
        continue;
      }
    }
    const begin = c === "\\" ? ENVIRONMENT.exec(src.slice(i, i + 24)) : null;
    if (begin) {
      const closer = `\\end{${begin[1]}}`;
      const close = src.indexOf(closer, i);
      if (close !== -1) {
        out += keep(src.slice(i, close + closer.length), true);
        i = close + closer.length;
        continue;
      }
    }
    if (c === "$" && next !== undefined && !/\s/.test(next)) {
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
        out += keep(src.slice(i + 1, close), true);
        i = close + 1;
        continue;
      }
    }
    out += c;
    i += 1;
  }
  return out;
}

/** The words of a task in markdown, as the page shows them, with each formula as its TeX key. */
export function taskPlain(markdown: string): string {
  const kept: string[] = [];
  const keep = (text: string, tex: boolean) => `\u0000${kept.push(tex ? texKey(text) : text) - 1}\u0000`;
  // The words a due quick check opens with are drawn as "From earlier", which is not the task.
  const words = markdown.replace(/^\s*(?:\*\*)?quick check\s+\d+(?:\s+of\s+\d+)?\s*[:.]?\s*(?:\*\*)?\s*[:.]?\s*/i, "");
  const text = setAside(words, keep)
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/(\*\*|__)(?=\S)([\s\S]*?\S)\1/g, "$2")
    .replace(/(?<![\p{L}\p{N}*])\*(?=\S)([\s\S]*?\S)\*(?![\p{L}\p{N}*])/gu, "$1")
    .replace(/(?<![\p{L}\p{N}_])_(?=\S)([\s\S]*?\S)_(?![\p{L}\p{N}_])/gu, "$1")
    // Marks left open at either end, where the task's words start or stop inside bold or italics.
    .replace(/^(\*{1,3}|_{1,3})(?=\S)|(?<=\S)(\*{1,3}|_{1,3})$/g, "")
    .replace(/\\([\\`*_{}[\]()#+\-.!|<>~$])/g, "$1");
  return normal(text.replace(/\u0000(\d+)\u0000/g, (_whole, n: string) => kept[Number(n)]));
}

/** Runs of white space as one space, and no-break spaces as spaces. */
export function normal(text: string): string {
  return text.replace(/[\s\u00a0]+/g, " ").trim();
}

/**
 * Where `task` (already plain) stands in the pieces, at its last place.
 * None when it is not there, or when it would begin or end inside a formula.
 */
export function locate(pieces: Piece[], task: string): Found | null {
  const wanted = normal(task);
  if (wanted === "") return null;
  let text = "";
  // For each character of `text`: its piece, and its place in that piece (-1 for a formula).
  const from: { piece: number; at: number }[] = [];
  let space = true;
  pieces.forEach((piece, index) => {
    // By UTF-16 unit, so places in `text` and in `from` agree.
    if (piece.atomic) {
      const tex = texKey(piece.text);
      if (tex === "") return;
      for (let at = 0; at < tex.length; at++) {
        text += tex[at];
        from.push({ piece: index, at: -1 });
      }
      space = false;
      return;
    }
    for (let at = 0; at < piece.text.length; at++) {
      const char = piece.text[at];
      if (/[\s\u00a0]/.test(char)) {
        if (!space) {
          text += " ";
          from.push({ piece: index, at });
          space = true;
        }
      } else {
        text += char;
        from.push({ piece: index, at });
        space = false;
      }
    }
  });
  const begin = text.lastIndexOf(wanted);
  if (begin === -1) return null;
  const finish = begin + wanted.length - 1;
  const first = from[begin];
  const last = from[finish];
  const before = from[begin - 1];
  const after = from[finish + 1];
  // A formula is one thing on the page: the task cannot take part of one.
  if (first.at === -1 && before?.piece === first.piece) return null;
  if (last.at === -1 && after?.piece === last.piece) return null;
  return {
    start: { piece: first.piece, at: first.at },
    end: { piece: last.piece, at: last.at === -1 ? -1 : last.at + 1 },
  };
}
