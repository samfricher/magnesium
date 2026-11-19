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
const marks: Record<string, number> = {};

export function mark(name: string): void {
  if (!(name in marks)) marks[name] = Date.now();
}
type Probe = { session: () => unknown; attach: (paths: string[]) => boolean };
let probe: Probe | null = null;
export function setProbe(given: Probe): void {
  probe = given;
}

let measuring: Promise<unknown> | null = null;

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!invokeFn) {
    const tauri = await import("@tauri-apps/api/core");
    invokeFn = tauri.invoke;
  }
  return invokeFn(cmd, args) as Promise<T>;
}

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

type Field = HTMLInputElement | HTMLTextAreaElement;

function visible(element: Element): boolean {
  if (!(element instanceof HTMLElement) && !(element instanceof SVGElement)) return false;
  if (element.closest("[hidden], [aria-hidden='true'], [inert]")) return false;
  if (element.checkVisibility?.() === false) return false;
  const rects = element.getClientRects();
  if (rects.length === 0) return false;
  const style = getComputedStyle(element);
  if (style.visibility === "hidden" || style.display === "none" || Number(style.opacity) === 0) return false;
  return Array.from(rects).some((rect) => rect.width > 0 && rect.height > 0);
}

function composer(): HTMLElement | null {
  const found = ["composer", "talk", "answer"]
    .map((name) => Array.from(document.querySelectorAll<HTMLElement>(`[data-drive="${name}"]`)).find(visible) ?? null)
    .find((element) => element !== null);
  if (!found) return null;
  const field = editable(found);
  if (!field) return null;
  if ((field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement) && (field.disabled || field.readOnly)) return null;
  if (field.getAttribute("aria-disabled") === "true") return null;
  return field;
}

function editable(element: Element | null): HTMLElement | null {
  if (!element) return null;
  if (element instanceof HTMLTextAreaElement) return element;
  if (element instanceof HTMLInputElement && !["button", "submit", "checkbox", "radio", "range", "file"].includes(element.type)) return element;
  if (element instanceof HTMLElement && element.isContentEditable) return element;
  return element.querySelector<HTMLElement>("textarea, input:not([type=button]):not([type=submit]):not([type=checkbox]):not([type=radio]), [contenteditable=''], [contenteditable='true']");
}

const collapse = (text: string) => text.replace(/\s+/g, " ").trim();
const CLICKABLE = "button, a[href], [role=button], [role=link], [role=tab], [role=menuitem], [role=option], label, summary, input, select, textarea, [tabindex], [data-drive]";

function find(target: string, driveOnly = false): HTMLElement | null {
  const named = Array.from(document.querySelectorAll<HTMLElement>(`[data-drive="${CSS.escape(target)}"]`)).filter(visible);
  if (named.length > 0 || driveOnly) return named[0] ?? null;
  const wanted = collapse(target).toLowerCase();
  const all = Array.from(document.body.querySelectorAll<HTMLElement>("*")).filter(
    (element) => !["SCRIPT", "STYLE", "TEMPLATE"].includes(element.tagName) && visible(element),
  );
  const label = (element: HTMLElement) =>
    collapse(element.innerText || element.getAttribute("aria-label") || element.getAttribute("title") || (element as HTMLInputElement).value || "").toLowerCase();
  const innermost = (match: (element: HTMLElement) => boolean) => {
    const hits = all.filter(match);
    return hits.filter((hit) => !hits.some((other) => other !== hit && hit.contains(other)));
  };
  for (const match of [(element: HTMLElement) => label(element) === wanted, (element: HTMLElement) => label(element).includes(wanted)]) {
    const hits = innermost(match);
    if (hits.length === 0) continue;
    const clickable = hits.map((hit) => hit.closest<HTMLElement>(CLICKABLE) ?? hit);
    return clickable.find((element) => element.matches(CLICKABLE)) ?? clickable[0];
  }
  return null;
}

function focusName(): string | null {
  const held = document.activeElement as HTMLElement | null;
  return held && held !== document.body ? (held.dataset.drive ?? held.dataset.role ?? held.className) : null;
}

function pdfInView(): { page: number; of: number; zoom: string | null; drawn: number; inkRead: string | null } | null {
  const pdf = Array.from(document.querySelectorAll<HTMLElement>(".doc-pdf")).find(visible);
  if (!pdf) return null;
  const pages = Array.from(pdf.querySelectorAll<HTMLElement>(".doc-page"));
  const page = pages.filter((each) => each.offsetTop <= pdf.scrollTop + 24).length || 1;
  const zoom = pdf.closest(".panel.side")?.querySelector<HTMLElement>("[data-drive=zoom-fit]")?.innerText ?? null;
  return { page, of: pages.length, zoom, drawn: pdf.querySelectorAll(".doc-page canvas").length, inkRead: pdf.getAttribute("data-ink-read") };
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

function setValue(field: Field, value: string): void {
  const proto = field instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
  setter?.call(field, value);
  field.dispatchEvent(new Event("input", { bubbles: true }));
}

function insert(target: HTMLElement, text: string): void {
  target.focus();
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

function percentile(sorted: number[], p: number): number {
  const at = Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1));
  return sorted[at];
}

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

let running = false;
let lastEventAt = 0;
let deltas = 0;
let turnsEnded = 0;
let stepsBegun = 0;
let marksSeen = 0;

type Entry =
  | { at: string; kind: "send" | "new_lesson" | "shown"; text: string; intent: Intent | null; sent?: string; from?: string }
  | { at: string; kind: "reply"; text: string; ok: boolean; interrupted: boolean; error: string | null }
  | { at: string; kind: "marked"; mark: unknown }
  | { at: string; kind: "plan_changed"; what: string[] }
  | { at: string; kind: "notice"; text: string }
  | { at: string; kind: "closing"; lines: string[] }
  | { at: string; kind: "step"; n: number; of: number; name: string };
const transcript: Entry[] = [];
let replyBlocks: string[] = [];
let replyStreaming = "";

const heard: { type: string; at: number; doing?: string | null }[] = [];

function note(event: AgentEvent): void {
  if (event.type !== "text_delta") heard.push({ type: event.type, at: performance.now(), ...(event.type === "working" ? { doing: event.doing } : {}) });
  if (heard.length > 400) heard.splice(0, heard.length - 400);
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
    case "closing":
      transcript.push({ at, kind: "closing", lines: event.lines });
      break;
    case "step_begun":
      transcript.push({ at, kind: "step", n: event.n, of: event.of, name: event.name });
      break;
  }
}

async function listen(): Promise<void> {
  const c = await core();
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
    if (event.type === "step_begun") stepsBegun += 1;
    if (event.type === "marked") marksSeen += 1;
    note(event);
  });
}

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

async function run(op: Op): Promise<Result> {
  switch (op.op) {
    case "type": {
      const target = document.activeElement as HTMLElement | null;
      const field = target && target !== document.body ? (editable(target) ?? composer()) : composer();
      if (!target || !field) return { ok: false, error: "no text field has focus" };
      if (op.replace && (field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement)) field.select();
      insert(field, String(op.text ?? ""));
      return { ok: true };
    }
    case "key": {
      press(String(op.key ?? ""), op as { meta?: boolean; shift?: boolean; alt?: boolean; ctrl?: boolean });
      return { ok: true, focused: describe(document.activeElement) };
    }
    case "switch": {
      const element = find(String(op.target ?? ""), op.by === "drive");
      if (!element) return { ok: false, error: `nothing visible is named ${JSON.stringify(op.target)}` };
      const ms = Number(op.ms ?? 800);
      const look = () => {
        const view = Array.from(document.querySelectorAll<HTMLElement>(".session.in-front, .views > .view")).find(visible);
        if (!view) return { words: 0, text: "", lines: [] as string[] };
        const lines = Array.from(view.querySelectorAll<HTMLElement>("h1, h2, h3, p, button, li, textarea"))
          .filter(visible)
          .slice(0, 8)
          .map((line) => `${collapse(line.innerText || line.getAttribute("aria-label") || "").slice(0, 24)} @${Math.round(line.getBoundingClientRect().top)}`);
        const text = collapse(view.innerText);
        return { words: text.length, text, lines };
      };
      const standsAt = (line: string) => line.slice(line.lastIndexOf(" @"));
      const parted = (a: string, b: string) => {
        let at = 0;
        while (at < a.length && at < b.length && a[at] === b[at]) at++;
        const from = Math.max(0, at - 20);
        return { was: a.slice(from, at + 30), now: b.slice(from, at + 30) };
      };
      const before = look();
      const started = performance.now();
      click(element);
      const frames: { at: number; words: number; text: string; lines: string[]; focus: string }[] = [];
      await new Promise<void>((done) => {
        const tick = (now: number) => {
          const seen = { ...look(), focus: describe(document.activeElement) ?? "" };
          const last = frames[frames.length - 1];
          if (!last || last.text !== seen.text || last.lines.join("|") !== seen.lines.join("|") || last.focus !== seen.focus) frames.push({ at: Math.round(now - started), ...seen });
          if (now - started < ms) requestAnimationFrame(tick);
          else done();
        };
        requestAnimationFrame(tick);
      });
      const brief = (frame: (typeof frames)[number], previous?: (typeof frames)[number]) => ({
        at: frame.at,
        words: frame.words,
        lines: frame.lines,
        focus: frame.focus,
        moved: previous ? frame.lines.some((line, n) => previous.lines[n] !== undefined && standsAt(line) !== standsAt(previous.lines[n])) || frame.lines.length !== previous.lines.length : false,
        ...(previous && previous.text !== frame.text ? { text: parted(previous.text, frame.text) } : {}),
      });
      const unchanged = (frame: (typeof frames)[number]) => frame.lines.join("|") === before.lines.join("|");
      const answered = frames.findIndex((frame) => !unchanged(frame));
      const shown = answered === -1 ? frames : frames.slice(answered);
      return {
        ok: true,
        before: before.lines.slice(0, 3),
        answered_at: answered === -1 ? null : frames[answered].at,
        first: shown[0] ? brief(shown[0]) : null,
        changes_after_first: shown.slice(1).map((frame, n) => brief(frame, shown[n])),
        focused: describe(document.activeElement),
      };
    }
    case "click": {
      const element = find(String(op.target ?? ""), op.by === "drive");
      if (!element) return { ok: false, error: `nothing visible is named ${JSON.stringify(op.target)}` };
      if (!op.timed) {
        click(element);
        return { ok: true, clicked: describe(element) };
      }
      await nextPaint();
      const started = performance.now();
      if (op.bare) element.click();
      else click(element);
      const painted = await nextPaint();
      const gaps: { at: number; ms: number }[] = [];
      await new Promise<void>((done) => {
        let last = painted;
        const tick = (now: number) => {
          gaps.push({ at: now, ms: now - last });
          last = now;
          if (now - started < Number(op.timed)) requestAnimationFrame(tick);
          else done();
        };
        requestAnimationFrame(tick);
      });
      const slow = gaps.filter((gap) => gap.ms > 50);
      return {
        ok: true,
        clicked: describe(element),
        paint_ms: Math.round(painted - started),
        longest_gap_ms: Math.round(Math.max(0, ...gaps.map((gap) => gap.ms))),
        slow_frames: slow.map((gap) => Math.round(gap.ms)),
        settled_ms: Math.round((slow.length > 0 ? slow[slow.length - 1].at : painted) - started),
      };
    }
    case "focus": {
      const element = find(String(op.target ?? ""), op.by === "drive");
      if (!element) return { ok: false, error: `nothing visible is named ${JSON.stringify(op.target)}` };
      const target = editable(element) ?? focusable(element);
      target.focus();
      return { ok: document.activeElement === target, focused: describe(document.activeElement) };
    }
    case "slide": {
      const element = find(String(op.target ?? ""), op.by === "drive");
      if (!(element instanceof HTMLInputElement) || element.type !== "range") return { ok: false, error: `no slider is named ${JSON.stringify(op.target)}` };
      setValue(element, String(op.value ?? ""));
      return { ok: true, value: Number(element.value) };
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
    case "text": {
      const root = document.documentElement;
      const was = root.hasAttribute("data-whole");
      root.setAttribute("data-whole", "");
      const text = document.body.innerText;
      if (!was) root.removeAttribute("data-whole");
      return { ok: true, text };
    }
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
      const heads = Array.from(document.querySelectorAll<HTMLElement>(".session.in-front .step-head"))
        .filter((head) => head.getClientRects().length > 0)
        .map((head) => collapse(head.textContent ?? "").replace(/^\d+\s*/, ""));
      const count = document.querySelector<HTMLElement>(".session.in-front .top-edge .top-count");
      const markSaid = document.querySelector<HTMLElement>('.session.in-front [data-role="mark-said"]')?.textContent ?? null;
      const passDays = Array.from(document.querySelectorAll<HTMLElement>('.session.in-front [data-role="pass-days"]')).map((days) => ({ day: Number(days.dataset.day), arriving: days.querySelector(".arriving") !== null, hung: !days.classList.contains("inline"), left: Math.round(days.getBoundingClientRect().left) }));
      return { ok: true, ...(found as object), files, heads, top_count: count ? collapse(count.textContent ?? "") : null, focus: focusName(), key_window: document.hasFocus(), document: pdfInView(), mark_said: markSaid, pass_days: passDays };
    }
    case "smooth": {
      const ms = Number(op.ms ?? 5000);
      const by = Number(op.scroll_by ?? 0);
      const every = Number(op.type_every ?? 0);
      const surface = document.querySelector<HTMLElement>(".session.in-front .surface");
      const line = composer();
      const started = performance.now();
      let last = started;
      let typed = 0;
      let nextType = started + every;
      let direction = 1;
      const gaps: number[] = [];
      await new Promise<void>((done) => {
        const tick = (now: number) => {
          gaps.push(now - last);
          last = now;
          if (surface && by > 0) {
            if (surface.scrollTop + surface.clientHeight >= surface.scrollHeight - 2) direction = -1;
            if (surface.scrollTop <= 0) direction = 1;
            surface.scrollTop += by * direction;
          }
          if (line && every > 0 && now >= nextType) {
            insert(line, "a");
            typed += 1;
            nextType = now + every;
          }
          if (now - started < ms) requestAnimationFrame(tick);
          else done();
        };
        requestAnimationFrame(tick);
      });
      const sorted = [...gaps].sort((a, b) => a - b);
      return {
        ok: true,
        frames: gaps.length,
        longestGapMs: Math.round(sorted[sorted.length - 1] ?? 0),
        medianGapMs: Math.round(sorted[Math.floor(sorted.length / 2)] ?? 0),
        over50: gaps.filter((gap) => gap > 50).length,
        over100: gaps.filter((gap) => gap > 100).length,
        typed,
        startedAt: Math.round(started),
        pdf: pdfInView(),
      };
    }
    case "watch": {
      const ms = Number(op.ms ?? 5000);
      const shown: { text: string; ms: number }[] = [];
      const started = performance.now();
      let current: string | null = null;
      let since = started;
      await new Promise<void>((done) => {
        const tick = (now: number) => {
          const parked = document.querySelector<HTMLElement>(".session.in-front .foot .parked");
          const text = parked ? `${parked.classList.contains("spent") ? "(spent) " : ""}${collapse(parked.innerText)}` : null;
          if (text !== current) {
            if (current !== null) shown.push({ text: current, ms: Math.round(now - since) });
            current = text;
            since = now;
          }
          if (now - started < ms) requestAnimationFrame(tick);
          else {
            if (current !== null) shown.push({ text: current, ms: Math.round(now - since) });
            done();
          }
        };
        requestAnimationFrame(tick);
      });
      return { ok: true, parked: shown };
    }
    case "still": {
      if (op.send) press("Enter", { alt: Boolean(op.alt) });
      const ms = Number(op.ms ?? 5000);
      const started = performance.now();
      const ended = turnsEnded;
      const begun = stepsBegun;
      const marks = marksSeen;
      let markedAt: number | null = null;
      const at = (now: number) => Math.round(now - started);
      const replyTop = () => {
        const replies = document.querySelectorAll<HTMLElement>(".session.in-front .now > article.turn .tutor");
        const last = replies[replies.length - 1];
        return last ? Math.round(last.getBoundingClientRect().top * 10) / 10 : null;
      };
      const drawn = () => Array.from(document.querySelectorAll<HTMLElement>(".session.in-front .step-head")).filter((head) => head.getClientRects().length > 0).length;
      let endedAt: number | null = null;
      let stepAt: number | null = null;
      let headAt: number | null = null;
      const headsBefore = drawn();
      let lastTop: number | null = null;
      let topAtStep: { before: number | null; after: number | null } = { before: null, after: null };
      const moves: { at: number; from: number | null; to: number | null }[] = [];
      let frames = 0;
      let scrolledPast: boolean | null = null;
      await new Promise<void>((done) => {
        const tick = (now: number) => {
          if (endedAt === null && turnsEnded > ended) endedAt = at(now);
          if (op.past && scrolledPast === null && endedAt !== null && at(now) > endedAt && replyTop() === lastTop) {
            const surface = document.querySelector<HTMLElement>(".session.in-front .surface");
            const reply = replyTop();
            scrolledPast = false;
            if (surface && reply !== null) {
              surface.scrollTop += reply - surface.getBoundingClientRect().top + 40;
              lastTop = replyTop();
              scrolledPast = lastTop !== null && lastTop < surface.getBoundingClientRect().top && stepsBegun === begun;
            }
          }
          const top = replyTop();
          if (stepAt === null && stepsBegun > begun) {
            stepAt = at(now);
            topAtStep = { before: lastTop, after: top };
          }
          if (headAt === null && drawn() !== headsBefore) headAt = at(now);
          if (markedAt === null && marksSeen > marks) markedAt = at(now);
          if (endedAt !== null) {
            frames += 1;
            if (lastTop !== null && top !== lastTop) moves.push({ at: at(now), from: lastTop, to: top });
          }
          lastTop = top;
          if (now - started < ms) requestAnimationFrame(tick);
          else done();
        };
        requestAnimationFrame(tick);
      });
      const surface = document.querySelector<HTMLElement>(".session.in-front .surface");
      return {
        ok: true,
        ended_at: endedAt,
        step_at: stepAt,
        marked_at: markedAt,
        head_at: headAt,
        heads_before: headsBefore,
        heads_after: drawn(),
        reply_top_at_step: topAtStep,
        frames_after_end: frames,
        moves_after_end: moves,
        scrollable: surface ? surface.scrollHeight > surface.clientHeight : null,
        scrolled_past: scrolledPast,
      };
    }
    case "drift": {
      if (op.send) press("Enter", { alt: Boolean(op.alt) });
      const ms = Number(op.ms ?? 8000);
      const upAt = op.up_at === undefined ? null : Number(op.up_at);
      const upBy = Number(op.up_by ?? 300);
      const surface = document.querySelector<HTMLElement>(".session.in-front .surface");
      if (!surface) return { ok: false, error: "no lesson in front" };
      const started = performance.now();
      const at = (now: number) => Math.round(now - started);
      const pick = (): HTMLElement | null => {
        const box = surface.getBoundingClientRect();
        const column = surface.querySelector<HTMLElement>(".column")?.getBoundingClientRect();
        const x = column ? column.left + column.width / 2 : box.left + box.width / 2;
        for (const y of [0.35, 0.5, 0.25, 0.65]) {
          const hit = document.elementFromPoint(x, box.top + box.height * y);
          const block = hit?.closest<HTMLElement>(".prose > *, .you, .record, .working, .step-head, article.turn > *");
          if (block && surface.contains(block) && !block.closest(".foot")) return block;
        }
        return null;
      };
      const blocks = (turn: Element) => Array.from(turn.children).flatMap((child) => (child.classList.contains("tutor") ? Array.from(child.querySelector(".prose")?.children ?? []) : [child]));
      const pathOf = (el: HTMLElement | null) => {
        const turn = el?.closest("article.turn");
        return turn && el ? { ms: turn.getAttribute("data-ms"), at: blocks(turn).indexOf(el) } : null;
      };
      const byPath = (path: { ms: string | null; at: number } | null) => {
        const turn = path ? surface.querySelector(`article.turn[data-ms="${path.ms}"]`) : null;
        const found = turn && path && path.at >= 0 ? blocks(turn)[path.at] : null;
        return found instanceof HTMLElement ? found : null;
      };
      let anchor = pick();
      let path = pathOf(anchor);
      let lastTop = anchor ? anchor.getBoundingClientRect().top : null;
      let scrolledUp = false;
      const moves: { at: number; by: number; from_foot: number; working: string | null; replaced?: boolean }[] = [];
      const words = (el: HTMLElement | null) => (el ? collapse(el.innerText ?? "").slice(0, 50) : null);
      const anchors: { at: number; words: string | null }[] = [{ at: 0, words: words(anchor) }];
      await new Promise<void>((done) => {
        const tick = (now: number) => {
          if (upAt !== null && !scrolledUp && now - started >= upAt) {
            scrolledUp = true;
            surface.scrollTop -= upBy;
            requestAnimationFrame(() => {
              anchor = pick();
              path = pathOf(anchor);
              lastTop = anchor ? anchor.getBoundingClientRect().top : null;
              anchors.push({ at: at(performance.now()), words: words(anchor) });
              requestAnimationFrame(tick);
            });
            return;
          }
          const working = document.querySelector<HTMLElement>(".session.in-front .working");
          const fromFoot = Math.round(surface.scrollHeight - surface.clientHeight - surface.scrollTop);
          if (anchor && !anchor.isConnected) {
            const again = byPath(path);
            const by = again && lastTop !== null ? Math.round(again.getBoundingClientRect().top - lastTop) : 0;
            if (!again || by !== 0) moves.push({ at: at(now), by, from_foot: fromFoot, working: working ? collapse(working.innerText) || "(cursor)" : null, replaced: true });
            anchor = again ?? pick();
            path = pathOf(anchor);
            lastTop = anchor ? anchor.getBoundingClientRect().top : null;
            if (!again) anchors.push({ at: at(now), words: words(anchor) });
          } else if (anchor && lastTop !== null) {
            const top = anchor.getBoundingClientRect().top;
            if (Math.abs(top - lastTop) > 0.5) moves.push({ at: at(now), by: Math.round(top - lastTop), from_foot: fromFoot, working: working ? collapse(working.innerText) || "(cursor)" : null });
            lastTop = top;
          }
          if (now - started < ms) requestAnimationFrame(tick);
          else done();
        };
        requestAnimationFrame(tick);
      });
      return {
        ok: true,
        moves,
        anchors,
        events: heard.filter((each) => each.at >= started).map((each) => ({ type: each.type, at: Math.round(each.at - started), ...(each.doing !== undefined ? { doing: each.doing } : {}) })),
      };
    }
    case "select": {
      const selection = window.getSelection()!;
      const read = () => {
        const text = collapse(selection.toString());
        const holds = op.holds === undefined ? undefined : String(op.holds).split("|").map((words) => text.includes(words));
        return { selected: text.slice(0, 80), chars: text.length, holds };
      };
      if (op.read) return { ok: true, ...read(), focused: describe(document.activeElement) };
      if (op.clear) {
        const started = performance.now();
        selection.removeAllRanges();
        return { ok: true, paint_ms: Math.round((await nextPaint()) - started) };
      }
      const blocks = Array.from(document.querySelectorAll<HTMLElement>(".session.in-front .prose > *, .session.in-front .prose li"));
      const holding = (words: string) => blocks.find((block) => collapse(block.textContent ?? "").includes(words)) ?? null;
      const first = holding(String(op.from ?? ""));
      const last = op.to === undefined ? first : holding(String(op.to));
      if (!first || !last) return { ok: false, error: `no passage holds ${JSON.stringify(first ? op.to : op.from)}` };
      if (!op.in_place) first.scrollIntoView({ block: "center" });
      const range = document.createRange();
      range.setStartBefore(first.firstChild ?? first);
      range.setEndAfter(last.lastChild ?? last);
      (document.activeElement as HTMLElement | null)?.blur();
      selection.removeAllRanges();
      await nextPaint();
      const started = performance.now();
      selection.addRange(range);
      const painted = await nextPaint();
      return { ok: true, ...read(), paint_ms: Math.round(painted - started) };
    }
    case "caret": {
      const line = composer();
      if (!(line instanceof HTMLTextAreaElement)) return { ok: false, error: "no lesson line in front" };
      if (op.at !== undefined) line.setSelectionRange(Number(op.at), Number(op.at));
      return { ok: true, at: line.selectionStart, length: line.value.length, text: line.value, focused: document.activeElement === line };
    }
    case "steady": {
      const text = Array.from(String(op.text ?? ""));
      const back = Number(op.back ?? 0);
      const every = Number(op.every ?? 40);
      const surface = document.querySelector<HTMLElement>(".session.in-front .surface");
      const line = composer();
      if (!surface || !(line instanceof HTMLTextAreaElement)) return { ok: false, error: "no lesson line in front" };
      line.focus();
      line.setSelectionRange(line.value.length, line.value.length);
      const keys: string[] = [...text, ...Array<string>(back).fill("\b")];
      const place = () => Math.round(line.getBoundingClientRect().bottom - surface.getBoundingClientRect().bottom);
      const parked = () => document.querySelector(".session.in-front .foot .parked") !== null;
      const raised = () => document.querySelector(".session.in-front .foot.raised") !== null;
      const places: number[] = [];
      let parkedFlips = 0;
      let raisedFlips = 0;
      let wasParked = parked();
      let wasRaised = raised();
      let next = performance.now();
      await new Promise<void>((done) => {
        const tick = (now: number) => {
          places.push(place());
          if (parked() !== wasParked) (parkedFlips += 1), (wasParked = !wasParked);
          if (raised() !== wasRaised) (raisedFlips += 1), (wasRaised = !wasRaised);
          if (keys.length > 0 && now >= next) {
            const key = keys.shift()!;
            if (key === "\b") document.execCommand("delete");
            else insert(line, key);
            next = now + every;
          }
          if (keys.length > 0 || performance.now() - next < 300) requestAnimationFrame(tick);
          else done();
        };
        requestAnimationFrame(tick);
      });
      const moves = places.slice(1).map((at, i) => Math.abs(at - places[i]));
      return {
        ok: true,
        frames: places.length,
        moved: moves.filter((move) => move > 1).length,
        largestMove: Math.max(0, ...moves),
        lowest: Math.max(...places),
        highest: Math.min(...places),
        parkedFlips,
        raisedFlips,
        rows: Math.round(line.getBoundingClientRect().height),
        from_foot: Math.round(surface.scrollHeight - surface.clientHeight - surface.scrollTop),
      };
    }
    case "document": {
      const pdf = pdfInView();
      return pdf ? { ok: true, ...pdf, focus: focusName() } : { ok: false, error: "no PDF is in view" };
    }
    case "attach": {
      const paths = Array.isArray(op.paths) ? op.paths.map(String) : op.path ? [String(op.path)] : [];
      if (paths.length === 0) return { ok: false, error: "name a path or paths" };
      if (!probe?.attach(paths)) return { ok: false, error: "no session is in front" };
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
        return { ok: true, from_foot: Math.round(view.scrollHeight - view.clientHeight - view.scrollTop) };
      }
      await nextPaint();
      return { ok: true };
    }
    case "menu":
      try {
        return {
          ok: true,
          menu: await invoke<unknown>("harness_menu"),
          replaced: await invoke<number>("harness_menu_replaced"),
          native: await invoke<unknown>("harness_menu_native"),
        };
      } catch (error) {
        return { ok: false, error: String(error) };
      }
    case "frame": {
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
  const field = element instanceof HTMLTextAreaElement || element instanceof HTMLInputElement ? ` value=${JSON.stringify(element.value.slice(0, 60))}` : "";
  return `${element.tagName.toLowerCase()}${drive ? `[data-drive=${drive}]` : ""}${text ? ` "${text}"` : ""}${field}`;
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

export function harnessOn(): boolean {
  return !!config && (config.drive || config.perf);
}
