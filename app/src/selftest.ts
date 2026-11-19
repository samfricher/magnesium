import type { Actions } from "./App";
import { attachedWords, splitAttached, tutorText, type LessonTurn } from "./lesson";
import { tinyPdf } from "./demoPdf";
import { drawPage, openPdf } from "./pdf";
import { hasTask, renderInline, renderMarkdown, renderPlain, setSourcesApart, withoutOpenMath } from "./render";
import { comboOf, keyName, parseCombo, keepPlace, placeKey, type Place } from "./prefs";
import { sourceRef } from "./sources";
import { readingSpot } from "./reading";
import { inTask } from "./selection";
import { rebuild } from "./surface/Session";
import type { Core, Mark, Turn as SavedTurn } from "./types";
import { withoutPaths } from "./words";

const wait = (ms: number) => new Promise((done) => window.setTimeout(done, ms));

async function until(what: string, test: () => boolean, limitMs = 30_000): Promise<void> {
  const started = Date.now();
  while (!test()) {
    if (Date.now() - started > limitMs) throw new Error(`timed out waiting for ${what}`);
    await wait(50);
  }
}

function within<T>(what: string, work: Promise<T>, limitMs = 20_000): Promise<T> {
  return Promise.race([work, new Promise<T>((_, no) => window.setTimeout(() => no(new Error(`${what} did not finish in ${limitMs / 1000} s`)), limitMs))]);
}

function press(key: string, mods: { meta?: boolean; shift?: boolean } = {}) {
  const target = (document.activeElement as HTMLElement | null) ?? document.body;
  target.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, metaKey: Boolean(mods.meta), shiftKey: Boolean(mods.shift) }));
}

function noRawFiles(where: string): void {
  const text = document.body.innerText;
  const found = [/Study\/[\w-]/, /\/Users\//, /\.magnesium/, /plan\.md/, /state\.json/, /settings\.conf/, /^#{1,3} /m, /\| --- \|/].find((pattern) => pattern.test(text));
  if (found) throw new Error(`${where} shows a path or file text (${found}): ${text.slice(0, 200)}`);
}

const NOT_THE_WINDOWS = '.tutor, .you, .said-quote, .question, .line, .course, .page-title, [role="tab"], [role="listitem"], script, style';
const RETIRED = [/\bworkspaces?\b/i, /\bstudy folders?\b/i, /\bcourse material\b/i, /\bthis course\b/i, /\bcourse page\b/i, /\bthis folder\b/i, /\bCourses\b/];

function noRetiredNames(where: string): void {
  const pieces: string[] = [];
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (!node.parentElement?.closest(NOT_THE_WINDOWS)) pieces.push(node.textContent ?? "");
  }
  for (const element of Array.from(document.body.querySelectorAll<HTMLElement>("[placeholder], [aria-label], [title]"))) {
    if (element.closest(NOT_THE_WINDOWS)) continue;
    for (const name of ["placeholder", "aria-label", "title"]) pieces.push(element.getAttribute(name) ?? "");
  }
  for (const piece of pieces) {
    const found = RETIRED.find((pattern) => pattern.test(piece));
    if (found) throw new Error(`${where} names a space by an old name (${found}): ${JSON.stringify(piece.trim().slice(0, 160))}`);
  }
}

export async function run(api: Core, actions: Actions): Promise<void> {
  const steps: Record<string, unknown> = {};
  let failure: string | null = null;
  const session = () => actions.session()?.state() ?? { open: false, earlier: [], live: null, cards: [], notices: [], startedMs: null };
  const idle = () => {
    const live = session().live;
    return live !== null && live.status === "idle" && tutorText(live) !== "";
  };
  try {
    const folder = (await api.initialFolder()) ?? "";
    const home = await api.home(folder);
    steps.home = { state: home.state, name: home.name };

    const setup = await api.setup();
    steps.setup = { runtime: setup.runtime.state, signed_in: setup.account.signed_in, paid_with: setup.account.paid_with };
    if (!setup.account.signed_in || setup.account.paid_with !== "plan") throw new Error("the stand-in agent was not read as signed in to a plan");
    const withKey = await api.setPayment("api_key");
    const back = await api.setPayment("plan");
    steps.payment = { key: withKey.problem !== null, plan: back.problem === null };
    if (withKey.problem === null || back.problem !== null) throw new Error("the choice of payment did not reach the agent's check");
    const about = await api.about();
    steps.about = about;
    if (!about.version) throw new Error("the window did not report its version");

    await until("the workspace's page", () => document.querySelector('[data-page="workspace"] [data-drive="new-session"]') !== null, 10_000);
    noRawFiles("the workspace's page");
    noRetiredNames("the workspace's page");
    steps.workspace = { title: document.querySelector(".page-title")?.textContent ?? null };

    actions.goWorkspace(null);
    await until("Home", () => document.querySelector('[data-page="home"] [data-drive="start"]') !== null, 10_000);
    await wait(100);
    steps.homePage = { rows: document.querySelectorAll('[data-page="home"] [data-row]').length, first: document.querySelector('[data-drive="start"]')?.textContent ?? null };
    if (document.activeElement?.getAttribute("data-drive") !== "start") throw new Error("Home's first row does not have the focus");
    noRawFiles("Home");
    noRetiredNames("Home");

    press(",", { meta: true });
    await until("the Settings tab", () => document.querySelector('[data-page="settings"]') !== null, 3000);
    if (document.querySelector('[data-drive="settings"]') !== null) throw new Error("Settings is in the tab bar twice");
    const sections = Array.from(document.querySelectorAll(".settings-page .section-head")).map((head) => head.textContent);
    steps.settings = { sections, keys: document.querySelectorAll(".key-row").length, colours: document.querySelectorAll('input[type="color"]').length };
    for (const wanted of ["Reading", "Appearance", "Keys", "Lessons", "About"]) if (!sections.includes(wanted)) throw new Error(`Settings has no ${wanted} section`);
    if (document.querySelectorAll('[data-page="settings"] textarea').length > 0) throw new Error("Settings shows a text to edit");
    noRawFiles("Settings");
    noRetiredNames("Settings");
    press("w", { meta: true });
    await until("Settings to close", () => document.querySelector('[data-page="settings"]') === null, 3000);

    press("k", { meta: true });
    await until("the command menu", () => document.querySelector('[data-layer="Commands"]') !== null, 3000);
    const commands = Array.from(document.querySelectorAll('[data-layer="Commands"] .row-label')).map((row) => row.textContent);
    steps.commands = commands;
    noRetiredNames("⌘K");
    if (commands.length > 6 || !commands.includes("Home") || !commands.includes("Settings") || !commands.includes("New session")) throw new Error(`⌘K holds other things: ${commands.join(", ")}`);
    press("Escape");
    await until("the command menu to go", () => document.querySelector('[data-layer="Commands"]') === null, 3000);

    const combo = comboOf({ key: "j", code: "KeyJ", metaKey: true, ctrlKey: false, altKey: false, shiftKey: true });
    steps.keys = { combo, name: combo ? keyName(combo) : null, none: parseCombo("none") };
    if (combo !== "cmd+shift+j" || keyName(combo) !== "⇧⌘J" || parseCombo("none") !== null) throw new Error(`a key was not read: ${JSON.stringify(steps.keys)}`);

    const plain = [withoutPaths("Added 5 files to Study/materials."), withoutPaths("Change Study/midterm-1/plan.md"), withoutPaths("in .magnesium/backups"), withoutPaths("1/v in s/µM")];
    steps.withoutPaths = plain;
    if (plain.join("|") !== "Added 5 files to the material.|Change plan.md|as a backup|1/v in s/µM") throw new Error(`a path reached the screen: ${plain.join(" | ")}`);

    {
      const minute = new Date(2026, 10, 13, 13, 15, 0).getTime();
      const row = (id: string, at = "2026-11-13 13:15", paper: string | null = null): Mark => ({ id, at, label: "sign of b", mark: "right", help: "none", cold: false, paper });
      const kept = (second: number, extra: Partial<SavedTurn> = {}): SavedTurn => ({ session: "s", ms: minute + second * 1000, learner: `answer ${second}`, tutor: "…", interrupted: false, intent: "answer", ...extra });
      const drawn = (turns: LessonTurn[]) => turns.map((turn) => (turn.marks ?? []).map((mark) => mark.id).join("+") || "-").join(" ");
      const posed = kept(0, { learner: "Go on", intent: "talk", noted: true, task: "Task 1", posed: true, mark_ids: [] });
      const triesTurns = rebuild([posed, kept(10, { noted: true, task: "Task 1", mark_ids: [] }), kept(20, { noted: true, task: "Task 1", mark_ids: [] }), kept(30, { noted: true, task: "Task 1", mark_ids: ["a-3"] })], [row("a-3")], "s");
      const reopened = {
        twoTasks: drawn(rebuild([kept(10, { mark_ids: ["a-1"] }), kept(20, { mark_ids: ["a-2"] })], [row("a-1"), row("a-2")], "s")),
        threeTries: drawn(triesTurns),
        noAnswerKept: drawn(rebuild([kept(10, { mark_ids: ["a-1"] }), kept(20, { mark_ids: [] })], [row("a-1"), row("t-9")], "s")),
        older: drawn(rebuild([kept(10), kept(130)], [row("a-1"), row("p-1", "2026-11-13 13:16", "Paper 1"), row("a-2", "2026-11-13 13:17")], "s")),
        mixed: drawn(rebuild([kept(10), kept(20, { mark_ids: ["a-2"] })], [row("a-1"), row("a-2")], "s")),
        noRecord: drawn(rebuild([kept(10, { mark_ids: ["a-1"] }), kept(20)], null, "s")),
      };
      steps.reopenedMarks = reopened;
      const wanted = { twoTasks: "a-1 a-2", threeTries: "- - - a-3", noAnswerKept: "a-1 -", older: "a-1 a-2", mixed: "a-1 a-2", noRecord: "- -" };
      for (const [name, want] of Object.entries(wanted)) if (reopened[name as keyof typeof wanted] !== want) throw new Error(`a reopened session put a mark on the wrong answer (${name}): ${reopened[name as keyof typeof wanted]}, not ${want}`);
      const heads = rebuild([kept(0, { step: { n: 1, of: 3, name: "One" } }), kept(10), kept(20, { step: { n: 2, of: 3, name: "Two" } })], [], "s")
        .map((turn) => (turn.step ? `${turn.step.n}/${turn.step.of} ${turn.step.name}` : "-"))
        .join(" | ");
      steps.reopenedSteps = heads;
      if (heads !== "1/3 One | - | 2/3 Two") throw new Error(`a reopened session lost where its steps began: ${heads}`);
    }

    const page = document.createElement("div");
    page.innerHTML = renderMarkdown(
      '![graph of $y=x^2$](plot.png) and [link](u "title $z$") with $a^b$\n\n<map name="m"><area href="https://example.com" shape="default"></map>',
    );
    const alt = page.querySelector("img")?.getAttribute("alt");
    const title = page.querySelector("a")?.getAttribute("title");
    steps.attributeMath = { alt, title, katex: page.querySelectorAll(".katex").length, areas: page.querySelectorAll("map, area").length };
    if (alt !== "graph of y=x^2" || title !== "title z") throw new Error(`mathematics in an attribute was not plain text: ${alt} / ${title}`);
    if (page.querySelectorAll(".katex").length !== 1) throw new Error("mathematics in the text did not draw");
    if (page.querySelectorAll("map, area").length !== 0) throw new Error("an image map was not removed");

    const line = document.createElement("div");
    line.innerHTML = renderInline("the cdf of $U = \\sqrt{Y}$ <b onclick=x>now</b>");
    const said = document.createElement("div");
    said.innerHTML = renderPlain("2*x*3 < 7 and $x^2$\nnext line <i>");
    steps.lineMath = { line: line.querySelectorAll(".katex").length, handlers: line.querySelectorAll("[onclick]").length, said: said.textContent };
    if (line.querySelectorAll(".katex").length !== 1) throw new Error("mathematics in a line of text did not draw");
    if (line.querySelectorAll("[onclick]").length !== 0) throw new Error("a line of text kept a handler");
    if (said.querySelectorAll(".katex").length !== 1 || said.querySelectorAll("i").length !== 0 || !said.textContent?.includes("2*x*3 < 7")) {
      throw new Error(`the learner's words were not kept as typed: ${said.innerHTML}`);
    }

    const price = `It costs $5 and that is all. ${"More words follow. ".repeat(12)}`;
    const cut = {
      inline: withoutOpenMath("So $x = \\frac{1}{2"),
      display: withoutOpenMath("So $x = 1$ and\n\n$$y = \\begin{pmatrix} 1"),
      whole: withoutOpenMath("So $x = 1$ and $$y = 2$$ done"),
      price: withoutOpenMath(price) === price,
    };
    steps.openMath = cut;
    if (cut.inline !== "So" || cut.display !== "So $x = 1$ and" || cut.whole !== "So $x = 1$ and $$y = 2$$ done" || !cut.price) {
      throw new Error(`a half-written formula was not held back: ${JSON.stringify(cut)}`);
    }

    const task = document.createElement("div");
    task.innerHTML = renderMarkdown("An explanation.\n\n**Task.** Find the eigenvalues of\n\n$$A$$\n\nOne line.");
    const box = task.querySelector("div.task");
    steps.task = { boxes: task.querySelectorAll("div.task").length, blocks: box?.children.length ?? 0, before: task.firstElementChild?.tagName };
    if (!box || box.children.length !== 3 || task.firstElementChild?.tagName !== "P" || box.querySelectorAll(".katex-display").length !== 1) {
      throw new Error(`the task was not set apart as one block: ${task.innerHTML.slice(0, 200)}`);
    }

    const cited = renderMarkdown("See [Week 4 slides, p. 12](Study/stats/week%204.pdf#page=12) and [the web](https://example.com).\n\n**Task.** Say why.");
    const shut = document.createElement("div");
    shut.innerHTML = setSourcesApart(cited, false);
    const open = document.createElement("div");
    open.innerHTML = setSourcesApart(cited, true);
    const ref = sourceRef("Study/stats/week%204.pdf#page=12");
    steps.sources = { shutLinks: shut.querySelectorAll("a").length, openLinks: open.querySelectorAll("a.source").length, ref };
    if (shut.querySelectorAll("a").length !== 1 || shut.querySelectorAll(".source-shut").length !== 1) throw new Error("a source link was not shut as text");
    if (open.querySelectorAll("a.source").length !== 1) throw new Error("a source link did not open once answered");
    if (ref?.path !== "Study/stats/week 4.pdf" || ref.page !== 12) throw new Error(`the page of a source link was not read: ${JSON.stringify(ref)}`);
    if (!hasTask("A line.\n\n**Task.** Say why.") || hasTask("Just a line.")) throw new Error("a task was not told from a reply with none");

    const pdf = await within("opening a PDF", openPdf(tinyPdf(["one", "two", "three"])));
    const canvas = document.createElement("canvas");
    await within("drawing a PDF page", drawPage(pdf, 2, canvas, 300));
    const pixels = canvas.getContext("2d")?.getImageData(0, 0, canvas.width, canvas.height).data ?? new Uint8ClampedArray();
    let ink = 0;
    for (let i = 0; i < pixels.length; i += 4) if (pixels[i] < 128) ink += 1;
    steps.pdf = { pages: pdf.numPages, width: canvas.width, ink };
    if (pdf.numPages !== 3 || ink < 200) throw new Error(`the PDF page did not draw: ${JSON.stringify(steps.pdf)}`);

    const pasted = await within("saving pasted text", api.addPastedText("Bayes' rule: posterior is proportional to prior times likelihood.", folder));
    const savedAs = pasted.saved[0];
    steps.material = { saved: pasted.saved.length, lines: pasted.lines };
    if (!savedAs?.startsWith("Study/materials/Pasted text ") || !savedAs.endsWith(".txt")) throw new Error(`pasted text was not saved as a text file: ${JSON.stringify(pasted)}`);
    const readBack = await within("reading a saved text file", api.source(savedAs, folder));
    if (readBack.kind !== "text" || !readBack.text?.includes("posterior")) throw new Error("a saved text file did not open as text");
    for (const outside of ["../outside.pdf", "/etc/hosts", "Study/midterm-1/papers/PaperB-key.pdf"]) {
      const refused = await within(`opening ${outside}`, api.source(outside, folder)).then(() => null, (problem) => String(problem));
      if (refused === null) throw new Error(`a source outside the study folder, or an answer key, opened: ${outside}`);
    }

    actions.goWorkspace(folder);
    await until("the material folded", () => document.querySelector('[data-page="workspace"] [data-drive="material"]') !== null, 10_000);
    if (document.querySelector('[data-page="workspace"] .file-row') !== null) throw new Error("the material was listed before it was opened");
    document.querySelector<HTMLElement>('[data-page="workspace"] [data-drive="material"]')!.click();
    await until("the material listed", () => document.querySelector('[data-page="workspace"] .file-row') !== null, 10_000);
    const titles = Array.from(document.querySelectorAll('[data-page="workspace"] .file-row .row-name')).map((row) => row.textContent ?? "");
    steps.materialTitles = titles;
    if (titles.some((title) => /\.[a-z0-9]{1,5}$/i.test(title) || title.includes("/"))) throw new Error(`material is listed by file name: ${titles.join(", ")}`);
    document.querySelector<HTMLElement>('[data-page="workspace"] .file-row')!.click();
    await until("the material beside the page", () => document.querySelector('[data-role="side-panel"] .side-tab.here') !== null, 5000);
    const sheet = document.querySelector('[data-role="side-panel"] .side-tab.here')?.textContent?.replace(/×$/, "") ?? "";
    steps.materialSheet = sheet;
    if (/\.[a-z0-9]{1,5}$/i.test(sheet) || sheet.includes("/")) throw new Error(`material opened under its file name: ${sheet}`);
    document.querySelector<HTMLElement>('[data-role="side-panel"] .side-tab.here .side-close')?.click();
    await until("the material to close", () => document.querySelector('[data-role="side-panel"]') === null, 3000);

    const pdfRow = Array.from(document.querySelectorAll<HTMLElement>('[data-page="workspace"] .file-row')).find((row) => row.textContent?.includes("Week 1 slides"));
    if (!pdfRow) throw new Error(`the PDF in the material is not listed: ${titles.join(", ")}`);
    pdfRow.click();
    const pdfPages = () => Array.from(document.querySelectorAll<HTMLElement>('[data-role="side-panel"] .doc-page'));
    await until("the PDF's pages beside the page", () => pdfPages().length === 2, 10_000);
    await until("the first page drawn with its words", () => {
      const first = pdfPages()[0];
      return first?.querySelector("canvas") != null && (first.querySelector(".textLayer")?.textContent ?? "").includes("Selectable words on page one");
    }, 10_000);
    const drawn = pdfPages()[0].querySelector("canvas")!;
    const drawnPixels = drawn.getContext("2d")?.getImageData(0, 0, drawn.width, drawn.height).data ?? new Uint8ClampedArray();
    let drawnInk = 0;
    for (let i = 0; i < drawnPixels.length; i += 4) if (drawnPixels[i] < 128) drawnInk += 1;
    const wordsLayer = pdfPages()[0].querySelector<HTMLElement>(".textLayer span");
    const wordsStyle = wordsLayer ? getComputedStyle(wordsLayer) : null;
    const selectable = wordsStyle !== null && (wordsStyle.userSelect || wordsStyle.webkitUserSelect) !== "none";
    const scroller = document.querySelector<HTMLElement>('[data-role="side-panel"] [data-role="doc-pages"]')!;
    scroller.focus();
    press("ArrowRight");
    await until("→ to turn to the second page", () => Math.abs(scroller.scrollTop - pdfPages()[1].offsetTop) < 40, 3000);
    steps.pdfView = { pages: pdfPages().length, ink: drawnInk, selectable, zoom: document.querySelector('[data-role="side-panel"] [data-drive="zoom-fit"]')?.textContent ?? null };
    if (drawnInk < 200) throw new Error(`the PDF's page did not draw in the panel: ${JSON.stringify(steps.pdfView)}`);
    if (!selectable) throw new Error("the PDF's words cannot be selected");
    document.querySelector<HTMLElement>('[data-role="side-panel"] .side-tab.here .side-close')?.click();
    await until("the PDF to close", () => document.querySelector('[data-role="side-panel"]') === null, 3000);

    const png = Uint8Array.from(atob("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="), (c) => c.charCodeAt(0));
    const kept = await within("handing in a pasted image", api.attachBytes("Pasted 10.41.png", png, "attached", folder));
    steps.attached = kept;
    if (!kept.startsWith("Study/handed-in/attached/")) throw new Error(`a pasted image was not handed in: ${kept}`);
    const asWork = attachedWords([kept], true);
    const toRead = attachedWords([kept], false);
    if (!asWork.startsWith("Photo of my working: ") || !/may be my working or material I’m learning from/.test(toRead)) throw new Error(`a file was not named in plain words: ${asWork} / ${toRead}`);
    if (splitAttached(`b = 5\n\n${toRead}`).files[0] !== kept || splitAttached(`b = 5\n\n${toRead}`).text !== "b = 5") throw new Error("a file named in a message was not read back");
    const before = `b = 5\n\nA file for you to read: ${kept}. It may be my working or course material.`;
    if (splitAttached(before).files[0] !== kept || splitAttached(before).text !== "b = 5") throw new Error("a file named in an older message was not read back");

    const listed = await api.workspaces();
    const plan = await api.plan(folder);
    steps.library = { workspaces: listed.length, first: listed[0]?.name, plan: { items: plan.items.length } };
    if (!listed.some((each) => each.folder === home.folder)) throw new Error("the open folder is not in the library");

    const front = <T extends Element = HTMLElement>(what: string) => document.querySelector<T>(`.session.in-front ${what}`);
    const modelRow = () => front('.line-menu [data-drive="model"]');
    const openModelRow = async (what: string, named: (word: string) => boolean, limit: number) => {
      let word: string | null = null;
      await until(what, () => {
        if (!modelRow()) front('[data-drive="more"]')?.click();
        word = modelRow()?.querySelector(".line-menu-detail")?.textContent?.replace(/\s*›$/, "") ?? null;
        return word !== null && named(word);
      }, limit);
      return word as string | null;
    };
    actions.newSession(folder);
    await until("a new session's first line, ending in More", () => front('[data-line="rest"] [data-drive="more"]') !== null, 10_000);
    const newTab = document.querySelector('[role="tab"][aria-selected="true"] .tab-name')?.textContent?.trim() ?? null;
    steps.newSessionTab = newTab;
    if (newTab !== "New session") throw new Error(`a new session's tab is not named New session: ${JSON.stringify(newTab)}`);
    noRetiredNames("a new session");
    const noStuck = front('[data-line="rest"] [data-drive="stuck"]') === null;
    actions.run("hint");
    await wait(300);
    const stillRest = front('[data-line="rest"]') !== null && !session().open;
    const beforeWord = await openModelRow("More's Model row, named from the list the window asked for", (word) => word !== "Default model", 20_000);
    modelRow()!.click();
    await until("the list of models, before any lesson", () => document.querySelectorAll('.session.in-front [data-drive="model-row"]').length > 0, 5000);
    const firstRows = document.querySelectorAll('.session.in-front [data-drive="model-row"]').length;
    press("Escape");
    await wait(150);
    steps.listBeforeLesson = { word: beforeWord, rows: firstRows, noStuck, hintKeyIdle: stillRest, closed: front(".model-list") === null, focus: document.activeElement?.getAttribute("data-drive") ?? null };
    if (!noStuck) throw new Error("Stuck is on the line before a lesson has begun");
    if (!stillRest) throw new Error("the hint key did something before a lesson had begun");
    if (beforeWord !== "Opus 5.5" || firstRows !== 5) throw new Error(`a new window did not have Claude Code's list before a lesson: ${JSON.stringify(steps.listBeforeLesson)}`);
    if (front(".model-list") !== null || document.activeElement?.getAttribute("data-drive") !== "composer") throw new Error(`Esc did not close the list of models: ${JSON.stringify(steps.listBeforeLesson)}`);
    actions.run("close-session");
    await until("the unstarted session's tab to close", () => actions.session() === null, 10_000);

    const message = "Self-check. Inline $a^b$ and a diagram:\n\n```mermaid\ngraph LR\n  A[rate] --> B[reciprocal]\n```";
    actions.continueRecent();
    await until("a session's tab", () => actions.session() !== null, 5000);
    await until("the first reply", idle);
    if (document.querySelector(".session.in-front .now.lesson") === null) throw new Error("the lesson did not fill its tab");
    {
      const choice = await api.models();
      steps.models = { rows: choice.models.length, current: choice.models.filter((row) => row.current).length, pick: choice.pick?.model ?? null, failed: choice.failed };
      if (choice.models.length !== 7 || choice.pick?.model !== "claude-opus-5-5" || choice.failed) throw new Error(`the agent's models did not come: ${JSON.stringify(steps.models)}`);
    }
    await actions.session()!.say(message, "talk");
    await until("the reply to be on screen", () => session().earlier.length >= 1 && idle());
    const reply = tutorText(session().live!);
    steps.reply = reply.slice(0, 120);
    if (!reply.includes("Self-check")) throw new Error(`the reply was not the stand-in's echo: ${reply.slice(0, 80)}`);
    if (session().live?.said !== "talk") throw new Error("talk was not sent as talk");

    await until("the page to draw the reply", () => document.querySelector(".session.in-front .tutor .prose") !== null, 5000);
    await wait(1500);
    steps.math = document.querySelectorAll(".session.in-front .tutor .katex").length;
    steps.diagram = document.querySelectorAll(".session.in-front .tutor .diagram svg").length;
    if (steps.math === 0) throw new Error("the formula did not draw");
    if (steps.diagram === 0) throw new Error("the diagram did not draw");

    {
      const line = () => document.querySelector<HTMLElement>(".session.in-front .line.lesson");
      document.querySelector<HTMLElement>('.session.in-front .line.lesson [data-drive="stuck"]')!.click();
      await until("Stuck's row in the line", () => line()?.querySelector('.line-menu [data-drive="hint"]') != null, 3000);
      const row = line()!.querySelector<HTMLElement>(".line-menu")!;
      const words = line()!.querySelector<HTMLElement>(".field-row")!;
      const labels = [...row.querySelectorAll(".line-menu-label")].map((label) => label.textContent);
      const above = row.getBoundingClientRect().bottom <= words.getBoundingClientRect().top + 1;
      const inFlow = getComputedStyle(row).position !== "absolute";
      const focused = document.activeElement?.getAttribute("data-drive");
      press("Escape");
      await wait(150);
      steps.stuckInLesson = { labels, above, inFlow, focused, closed: line()?.querySelector(".line-menu") == null, back: document.activeElement?.getAttribute("data-drive") ?? null };
      if (labels.join(" / ") !== "Hint / I’m lost / I don’t know") throw new Error(`Stuck's rows are not named as the keys are: ${labels.join(" / ")}`);
      if (!above || !inFlow) throw new Error(`Stuck's rows are not a row above the words: ${JSON.stringify(steps.stuckInLesson)}`);
      if (focused !== "hint") throw new Error(`the keyboard did not go to Stuck's first row: ${focused}`);
      if (line()?.querySelector(".line-menu") != null || document.activeElement?.getAttribute("data-drive") !== "composer") throw new Error(`Esc did not put Stuck's rows away: ${JSON.stringify(steps.stuckInLesson)}`);
    }

    {
      const root = document.documentElement;
      const was = root.dataset.field;
      const words = () => document.querySelector<HTMLElement>(".session.in-front .line.lesson .field-row")!;
      const look = (field: string) => {
        root.dataset.field = field;
        const rule = getComputedStyle(words(), "::after");
        const mark = getComputedStyle(words(), "::before");
        const hang = parseFloat(getComputedStyle(root).getPropertyValue("--hang")) || 0;
        return {
          rule: rule.content !== "none" && rule.borderTopStyle === "solid" ? rule.borderTopWidth : null,
          from: rule.content !== "none" ? Math.round(parseFloat(rule.left) + hang) : null,
          to: rule.content !== "none" ? Math.round(parseFloat(rule.right)) : null,
          mark: mark.content,
        };
      };
      steps.lineLooks = { bare: look("bare"), ruled: look("ruled"), soft: look("soft") };
      if (was === undefined) delete root.dataset.field;
      else root.dataset.field = was;
      const { bare, ruled, soft } = steps.lineLooks as Record<string, ReturnType<typeof look>>;
      if (ruled.rule !== "1px" || ruled.from !== 0 || ruled.to !== 0) throw new Error(`the ruled line has no hairline from its "›" to the column's end: ${JSON.stringify(steps.lineLooks)}`);
      if (ruled.mark !== bare.mark || bare.mark === "none") throw new Error(`the ruled line does not hang the bare line's "›": ${JSON.stringify(steps.lineLooks)}`);
      if (bare.rule !== null || soft.rule !== null) throw new Error(`a look other than the ruled line drew a rule: ${JSON.stringify(steps.lineLooks)}`);
    }

    {
      const more = () => document.querySelector<HTMLElement>('.session.in-front .line.lesson [data-drive="more"]');
      const lessonWord = () => document.querySelector<HTMLElement>('.session.in-front .line-menu [data-drive="model"] .line-menu-detail');
      await until("More on the lesson's line", () => more() !== null, 5000);
      const openMore = async () => {
        if (lessonWord() === null) more()!.click();
        await until("the model under More", () => lessonWord() !== null, 3000);
      };
      const choose = async (value: string, level: string) => {
        if (document.querySelector(".session.in-front .model-list") === null) {
          await openMore();
          document.querySelector<HTMLElement>('.session.in-front .line-menu [data-drive="model"]')!.click();
        }
        await until("the list over the lesson's line", () => document.querySelector(".session.in-front .model-list") !== null, 3000);
        document.querySelector<HTMLElement>(`.session.in-front [data-drive="model-row"][data-value="${value}"]`)!.click();
        await until(`${value} chosen`, () => document.querySelector<HTMLElement>('.session.in-front [data-drive="model-row"].on')?.dataset.value === value, 5000);
        document.querySelector<HTMLElement>(`.session.in-front [data-drive="model-effort"][data-level="${level}"]`)!.click();
        await until(`${level} effort chosen`, () => document.querySelector(`.session.in-front [data-drive="model-effort"][data-level="${level}"].chosen`) !== null, 5000);
        press("Escape");
        await wait(150);
      };
      const ranOn = async () => {
        const before = session().earlier.length;
        await actions.session()!.say("RUNS_ON", "talk");
        await until("the reply naming the model", () => session().earlier.length > before && idle());
        return tutorText(session().live!);
      };
      await choose("haiku", "low");
      await openMore();
      const changedWord = lessonWord()?.firstChild?.textContent ?? null;
      press("Escape");
      await wait(150);
      const changed = await ranOn();
      await choose("opus", "usual");
      const back = await ranOn();
      steps.lessonModel = { word: changedWord, changed, back, focus: document.activeElement?.getAttribute("data-drive") ?? null };
      if (changedWord !== "Haiku 5.5\u00a0·\u00a0Low" || changed !== "On claude-haiku-5-5 at low." || back !== "On claude-opus-5-5 at medium.")
        throw new Error(`the running session did not change its model: ${JSON.stringify(steps.lessonModel)}`);
    }

    const writing = document.querySelector<HTMLTextAreaElement>('.session.in-front [data-drive="composer"]')!;
    writing.focus();
    await wait(2100);
    await actions.session()!.say("PERMISSION", "talk");
    await until("the permission card", () => session().cards.length > 0);
    await wait(100);
    steps.cardTookKeys = document.activeElement?.getAttribute("data-drive") ?? null;
    if (steps.cardTookKeys !== "allow") throw new Error(`a card over an idle, empty line did not take the keyboard: ${steps.cardTookKeys}`);
    steps.card = document.querySelector(".card .record-head")?.textContent ?? null;
    const card = {
      head: steps.card,
      what: document.querySelector(".card .card-what")?.textContent ?? null,
      exact: document.querySelector(".card .card-detail")?.textContent ?? null,
      always: document.querySelector('.card [data-drive="allow-always"]')?.textContent ?? null,
    };
    steps.cardWords = card;
    if (card.head !== "The tutor asks to run a command" || card.what !== "Build the paper as a PDF" || card.exact !== "tectonic paper.tex" || card.always !== "or always allow tectonic commands")
      throw new Error(`the permission card is not in the learner's words: ${JSON.stringify(card)}`);
    noRetiredNames("a permission card");
    await api.decide(session().cards[0].id, "allow_once");
    await until("the reply after the permission", () => session().cards.length === 0 && idle());

    const writeOn = (value: string) => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(writing, value);
      writing.dispatchEvent(new Event("input", { bubbles: true }));
    };
    writing.focus();
    writeOn("and the second part");
    await actions.session()!.say("PERMISSION", "talk");
    await until("the second permission card", () => session().cards.length > 0);
    await wait(100);
    steps.cardKeys = { stayed: document.activeElement === writing, words: writing.value };
    if (document.activeElement !== writing || writing.value !== "and the second part")
      throw new Error(`a card took the keyboard from words on the line: ${JSON.stringify(steps.cardKeys)}`);
    press("Tab", { shift: true });
    if (document.activeElement?.getAttribute("data-drive") !== "allow") throw new Error(`⇧⇥ from the line did not reach the card: ${document.activeElement?.outerHTML.slice(0, 80)}`);
    writeOn("");
    await api.decide(session().cards[0].id, "refuse");
    await until("the reply after the refusal", () => session().cards.length === 0 && idle());
    await wait(100);
    if (document.querySelector(".session.in-front .to-card")) throw new Error("the way to a card is still under the line after the card was answered");

    await actions.session()!.say(`my answer is $x^2$\n\n${toRead}`, "talk");
    await until("the reply to the answer", () => idle() && session().live?.learner.includes("my answer") === true);
    steps.saidMath = document.querySelectorAll(".session.in-front .you .katex").length;
    if (steps.saidMath === 0) throw new Error("the mathematics in the learner's own line did not draw");
    if (!document.querySelector(".session.in-front .sent-files")?.textContent?.includes(kept.split("/").pop() ?? kept)) throw new Error("the file sent was not named under the message");

    {
      const line = document.querySelector<HTMLTextAreaElement>('.session.in-front [data-drive="composer"]')!;
      const write = (value: string) => {
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(line, value);
        line.dispatchEvent(new Event("input", { bubbles: true }));
      };
      const press = (init: KeyboardEventInit) => line.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }));
      if (actions.session()?.probe().task !== null) throw new Error("a task is open before one was set");
      line.focus();
      write("Here is my working");
      press({ key: "Enter" });
      await until("the words sent with no task in view", () => session().live?.learner === "Here is my working", 3000);
      const plain = session().live?.said ?? null;
      await until("the reply to them", () => idle(), 10_000);
      line.focus();
      write("A question, not an answer");
      press({ key: "Enter", altKey: true });
      await until("the words sent with ⌥↩ and no task in view", () => session().live?.learner === "A question, not an answer", 3000);
      const talk = session().live?.said ?? null;
      await until("the reply to the question", () => idle(), 10_000);
      steps.unseenTask = { plain, talk };
      if (plain !== null) throw new Error(`with no task in view, words on the line were sent as ${plain}: the tutor could not mark them`);
      if (talk !== "talk") throw new Error(`with no task in view, ⌥↩ sent the words as ${String(talk)}, not as talk`);
    }

    await actions.session()!.say("A task, please.\n\n**Task.** What is 2 + 2?", "talk");
    await until("the task open", () => idle() && actions.session()?.probe().task !== null, 10_000);
    actions.run("symbols");
    await until("the symbols", () => document.querySelector(".session.in-front .symbols") !== null, 3000);
    const field = document.querySelector<HTMLTextAreaElement>('.session.in-front [data-drive="composer"]')!;
    const setField = (value: string) => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(field, value);
      field.dispatchEvent(new Event("input", { bubbles: true }));
    };
    const onField = (init: KeyboardEventInit) => field.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }));
    field.focus();
    setField("√4");
    onField({ key: "Escape" });
    await until("the symbols to close", () => document.querySelector(".session.in-front .symbols") === null, 3000);
    onField({ key: "Escape" });
    await wait(100);
    const onLine = field.value;
    steps.escape = { onLine, away: document.activeElement !== field };
    if (onLine !== "√4") throw new Error(`Esc lost the words on the line: ${onLine}`);
    if (document.activeElement === field) throw new Error("a second Esc did not put the line away");

    field.focus();
    setField("it is 4, I think");
    onField({ key: "Enter", altKey: true });
    await until("the words sent as talk", () => session().live?.learner === "it is 4, I think", 3000);
    if (session().live?.said !== "talk") throw new Error(`⌥↩ sent the words as ${String(session().live?.said)}, not as talk`);
    await until("the reply to the talk", () => idle(), 10_000);

    const passage = [...document.querySelectorAll<HTMLElement>(".session.in-front .tutor .prose p")].find((p) => (p.textContent ?? "").trim().length > 8);
    if (!passage) throw new Error("no passage of the tutor's to select");
    const range = document.createRange();
    range.selectNodeContents(passage);
    (document.activeElement as HTMLElement | null)?.blur();
    window.getSelection()!.removeAllRanges();
    window.getSelection()!.addRange(range);
    document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "W", bubbles: true, cancelable: true }));
    await until("the line to quote the passage", () => document.querySelector(".session.in-front .line.lesson .quote") !== null, 3000);
    setField("Why?");
    const lessonPage = document.querySelector<HTMLElement>(".session.in-front main.surface")!;
    const late = { held: 0, released: 0 };
    let heldAt: number | null = null;
    const hold = (event: Event) => {
      if (event.target !== lessonPage || !event.isTrusted) return;
      event.stopImmediatePropagation();
      late.held += 1;
      heldAt ??= lessonPage.scrollHeight;
    };
    const release = () => {
      heldAt = null;
      late.released += 1;
      lessonPage.dispatchEvent(new Event("scroll"));
    };
    const grown = new MutationObserver(() => {
      if (heldAt !== null && lessonPage.scrollHeight > heldAt + 6) release();
    });
    window.addEventListener("scroll", hold, true);
    grown.observe(lessonPage, { childList: true, subtree: true, characterData: true });
    onField({ key: "Enter" });
    await until("the quoted words sent", () => (session().live?.learner ?? "").startsWith("> "), 3000);
    steps.quoted = { said: session().live?.said, drawn: document.querySelector(".session.in-front .said-quote") !== null };
    if (session().live?.said !== "talk") throw new Error("a message quoting a passage was not sent as talk");
    await until("the reply to the quoted words", () => idle(), 10_000);
    window.removeEventListener("scroll", hold, true);
    grown.disconnect();
    if (heldAt !== null) release();
    steps.lateScroll = late;
    setField("");

    const fromEnd = () => {
      const lesson = document.querySelector<HTMLElement>(".session.in-front main.surface");
      return lesson ? Math.round(lesson.scrollHeight - lesson.clientHeight - lesson.scrollTop) : null;
    };
    const drift: Record<string, number | null> = { afterQuote: fromEnd() };
    steps.drift = drift;

    steps.controls = ["more", "plan-toggle", "settings", "tab-1", "tab-2", "home", "new-tab"].filter((name) => document.querySelector(`[data-drive="${name}"]`) !== null);
    if ((steps.controls as string[]).length !== 7) throw new Error(`a visible control is missing: ${JSON.stringify(steps.controls)}`);
    const planWord = document.querySelector('[data-drive="plan-toggle"]')?.textContent ?? null;
    if (planWord !== "Plan") throw new Error(`the tab bar's switch for the plan says ${JSON.stringify(planWord)}`);
    press("k", { meta: true });
    await until("the command menu, in a session", () => document.querySelector('[data-drive="command-plan"]') !== null, 3000);
    const planCommand = document.querySelector('[data-drive="command-plan"] .row-label')?.textContent ?? null;
    press("Escape");
    await until("the command menu to go, in a session", () => document.querySelector('[data-layer="Commands"]') === null, 3000);
    if (planCommand !== "Show plan") throw new Error(`⌘K's command for the plan says ${JSON.stringify(planCommand)}`);
    actions.run("plan");
    await until("the plan beside the lesson", () => document.querySelector(".session.in-front .panel") !== null, 3000);
    await wait(300);
    drift.planShown = fromEnd();
    actions.run("plan");
    await until("the plan to go", () => document.querySelector(".session.in-front .panel") === null, 3000);
    await wait(300);
    drift.planHidden = fromEnd();
    if ((drift.afterQuote ?? 0) > 6 || (drift.planHidden ?? 0) > 6) throw new Error(`the lesson let go of its end: ${JSON.stringify(drift)}`);

    const sessionTab = actions.active();
    actions.run("home");
    await until("Home", () => actions.active() === "home", 3000);
    await wait(300);
    actions.run("tab", sessionTab);
    await until("the session in front again", () => actions.active() === sessionTab, 3000);
    await wait(300);
    steps.backFocus = document.activeElement?.getAttribute("data-drive") ?? document.activeElement?.tagName ?? null;
    const backLine = actions.session()?.probe().line;
    const lesson = document.querySelector<HTMLElement>(".session.in-front main.surface");
    const where = lesson ? { fromFoot: Math.round(lesson.scrollHeight - lesson.clientHeight - lesson.scrollTop), footTop: Math.round(document.querySelector(".session.in-front .foot")?.getBoundingClientRect().top ?? -1), bottom: Math.round(lesson.getBoundingClientRect().bottom) } : null;
    if (steps.backFocus !== "composer") throw new Error(`the session's line (${String(backLine)}, ${JSON.stringify(where)}) did not take the focus back: ${String(steps.backFocus)}`);

    const progress = await api.progress(null, folder);
    steps.progress = { confirmed: progress.confirmed.length, message: progress.message };

    await actions.session()!.say("One more.\n\n**Task.** What is 3 + 3?", "talk");
    await until("the task the session is left with", () => idle() && actions.session()?.probe().task?.includes("3 + 3") === true, 10_000);

    actions.run("close-session");
    await until("the session's tab to close", () => actions.session() === null && actions.tabs().length === 1, 10_000);
    const after = await api.home(folder);
    steps.afterStop = { can_resume: after.can_resume, stopped_at: after.last?.stopped_at ?? null };
    if (!after.can_resume) throw new Error("the lesson was not kept for the next sitting");
    const sessions = await api.lessons(folder);
    steps.sessions = sessions.map((each) => ({ title: each.title, turns: each.turns }));
    if (sessions.length === 0 || sessions[0].turns === 0) throw new Error("the session was not listed on its workspace's page");
    const turns = await api.lessonTurns(folder, sessions[0].session);
    if (turns.length === 0) throw new Error("the session's turns were not kept to be read again");
    if (!turns.some((turn) => turn.intent === "talk")) throw new Error("a kept turn does not say it was sent as talk");
    if (turns.find((turn) => turn.learner === "Here is my working")?.intent != null) throw new Error("words sent with no task in view were kept as sent with an intent");

    const leftByOlderWindow: Record<string, unknown> = { task: "**Task.** What is 3 + 3?" };
    keepPlace(placeKey(folder, sessions[0].session, ""), leftByOlderWindow as Partial<Place>);
    actions.openSession(folder, sessions[0].session, sessions[0].title ?? "");
    await until("the session again", () => document.querySelector(".session.in-front .now.lesson article") !== null, 10_000);
    await wait(1500);
    const folded = {
      page: actions.session()?.probe().page ?? null,
      words: document.querySelector(".session.in-front .answer-task")?.textContent ?? null,
      lessonHidden: (document.querySelector<HTMLElement>(".session.in-front .now.lesson article")?.offsetHeight ?? -1) === 0,
      line: document.querySelector('.session.in-front [data-drive="look-back"]')?.textContent ?? null,
    };
    steps.reopenedFolded = folded;
    if (folded.page !== "folded" || !folded.words?.includes("3 + 3") || !folded.lessonHidden || !folded.line?.includes("Show the lesson"))
      throw new Error(`a session opened again with its task open is not the task alone: ${JSON.stringify(folded)}`);
    press("ArrowUp", { meta: true });
    await until("⌘↑ to open the lesson over the task", () => actions.session()?.probe().page === "open", 3000);
    await wait(300);
    const openLine = document.querySelector('.session.in-front [data-drive="look-back"]')?.textContent ?? null;
    if (!openLine?.includes("Hide the lesson")) throw new Error(`the open lesson's line does not say it hides the lesson: ${JSON.stringify(openLine)}`);
    const head = document.querySelector(".session.in-front .now.lesson .head")!.getBoundingClientRect().top;
    const first = document.querySelector(".session.in-front .now.lesson article")!.getBoundingClientRect().top;
    const again = await api.lessonTurns(folder, sessions[0].session);
    steps.reopened = { headAbove: head < first, turns: again.length, open: session().open };
    if (!(head < first)) throw new Error("a reopened session drew its turns above its heading");
    if (again.length !== turns.length || session().open) throw new Error("opening a kept session sent something");
    if (!document.querySelector(".session.in-front .you.talk")) throw new Error("talk in a reopened session is not drawn as talk");
    const probe = actions.session()?.probe();
    steps.reopened = { ...(steps.reopened as object), line: probe?.line, task: probe?.task !== null };
    if (probe?.line === "raised") throw new Error("a reopened session opened with its line raised over the page");

    {
      const lineNow = () => document.querySelector<HTMLTextAreaElement>('.session.in-front [data-drive="composer"]')!;
      const view = document.querySelector<HTMLElement>(".session.in-front main.surface")!;
      const write = (value: string) => {
        const line = lineNow();
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(line, value);
        line.dispatchEvent(new Event("input", { bubbles: true }));
      };
      const frame = () =>
        within("drawing the window's next frame (a covered window draws none)", new Promise<void>((done) => requestAnimationFrame(() => done())), 10_000);
      const copy = () => document.querySelector(".session.in-front .foot .parked") !== null;
      const place = () => Math.round(lineNow().getBoundingClientRect().bottom - view.getBoundingClientRect().bottom);

      lineNow().focus();
      let words = "one\ntwo\nthree\nfour\nfive\nsix";
      write(words);
      await wait(300);
      const scrolls = view.scrollHeight - view.clientHeight;
      const rest = place();
      let lowest = rest;
      for (let n = 0; n < 14; n++) {
        words = words.slice(0, -1);
        write(words);
        await frame();
        await frame();
        lowest = Math.max(lowest, place());
      }
      await wait(200);
      const fromFoot = fromEnd() ?? 0;
      write("");

      await actions.session()!.say("Before that.", "talk");
      await until("the reply that opens the session again", () => idle() && session().live?.learner === "Before that.", 15_000);
      lineNow().focus();
      write("the caret stays in these words");
      lineNow().setSelectionRange(9, 9);
      void actions.session()!.say("By the way.", "talk");
      await until("the reply that ends while words are on the line", () => idle() && session().live?.learner === "By the way.", 15_000);
      await wait(300);
      const caret = lineNow().selectionStart;
      write("");

      const tall = `SLOW\n${Array.from({ length: 60 }, (_, n) => `row ${n + 1}`).join("\n")}`;
      void actions.session()!.say(tall, "talk");
      await until("the slow reply to begin", () => session().live?.learner.startsWith("SLOW") === true && !idle(), 10_000);
      let looks = 0;
      let copies = 0;
      const began = performance.now();
      while (performance.now() - began < 1500) {
        looks += 1;
        if (copy()) copies += 1;
        await wait(50);
      }
      await api.interrupt();
      await until("the slow reply to stop", () => idle(), 15_000);
      const atStop = copy();
      const afterStop: boolean[] = [];
      for (let n = 0; n < 20; n++) {
        afterStop.push(copy());
        await wait(50);
      }
      const changed = afterStop.filter((each) => each !== atStop).length;

      steps.steadyLine = { scrolls, rest, lowest, fromFoot, caret, looks, copies, atStop, changed };
      const wrong: string[] = [];
      if (scrolls < 100) wrong.push(`the lesson was too short to scroll (${scrolls} px), so the line could not be seen to move`);
      if (lowest - rest > 2 || fromFoot > 6) wrong.push(`taking words off the line left it ${lowest - rest} px under its place, the lesson ${fromFoot} px from its end`);
      if (caret !== 9) wrong.push(`a reply ending moved the caret from 9 to ${caret}`);
      if (copies === 0) wrong.push(`the task's copy did not come in while the reply to a question streamed with the task out of view, in ${looks} looks`);
      if (changed > 0) wrong.push(`the task's copy came or went after the reply had ended, in ${changed} of ${afterStop.length} looks`);
      if (wrong.length > 0) throw new Error(wrong.join("; "));
    }
    await actions.session()!.say("6", "answer");
    await until("the reply to the reopened session's answer", () => idle() && session().live?.learner.includes("6") === true, 15_000);
    const afterAnswer = actions.session()?.probe().task ?? null;
    await actions.session()!.say("Another.\n\n**Task.** What is 5 + 5?", "talk");
    await until("the reply that poses the next task", () => idle() && session().live?.learner.includes("Another") === true, 15_000);
    const nextTask = actions.session()?.probe().task ?? null;
    steps.reopenedTask = { left: probe?.task ?? null, afterAnswer, next: nextTask };
    if (!probe?.task?.includes("3 + 3")) throw new Error(`a reopened session did not have the task it was left with: ${JSON.stringify(steps.reopenedTask)}`);
    if (afterAnswer !== null) throw new Error(`a task answered in a reopened session stayed open: ${JSON.stringify(steps.reopenedTask)}`);
    if (!nextTask?.includes("5 + 5")) throw new Error(`the task posed in a reopened session is not the open one: ${JSON.stringify(steps.reopenedTask)}`);

    const keptTurns = async () => (await api.lessonTurns(folder, sessions[0].session)).length;
    const keptBefore = await keptTurns();
    const lessonLine = () => document.querySelector<HTMLTextAreaElement>('.session.in-front [data-drive="composer"]')!;
    lessonLine().focus();
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(lessonLine(), "EXPIRED 10");
    lessonLine().dispatchEvent(new Event("input", { bubbles: true }));
    lessonLine().dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    await until("the notice that Claude Code is signed out", () => actions.session()?.probe().busy === false && session().notices.some((notice) => notice.kind === "sign_in"), 15_000);
    await wait(300);
    const signedOut = {
      shownAsReply: document.querySelector(".session.in-front .now.lesson")?.textContent?.includes("Failed to authenticate") ?? true,
      onLine: lessonLine().value,
      task: actions.session()?.probe().task ?? null,
      kept: (await keptTurns()) - keptBefore,
      signIn: document.querySelector('.session.in-front [data-drive="sign-in"]') !== null,
      home: "",
    };
    const lessonTab = actions.active();
    actions.run("home");
    await until("Home to ask for the sign-in", () => document.querySelector('[data-page="home"] h1')?.textContent?.includes("Sign in") === true, 10_000).catch(() => {});
    signedOut.home = document.querySelector('[data-page="home"] h1')?.textContent ?? "";
    actions.run("tab", lessonTab);
    await until("the session in front again", () => actions.active() === lessonTab, 3000);
    steps.signedOut = signedOut;
    if (signedOut.shownAsReply) throw new Error("Claude Code's line for a sign-in that ran out was drawn in the lesson");
    if (signedOut.onLine !== "EXPIRED 10") throw new Error(`an answer that reached no tutor is not back on the line: ${JSON.stringify(signedOut)}`);
    if (!signedOut.task?.includes("5 + 5")) throw new Error(`an answer that reached no tutor closed its task: ${JSON.stringify(signedOut)}`);
    if (signedOut.kept !== 0) throw new Error(`an answer that reached no tutor was kept as a turn: ${JSON.stringify(signedOut)}`);
    if (!signedOut.signIn) throw new Error("the notice that Claude Code is signed out has no Sign in");
    if (!signedOut.home.includes("Sign in")) throw new Error(`Home still took Claude Code as signed in: ${signedOut.home}`);
    document.querySelector<HTMLElement>('.session.in-front [data-drive="sign-in"]')!.click();
    let signedIn = false;
    for (let tries = 0; tries < 100 && !signedIn; tries++) {
      signedIn = (await api.account()).signed_in;
      if (!signedIn) await wait(150);
    }
    if (!signedIn) throw new Error("Sign in on the notice did not start Claude Code's sign-in");
    lessonLine().focus();
    lessonLine().dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    await until("the reply to the answer sent again", () => idle() && session().live?.learner === "EXPIRED 10" && (session().live?.done.join(" ").includes("You said") ?? false), 15_000);
    const sentAgain = { kept: (await keptTurns()) - keptBefore, task: actions.session()?.probe().task ?? null, onLine: lessonLine().value };
    steps.sentAgain = sentAgain;
    if (sentAgain.kept !== 1 || sentAgain.task !== null || sentAgain.onLine !== "") throw new Error(`the answer sent again after the sign-in was not taken as one answer: ${JSON.stringify(sentAgain)}`);

    {
      const view = document.querySelector<HTMLElement>(".session.in-front main.surface")!;
      const frame = () => within("drawing the window's next frame (a covered window draws none)", new Promise<void>((done) => requestAnimationFrame(() => done())), 10_000);
      const line = () => document.querySelector<HTMLTextAreaElement>('.session.in-front [data-drive="composer"]')!;
      const writeLine = (value: string) => {
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(line(), value);
        line().dispatchEvent(new Event("input", { bubbles: true }));
      };
      const key = (init: KeyboardEventInit, on: EventTarget = document.body) => on.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }));
      const paragraphs = (name: string, count: number) =>
        Array.from({ length: count }, (_, n) => `${name} paragraph ${n + 1} says that the half-width falls because $t^*$ also falls, so that the reply runs well past one screen.`).join("\n\n");
      const block = (words: string) => [...document.querySelectorAll<HTMLElement>(".session.in-front .now > article.turn .tutor .prose > *")].reverse().find((each) => (each.textContent ?? "").includes(words)) ?? null;
      const topOf = (words: string) => block(words)?.getBoundingClientRect().top ?? Number.NaN;
      const written = (words: string) => {
        const live = session().live;
        return live !== null && tutorText(live).includes(words);
      };
      const select = (el: Element) => {
        (document.activeElement as HTMLElement | null)?.blur();
        const range = document.createRange();
        range.selectNodeContents(el);
        window.getSelection()!.removeAllRanges();
        window.getSelection()!.addRange(range);
      };
      const clearLine = async () => {
        line().focus();
        writeLine("");
        key({ key: "Backspace" }, line());
        key({ key: "Escape" }, line());
        await frame();
      };
      const steady: Record<string, unknown> = {};
      steps.steadyPage = steady;

      const sample = document.createElement("div");
      sample.style.cssText = "position: absolute; left: -9999px; top: 0; width: 400px";
      sample.innerHTML = '<article class="turn open"><div class="tutor"><div class="prose"><p>Read the interval first.</p><p>What is <em>nine</em> plus 9?</p><p>After it.</p></div></div></article>';
      document.body.appendChild(sample);
      const [lead, question, after] = Array.from(sample.querySelectorAll("p"));
      const span = (from: Node, at: number, to: Node, end: number) => {
        const range = document.createRange();
        range.setStart(from, at);
        range.setEnd(to, end);
        return range;
      };
      const posed = "What is *nine* plus 9?";
      const judged = {
        paragraph: inTask(span(question, 0, question, question.childNodes.length), posed),
        tripleClick: inTask(span(question, 0, after, 0), posed),
        words: inTask(span(question.lastChild!, 1, question.lastChild!, 5), posed),
        lead: inTask(span(lead, 0, lead, lead.childNodes.length), posed),
        leadAndTask: inTask(span(lead, 0, question, question.childNodes.length), posed),
        wordInLead: inTask(span(lead.firstChild!, 0, lead.firstChild!, 4), posed),
      };
      sample.remove();
      steady.judged = judged;
      if (!judged.paragraph || !judged.tripleClick || !judged.words || judged.lead || judged.leadAndTask || judged.wordInLead) throw new Error(`a selection was judged the task, or not, by where its ends sit: ${JSON.stringify(judged)}`);

      const pane = document.createElement("div");
      pane.style.cssText = "position: absolute; left: -9999px; top: 0; width: 400px; height: 200px; overflow-y: auto; overflow-anchor: none";
      const blocks = Array.from({ length: 8 }, (_, n) => `<p style="height: 60px; margin: 0">Block ${n + 1}</p>`).join("");
      pane.innerHTML = `<section class="now"><article class="turn" data-ms="1"><p class="you" style="margin: 0">Asked.</p><div class="tutor"><div class="prose">${blocks}</div></div></article><div class="foot" style="height: 40px"></div></section>`;
      document.body.appendChild(pane);
      const page = pane.querySelector<HTMLElement>(".now")!;
      const place = readingSpot(() => pane, () => page);
      place.settle();
      pane.scrollTop -= 230;
      place.scrolled();
      const atTop = [...page.querySelectorAll<HTMLElement>(".prose > p")].find((each) => each.getBoundingClientRect().bottom > pane.getBoundingClientRect().top + 1)!;
      const name = atTop.textContent ?? "";
      const was = atTop.getBoundingClientRect().top;
      const prose = page.querySelector<HTMLElement>(".prose")!;
      prose.innerHTML = blocks;
      const heading = document.createElement("h2");
      heading.style.cssText = "height: 30px; margin: 0";
      page.querySelector(".tutor")!.before(heading);
      place.settle();
      const redrawn = [...prose.children].find((each) => each.textContent === name)!.getBoundingClientRect().top;
      pane.remove();
      steady.redrawn = { name, moved: Math.round(redrawn - was) };
      if (Math.abs(redrawn - was) > 1) throw new Error(`a paragraph redrawn with a heading added above it was not held: ${JSON.stringify(steady.redrawn)}`);

      await actions.session()!.say(`${paragraphs("Earlier", 10)}\n\n**Task.** What is 7 + 7?`, "talk");
      await until("the long reply that poses a task", () => idle() && actions.session()?.probe().task?.includes("7 + 7") === true, 15_000);

      const copied = () => document.querySelector(".session.in-front .foot .parked") !== null;
      const watchCopy = async (words: string, said: "talk" | "help" | "answer") => {
        const before = copied();
        void actions.session()!.say(words, said);
        await until(`"${words}" sent`, () => session().live?.learner === words, 10_000);
        const seen: { copy: boolean; streaming: boolean }[] = [];
        while (!idle()) {
          seen.push({ copy: copied(), streaming: session().live?.status === "writing" });
          await frame();
        }
        for (let n = 0; n < 12; n++) {
          seen.push({ copy: copied(), streaming: false });
          await frame();
        }
        const ended = seen.map((each) => each.streaming).lastIndexOf(true);
        const atEnd = ended >= 0 ? seen[ended].copy : null;
        return {
          before,
          cameWhileStreaming: seen.some((each) => each.copy && each.streaming),
          shownInLooks: seen.filter((each) => each.copy).length,
          looks: seen.length,
          changedAfterStreaming: atEnd === null ? 0 : seen.slice(ended + 1).filter((each) => each.copy !== atEnd).length,
        };
      };
      const sideQuestion = await watchCopy("PACED SAVING LONGER A side question: why does the interval narrow?", "talk");
      steady.copyForQuestion = sideQuestion;
      if (sideQuestion.before || !sideQuestion.cameWhileStreaming || sideQuestion.changedAfterStreaming > 0)
        throw new Error(`with the task on screen, a question's reply that pushed it away did not bring its copy in while it streamed, or the copy changed after: ${JSON.stringify(sideQuestion)}`);
      const lost = await watchCopy("PACED SAVING LONGER I'm lost", "help");
      steady.copyForHelp = lost;
      if (!lost.before || lost.shownInLooks !== lost.looks) throw new Error(`"I'm lost" was not treated as a question: the task's copy went: ${JSON.stringify(lost)}`);
      const answered = await watchCopy("PACED SAVING 14", "answer");
      steady.copyForAnswer = answered;
      if (!answered.before || answered.shownInLooks > 0) throw new Error(`after an answer the task's copy stayed or came back: ${JSON.stringify(answered)}`);

      void actions.session()!.say(`PACED SAVING\n\n${paragraphs("Ending", 4)}\n\n**Task.** What is 7 + 7?`, "talk");
      await until("the paced reply's last words", () => written("Ending paragraph 4 says") && written("What is 7 + 7?"), 15_000);
      await frame();
      const atEnd: number[] = [];
      let sawSaving = false;
      while (!idle()) {
        atEnd.push(topOf("Ending paragraph 3 says"));
        sawSaving ||= document.querySelector(".session.in-front .working")?.textContent?.includes("saving") === true;
        await frame();
      }
      for (let n = 0; n < 10; n++) {
        atEnd.push(topOf("Ending paragraph 3 says"));
        await frame();
      }
      steady.atEnd = { sawSaving, spread: Math.round(Math.max(...atEnd) - Math.min(...atEnd)) };
      if (!sawSaving) throw new Error("the stand-in's reply never showed that it was saving the work");
      if (Math.max(...atEnd) - Math.min(...atEnd) > 1) throw new Error(`at the end of the page, the reply moved while the tutor saved and finished: ${JSON.stringify(steady.atEnd)}`);

      void actions.session()!.say(`PACED SAVING\n\n${paragraphs("Reading", 14)}\n\n**Task.** What is 8 + 8?`, "talk");
      await until("the paced reply to run past a screen", () => written("Reading paragraph 8 says"), 15_000);
      await frame();
      const reading = [...document.querySelectorAll<HTMLElement>(".session.in-front .now > article.turn:last-of-type .tutor .prose > p")].find((each) => each.getBoundingClientRect().top > view.getBoundingClientRect().top + 60);
      const held = reading?.textContent?.match(/Reading paragraph \d+ says/)?.[0] ?? "";
      if (!held) throw new Error("no paragraph of the paced reply was in view to hold");
      const before = topOf(held);
      const spacer = document.createElement("div");
      spacer.style.height = "120px";
      view.scrollTop -= 300;
      document.querySelector(".session.in-front .now > article.turn")!.prepend(spacer);
      await frame();
      await frame();
      const sameFrame = Math.round(topOf(held) - (before + 300));
      line().focus();
      writeLine("w");
      await frame();
      const raised = document.querySelector(".session.in-front .foot.raised") !== null;
      const kept: number[] = [];
      let strip: { inside: boolean } | null = null;
      while (!idle()) {
        kept.push(topOf(held));
        const working = document.querySelector<HTMLElement>(".session.in-front .foot.raised > .working");
        if (working?.textContent?.includes("saving")) strip = { inside: working.getBoundingClientRect().top >= working.parentElement!.getBoundingClientRect().top };
        await frame();
      }
      for (let n = 0; n < 10; n++) {
        kept.push(topOf(held));
        await frame();
      }
      spacer.remove();
      await frame();
      await frame();
      kept.push(topOf(held));
      const task = actions.session()?.probe().task ?? "";
      steady.readingBack = { held, sameFrame, raised, strip, spread: Math.round(Math.max(...kept) - Math.min(...kept)), task };
      if (Math.abs(sameFrame) > 1) throw new Error(`a change above the page in the frame the reader scrolled moved the paragraph read by ${sameFrame} px: ${JSON.stringify(steady.readingBack)}`);
      if (!task.includes("8 + 8")) throw new Error(`the paced reply's task did not become the open one: ${JSON.stringify(steady.readingBack)}`);
      if (Math.max(...kept) - Math.min(...kept) > 1) throw new Error(`scrolled up, the paragraph read moved while the reply set a new task, saved and ended: ${JSON.stringify(steady.readingBack)}`);
      if (!raised || strip?.inside !== true) throw new Error(`with the line raised, "saving your work" was not inside the line's panel: ${JSON.stringify(steady.readingBack)}`);
      await clearLine();

      const passage = block("Reading paragraph 5 says")!;
      select(passage);
      for (const type of ["pointerdown", "mousedown", "pointerup", "mouseup", "click"]) passage.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, button: 0 }));
      await frame();
      const afterClick = { selected: (window.getSelection()?.toString() ?? "").includes("Reading paragraph 5 says"), focus: document.activeElement?.getAttribute("data-drive") ?? document.activeElement?.tagName ?? null };
      steady.afterClick = afterClick;
      if (!afterClick.selected || afterClick.focus === "composer") throw new Error(`the click that ends a drag lost the selection: ${JSON.stringify(afterClick)}`);

      select(block("Reading paragraph 5 says")!);
      key({ key: "w" });
      await frame();
      await frame();
      const quote = document.querySelector<HTMLElement>(".session.in-front .line.lesson .quote");
      const field = document.querySelector<HTMLElement>(".session.in-front .foot .field-row")!.getBoundingClientRect();
      const overPassage = {
        page: actions.session()?.probe().page ?? null,
        tex: quote?.querySelector('annotation[encoding="application/x-tex"]')?.textContent ?? null,
        doubled: /t\s*∗\s*t\s*∗/.test(quote?.textContent ?? ""),
        lineInView: field.bottom <= view.getBoundingClientRect().bottom + 1,
      };
      steady.overPassage = overPassage;
      if (overPassage.page !== "open") throw new Error(`typing over a passage that is not the task folded the lesson: ${JSON.stringify(overPassage)}`);
      if (overPassage.tex !== "t^*" || overPassage.doubled) throw new Error(`the quote did not keep the passage's formula as one formula: ${JSON.stringify(overPassage)}`);
      if (!overPassage.lineInView) throw new Error(`the line holding the quote was left below the window: ${JSON.stringify(overPassage)}`);
      await clearLine();

      const asked = document.querySelector<HTMLElement>(".session.in-front .turn.open .prose p.task") ?? block("8 + 8");
      if (!asked) throw new Error("the open task's paragraph is not on the page");
      select(asked);
      key({ key: "w" });
      await frame();
      const overTask = { page: actions.session()?.probe().page ?? null, quoted: document.querySelector(".session.in-front .line.lesson .quote") !== null };
      steady.overTask = overTask;
      if (overTask.page !== "folded" || !overTask.quoted) throw new Error(`typing over the whole task did not fold the lesson into the task: ${JSON.stringify(overTask)}`);
      await clearLine();
      actions.run("look-back");
      await until("the lesson open again over the task", () => actions.session()?.probe().page === "open", 3000);

      window.getSelection()!.removeAllRanges();
      line().focus();
      writeLine("draft");
      line().blur();
      key({ key: "ArrowDown" });
      const reads = document.activeElement !== line();
      key({ key: "v", metaKey: true });
      const pastes = document.activeElement === line();
      line().blur();
      key({ key: "Backspace" });
      const erases = document.activeElement === line();
      line().blur();
      key({ key: "Dead" });
      const accents = document.activeElement === line();
      steady.keys = { reads, pastes, erases, accents };
      if (!reads || !pastes || !erases || !accents) throw new Error(`from the page, a key that writes did not reach the line, or a reading key did: ${JSON.stringify(steady.keys)}`);
      await clearLine();
    }
    actions.run("close-session");
    await until("the session's tab to close again", () => actions.session() === null, 10_000);

    actions.newSession(folder);
    const word = await openModelRow("a new session's model, under More", () => true, 10_000);
    modelRow()!.click();
    await until("the list of models", () => document.querySelector(".session.in-front .model-list") !== null, 3000);
    const rows = document.querySelectorAll('.session.in-front [data-drive="model-row"]').length;
    const chosen = document.querySelector<HTMLElement>('.session.in-front [data-drive="model-row"].on')?.dataset.value ?? null;
    press("Escape");
    await wait(150);
    steps.startLine = { word, rows, chosen, closed: document.querySelector(".session.in-front .model-list") === null, focus: document.activeElement?.getAttribute("data-drive") ?? null };
    if (word !== "Opus 5.5" || rows !== 5 || chosen !== "opus") throw new Error(`the start line's model is not the pick: ${JSON.stringify(steps.startLine)}`);
    if (document.querySelector(".session.in-front .model-list") !== null || document.activeElement?.getAttribute("data-drive") !== "composer") throw new Error(`Esc did not close the list and go back to the line: ${JSON.stringify(steps.startLine)}`);
    actions.run("close-session");
    await until("the new session's tab to close", () => actions.session() === null, 10_000);
    await wait(600);
    const left = (await api.windowState()) as { v?: number; tabs?: unknown[] } | null;
    steps.windowState = left ? { v: left.v, tabs: left.tabs?.length } : null;
    if (left?.v !== 1) throw new Error("how the window was left was not kept");
  } catch (problem) {
    failure = String(problem);
  }
  const errors = [...window.__mgErrors];
  await api.selftestReport({ ok: failure === null && errors.length === 0, failure, errors, steps });
}
