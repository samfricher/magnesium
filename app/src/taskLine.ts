export type Piece = { text: string; atomic: boolean };

export type Found = { start: { piece: number; at: number }; end: { piece: number; at: number } };

const ENVIRONMENT = /^\\begin\{([A-Za-z]+\*?)\}/;

export function texKey(tex: string): string {
  return tex.replace(/\\text\{([^{}]*)\}/g, "$1").replace(/[\s{}]+/g, "");
}

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

export function taskPlain(markdown: string): string {
  const kept: string[] = [];
  const keep = (text: string, tex: boolean) => `\u0000${kept.push(tex ? texKey(text) : text) - 1}\u0000`;
  const words = markdown.replace(/^\s*(?:\*\*)?quick check\s+\d+(?:\s+of\s+\d+)?\s*[:.]?\s*(?:\*\*)?\s*[:.]?\s*/i, "");
  const text = setAside(words, keep)
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/(\*\*|__)(?=\S)([\s\S]*?\S)\1/g, "$2")
    .replace(/(?<![\p{L}\p{N}*])\*(?=\S)([\s\S]*?\S)\*(?![\p{L}\p{N}*])/gu, "$1")
    .replace(/(?<![\p{L}\p{N}_])_(?=\S)([\s\S]*?\S)_(?![\p{L}\p{N}_])/gu, "$1")
    .replace(/^(\*{1,3}|_{1,3})(?=\S)|(?<=\S)(\*{1,3}|_{1,3})$/g, "")
    .replace(/\\([\\`*_{}[\]()#+\-.!|<>~$])/g, "$1");
  return normal(text.replace(/\u0000(\d+)\u0000/g, (_whole, n: string) => kept[Number(n)]));
}

export function normal(text: string): string {
  return text.replace(/[\s\u00a0]+/g, " ").trim();
}

export function locate(pieces: Piece[], task: string): Found | null {
  const wanted = normal(task);
  if (wanted === "") return null;
  let text = "";
  const from: { piece: number; at: number }[] = [];
  let space = true;
  pieces.forEach((piece, index) => {
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
  if (first.at === -1 && before?.piece === first.piece) return null;
  if (last.at === -1 && after?.piece === last.piece) return null;
  return {
    start: { piece: first.piece, at: first.at },
    end: { piece: last.piece, at: last.at === -1 ? -1 : last.at + 1 },
  };
}

export function ruleAction(boxes: number, skipped: boolean): "draw" | "clear" | "keep" {
  if (boxes > 0) return "draw";
  return skipped ? "keep" : "clear";
}
