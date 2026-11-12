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
//   click {target}          a data-drive name, else exact visible text, else
//                           a substring of visible text, among visible elements
//   focus {target}          the same lookup, then focus
//   wait_idle {timeout_ms}  no reply running and nothing pending for 1.5 s
//   wait {ms}
//   text                    the page's visible text
//   perf                    measure input latency in the composer now
//   scheme {scheme}         "dark" or "light": sets <html data-scheme>
//   quit

import { core, isDemo } from "./bridge";

type Config = { drive: boolean; perf: boolean; started_ms: number };
type Op = { id: number; op: string; [key: string]: unknown };
type Result = { ok: boolean; error?: string; [key: string]: unknown };
type Latency = { samples: number; median_ms: number; p95_ms: number; max_ms: number };

const QUIET_MS = 1500;
const KEYSTROKES = 60;

let readyCalled = false;
let config: Config | null = null;
let invokeFn: ((cmd: string, args?: Record<string, unknown>) => Promise<unknown>) | null = null;
const perfResult: { startup_ms?: number; ready_ms?: number; input?: Latency; input_error?: string } = {};
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

function composer(): HTMLElement | null {
  const found = document.querySelector<HTMLElement>('[data-drive="composer"]');
  if (!found || !visible(found)) return null;
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

/** The element a target names: a data-drive name, else exact visible text, else part of it. */
function find(target: string): HTMLElement | null {
  const named = Array.from(document.querySelectorAll<HTMLElement>(`[data-drive="${CSS.escape(target)}"]`)).filter(visible);
  if (named.length > 0) return named[0];
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

async function listen(): Promise<void> {
  const c = await core();
  await c.onEvent((event) => {
    lastEventAt = Date.now();
    if (event.type === "turn_started") running = true;
    if (event.type === "turn_ended" || event.type === "exited") running = false;
  });
}

async function waitIdle(timeoutMs: number): Promise<Result> {
  const start = Date.now();
  let quietSince = 0;
  while (Date.now() - start < timeoutMs) {
    const idle = !running && (await invoke<boolean>("harness_idle").catch(() => true));
    const now = Date.now();
    if (idle && now - lastEventAt >= QUIET_MS) {
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
      const field = editable(target);
      if (!target || !field) return { ok: false, error: "no text field has focus" };
      insert(field, String(op.text ?? ""));
      return { ok: true };
    }
    case "key": {
      press(String(op.key ?? ""), op as { meta?: boolean; shift?: boolean; alt?: boolean; ctrl?: boolean });
      return { ok: true, focused: describe(document.activeElement) };
    }
    case "click": {
      const element = find(String(op.target ?? ""));
      if (!element) return { ok: false, error: `nothing visible is named ${JSON.stringify(op.target)}` };
      click(element);
      return { ok: true, clicked: describe(element) };
    }
    case "focus": {
      const element = find(String(op.target ?? ""));
      if (!element) return { ok: false, error: `nothing visible is named ${JSON.stringify(op.target)}` };
      const target = editable(element) ?? focusable(element);
      target.focus();
      return { ok: document.activeElement === target, focused: describe(document.activeElement) };
    }
    case "wait_idle":
      return waitIdle(Number(op.timeout_ms ?? 120_000));
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
      if (scheme !== "dark" && scheme !== "light") return { ok: false, error: "scheme is dark or light" };
      document.documentElement.dataset.scheme = scheme;
      await nextPaint();
      return { ok: true };
    }
    case "ready":
      return { ok: true, ready: readyCalled, startup_ms: perfResult.startup_ms };
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
