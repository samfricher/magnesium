/**
 * Start a new paragraph where a task's words begin, when the tutor wrote the
 * task at the end of a paragraph that also teaches or gives feedback. The
 * window can only set apart whole blocks, so without this the line beside the
 * task would take in the whole paragraph.
 *
 * Only words the reply holds exactly are split off, and only in a plain
 * paragraph: never inside a list, quote, table, heading, code or formula,
 * and never where the words start inside bold, italics, a link or an inline
 * formula, where a blank line would change what the reply says. The words
 * must run to the end of their paragraph. Otherwise the reply is left as it
 * is.
 */
export function splitAtTask(text: string, task: string): string {
  const words = task.trim();
  if (words === "") return text;
  const at = text.lastIndexOf(words);
  if (at <= 0) return text;
  const before = text.slice(0, at);
  // Already a block of its own.
  if (/\n[ \t]*\n[ \t]*$/.test(before)) return text;
  // The rest of the paragraph after the words is empty.
  const after = text.slice(at + words.length);
  if (!/^[ \t]*(\n[ \t]*\n[\s\S]*|\n?[ \t]*)$/.test(after)) return text;
  // The paragraph the words are in.
  const blank = before.search(/\n[ \t]*\n(?![\s\S]*\n[ \t]*\n)/);
  const start = blank === -1 ? 0 : before.indexOf("\n", blank + 1) + 1;
  const paragraph = text.slice(start, at);
  const lines = paragraph.split("\n");
  if (lines.some((line) => /^\s{0,3}([-*+]\s|\d+[.)]\s|>|#|\||```|~~~|\$\$|\\\[)/.test(line) || /^( {4}|\t)/.test(line))) return text;
  // Not inside a code fence opened earlier in the reply.
  if ((before.match(/^\s{0,3}(```|~~~)/gm)?.length ?? 0) % 2 === 1) return text;
  if (opensInline(paragraph)) return text;
  return `${before.replace(/[ \t]*\n?[ \t]*$/, "")}\n\n${text.slice(at)}`;
}

/**
 * Whether the text before the task's words, in their paragraph, leaves
 * something open that a blank line would break: code, a formula, bold,
 * italics, or a link's words or address.
 */
function opensInline(before: string): boolean {
  const count = (pattern: RegExp) => before.match(pattern)?.length ?? 0;
  if (count(/`/g) % 2 === 1) return true;
  if (count(/(?<!\\)\$/g) % 2 === 1) return true;
  if (count(/\\\(/g) > count(/\\\)/g)) return true;
  if (count(/(?<!\\)\[/g) > count(/(?<!\\)\]/g)) return true;
  if (/\]\([^)]*$/.test(before)) return true;
  // Bold and italics, as delimiters: an asterisk anywhere, an underscore only
  // at a word's edge, since snake_case is no emphasis.
  const stars = before.match(/(?<!\\)\*+/g) ?? [];
  const unders = before.match(/(?<![\p{L}\p{N}])_+|_+(?![\p{L}\p{N}])/gu) ?? [];
  for (const runs of [stars, unders]) {
    let strong = 0;
    let em = 0;
    for (const run of runs) {
      strong += Math.floor(run.length / 2);
      em += run.length % 2;
    }
    if (strong % 2 === 1 || em % 2 === 1) return true;
  }
  return false;
}
