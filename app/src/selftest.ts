// A check the window runs on itself when started with MAGNESIUM_APP_SELFTEST.
//
// It drives the real window, against whatever agent program the environment
// names. scripts/selftest.sh names the stand-in, so the check starts no model.
// It proves the parts that only exist once the window is running: the calls
// between the page and the shell, the content policy, that mathematics, a
// diagram and a PDF page draw (and a PDF opens beside a page, with words that
// can be selected), and that the window's structure holds: Home
// with its first row, a workspace's page, Settings in a tab, ⌘K with its few
// commands, a session in a tab with talk sent as talk, the plan beside it,
// a file attached by name, and no path or file text shown anywhere.

import type { Actions } from "./App";
import { attachedWords, splitAttached, tutorText, type LessonTurn } from "./lesson";
import { tinyPdf } from "./demoPdf";
import { drawPage, openPdf } from "./pdf";
import { hasTask, renderInline, renderMarkdown, renderPlain, setSourcesApart, withoutOpenMath } from "./render";
import { comboOf, keyName, parseCombo, keepPlace, placeKey, type Place } from "./prefs";
import { sourceRef } from "./sources";
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

/** A step that never finishes is a failure with a name, not a window that says nothing. */
function within<T>(what: string, work: Promise<T>, limitMs = 20_000): Promise<T> {
  return Promise.race([work, new Promise<T>((_, no) => window.setTimeout(() => no(new Error(`${what} did not finish in ${limitMs / 1000} s`)), limitMs))]);
}

/** A key pressed on the page, as the learner would press it. */
function press(key: string, mods: { meta?: boolean; shift?: boolean } = {}) {
  const target = (document.activeElement as HTMLElement | null) ?? document.body;
  target.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, metaKey: Boolean(mods.meta), shiftKey: Boolean(mods.shift) }));
}

/** What the window shows as interface must hold no path, no Markdown source and no settings file. */
function noRawFiles(where: string): void {
  const text = document.body.innerText;
  const found = [/Study\/[\w-]/, /\/Users\//, /\.magnesium/, /plan\.md/, /state\.json/, /settings\.conf/, /^#{1,3} /m, /\| --- \|/].find((pattern) => pattern.test(text));
  if (found) throw new Error(`${where} shows a path or file text (${found}): ${text.slice(0, 200)}`);
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

    // The setup calls, through the shell. The stand-in is signed in to a plan
    // and finds no API key, so choosing a key says the plan would be used.
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

    // The folder named at the start opens its page in the Home tab: a visible New session, and no path.
    await until("the workspace's page", () => document.querySelector('[data-page="workspace"] [data-drive="new-session"]') !== null, 10_000);
    noRawFiles("the workspace's page");
    steps.workspace = { title: document.querySelector(".page-title")?.textContent ?? null };

    // Home: every workspace, and a first row Return starts from.
    actions.goWorkspace(null);
    await until("Home", () => document.querySelector('[data-page="home"] [data-drive="start"]') !== null, 10_000);
    await wait(100);
    steps.homePage = { rows: document.querySelectorAll('[data-page="home"] [data-row]').length, first: document.querySelector('[data-drive="start"]')?.textContent ?? null };
    if (document.activeElement?.getAttribute("data-drive") !== "start") throw new Error("Home's first row does not have the focus");
    noRawFiles("Home");

    // Settings: a page in its own tab, with its sections, no file and no path.
    press(",", { meta: true });
    await until("the Settings tab", () => document.querySelector('[data-page="settings"]') !== null, 3000);
    // Settings is named once in the tab bar: its tab, not the bar's button as well.
    if (document.querySelector('[data-drive="settings"]') !== null) throw new Error("Settings is in the tab bar twice");
    const sections = Array.from(document.querySelectorAll(".settings-page .section-head")).map((head) => head.textContent);
    steps.settings = { sections, keys: document.querySelectorAll(".key-row").length, colours: document.querySelectorAll('input[type="color"]').length };
    for (const wanted of ["Reading", "Appearance", "Keys", "Lessons", "About"]) if (!sections.includes(wanted)) throw new Error(`Settings has no ${wanted} section`);
    if (document.querySelectorAll('[data-page="settings"] textarea').length > 0) throw new Error("Settings shows a text to edit");
    noRawFiles("Settings");
    press("w", { meta: true });
    await until("Settings to close", () => document.querySelector('[data-page="settings"]') === null, 3000);

    // ⌘K holds only the window's own few commands, and goes with Esc.
    press("k", { meta: true });
    await until("the command menu", () => document.querySelector('[data-layer="Commands"]') !== null, 3000);
    const commands = Array.from(document.querySelectorAll('[data-layer="Commands"] .row-label')).map((row) => row.textContent);
    steps.commands = commands;
    if (commands.length > 6 || !commands.includes("Home") || !commands.includes("Settings") || !commands.includes("New session")) throw new Error(`⌘K holds other things: ${commands.join(", ")}`);
    press("Escape");
    await until("the command menu to go", () => document.querySelector('[data-layer="Commands"]') === null, 3000);

    // The learner's keys: a combination is read from a key press and written as the Mac writes it.
    const combo = comboOf({ key: "j", code: "KeyJ", metaKey: true, ctrlKey: false, altKey: false, shiftKey: true });
    steps.keys = { combo, name: combo ? keyName(combo) : null, none: parseCombo("none") };
    if (combo !== "cmd+shift+j" || keyName(combo) !== "⇧⌘J" || parseCombo("none") !== null) throw new Error(`a key was not read: ${JSON.stringify(steps.keys)}`);

    // The core's own words reach the screen with no path in them.
    const plain = [withoutPaths("Added 5 files to Study/materials."), withoutPaths("Change Study/midterm-1/plan.md"), withoutPaths("in .magnesium/backups"), withoutPaths("1/v in s/µM")];
    steps.withoutPaths = plain;
    if (plain.join("|") !== "Added 5 files to the material.|Change plan.md|as a backup|1/v in s/µM") throw new Error(`a path reached the screen: ${plain.join(" | ")}`);

    // A reopened session shows each of the record's marks under the answer it
    // was shown on live, by the record's id, also when several answers fall in
    // one minute. A turn kept before the window kept ids matches by the minute.
    {
      const minute = new Date(2026, 10, 13, 13, 15, 0).getTime();
      const row = (id: string, at = "2026-11-13 13:15", paper: string | null = null): Mark => ({ id, at, label: "sign of b", mark: "right", help: "none", cold: false, paper });
      const kept = (second: number, extra: Partial<SavedTurn> = {}): SavedTurn => ({ session: "s", ms: minute + second * 1000, learner: `answer ${second}`, tutor: "…", interrupted: false, intent: "answer", ...extra });
      // Each turn as "a-1+a-2" for its marks, "-" for none.
      const drawn = (turns: LessonTurn[]) => turns.map((turn) => (turn.marks ?? []).map((mark) => mark.id).join("+") || "-").join(" ");
      const posed = kept(0, { learner: "Go on", intent: "talk", noted: true, task: "Task 1", posed: true, mark_ids: [] });
      const triesTurns = rebuild([posed, kept(10, { noted: true, task: "Task 1", mark_ids: [] }), kept(20, { noted: true, task: "Task 1", mark_ids: [] }), kept(30, { noted: true, task: "Task 1", mark_ids: ["a-3"] })], [row("a-3")], "s");
      const reopened = {
        // Answers to two tasks in one minute, each marked.
        twoTasks: drawn(rebuild([kept(10, { mark_ids: ["a-1"] }), kept(20, { mark_ids: ["a-2"] })], [row("a-1"), row("a-2")], "s")),
        // Three tries at one task in one minute; the record marked the third.
        threeTries: drawn(triesTurns),
        // A row no kept answer had: from a terminal, at the same minute.
        noAnswerKept: drawn(rebuild([kept(10, { mark_ids: ["a-1"] }), kept(20, { mark_ids: [] })], [row("a-1"), row("t-9")], "s")),
        // Turns an earlier window kept, with no ids: by the minute, as before, and never a paper's row.
        older: drawn(rebuild([kept(10), kept(130)], [row("a-1"), row("p-1", "2026-11-13 13:16", "Paper 1"), row("a-2", "2026-11-13 13:17")], "s")),
        // An older turn and a newer one in the same minute: the older one does not take the newer one's mark.
        mixed: drawn(rebuild([kept(10), kept(20, { mark_ids: ["a-2"] })], [row("a-1"), row("a-2")], "s")),
        // A runtime that cannot list its marks: no mark.
        noRecord: drawn(rebuild([kept(10, { mark_ids: ["a-1"] }), kept(20)], null, "s")),
      };
      steps.reopenedMarks = reopened;
      const wanted = { twoTasks: "a-1 a-2", threeTries: "- - - a-3", noAnswerKept: "a-1 -", older: "a-1 a-2", mixed: "a-1 a-2", noRecord: "- -" };
      for (const [name, want] of Object.entries(wanted)) if (reopened[name as keyof typeof wanted] !== want) throw new Error(`a reopened session put a mark on the wrong answer (${name}): ${reopened[name as keyof typeof wanted]}, not ${want}`);
      // The step a turn's reply moved on to comes back with that turn, and a turn that began none has no head.
      const heads = rebuild([kept(0, { step: { n: 1, of: 3, name: "One" } }), kept(10), kept(20, { step: { n: 2, of: 3, name: "Two" } })], [], "s")
        .map((turn) => (turn.step ? `${turn.step.n}/${turn.step.of} ${turn.step.name}` : "-"))
        .join(" | ");
      steps.reopenedSteps = heads;
      if (heads !== "1/3 One | - | 2/3 Two") throw new Error(`a reopened session lost where its steps began: ${heads}`);
    }

    // Mathematics in an attribute is plain text; in the text it is KaTeX. Image maps are dropped.
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

    // One line of text anywhere draws its mathematics, and the learner's own
    // words keep every character that is not mathematics.
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

    // A reply that is still being written never shows the source of a
    // formula it is in the middle of. A price is not taken for a formula.
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

    // A task that runs over several blocks is one block, to the end of the reply.
    const task = document.createElement("div");
    task.innerHTML = renderMarkdown("An explanation.\n\n**Task.** Find the eigenvalues of\n\n$$A$$\n\nOne line.");
    const box = task.querySelector("div.task");
    steps.task = { boxes: task.querySelectorAll("div.task").length, blocks: box?.children.length ?? 0, before: task.firstElementChild?.tagName };
    if (!box || box.children.length !== 3 || task.firstElementChild?.tagName !== "P" || box.querySelectorAll(".katex-display").length !== 1) {
      throw new Error(`the task was not set apart as one block: ${task.innerHTML.slice(0, 200)}`);
    }

    // A link to a PDF or a text file is a source: shut as plain text until the
    // task is answered, open as a link after. A web link stays as it is.
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

    // The PDF reader and its worker load in this window, from its own files and under its content policy.
    const pdf = await within("opening a PDF", openPdf(tinyPdf(["one", "two", "three"])));
    const canvas = document.createElement("canvas");
    await within("drawing a PDF page", drawPage(pdf, 2, canvas, 300));
    const pixels = canvas.getContext("2d")?.getImageData(0, 0, canvas.width, canvas.height).data ?? new Uint8ClampedArray();
    let ink = 0;
    for (let i = 0; i < pixels.length; i += 4) if (pixels[i] < 128) ink += 1;
    steps.pdf = { pages: pdf.numPages, width: canvas.width, ink };
    if (pdf.numPages !== 3 || ink < 200) throw new Error(`the PDF page did not draw: ${JSON.stringify(steps.pdf)}`);

    // Course material through the shell: pasted text is saved as a text file in
    // the study folder; the core reads it back, and refuses what lies outside the folder.
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

    // The workspace lists its material by title, with no extension and no path, and shows it in the window.
    actions.goWorkspace(folder);
    // The material is folded to its count, and opens in place.
    await until("the material folded", () => document.querySelector('[data-page="workspace"] [data-drive="material"]') !== null, 10_000);
    if (document.querySelector('[data-page="workspace"] .file-row') !== null) throw new Error("the material was listed before it was opened");
    document.querySelector<HTMLElement>('[data-page="workspace"] [data-drive="material"]')!.click();
    await until("the material listed", () => document.querySelector('[data-page="workspace"] .file-row') !== null, 10_000);
    const titles = Array.from(document.querySelectorAll('[data-page="workspace"] .file-row .row-name')).map((row) => row.textContent ?? "");
    steps.materialTitles = titles;
    if (titles.some((title) => /\.[a-z0-9]{1,5}$/i.test(title) || title.includes("/"))) throw new Error(`material is listed by file name: ${titles.join(", ")}`);
    document.querySelector<HTMLElement>('[data-page="workspace"] .file-row')!.click();
    // It opens beside the page, in the panel, named by its title in the panel's switcher.
    await until("the material beside the page", () => document.querySelector('[data-role="side-panel"] .side-tab.here') !== null, 5000);
    const sheet = document.querySelector('[data-role="side-panel"] .side-tab.here')?.textContent?.replace(/×$/, "") ?? "";
    steps.materialSheet = sheet;
    if (/\.[a-z0-9]{1,5}$/i.test(sheet) || sheet.includes("/")) throw new Error(`material opened under its file name: ${sheet}`);
    document.querySelector<HTMLElement>('[data-role="side-panel"] .side-tab.here .side-close')?.click();
    await until("the material to close", () => document.querySelector('[data-role="side-panel"]') === null, 3000);

    // A PDF opens beside the page as its pages: drawn, their words in a layer the learner can select, and
    // → turns to the next page.
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

    // A pasted image is handed in as a file, and named in the message by the place it was written.
    const png = Uint8Array.from(atob("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="), (c) => c.charCodeAt(0));
    const kept = await within("handing in a pasted image", api.attachBytes("Pasted 10.41.png", png, "attached", folder));
    steps.attached = kept;
    if (!kept.startsWith("Study/handed-in/attached/")) throw new Error(`a pasted image was not handed in: ${kept}`);
    const asWork = attachedWords([kept], true);
    const toRead = attachedWords([kept], false);
    if (!asWork.startsWith("Photo of my working: ") || !/may be my working or course material/.test(toRead)) throw new Error(`a file was not named in plain words: ${asWork} / ${toRead}`);
    if (splitAttached(`b = 5\n\n${toRead}`).files[0] !== kept || splitAttached(`b = 5\n\n${toRead}`).text !== "b = 5") throw new Error("a file named in a message was not read back");

    // The library, the plan and the folder's sessions, through the shell.
    const listed = await api.workspaces();
    const plan = await api.plan(folder);
    steps.library = { workspaces: listed.length, first: listed[0]?.name, plan: { items: plan.items.length } };
    if (!listed.some((each) => each.folder === home.folder)) throw new Error("the open folder is not in the library");

    // A session's first line is the lesson's line. Before a lesson there is nothing to be stuck on: it ends in More
    // alone, and the help keys do nothing.
    // Before the first lesson the window has the list: it asked Claude Code as it opened, with no lesson started.
    // More's Model row names the default a new window took from it, the list has its rows, and Esc closes it.
    const front = <T extends Element = HTMLElement>(what: string) => document.querySelector<T>(`.session.in-front ${what}`);
    const modelRow = () => front('.line-menu [data-drive="model"]');
    const openModelRow = async (what: string, named: (word: string) => boolean, limit: number) => {
      let word: string | null = null;
      await until(what, () => {
        if (!modelRow()) front('[data-drive="more"]')?.click();
        // The row ends in "›", for what it opens.
        word = modelRow()?.querySelector(".line-menu-detail")?.textContent?.replace(/\s*›$/, "") ?? null;
        return word !== null && named(word);
      }, limit);
      return word as string | null;
    };
    actions.newSession(folder);
    await until("a new session's first line, ending in More", () => front('[data-line="rest"] [data-drive="more"]') !== null, 10_000);
    // A session not begun is named "New session" in its tab: the page above it already says the course.
    const newTab = document.querySelector('[role="tab"][aria-selected="true"] .tab-name')?.textContent?.trim() ?? null;
    steps.newSessionTab = newTab;
    if (newTab !== "New session") throw new Error(`a new session's tab is not named New session: ${JSON.stringify(newTab)}`);
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

    // Start: Home's first row, which opens the session in a tab of its own.
    const message = "Self-check. Inline $a^b$ and a diagram:\n\n```mermaid\ngraph LR\n  A[rate] --> B[reciprocal]\n```";
    actions.continueRecent();
    await until("a session's tab", () => actions.session() !== null, 5000);
    await until("the first reply", idle);
    if (document.querySelector(".session.in-front .now.lesson") === null) throw new Error("the lesson did not fill its tab");
    // The models came with the conversation, in the agent's own words, and a new window took its default by full id.
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

    // Stuck in a lesson: its rows open in the line, above the words, not over the page; Esc puts them away and
    // the keyboard goes back to the line.
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

    // The line's three looks: the ruled line keeps the bare line's "›" and adds one hairline under the words, from
    // the "›" to the column's end; the bare line and the soft field draw none. Set on the root as applyPrefs sets it.
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
          // The rule starts where the "›" hangs and ends at the row's right edge.
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

    // The model on the lesson's line: under More, the same list as the start line, now changing the running session.
    // A change goes to the agent at once; its next reply says what it ran on. It is set back so later steps see the pick.
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
      // The row reads "Haiku 5.5 · Low ›", joined by no-break spaces; the arrow is drawn, not said.
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

    // A card that arrives over an empty line no key has touched for a while takes the keyboard on its first choice.
    const writing = document.querySelector<HTMLTextAreaElement>('.session.in-front [data-drive="composer"]')!;
    writing.focus();
    await wait(2100);
    await actions.session()!.say("PERMISSION", "talk");
    await until("the permission card", () => session().cards.length > 0);
    await wait(100);
    steps.cardTookKeys = document.activeElement?.getAttribute("data-drive") ?? null;
    if (steps.cardTookKeys !== "allow") throw new Error(`a card over an idle, empty line did not take the keyboard: ${steps.cardTookKeys}`);
    steps.card = document.querySelector(".card .record-head")?.textContent ?? null;
    // The card says what the tutor asks in the learner's words, shows the command itself, and names what "always"
    // would allow by the rule the agent suggests saving.
    const card = {
      head: steps.card,
      what: document.querySelector(".card .card-what")?.textContent ?? null,
      exact: document.querySelector(".card .card-detail")?.textContent ?? null,
      always: document.querySelector('.card [data-drive="allow-always"]')?.textContent ?? null,
    };
    steps.cardWords = card;
    if (card.head !== "The tutor asks to run a command" || card.what !== "Build the paper as a PDF" || card.exact !== "tectonic paper.tex" || card.always !== "or always allow tectonic commands")
      throw new Error(`the permission card is not in the learner's words: ${JSON.stringify(card)}`);
    await api.decide(session().cards[0].id, "allow_once");
    await until("the reply after the permission", () => session().cards.length === 0 && idle());

    // One that arrives while there are words on the line leaves the keyboard there, so Space or Return cannot allow
    // by accident; ⇧⇥ from the line goes to the card's first choice.
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
    // With no card waiting, the line's way to a card is out of the page, not only hidden.
    await wait(100);
    if (document.querySelector(".session.in-front .to-card")) throw new Error("the way to a card is still under the line after the card was answered");

    // The learner's own line on screen draws its mathematics too, and a file sent with it is named, not its path.
    await actions.session()!.say(`my answer is $x^2$\n\n${toRead}`, "talk");
    await until("the reply to the answer", () => idle() && session().live?.learner.includes("my answer") === true);
    steps.saidMath = document.querySelectorAll(".session.in-front .you .katex").length;
    if (steps.saidMath === 0) throw new Error("the mathematics in the learner's own line did not draw");
    if (!document.querySelector(".session.in-front .sent-files")?.textContent?.includes(kept.split("/").pop() ?? kept)) throw new Error("the file sent was not named under the message");

    // No task the page can see, on Claude Code's engine, where the tutor may have set one it did not save: words on
    // the line go with nothing said of them, so the tutor may mark them. ⌥↩ still sends them as talk.
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

    // One line with a task open. Esc closes the symbols first, then puts the line away; the words stay.
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

    // ⌥↩ sends the words as talk, with a task open: never an answer.
    field.focus();
    setField("it is 4, I think");
    onField({ key: "Enter", altKey: true });
    await until("the words sent as talk", () => session().live?.learner === "it is 4, I think", 3000);
    if (session().live?.said !== "talk") throw new Error(`⌥↩ sent the words as ${String(session().live?.said)}, not as talk`);
    await until("the reply to the talk", () => idle(), 10_000);

    // A key typed with a passage selected: the line quotes it, and the message goes as talk.
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
    // A scroll event reaches the page a frame late, and later still on a busy Mac. This send is from the folded page,
    // which opens and changes its height twice before the reply's first words. The lesson's scroll events are held
    // back here until the page has grown: the order that let the lesson go of its end one run in six under load.
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

    // How far the lesson is from its end after each step that changes its layout: the line is its last line.
    const fromEnd = () => {
      const lesson = document.querySelector<HTMLElement>(".session.in-front main.surface");
      return lesson ? Math.round(lesson.scrollHeight - lesson.clientHeight - lesson.scrollTop) : null;
    };
    const drift: Record<string, number | null> = { afterQuote: fromEnd() };
    steps.drift = drift;

    // The quiet controls, and the plan beside the lesson, shown and hidden.
    steps.controls = ["more", "plan-toggle", "settings", "tab-1", "tab-2", "home", "new-tab"].filter((name) => document.querySelector(`[data-drive="${name}"]`) !== null);
    if ((steps.controls as string[]).length !== 7) throw new Error(`a visible control is missing: ${JSON.stringify(steps.controls)}`);
    const planWord = document.querySelector('[data-drive="plan-toggle"]')?.textContent ?? null;
    if (planWord !== "Plan") throw new Error(`the tab bar's switch for the plan says ${JSON.stringify(planWord)}`);
    // ⌘K names the same action by the same word.
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
    // The line is the lesson's last line: a reply that grew the page, and the panel shown and hidden, leave it in view.
    if ((drift.afterQuote ?? 0) > 6 || (drift.planHidden ?? 0) > 6) throw new Error(`the lesson let go of its end: ${JSON.stringify(drift)}`);

    // Home, then the session's tab again: its line takes the focus back.
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

    // Left with a task open, for the reopened session below.
    await actions.session()!.say("One more.\n\n**Task.** What is 3 + 3?", "talk");
    await until("the task the session is left with", () => idle() && actions.session()?.probe().task?.includes("3 + 3") === true, 10_000);

    // Close the session: its tab goes, and the lesson is kept for the next sitting.
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

    // Opened again, it is as it was left: its heading at the top, its turns under it, and nothing is sent.
    // A window quit with the task open kept the task's words with the session's place, and windows before this one
    // read them back: the place here is as such a window left it.
    const leftByOlderWindow: Record<string, unknown> = { task: "**Task.** What is 3 + 3?" };
    keepPlace(placeKey(folder, sessions[0].session, ""), leftByOlderWindow as Partial<Place>);
    actions.openSession(folder, sessions[0].session, sessions[0].title ?? "");
    await until("the session again", () => document.querySelector(".session.in-front .now.lesson article") !== null, 10_000);
    await wait(1500);
    // With its task open and nothing new to read, the page is the task: its own words over the line, the lesson
    // folded away. ⌘↑ opens the lesson, and nothing of that is sent.
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

    // Writing and correcting on the line, with the lesson open over its task and at its end. Each of these moved
    // something the learner had not moved: taking words off a line of several rows left the line under the window's
    // edge, a reply ending sent the caret to the end of the words, and the task's copy over the line stayed under
    // an answer while the tutor wrote. All three are looked at before anything is said to be wrong.
    {
      const lineNow = () => document.querySelector<HTMLTextAreaElement>('.session.in-front [data-drive="composer"]')!;
      const view = document.querySelector<HTMLElement>(".session.in-front main.surface")!;
      const write = (value: string) => {
        const line = lineNow();
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(line, value);
        line.dispatchEvent(new Event("input", { bubbles: true }));
      };
      // WebKit draws no frame for a window another window covers, so this wait has a limit: without one the check
      // stopped here saying nothing, and the launcher could only report that the page never answered.
      const frame = () =>
        within("drawing the window's next frame (a covered window draws none)", new Promise<void>((done) => requestAnimationFrame(() => done())), 10_000);
      const copy = () => document.querySelector(".session.in-front .foot .parked") !== null;
      // Where the line's bottom is against the bottom of the lesson's view: it should not change as rows go.
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

      // The session is opened by a first message, so the line is not drawn anew under the words of the next step.
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

      // A message tall enough to put the reply that set the task out of view, and a reply written slowly.
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
      // Once the window has heard which task is open, its copy is over the line again: it is held back, not gone.
      let back = false;
      for (let n = 0; n < 200 && !back; n++) {
        back = copy();
        if (!back) await wait(50);
      }

      steps.steadyLine = { scrolls, rest, lowest, fromFoot, caret, looks, copies, back };
      const wrong: string[] = [];
      if (scrolls < 100) wrong.push(`the lesson was too short to scroll (${scrolls} px), so the line could not be seen to move`);
      if (lowest - rest > 2 || fromFoot > 6) wrong.push(`taking words off the line left it ${lowest - rest} px under its place, the lesson ${fromFoot} px from its end`);
      if (caret !== 9) wrong.push(`a reply ending moved the caret from 9 to ${caret}`);
      if (copies > 0) wrong.push(`the task's copy was over the line while the tutor wrote, in ${copies} of ${looks} looks`);
      if (!back) wrong.push("the task's copy did not come back over the line after the reply ended");
      if (wrong.length > 0) throw new Error(wrong.join("; "));
    }
    // The task it was left with is open again, and goes when it is answered. Claude Code's engine never says which
    // task is open, so the window finds it in the turns: the next task the tutor poses is then the open one.
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

    // Claude Code's sign-in runs out while the lesson is open. An answer sent then reaches no tutor: Claude Code's
    // own line for it is not drawn or kept as the tutor's reply, the answer is back on the line, its task stays
    // open, and Home asks for the sign-in. Signed in from the notice, Return sends the answer.
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
    actions.run("close-session");
    await until("the session's tab to close again", () => actions.session() === null, 10_000);

    // A new session's first line names its model under More and opens the agent's list; Esc gives the keyboard back to the line.
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
