// The evidence harness: a dev-only hook that lets the real window be driven,
// photographed and timed. It does nothing unless the window was started with
// MAGNESIUM_APP_DRIVE or MAGNESIUM_APP_PERF set (see src-tauri/src/harness.rs),
// and nothing at all in the browser demo.
//
// Every concept calls `ready()` once its first screen has its real content
// painted (not a skeleton). Startup is measured from process start to the
// paint after that call.
//
// Drive ops (scripts/drive.mjs sends them; each is answered {ok, error?, ...}):
//   type {text}             into the focused field (input, textarea, contenteditable)
//   key {key, meta?, shift?, alt?, ctrl?}   on the focused element; an Enter
//                           nobody handled submits as a real one would
//   click {target, by?}     a data-drive name, else exact visible text, else
//                           a substring of visible text, among visible
//                           elements; with by: "drive", the name only
//   focus {target, by?}     the same lookup, then focus
//   wait_idle {timeout_ms, quiet_ms?}  no reply running and nothing pending
//                           (no marked or plan_changed to come) for 1.5 s
//   wait_reply {timeout_ms} the tutor's reply has begun to show (or ended)
//   wait {ms}
//   text                    the page's visible text
//   perf                    measure input latency in the composer now
//   scheme {scheme}         "dark" or "light": sets <html data-scheme>;
//                           "system" takes it off again
//   plan                    the open folder's plan, as core.plan() reads it
//   transcript              every message sent (with its intent), every
//                           reply, and every marked and plan_changed, so far
//   ready                   whether ready() was called, and the startup time
//   errors                  what went wrong on the page so far
//   session                 the session in front: its phase, where the line
//                           is (task or talk), whether it is in the open task,
//                           the open task's words, the files on the line,
//                           where the keyboard is, and the PDF in view
//   document                the PDF in view: the page at its top, how many,
//                           its zoom, how many pages hold a picture, and
//                           where the keyboard is
//   attach {path|paths}     put files on the line of the session in front, as
//                           a drop does (photos and PDFs), for a harness that
//                           cannot drive the Mac's Open panel
//   menu                    the macOS menu bar as data: each menu's title, and
//                           each item's title, id, enabled state and key
//   frame {width?, height?, x?, y?, on_top?}  size and place the window, as
//                           dragging its corner or title bar would, let it
//                           fall behind other windows (on_top false: the
//                           harness keeps a driven window on top), and say
//                           its frame
//   menu_click {path}       choose a menu item, ["File", "Attach…"], exactly as
//                           a click does: the same handler, the same event
//   scroll {target?, to?, by?}  bring a target (as click names it) to the top
//                           of its view; or move the view in front (a page,
//                           or the session's surface) to "top" or "bottom",
//                           or by some pixels, as a wheel would, and say how
//                           far it then is from its foot ({by: 0} only reads)
//   quit
//
// The names a harness clicks by (data-drive): "start" (Home's first row),
// "composer", "answer" and "talk" (the session's line), "home", "new-session",
// "settings", "plan-toggle", "attach", "tab-<n>" (the n-th tab, from 1),
// "hint", "lost", "dont-know", "symbols" (under the answer line).

import { core, isDemo } from "./bridge";
import type { AgentEvent, Intent } from "./types";

type Config = { drive: boolean; perf: boolean; started_ms: number };
type Op = { id: number; op: string; [key: string]: unknown };
type Result = { ok: boolean; error?: string; [key: string]: unknown };
type Latency = { samples: number; median_ms: number; p95_ms: number; max_ms: number };

const QUIET_MS = 1500;
const KEYSTROKES = 60;

let readyCalled = false;
let config: Config | null = null;
let invokeFn: ((cmd: string, args?: Record<string, unknown>) => Promise<unknown>) | null = null;
const perfResult: { startup_ms?: number; ready_ms?: number; input?: Latency; input_error?: string; marks?: Record<string, number> } = {};
// Moments on the way to the first screen, by name, as wall-clock ms: written with the startup, from process start.
const marks: Record<string, number> = {};

/** Note a moment on the way to the first screen, for the perf file. Does nothing without perf on. */
export function mark(name: string): void {
  if (!(name in marks)) marks[name] = Date.now();
}
/** What the window lets the harness ask of the session in front. Set by the page; used only by the drive loop. */
type Probe = { session: () => unknown; attach: (paths: string[]) => boolean };
let probe: Probe | null = null;
export function setProbe(given: Probe): void {
  probe = given;
}

// A measurement running holds the drive loop, so nothing types into the composer meanwhile.
let measuring: Promise<unknown> | null = null;

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!invokeFn) {
    const tauri = await import("@tauri-apps/api/core");
    invokeFn = tauri.invoke;
  }
  return invokeFn(cmd, args) as Promise<T>;
}

/** The paint after now: the next frame, then a task after it. */
function nextPaint(): Promise<number> {
  return new Promise((done) => {
    requestAnimationFrame(() => {
      const channel = new MessageChannel();
      channel.port1.onmessage = () => done(performance.now());
      channel.port2.postMessage(0);
    });
  });
}

const sleep = (ms: number) => new Promise((done) => window.setTimeout(done, ms));

/** Call once, when the first screen's real content is painted. Later calls do nothing. */
export function ready(): void {
  if (readyCalled) return;
  readyCalled = true;
  const calledAt = Date.now();
  void configured.then(async (found) => {
    if (!found?.perf) return;
    await nextPaint();
    perfResult.startup_ms = Date.now() - found.started_ms;
    perfResult.ready_ms = calledAt - found.started_ms;
    perfResult.marks = Object.fromEntries(Object.entries(marks).map(([name, at]) => [name, at - found.started_ms]));
    await writePerf();
    // Input latency, once a composer can be typed into.
    for (;;) {
      if (composer()) break;
      await sleep(250);
    }
    await measureInput();
  });
}

async function writePerf(): Promise<void> {
  await invoke("harness_write", { name: "perf", value: { ...perfResult, measured_at: new Date().toISOString() } }).catch(() => {});
}

// ---------------------------------------------------------------------------
// Elements

type Field = HTMLInputElement | HTMLTextAreaElement;

function visible(element: Element): boolean {
  if (!(element instanceof HTMLElement) && !(element instanceof SVGElement)) return false;
  if (element.closest("[hidden], [aria-hidden='true'], [inert]")) return false;
  const rects = element.getClientRects();
  if (rects.length === 0) return false;
  const style = getComputedStyle(element);
  if (style.visibility === "hidden" || style.display === "none" || Number(style.opacity) === 0) return false;
  return Array.from(rects).some((rect) => rect.width > 0 && rect.height > 0);
}

/** The field typing is timed in: the composer, or else the field for talk, or else the one for an answer. */
function composer(): HTMLElement | null {
  const found = ["composer", "talk", "answer"]
    .map((name) => document.querySelector<HTMLElement>(`[data-drive="${name}"]`))
    .find((element) => element !== null && visible(element));
  if (!found) return null;
  const field = editable(found);
  if (!field) return null;
  if ((field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement) && (field.disabled || field.readOnly)) return null;
  if (field.getAttribute("aria-disabled") === "true") return null;
  return field;
}

/** The text field an element is, or holds. */
function editable(element: Element | null): HTMLElement | null {
  if (!element) return null;
  if (element instanceof HTMLTextAreaElement) return element;
  if (element instanceof HTMLInputElement && !["button", "submit", "checkbox", "radio", "range", "file"].includes(element.type)) return element;
  if (element instanceof HTMLElement && element.isContentEditable) return element;
  return element.querySelector<HTMLElement>("textarea, input:not([type=button]):not([type=submit]):not([type=checkbox]):not([type=radio]), [contenteditable=''], [contenteditable='true']");
}

const collapse = (text: string) => text.replace(/\s+/g, " ").trim();
const CLICKABLE = "button, a[href], [role=button], [role=link], [role=tab], [role=menuitem], [role=option], label, summary, input, select, textarea, [tabindex], [data-drive]";

/**
 * The element a target names: a data-drive name, else exact visible text,
 * else part of it. With `driveOnly`, a data-drive name and nothing else.
 */
function find(target: string, driveOnly = false): HTMLElement | null {
  const named = Array.from(document.querySelectorAll<HTMLElement>(`[data-drive="${CSS.escape(target)}"]`)).filter(visible);
  if (named.length > 0 || driveOnly) return named[0] ?? null;
  const wanted = collapse(target).toLowerCase();
  const all = Array.from(document.body.querySelectorAll<HTMLElement>("*")).filter(
    (element) => !["SCRIPT", "STYLE", "TEMPLATE"].includes(element.tagName) && visible(element),
  );
  const label = (element: HTMLElement) =>
    collapse(element.innerText || element.getAttribute("aria-label") || element.getAttribute("title") || (element as HTMLInputElement).value || "").toLowerCase();
  // The innermost elements that match: no child of theirs matches too.
  const innermost = (match: (element: HTMLElement) => boolean) => {
    const hits = all.filter(match);
    return hits.filter((hit) => !hits.some((other) => other !== hit && hit.contains(other)));
  };
  for (const match of [(element: HTMLElement) => label(element) === wanted, (element: HTMLElement) => label(element).includes(wanted)]) {
    const hits = innermost(match);
    if (hits.length === 0) continue;
    // Prefer something that can be clicked.
    const clickable = hits.map((hit) => hit.closest<HTMLElement>(CLICKABLE) ?? hit);
    return clickable.find((element) => element.matches(CLICKABLE)) ?? clickable[0];
  }
  return null;
}

/** Where the keyboard is: the element's drive name, role or class. */
function focusName(): string | null {
  const held = document.activeElement as HTMLElement | null;
  return held && held !== document.body ? (held.dataset.drive ?? held.dataset.role ?? held.className) : null;
}

/** The PDF in view, as a reader sees it: the page at its top, how many, how large it is drawn, and how many pages hold a picture now. */
function pdfInView(): { page: number; of: number; zoom: string | null; drawn: number } | null {
  const pdf = Array.from(document.querySelectorAll<HTMLElement>(".doc-pdf")).find(visible);
  if (!pdf) return null;
  const pages = Array.from(pdf.querySelectorAll<HTMLElement>(".doc-page"));
  const page = pages.filter((each) => each.offsetTop <= pdf.scrollTop + 24).length || 1;
  const zoom = pdf.closest(".panel.side")?.querySelector<HTMLElement>("[data-drive=zoom-fit]")?.innerText ?? null;
  return { page, of: pages.length, zoom, drawn: pdf.querySelectorAll(".doc-page canvas").length };
}

function focusable(element: HTMLElement): HTMLElement {
  if (element.matches("button, a[href], input, select, textarea, [tabindex], [contenteditable='true'], [contenteditable='']")) return element;
  return element.querySelector<HTMLElement>("button, a[href], input, select, textarea, [tabindex], [contenteditable='true'], [contenteditable='']") ?? element;
}

function tabbables(): HTMLElement[] {
  return Array.from(
    document.querySelectorAll<HTMLElement>("button, a[href], input, select, textarea, [tabindex], [contenteditable='true'], [contenteditable='']"),
  ).filter((element) => element.tabIndex >= 0 && !(element as HTMLButtonElement).disabled && visible(element));
}

// ---------------------------------------------------------------------------
// Typing

/** Set a React-controlled field's value as the browser would, so React sees it. */
function setValue(field: Field, value: string): void {
  const proto = field instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
  setter?.call(field, value);
  field.dispatchEvent(new Event("input", { bubbles: true }));
}

/** Insert text at the caret of the focused field. */
function insert(target: HTMLElement, text: string): void {
  target.focus();
  // A real edit when the engine allows it: React and contenteditable both see it.
  if (document.execCommand("insertText", false, text)) return;
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
    const start = target.selectionStart ?? target.value.length;
    const end = target.selectionEnd ?? target.value.length;
    setValue(target, target.value.slice(0, start) + text + target.value.slice(end));
    target.setSelectionRange(start + text.length, start + text.length);
  } else {
    target.textContent = (target.textContent ?? "") + text;
    target.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: text }));
  }
}

function keyEvent(type: string, key: string, mods: { meta?: boolean; shift?: boolean; alt?: boolean; ctrl?: boolean }): KeyboardEvent {
  const code = key.length === 1 ? (/[a-z]/i.test(key) ? `Key${key.toUpperCase()}` : /\d/.test(key) ? `Digit${key}` : "") : key;
  return new KeyboardEvent(type, {
    key,
    code,
    bubbles: true,
    cancelable: true,
    composed: true,
    metaKey: !!mods.meta,
    shiftKey: !!mods.shift,
    altKey: !!mods.alt,
    ctrlKey: !!mods.ctrl,
  });
}

/** A key on the focused element, and what the browser would do with it if nobody did. */
function press(key: string, mods: { meta?: boolean; shift?: boolean; alt?: boolean; ctrl?: boolean }): void {
  const target = (document.activeElement as HTMLElement | null) ?? document.body;
  const down = keyEvent("keydown", key, mods);
  const handled = !target.dispatchEvent(down);
  if (!handled && key.length === 1 && !mods.meta && !mods.ctrl) {
    const field = editable(target);
    if (field === target) insert(target, key);
  }
  if (!handled) {
    if (key === "Enter") {
      if (target instanceof HTMLButtonElement || target.matches("a[href], [role=button], summary")) target.click();
      else if (target instanceof HTMLInputElement && target.form) target.form.requestSubmit();
      else if ((target instanceof HTMLTextAreaElement || target.isContentEditable) && !mods.meta && !mods.ctrl) insert(target, "\n");
    } else if (key === " " && (target instanceof HTMLButtonElement || target.matches("[role=button], summary"))) {
      target.click();
    } else if (key === "Tab") {
      const list = tabbables();
      const at = list.indexOf(target);
      const next = list[(at + (mods.shift ? -1 : 1) + list.length) % list.length];
      next?.focus();
    }
  }
  target.dispatchEvent(keyEvent("keyup", key, mods));
}

function click(element: HTMLElement): void {
  element.scrollIntoView({ block: "nearest" });
  const rect = element.getBoundingClientRect();
  const at = { bubbles: true, cancelable: true, composed: true, clientX: rect.left + rect.width / 2, clientY: rect.top + rect.height / 2, button: 0 };
  element.dispatchEvent(new PointerEvent("pointerdown", { ...at, pointerType: "mouse", isPrimary: true }));
  element.dispatchEvent(new MouseEvent("mousedown", at));
  if (element.matches("button, a[href], input, select, textarea, [tabindex]")) element.focus();
  element.dispatchEvent(new PointerEvent("pointerup", { ...at, pointerType: "mouse", isPrimary: true }));
  element.dispatchEvent(new MouseEvent("mouseup", at));
  element.click();
}

// ---------------------------------------------------------------------------
// Input latency

function percentile(sorted: number[], p: number): number {
  const at = Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1));
  return sorted[at];
}

/** Type 60 keystrokes into the composer, each timed to the next paint, then put its text back. */
async function measureInput(): Promise<Latency | { error: string }> {
  if (measuring) await measuring;
  const run = (async () => {
    const field = composer();
    if (!field) {
      perfResult.input_error = "no composer to type into";
      await writePerf();
      return { error: perfResult.input_error };
    }
    const isField = field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement;
    const before = isField ? (field as Field).value : field.innerHTML;
    const typed = isField ? (field as Field).value : field.textContent ?? "";
    const focusedBefore = document.activeElement as HTMLElement | null;
    field.focus();
    const samples: number[] = [];
    let text = typed;
    for (let i = 0; i < KEYSTROKES; i++) {
      const key = "abcdefghijklmnopqrstuvwxyz"[i % 26];
      await nextPaint();
      const start = performance.now();
      field.dispatchEvent(keyEvent("keydown", key, {}));
      text += key;
      if (isField) setValue(field as Field, text);
      else {
        field.textContent = text;
        field.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: key }));
      }
      field.dispatchEvent(keyEvent("keyup", key, {}));
      const painted = await nextPaint();
      samples.push(painted - start);
    }
    if (isField) setValue(field as Field, before);
    else {
      field.innerHTML = before;
      field.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "deleteContent" }));
    }
    focusedBefore?.focus();
    const sorted = [...samples].sort((a, b) => a - b);
    const round = (n: number) => Math.round(n * 100) / 100;
    const latency: Latency = {
      samples: samples.length,
      median_ms: round(percentile(sorted, 50)),
      p95_ms: round(percentile(sorted, 95)),
      max_ms: round(sorted[sorted.length - 1]),
    };
    perfResult.input = latency;
    delete perfResult.input_error;
    await writePerf();
    return latency;
  })();
  measuring = run;
  try {
    return await run;
  } finally {
    measuring = null;
  }
}

// ---------------------------------------------------------------------------
// Waiting for the tutor

let running = false;
let lastEventAt = 0;
let deltas = 0;
let turnsEnded = 0;

type Entry =
  | { at: string; kind: "send" | "new_lesson" | "shown"; text: string; intent: Intent | null; sent?: string; from?: string }
  | { at: string; kind: "reply"; text: string; ok: boolean; interrupted: boolean; error: string | null }
  | { at: string; kind: "marked"; mark: unknown }
  | { at: string; kind: "plan_changed"; what: string[] }
  | { at: string; kind: "notice"; text: string };
const transcript: Entry[] = [];
let replyBlocks: string[] = [];
let replyStreaming = "";

function note(event: AgentEvent): void {
  const at = new Date().toISOString();
  switch (event.type) {
    case "turn_started":
      replyBlocks = [];
      replyStreaming = "";
      break;
    case "text_delta":
      replyStreaming += event.text;
      break;
    case "text_done":
      replyBlocks.push(event.text);
      replyStreaming = "";
      break;
    case "turn_ended": {
      const text = [...replyBlocks, replyStreaming].filter((block) => block.trim() !== "").join("\n\n");
      transcript.push({ at, kind: "reply", text, ok: event.ok, interrupted: event.interrupted, error: event.error });
      replyBlocks = [];
      replyStreaming = "";
      break;
    }
    case "marked":
      transcript.push({ at, kind: "marked", mark: event.mark });
      break;
    case "plan_changed":
      transcript.push({ at, kind: "plan_changed", what: event.what });
      break;
    case "notice":
      transcript.push({ at, kind: "notice", text: event.text });
      break;
  }
}

async function listen(): Promise<void> {
  const c = await core();
  // Every message the screens send is noted with its intent, for the
  // scenario runner's transcript. The screens call these on the same object.
  const send = c.send.bind(c);
  const sendNewLesson = c.sendNewLesson.bind(c);
  const sendShown = c.sendShown.bind(c);
  c.send = (text: string, intent?: Intent) => {
    transcript.push({ at: new Date().toISOString(), kind: "send", text, intent: intent ?? null });
    return send(text, intent);
  };
  c.sendNewLesson = (goal: string) => {
    transcript.push({ at: new Date().toISOString(), kind: "new_lesson", text: goal, intent: null, from: new Error().stack?.split("\n").slice(1, 9).join(" | ") });
    return sendNewLesson(goal);
  };
  c.sendShown = (shown: string, sent: string) => {
    transcript.push({ at: new Date().toISOString(), kind: "shown", text: shown, intent: null, sent });
    return sendShown(shown, sent);
  };
  await c.onEvent((event) => {
    lastEventAt = Date.now();
    if (event.type === "turn_started") running = true;
    if (event.type === "text_delta" || event.type === "text_done") deltas += 1;
    if (event.type === "turn_ended" || event.type === "exited") {
      running = false;
      turnsEnded += 1;
    }
    note(event);
  });
}

/** Resolves once the tutor's reply shows a few pieces, or a turn ends. */
async function waitReply(timeoutMs: number): Promise<Result> {
  const start = Date.now();
  const from = deltas;
  const ended = turnsEnded;
  while (Date.now() - start < timeoutMs) {
    if (deltas - from >= 3 || turnsEnded > ended) {
      await nextPaint();
      return { ok: true, waited_ms: Date.now() - start, ended: turnsEnded > ended };
    }
    await sleep(20);
  }
  return { ok: false, error: `no reply within ${timeoutMs} ms`, waited_ms: Date.now() - start };
}

async function waitIdle(timeoutMs: number, quietMs = QUIET_MS): Promise<Result> {
  const start = Date.now();
  let quietSince = 0;
  while (Date.now() - start < timeoutMs) {
    const idle = !running && (await invoke<boolean>("harness_idle").catch(() => true));
    const now = Date.now();
    if (idle && now - Math.max(lastEventAt, start) >= quietMs) {
      quietSince ||= now;
      if (now - quietSince >= 200) return { ok: true, waited_ms: now - start };
    } else quietSince = 0;
    await sleep(100);
  }
  return { ok: false, error: `still busy after ${timeoutMs} ms`, waited_ms: Date.now() - start };
}

// ---------------------------------------------------------------------------
// The drive loop

async function run(op: Op): Promise<Result> {
  switch (op.op) {
    case "type": {
      const target = document.activeElement as HTMLElement | null;
      // With nothing focused the page itself is the active element: the line in view takes the words, never the
      // first field in the page, which may be another tab's.
      const field = target && target !== document.body ? (editable(target) ?? composer()) : composer();
      if (!target || !field) return { ok: false, error: "no text field has focus" };
      // "replace": the words take the place of all that is in the field, as after ⌘A.
      if (op.replace && (field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement)) field.select();
      insert(field, String(op.text ?? ""));
      return { ok: true };
    }
    case "key": {
      press(String(op.key ?? ""), op as { meta?: boolean; shift?: boolean; alt?: boolean; ctrl?: boolean });
      return { ok: true, focused: describe(document.activeElement) };
    }
    case "click": {
      const element = find(String(op.target ?? ""), op.by === "drive");
      if (!element) return { ok: false, error: `nothing visible is named ${JSON.stringify(op.target)}` };
      click(element);
      return { ok: true, clicked: describe(element) };
    }
    case "focus": {
      const element = find(String(op.target ?? ""), op.by === "drive");
      if (!element) return { ok: false, error: `nothing visible is named ${JSON.stringify(op.target)}` };
      const target = editable(element) ?? focusable(element);
      target.focus();
      return { ok: document.activeElement === target, focused: describe(document.activeElement) };
    }
    case "wait_idle":
      return waitIdle(Number(op.timeout_ms ?? 120_000), op.quiet_ms === undefined ? QUIET_MS : Number(op.quiet_ms));
    case "wait_reply":
      return waitReply(Number(op.timeout_ms ?? 120_000));
    case "plan": {
      const c = await core();
      try {
        return { ok: true, plan: await c.plan() };
      } catch (error) {
        return { ok: false, error: String(error) };
      }
    }
    case "transcript":
      return { ok: true, transcript };
    case "wait":
      await sleep(Number(op.ms ?? 0));
      return { ok: true };
    case "text":
      return { ok: true, text: document.body.innerText };
    case "perf": {
      const measured = await measureInput();
      return "error" in measured ? { ok: false, error: measured.error } : { ok: true, ...measured, startup_ms: perfResult.startup_ms };
    }
    case "scheme": {
      const scheme = String(op.scheme ?? op.value ?? "");
      if (scheme === "system") delete document.documentElement.dataset.scheme;
      else if (scheme === "dark" || scheme === "light") document.documentElement.dataset.scheme = scheme;
      else return { ok: false, error: "scheme is dark, light or system" };
      await nextPaint();
      return { ok: true };
    }
    case "ready":
      return { ok: true, ready: readyCalled, startup_ms: perfResult.startup_ms };
    case "errors":
      return { ok: true, errors: [...(window.__mgErrors ?? [])] };
    case "session": {
      const found = probe?.session() ?? null;
      if (!found) return { ok: false, error: "no session is in front" };
      const files = Array.from(document.querySelectorAll(".session.in-front .attached .file")).map((file) => collapse((file as HTMLElement).innerText).replace(/\s*×$/, ""));
      return { ok: true, ...(found as object), files, focus: focusName(), document: pdfInView() };
    }
    case "document": {
      const pdf = pdfInView();
      return pdf ? { ok: true, ...pdf, focus: focusName() } : { ok: false, error: "no PDF is in view" };
    }
    case "attach": {
      const paths = Array.isArray(op.paths) ? op.paths.map(String) : op.path ? [String(op.path)] : [];
      if (paths.length === 0) return { ok: false, error: "name a path or paths" };
      if (!probe?.attach(paths)) return { ok: false, error: "no session is in front" };
      // The files are handed in to the study folder before they show on the line.
      for (let i = 0; i < 50 && document.querySelectorAll(".session.in-front .attached .file").length === 0; i++) await sleep(100);
      await nextPaint();
      return { ok: true };
    }
    case "scroll": {
      if (op.target !== undefined) {
        const element = find(String(op.target), op.by === "drive");
        if (!element) return { ok: false, error: `nothing visible is named ${JSON.stringify(op.target)}` };
        element.scrollIntoView({ block: "start" });
      } else {
        const view = Array.from(document.querySelectorAll<HTMLElement>(".session.in-front .surface, .views > .view")).find(visible);
        if (!view) return { ok: false, error: "no view is in front" };
        if (op.to === "top") view.scrollTop = 0;
        else if (op.to === "bottom") view.scrollTop = view.scrollHeight;
        else view.scrollTop += Number(op.by ?? 0);
        await nextPaint();
        // Where the view is now, from its foot: { by: 0 } reads it without moving it.
        return { ok: true, from_foot: Math.round(view.scrollHeight - view.clientHeight - view.scrollTop) };
      }
      await nextPaint();
      return { ok: true };
    }
    case "menu":
      try {
        return { ok: true, menu: await invoke<unknown>("harness_menu") };
      } catch (error) {
        return { ok: false, error: String(error) };
      }
    case "frame": {
      // The window's size and place, set or read; the page then has a frame to refit to.
      const num = (value: unknown) => (typeof value === "number" ? value : undefined);
      try {
        const onTop = typeof op.on_top === "boolean" ? op.on_top : undefined;
        const frame = await invoke<unknown>("harness_frame", { width: num(op.width), height: num(op.height), x: num(op.x), y: num(op.y), onTop });
        await sleep(300);
        return { ok: true, frame, inner: { width: window.innerWidth, height: window.innerHeight } };
      } catch (error) {
        return { ok: false, error: String(error) };
      }
    }
    case "menu_click": {
      const path = Array.isArray(op.path) ? op.path.map(String) : [];
      try {
        const id = await invoke<string>("harness_menu_click", { path });
        // The page has the menu's event before the answer goes back.
        await nextPaint();
        return { ok: true, id };
      } catch (error) {
        return { ok: false, error: String(error) };
      }
    }
    default:
      return { ok: false, error: `no such op: ${op.op}` };
  }
}

function describe(element: Element | null): string | null {
  if (!element) return null;
  const drive = element.getAttribute("data-drive");
  const text = collapse((element as HTMLElement).innerText ?? "").slice(0, 60);
  return `${element.tagName.toLowerCase()}${drive ? `[data-drive=${drive}]` : ""}${text ? ` "${text}"` : ""}`;
}

async function driveLoop(): Promise<void> {
  for (;;) {
    const op = await invoke<Op | null>("harness_next").catch(() => null);
    if (!op) {
      await sleep(100);
      continue;
    }
    if (op.op === "quit") {
      await invoke("harness_done", { id: op.id, result: { ok: true } }).catch(() => {});
      await invoke("harness_quit");
      return;
    }
    if (measuring) await measuring;
    let result: Result;
    try {
      result = await run(op);
    } catch (error) {
      result = { ok: false, error: String(error) };
    }
    await invoke("harness_done", { id: op.id, result }).catch(() => {});
  }
}

const configured: Promise<Config | null> = isDemo
  ? Promise.resolve(null)
  : invoke<Config>("harness_config")
      .then((found) => {
        config = found;
        if (found.drive || found.perf) void listen();
        if (found.drive) void driveLoop();
        return found;
      })
      .catch(() => null);

/** Whether the harness is on: for a screen that wants to skip an animation under test. */
export function harnessOn(): boolean {
  return !!config && (config.drive || config.perf);
}
