// A check the window runs on itself when started with MAGNESIUM_APP_SELFTEST.
//
// It drives the real surface inside the real window, against whatever agent
// program the environment names. scripts/selftest.sh names the stand-in, so
// the check starts no model. It proves the parts that only exist once the
// window is running: the calls between the page and the shell, the content
// policy, that mathematics, a diagram and a PDF page draw, and that the
// surface's own structure holds: the next thing on the first screen, the line
// under it, a layer summoned by a key, talk sent as talk.

import type { Actions } from "./App";
import { tutorText, type SessionState } from "./lesson";
import { tinyPdf } from "./demoPdf";
import { drawPage, openPdf } from "./pdf";
import { hasTask, renderInline, renderMarkdown, renderPlain, setSourcesApart, withoutOpenMath } from "./render";
import { sourceRef } from "./sources";
import type { Core } from "./types";

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
function press(key: string, mods: { meta?: boolean } = {}) {
  const target = (document.activeElement as HTMLElement | null) ?? document.body;
  target.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, metaKey: Boolean(mods.meta) }));
}

export async function run(api: Core, actions: Actions, session: () => SessionState): Promise<void> {
  const steps: Record<string, unknown> = {};
  let failure: string | null = null;
  const idle = () => {
    const live = session().live;
    return live !== null && live.status === "idle" && tutorText(live) !== "";
  };
  try {
    const home = await api.home();
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

    // The first screen: the next thing, which starts the lesson, and the line under it.
    await until("the first screen", () => document.querySelector('[data-drive="start"]') !== null || document.querySelector(".now.rest .next") !== null, 10_000);
    steps.firstScreen = {
      next: document.querySelector(".now.rest .next")?.textContent ?? null,
      line: document.querySelector('.now.rest [data-drive="talk"]') !== null,
    };
    if (!steps.firstScreen || !(steps.firstScreen as { line: boolean }).line) throw new Error("the first screen has no line to write on");

    // A layer comes with its key and goes with Esc.
    press(",", { meta: true });
    await until("the settings layer", () => document.querySelector('[data-layer="Settings"]') !== null, 3000);
    press("Escape");
    await until("the settings layer to go", () => document.querySelector('[data-layer="Settings"]') === null, 3000);
    steps.layer = true;

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
    const pasted = await within("saving pasted text", api.addPastedText("Bayes' rule: posterior is proportional to prior times likelihood."));
    const savedAs = pasted.saved[0];
    steps.material = { saved: pasted.saved.length, lines: pasted.lines };
    if (!savedAs?.startsWith("Study/materials/Pasted text ") || !savedAs.endsWith(".txt")) throw new Error(`pasted text was not saved as a text file: ${JSON.stringify(pasted)}`);
    const readBack = await within("reading a saved text file", api.source(savedAs));
    if (readBack.kind !== "text" || !readBack.text?.includes("posterior")) throw new Error("a saved text file did not open as text");
    for (const outside of ["../outside.pdf", "/etc/hosts", "Study/midterm-1/papers/PaperB-key.pdf"]) {
      const refused = await within(`opening ${outside}`, api.source(outside)).then(() => null, (problem) => String(problem));
      if (refused === null) throw new Error(`a source outside the study folder, or an answer key, opened: ${outside}`);
    }

    // The library, the plan and the saved conversation, through the shell.
    const cards = await api.library();
    const plan = await api.plan();
    const saved = await api.turns();
    steps.library = { cards: cards.length, first: cards[0]?.name, plan: { items: plan.items.length, path: plan.path }, saved: saved.length };
    if (!cards.some((card) => card.folder === home.folder)) throw new Error("the open folder is not in the library");

    // Start: the next thing, as Return on the first screen does.
    const message = "Self-check. Inline $a^b$ and a diagram:\n\n```mermaid\ngraph LR\n  A[rate] --> B[reciprocal]\n```";
    await actions.start();
    await until("the first reply", idle);
    if (document.querySelector(".now.lesson") === null) throw new Error("the lesson did not fill the surface");
    await actions.say(message, "talk");
    await until("the reply to be on screen", () => session().earlier.length >= 1 && idle());
    const reply = tutorText(session().live!);
    steps.reply = reply.slice(0, 120);
    if (!reply.includes("Self-check")) throw new Error(`the reply was not the stand-in's echo: ${reply.slice(0, 80)}`);
    if (session().live?.said !== "talk") throw new Error("talk was not sent as talk");

    await until("the page to draw the reply", () => document.querySelector(".tutor .prose") !== null, 5000);
    await wait(1500);
    steps.math = document.querySelectorAll(".tutor .katex").length;
    steps.diagram = document.querySelectorAll(".tutor .diagram svg").length;
    if (steps.math === 0) throw new Error("the formula did not draw");
    if (steps.diagram === 0) throw new Error("the diagram did not draw");

    await actions.say("PERMISSION", "talk");
    await until("the permission card", () => session().cards.length > 0);
    steps.card = document.querySelector(".card .record-head")?.textContent ?? null;
    await api.decide(session().cards[0].id, "allow_once");
    await until("the reply after the permission", () => session().cards.length === 0 && idle());

    // The learner's own line on screen draws its mathematics too.
    await actions.say("my answer is $x^2$", "talk");
    await until("the reply to the answer", () => idle() && session().live?.learner.includes("my answer") === true);
    steps.saidMath = document.querySelectorAll(".you .katex").length;
    if (steps.saidMath === 0) throw new Error("the mathematics in the learner's own line did not draw");

    const rail = await api.rail();
    steps.rail = { goal: rail.goal, steps: rail.steps.length };
    const progress = await api.progress(null);
    steps.progress = { confirmed: progress.confirmed.length, message: progress.message };

    // The plan's layer comes with ⌘P over the lesson.
    press("p", { meta: true });
    await until("the plan's layer", () => document.querySelector('[data-layer="The plan"]') !== null, 3000);
    press("Escape");
    await until("the plan's layer to go", () => document.querySelector('[data-layer="The plan"]') === null, 3000);

    // Stop here: the lesson is kept for the next sitting, and the surface is at rest again.
    await actions.stop();
    await until("the surface at rest", () => document.querySelector(".now.rest") !== null, 10_000);
    const after = await api.home();
    steps.afterStop = { can_resume: after.can_resume, stopped_at: after.last?.stopped_at ?? null };
    if (!after.can_resume) throw new Error("the lesson was not kept for the next sitting");
    const kept = await api.turns();
    if (kept.length === 0) throw new Error("the conversation was not kept to be drawn above the next thing");
  } catch (problem) {
    failure = String(problem);
  }
  const errors = [...window.__mgErrors];
  await api.selftestReport({ ok: failure === null && errors.length === 0, failure, errors, steps });
}
