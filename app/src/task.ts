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

/** Words that open a sentence about the last answer or the lesson, not the task itself. */
const LEAD = /^(?:right|not quite|not yet|correct|good|yes|no|close|exactly|nice|great|back to it|welcome back|next step|next|let's|let us|ok|okay|so|now|that's|that is|almost)\b/i;

/**
 * Where the task begins in a paragraph that ends with a question: after the
 * sentences that answer the last message or move the lesson on ("Right: …",
 * "Next step."), so the task holds the question and what it needs.
 */
function taskStart(paragraph: string): number {
  const boundary = /[.!:]\s+(?=[A-Z$\\(*"“])/g;
  const starts = [0];
  let found: RegExpExecArray | null;
  while ((found = boundary.exec(paragraph)) !== null) starts.push(found.index + found[0].length);
  let at = 0;
  for (let i = 0; i < starts.length - 1; i++) {
    if (!LEAD.test(paragraph.slice(starts[i]).trimStart())) break;
    at = starts[i + 1];
  }
  return at;
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
  const at = taskStart(last);
  const before = [...parts.slice(0, -1), last.slice(0, at).trim()].filter((part) => part !== "").join("\n\n");
  return { before, task: last.slice(at).trim() };
}

/**
 * Split a reply at the task the record names, in the tutor's own words. A
 * reply that holds them puts the task block where they begin; one that does
 * not ends with the task as the window reads it, else its last paragraph.
 */
export function splitAt(reply: string, task: string): Split {
  const squash = (text: string) => text.replace(/\*\*/g, "").replace(/\s+/g, " ").trim();
  const want = squash(task).slice(0, 48);
  if (want.length >= 8) {
    const parts = reply.split(/\n{2,}/);
    for (let i = parts.length - 1; i >= 0; i--) {
      const flat = squash(parts[i]);
      const at = flat.indexOf(want);
      if (at === -1) continue;
      // Only a short label stands before the task's words ("Task.", "Quick check 1:"): the paragraph is the task.
      const label = at > 0 && at <= 24 && /^(?:task|question|check|quick check|paper redo|redo|repair|exercise)(?:\s*\d+)?\s*[.:]?\s*$/i.test(flat.slice(0, at));
      if (at === 0 || label) return { before: parts.slice(0, i).join("\n\n"), task: parts.slice(i).join("\n\n") };
      // The task starts part way through this paragraph: split it at the same words.
      const words = want.split(" ").slice(0, 3).join(" ");
      const raw = parts[i].indexOf(words);
      if (raw > 0) return { before: [...parts.slice(0, i), parts[i].slice(0, raw).trim()].filter(Boolean).join("\n\n"), task: [parts[i].slice(raw), ...parts.slice(i + 1)].join("\n\n").trim() };
      return { before: parts.slice(0, i).join("\n\n"), task: parts.slice(i).join("\n\n") };
    }
  }
  // The tutor said it again in other words: the paragraphs at the end that carry most of the task's words.
  const parts = reply.trim().split(/\n{2,}/);
  const want2 = contentWords(task);
  if (want2.size >= 3) {
    const seen = new Set<string>();
    for (let i = parts.length - 1; i >= 0; i--) {
      contentWords(parts[i]).forEach((word) => seen.add(word));
      if (cover(want2, seen) >= 0.6) {
        let start = i;
        while (start < parts.length - 1 && cover(want2, contentWords(parts[start])) === 0) start++;
        // The task starts at the question, not at the tutor's lead-in ("Back to it.").
        const lead = taskStart(parts[start]);
        const before = [...parts.slice(0, start), parts[start].slice(0, lead).trim()].filter(Boolean).join("\n\n");
        return { before, task: [parts[start].slice(lead), ...parts.slice(start + 1)].join("\n\n").trim() };
      }
    }
  }
  const found = splitTask(reply);
  if (found) return found;
  return { before: parts.slice(0, -1).join("\n\n"), task: parts[parts.length - 1] };
}

/** The words of four letters or more in a text, lower case, markup and mathematics marks left out. */
function contentWords(text: string): Set<string> {
  return new Set(text.replace(/[*$\\{}_^]/g, " ").toLowerCase().match(/[a-z0-9]{4,}/g) ?? []);
}

const cover = (want: Set<string>, have: Set<string>) => {
  let hit = 0;
  want.forEach((word) => {
    if (have.has(word)) hit += 1;
  });
  return want.size === 0 ? 0 : hit / want.size;
};

/** How much of the open task a reply says again, from 0 to 1: a reply to "back to the task" that restates it, in any words. */
export function restates(reply: string, task: string): number {
  return cover(contentWords(task), contentWords(reply));
}

/** The first line of a task, as a short line for the pinned header and the stop line. */
export function taskLine(task: string): string {
  // Only a short label such as "**Task.**" goes; a whole question in bold stays, unbolded.
  const plain = task
    .replace(/^\s*\*\*[^*]{1,24}?[.:]\s*\*\*\s*/, "")
    .replace(/\*\*/g, "")
    .replace(/\s+/g, " ")
    .trim();
  return plain.length > 140 ? `${plain.slice(0, 137).trimEnd()}…` : plain;
}
