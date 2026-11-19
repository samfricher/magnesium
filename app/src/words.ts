import { opening, saidByTheWindow } from "./lesson";
import type { Mark, Plan, PlanItem } from "./types";

export function displayName(name: string): string {
  return name.replace(/[-_]+/g, " ").trim() || name;
}

const DAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const LONG_DAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const LONG_MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

function dateOf(day: string): Date | null {
  const found = /^(\d{4})-(\d{2})-(\d{2})/.exec(day);
  return found ? new Date(Number(found[1]), Number(found[2]) - 1, Number(found[3]), 12) : null;
}

export function shortDay(day: string): string {
  const date = dateOf(day);
  return date ? `${DAYS[date.getDay()]} ${date.getDate()} ${MONTHS[date.getMonth()]}` : day;
}

export function longDay(day: string): string {
  const date = dateOf(day);
  return date ? `${LONG_DAYS[date.getDay()]} ${date.getDate()} ${LONG_MONTHS[date.getMonth()]}` : day;
}

const HELP_WORDS: Record<string, string> = {
  hint: "with a hint",
  step: "with a step shown",
  solution: "after the worked solution",
};

export function markWords(mark: Mark): string {
  const help = HELP_WORDS[mark.help] ?? null;
  const on = mark.paper ? ` on ${mark.paper}` : "";
  switch (mark.mark) {
    case "right":
      if (help) return `Marked right${on}, ${help}.`;
      return mark.cold ? `Marked right${on}, on your own.` : `Marked right${on}.`;
    case "partly":
      return help ? `Marked partly right${on}, ${help}.` : `Marked partly right${on}.`;
    case "wrong":
      return `Not right yet${on}.`;
    case "declined":
      return "Left for another day.";
    default:
      return `Marked ${mark.mark}${on}.`;
  }
}

const PASS_WORDS = ["First", "Second", "Third"];

const isoDay = (date: Date) => `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;

export function fromToday(day: string, now = new Date()): "past" | "tomorrow" | "later" {
  if (day <= isoDay(now)) return "past";
  return day === isoDay(new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1)) ? "tomorrow" : "later";
}

function passStep(mark: Mark): { said: string; next: string | null } | null {
  const said = mark.mark === "right" && mark.help === "none" && mark.pass ? PASS_WORDS[mark.pass - 1] : undefined;
  if (!said) return null;
  return { said, next: mark.pass && mark.pass < 3 && mark.next_day ? PASS_WORDS[mark.pass].toLowerCase() : null };
}

export function passWords(mark: Mark, now = new Date()): string | null {
  const step = passStep(mark);
  if (!step) return null;
  if (!step.next || !mark.next_day) return `${step.said} of three days.`;
  const from = fromToday(mark.next_day, now);
  const when = from === "past" ? "today" : from === "tomorrow" ? "from tomorrow" : `from ${shortDay(mark.next_day)}`;
  return `The ${step.next} day can count ${when}.`;
}

export function passDaySpoken(mark: Mark): string | null {
  const step = passStep(mark);
  return step && step.next && mark.next_day ? `${step.said} of three days.` : null;
}

export function passDay(mark: Mark): number | null {
  if (!passStep(mark)) return null;
  return mark.pass && mark.pass >= 1 && mark.pass <= 3 ? mark.pass : null;
}

export function markShort(mark: Mark): string {
  const help = HELP_WORDS[mark.help] ?? null;
  switch (mark.mark) {
    case "right":
      return help ? `right, ${help}` : mark.cold ? "right, on your own" : "right";
    case "partly":
      return help ? `partly right, ${help}` : "partly right";
    case "wrong":
      return "not right yet";
    case "declined":
      return "left for later";
    default:
      return mark.mark;
  }
}

export type Change = { what: string; detail: string; route?: boolean };

export type Moved = "up" | "down" | "new" | "day";

export type PlanDiff = {
  changes: Change[];
  moved: Record<string, Moved>;
  examFrom: string | null;
};

const order = (items: PlanItem[]) => new Map(items.map((item, at) => [item.id, at]));

export function planDiff(before: Plan | null, after: Plan): PlanDiff {
  const changes: Change[] = [];
  const moved: Record<string, Moved> = {};
  let examFrom: string | null = null;
  if (!before) return { changes, moved, examFrom };
  const title = after.title ?? before.title ?? "The exam";
  if (before.exam_date !== after.exam_date) {
    if (before.exam_date && after.exam_date) {
      examFrom = before.exam_date;
      changes.push({ what: title, detail: `${shortDay(before.exam_date)} → ${shortDay(after.exam_date)}` });
    } else if (after.exam_date) changes.push({ what: title, detail: `now on ${shortDay(after.exam_date)}` });
    else changes.push({ what: title, detail: "no date now" });
  }
  const was = order(before.items);
  const now = order(after.items);
  const kept = after.items.filter((item) => was.has(item.id));
  const keptBefore = [...kept].sort((a, b) => (was.get(a.id) ?? 0) - (was.get(b.id) ?? 0));
  kept.forEach((item, at) => {
    const from = keptBefore.findIndex((other) => other.id === item.id);
    if (from > at) moved[item.id] = "up";
    else if (from < at) moved[item.id] = "down";
  });
  for (const item of kept) {
    if (moved[item.id] !== "up") continue;
    const next = kept[kept.indexOf(item) + 1];
    changes.push({ what: item.title, detail: next ? `now before ${next.title}` : "moved up" });
  }
  const redayed = kept.filter((item) => {
    const old = before.items.find((each) => each.id === item.id);
    return old && (old.day ?? "") !== (item.day ?? "") && item.day;
  });
  for (const item of redayed) if (!moved[item.id]) moved[item.id] = "day";
  if (redayed.length >= 3) changes.push({ what: "The blocks", detail: `re-spaced from ${shortDay(redayed[0].day!)}` });
  else for (const item of redayed) changes.push({ what: item.title, detail: `now on ${shortDay(item.day!)}` });
  for (const item of after.items) {
    if (!was.has(item.id)) {
      moved[item.id] = "new";
      changes.push({ what: item.title, detail: "added" });
    }
  }
  for (const item of before.items) if (!now.has(item.id)) changes.push({ what: item.title, detail: "taken off" });
  if ((before.goal ?? "") !== (after.goal ?? "") && after.goal) changes.push({ what: "The goal", detail: after.goal, route: true });
  const route = (plan: Plan) => plan.steps.map((step) => step.step).join("\n");
  if (route(before) !== route(after) && after.steps.length > 0) changes.push({ what: "The route", detail: `${after.steps.length} steps`, route: true });
  if (changes.length === 0 && (before.text ?? "") !== (after.text ?? "")) changes.push({ what: "The plan file", detail: "rewritten" });
  return { changes, moved, examFrom };
}

export function firstWords(text: string, limit = 90): string {
  const plain = text
    .replace(/^\s*\*\*[^*]{0,40}\*\*[.:]?\s*/, "")
    .replace(/\s+/g, " ")
    .trim();
  if (plain.length <= limit) return plain;
  const sentences = plain.split(/(?<=[.?!])\s+/).filter((each) => each !== "");
  const question = [...sentences].reverse().find((each) => each.endsWith("?"));
  if (question && question.length <= limit) return question;
  let cut = plain.slice(0, limit - 1);
  if ((cut.match(/(?<!\\)\$/g)?.length ?? 0) % 2 === 1) cut = cut.slice(0, cut.lastIndexOf("$"));
  return `${cut.trimEnd()}…`;
}

export function stoppedWords(line: string): string {
  const text = line.trim();
  return /[.?!]$/.test(text) ? `You stopped at: ${text}` : `You stopped at ${text}.`;
}

export function whenOpened(ms: number | null | undefined, now = new Date()): string {
  if (!ms) return "";
  const then = new Date(ms);
  const day = (date: Date) => new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  const days = Math.round((day(now) - day(then)) / 86_400_000);
  const time = `${String(then.getHours()).padStart(2, "0")}:${String(then.getMinutes()).padStart(2, "0")}`;
  if (days <= 0) return `today, ${time}`;
  if (days === 1) return "yesterday";
  if (days < 7) return LONG_DAYS[then.getDay()];
  const short = `${DAYS[then.getDay()]} ${then.getDate()} ${MONTHS[then.getMonth()]}`;
  return then.getFullYear() === now.getFullYear() ? short : `${short} ${then.getFullYear()}`;
}

export function sessionName(info: { title: string | null; opening: string }, fallback: string | null): string {
  if (info.title) return info.title;
  const topic = opening(info.opening)?.topic;
  if (topic) return topic.replace(/^[a-z]/, (letter) => letter.toUpperCase());
  const plain = info.opening.trim();
  if (plain && !saidByTheWindow(plain)) return firstWords(plain, 80);
  return fallback ?? "A session";
}

export function namedByItsWords(info: { title: string | null; opening: string }): boolean {
  const said = info.opening.trim();
  if (said === "" || saidByTheWindow(said)) return false;
  const name = info.title?.trim() ?? "";
  return name === "" || name === said || name === firstWords(said, 80);
}

export function goalName(goal: string | null | undefined, said = ""): string | null {
  const name = firstWords(withoutPaths(goal ?? ""), 80);
  if (name === "") return null;
  const plain = (text: string) => text.replace(/\s+/g, " ").trim().toLowerCase();
  const words = plain(said);
  return words !== "" && (plain(name) === words || words.startsWith(plain(name))) ? null : name;
}

const SEGMENT = String.raw`[^\s/"'“”,;:()<>]+`;
const ABSOLUTE = new RegExp(String.raw`(?:~|/(?:Users|private|var|tmp|Volumes|Library|opt|usr|home))(?:/${SEGMENT})+`, "g");
const IN_FOLDER = new RegExp(String.raw`(?:\.\./)*\b(?:Study|\.magnesium)(?:/${SEGMENT})+`, "g");

export function withoutPaths(text: string): string {
  return text
    .replace(/\bin \.magnesium\/backups\b/g, "as a backup")
    .replace(/\b(?:in|to) Study\/materials(?=[.\s]|$)/g, (found) => `${found.split(" ")[0]} the material`)
    .replace(ABSOLUTE, (path) => path.split("/").pop() ?? path)
    .replace(IN_FOLDER, (path) => path.split("/").pop() ?? path);
}

export function materialTitle(name: string): string {
  const parts = name.split("/").filter(Boolean);
  const file = (parts.pop() ?? name).replace(/\b(week|wk|lecture|lec|chapter|ch|unit|lab|part|session|tutorial|tut|problem|set|hw|homework|quiz|exam|midterm|paper|slides?)(\d)/gi, "$1 $2");
  const bare = file.replace(/\.[a-z0-9]{1,5}$/i, "").replace(/_+/g, " ").replace(/(?<=\p{L})-+|-+(?=\p{L})/gu, " ").replace(/\s+/g, " ").trim() || file;
  const titled = bare.charAt(0).toUpperCase() + bare.slice(1);
  return parts.length > 0 ? `${titled} (${parts.join(", ")})` : titled;
}

export function materialKind(name: string): string {
  const ext = (name.split(".").pop() ?? "").toLowerCase();
  if (ext === "pdf") return "PDF";
  if (["md", "markdown", "txt", "rst", "tex"].includes(ext)) return "notes";
  if (["png", "jpg", "jpeg", "gif", "webp", "heic"].includes(ext)) return "image";
  if (["csv", "json"].includes(ext)) return "data";
  return "";
}
