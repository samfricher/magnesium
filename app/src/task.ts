// Where a reply sets a task. The window learns it from the reply itself: no
// event says so. The engine keeps the real open task, so reading one where
// there is none costs no more than today (the tutor judges what is sent as
// an answer), while talk, sent from outside the task, is never marked.

export type Split = { before: string; task: string };

/** The words a tutor opens a task with, bold or followed by a colon or a number. */
const OPENER = /^\s*(?:\*\*)?(?:task|question|check|quick check|paper redo|redo|repair|your turn|try it|try this|now you|exercise)\b[^\n]{0,24}?(?:[.:]\s*\*\*|\*\*\s*[.:]?|\s\d+\s*:|:)/i;

/** The blocks of a reply, split at blank lines, keeping fenced code and display maths whole. */
function blocks(src: string): string[] {
  const lines = src.split("\n");
  const out: string[] = [];
  let current: string[] = [];
  let fence: string | null = null;
  let display = false;
  for (const line of lines) {
    const trimmed = line.trim();
    if (fence) {
      current.push(line);
      if (trimmed.startsWith(fence)) fence = null;
      continue;
    }
    const opens = /^(```|~~~)/.exec(trimmed);
    if (opens) {
      fence = opens[1];
      current.push(line);
      continue;
    }
    if (trimmed === "$$" || (trimmed.startsWith("$$") && !trimmed.slice(2).includes("$$"))) display = !display;
    if (trimmed === "" && !display) {
      if (current.length > 0) out.push(current.join("\n"));
      current = [];
      continue;
    }
    current.push(line);
  }
  if (current.length > 0) out.push(current.join("\n"));
  return out;
}

/** The last sentence of a paragraph that ends with a question: where the question begins. */
function questionStart(paragraph: string): number {
  const text = paragraph.trimEnd();
  // The boundary before the question: a full stop, a colon or an exclamation
  // mark, then a space, then the question's first word.
  let start = 0;
  const boundary = /([.!:])\s+(?=[A-Z$\\(*"“])/g;
  let found: RegExpExecArray | null;
  const end = text.length - 1;
  while ((found = boundary.exec(text)) !== null) {
    if (found.index < end) start = found.index + found[0].length;
  }
  return start;
}

const endsWithQuestion = (text: string) => /\?\s*(?:\*\*|\*|_)?\s*$|\?\s*\$\s*$/.test(text.trim());

/**
 * The task a reply sets, and what comes before it, or null when it sets none.
 * A task opens with a word such as "Task." or "Quick check 1:"; failing that,
 * a reply whose last paragraph ends with a question sets that question.
 */
export function splitTask(reply: string): Split | null {
  const parts = blocks(reply);
  if (parts.length === 0) return null;
  for (let i = parts.length - 1; i >= 0; i--) {
    if (OPENER.test(parts[i])) {
      return { before: parts.slice(0, i).join("\n\n"), task: parts.slice(i).join("\n\n") };
    }
  }
  const last = parts[parts.length - 1];
  if (!endsWithQuestion(last)) return null;
  // A heading, a list or a quotation that ends in a question is not a task.
  if (/^\s*(#|>|[-*+] |\d+\. )/.test(last) && !/^\s*\d+\. /.test(last)) return null;
  const at = questionStart(last);
  const before = [...parts.slice(0, -1), last.slice(0, at).trim()].filter((part) => part !== "").join("\n\n");
  return { before, task: last.slice(at).trim() };
}

/** The first line of a task, as a short line for the pinned header and the stop line. */
export function taskLine(task: string): string {
  const plain = task
    .replace(/^\s*\*\*([^*]+)\*\*\s*/, "")
    .replace(/\s+/g, " ")
    .trim();
  return plain.length > 140 ? `${plain.slice(0, 137).trimEnd()}…` : plain;
}
