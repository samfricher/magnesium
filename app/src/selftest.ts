// A check the window runs on itself when started with MAGNESIUM_APP_SELFTEST.
//
// It drives the real window, against whatever agent program the environment
// names. scripts/selftest.sh names the stand-in, so the check starts no model.
// It proves the parts that only exist once the window is running: the calls
// between the page and the shell, the content policy, that mathematics, a
// diagram and a PDF page draw, and that the window's structure holds: Home
// with its first row, a workspace's page, Settings in a tab, ⌘K with its few
// commands, a session in a tab with talk sent as talk, the plan beside it,
// a file attached by name, and no path or file text shown anywhere.

import type { Actions } from "./App";
import { attachedWords, splitAttached, tutorText } from "./lesson";
import { tinyPdf } from "./demoPdf";
import { drawPage, openPdf } from "./pdf";
import { hasTask, renderInline, renderMarkdown, renderPlain, setSourcesApart, withoutOpenMath } from "./render";
import { comboOf, keyName, parseCombo } from "./prefs";
import { sourceRef } from "./sources";
import type { Core } from "./types";
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
    steps.homePage = { rows: document.querySelectorAll('[data-page="home"] .home-row').length, first: document.querySelector('[data-drive="start"]')?.textContent ?? null };
    if (document.activeElement?.getAttribute("data-drive") !== "start") throw new Error("Home's first row does not have the focus");
    noRawFiles("Home");

    // Settings: a page in its own tab, with its sections, no file and no path.
    press(",", { meta: true });
    await until("the Settings tab", () => document.querySelector('[data-page="settings"]') !== null, 3000);
    const sections = Array.from(document.querySelectorAll(".settings-page .section-head")).map((head) => head.textContent);
    steps.settings = { sections, keys: document.querySelectorAll(".key-row").length, colours: document.querySelectorAll('input[type="color"]').length };
    for (const wanted of ["Reading", "Appearance", "Keys", "Lessons", "About"]) if (!sections.includes(wanted)) throw new Error(`Settings has no ${wanted} section`);
    if (document.querySelectorAll("textarea").length > 0) throw new Error("Settings shows a text to edit");
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
    if (plain.join("|") !== "Added 5 files to the course material.|Change plan.md|as a backup|1/v in s/µM") throw new Error(`a path reached the screen: ${plain.join(" | ")}`);

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
    await until("the material listed", () => document.querySelector('[data-page="workspace"] .file-row') !== null, 10_000);
    const titles = Array.from(document.querySelectorAll('[data-page="workspace"] .file-row .row-name')).map((row) => row.textContent ?? "");
    steps.materialTitles = titles;
    if (titles.some((title) => /\.[a-z0-9]{1,5}$/i.test(title) || title.includes("/"))) throw new Error(`material is listed by file name: ${titles.join(", ")}`);
    document.querySelector<HTMLElement>('[data-page="workspace"] .file-row')!.click();
    await until("the material in the window", () => document.querySelector('[data-role="source-panel"] .sheet-name') !== null, 5000);
    const sheet = document.querySelector('[data-role="source-panel"] .sheet-name')?.textContent ?? "";
    steps.materialSheet = sheet;
    if (/\.[a-z0-9]{1,5}$/i.test(sheet)) throw new Error(`material opened under its file name: ${sheet}`);
    press("Escape");
    await until("the material to close", () => document.querySelector('[data-role="source-panel"]') === null, 3000);

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

    // Start: Home's first row, which opens the session in a tab of its own.
    const message = "Self-check. Inline $a^b$ and a diagram:\n\n```mermaid\ngraph LR\n  A[rate] --> B[reciprocal]\n```";
    actions.continueRecent();
    await until("a session's tab", () => actions.session() !== null, 5000);
    await until("the first reply", idle);
    if (document.querySelector(".session.in-front .now.lesson") === null) throw new Error("the lesson did not fill its tab");
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

    await actions.session()!.say("PERMISSION", "talk");
    await until("the permission card", () => session().cards.length > 0);
    steps.card = document.querySelector(".card .record-head")?.textContent ?? null;
    await api.decide(session().cards[0].id, "allow_once");
    await until("the reply after the permission", () => session().cards.length === 0 && idle());

    // The learner's own line on screen draws its mathematics too, and a file sent with it is named, not its path.
    await actions.session()!.say(`my answer is $x^2$\n\n${toRead}`, "talk");
    await until("the reply to the answer", () => idle() && session().live?.learner.includes("my answer") === true);
    steps.saidMath = document.querySelectorAll(".session.in-front .you .katex").length;
    if (steps.saidMath === 0) throw new Error("the mathematics in the learner's own line did not draw");
    if (!document.querySelector(".session.in-front .sent-files")?.textContent?.includes(kept.split("/").pop() ?? kept)) throw new Error("the file sent was not named under the message");

    // Esc in a task goes to talk in one press, with the symbols open and words on the line, and keeps the words.
    await actions.session()!.say("A task, please.\n\n**Task.** What is 2 + 2?", "talk");
    await until("the line in the task", () => document.querySelector('.session.in-front .line[data-line="task"]') !== null, 10_000);
    actions.run("symbols");
    await until("the symbols", () => document.querySelector(".session.in-front .symbols") !== null, 3000);
    const field = document.querySelector<HTMLTextAreaElement>('.session.in-front [data-drive="composer"]')!;
    const setField = (value: string) => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(field, value);
      field.dispatchEvent(new Event("input", { bubbles: true }));
    };
    field.focus();
    setField("√4");
    field.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await until("the line on talk", () => document.querySelector('.session.in-front .line[data-line="talk"]') !== null, 3000);
    const onLine = document.querySelector<HTMLTextAreaElement>('.session.in-front [data-drive="composer"]')?.value;
    steps.escape = { onLine, symbols: document.querySelector(".session.in-front .symbols") !== null };
    if (onLine !== "√4") throw new Error(`Esc lost the words on the line: ${onLine}`);
    if (document.querySelector(".session.in-front .symbols") !== null) throw new Error("Esc left the symbols open");
    setField("");

    // The quiet controls, and the plan beside the lesson, shown and hidden.
    steps.controls = ["attach", "plan-toggle", "settings", "tab-1", "tab-2", "home", "new-session"].filter((name) => document.querySelector(`[data-drive="${name}"]`) !== null);
    if ((steps.controls as string[]).length !== 7) throw new Error(`a visible control is missing: ${JSON.stringify(steps.controls)}`);
    actions.run("plan");
    await until("the plan beside the lesson", () => document.querySelector(".session.in-front .panel") !== null, 3000);
    actions.run("plan");
    await until("the plan to go", () => document.querySelector(".session.in-front .panel") === null, 3000);

    const progress = await api.progress(null, folder);
    steps.progress = { confirmed: progress.confirmed.length, message: progress.message };

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

    // Opened again, it is as it was left: its heading at the top, its turns under it, and nothing is sent.
    actions.openSession(folder, sessions[0].session, sessions[0].title ?? "");
    await until("the session again", () => document.querySelector(".session.in-front .now.lesson article") !== null, 10_000);
    await wait(1500);
    const head = document.querySelector(".session.in-front .now.lesson .head")!.getBoundingClientRect().top;
    const first = document.querySelector(".session.in-front .now.lesson article")!.getBoundingClientRect().top;
    const again = await api.lessonTurns(folder, sessions[0].session);
    steps.reopened = { headAbove: head < first, turns: again.length, open: session().open };
    if (!(head < first)) throw new Error("a reopened session drew its turns above its heading");
    if (again.length !== turns.length || session().open) throw new Error("opening a kept session sent something");
    if (!document.querySelector(".session.in-front .you.talk")) throw new Error("talk in a reopened session is not drawn as talk");
    const probe = actions.session()?.probe();
    steps.reopened = { ...(steps.reopened as object), line: probe?.line, task: probe?.task !== null };
    if (probe?.task && probe.line !== "task") throw new Error("a reopened session's open task does not hold the caret");
    actions.run("close-session");
    await until("the session's tab to close again", () => actions.session() === null, 10_000);
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
