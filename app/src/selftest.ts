// A check the window runs on itself when started with MAGNESIUM_APP_SELFTEST.
//
// It drives the real page inside the real window, against whatever agent
// program the environment names. scripts/selftest.sh names the stand-in, so
// the check starts no model. It proves the parts that only exist once the
// window is running: the calls between the page and the shell, the content
// policy, that mathematics, a diagram and a PDF page draw, that talk is told
// from an answer by where it was written, and stop and resume.

import type { Actions } from "./App";
import { tinyPdf } from "./demoPdf";
import { readPage } from "./page";
import { renderInline, renderMarkdown, renderPlain, setSourcesApart, withoutOpenMath } from "./render";
import { sourceRef } from "./sources";
import { splitTask } from "./task";
import type { Core } from "./types";
import { drawPage, openPdf } from "./ui/pdf";

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

export async function run(api: Core, actions: Actions): Promise<void> {
  const steps: Record<string, unknown> = {};
  let failure: string | null = null;
  const idle = () => {
    const live = actions.session().live;
    return live !== null && live.status === "idle" && [...live.done, live.streaming].join("").trim() !== "";
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

    // A task is found where the tutor sets it, over several blocks, and a
    // reply with no task sets none.
    const task = splitTask("An explanation.\n\n**Task.** Find the eigenvalues of\n\n$$A$$\n\nOne line.");
    const question = splitTask("Right: that is it. Next step. A sample of 25 has s = 10. What is the standard error?");
    steps.task = { task: task?.task.slice(0, 30), question: question?.task, none: splitTask("Say when you want to go back to the task.") };
    if (!task || !task.task.startsWith("**Task.**") || !task.task.includes("$$A$$") || task.before !== "An explanation.") throw new Error(`the task was not set apart: ${JSON.stringify(task)}`);
    if (question?.task !== "A sample of 25 has s = 10. What is the standard error?") throw new Error(`a question was not read as the task: ${JSON.stringify(question)}`);
    if (splitTask("Say when you want to go back to the task.") !== null) throw new Error("a reply with no task was read as setting one");

    // Where a message is written decides what it is: inside the open task it
    // is an answer, on the page it is talk, and a task stays open until the
    // record marks what was sent for it.
    const read = readPage(
      {
        open: true,
        earlier: [
          { session: "", ms: 1, learner: "", tutor: "**Task.** What is 2 + 2?", interrupted: false },
          { session: "", ms: 2, learner: "By the way, why?", tutor: "Because.", interrupted: false },
        ],
        live: null,
        cards: [],
        notices: [],
        startedMs: null,
      },
      (turn) => (turn.ms === 2 ? { said: "talk", marks: [] } : null),
    );
    steps.page = { open: read.open, said: read.exchanges.map((ex) => ex.said) };
    if (read.open !== 0 || read.exchanges[1].said !== "talk") throw new Error(`talk closed the task or was read as an answer: ${JSON.stringify(steps.page)}`);

    // A link to a PDF or a text file is a source: shut as plain text until the
    // task is answered, open as a link after.
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

    // The PDF reader and its worker load in this window, from its own files and under its content policy.
    const pdf = await within("opening a PDF", openPdf(tinyPdf(["one", "two", "three"])));
    const canvas = document.createElement("canvas");
    await within("drawing a PDF page", drawPage(pdf, 2, canvas, 300));
    const pixels = canvas.getContext("2d")?.getImageData(0, 0, canvas.width, canvas.height).data ?? new Uint8ClampedArray();
    let ink = 0;
    for (let i = 0; i < pixels.length; i += 4) if (pixels[i] < 128) ink += 1;
    steps.pdf = { pages: pdf.numPages, width: canvas.width, ink };
    if (pdf.numPages !== 3 || ink < 200) throw new Error(`the PDF page did not draw: ${JSON.stringify(steps.pdf)}`);

    // Course material through the shell: pasted text is saved as a text file
    // in the study folder, listed among its files, read back, and nothing
    // outside the folder, or an answer key, is read.
    const pasted = await within("saving pasted text", api.addPastedText("Bayes' rule: posterior is proportional to prior times likelihood."));
    const savedAs = pasted.saved[0];
    steps.material = { saved: pasted.saved.length, lines: pasted.lines };
    if (!savedAs?.startsWith("Study/materials/Pasted text ") || !savedAs.endsWith(".txt")) throw new Error(`pasted text was not saved as a text file: ${JSON.stringify(pasted)}`);
    const listed = await within("listing the folder's files", api.studyFiles());
    steps.files = { files: listed.files.length, keys: listed.keys };
    if (!listed.files.some((file) => file.path === savedAs)) throw new Error("the saved text file is not among the folder's files");
    const readBack = await within("reading a saved text file", api.source(savedAs));
    if (readBack.kind !== "text" || !readBack.text?.includes("posterior")) throw new Error("a saved text file did not open as text");
    for (const outside of ["../outside.pdf", "/etc/hosts", "Study/midterm-1/papers/PaperB-key.pdf"]) {
      const refused = await within(`opening ${outside}`, api.source(outside)).then(() => null, (problem) => String(problem));
      if (refused === null) throw new Error(`a source outside the study folder, or an answer key, opened: ${outside}`);
    }
    // The window writes only a plan file, and only one that is there.
    const wrote = await api.writePlan("Study/materials/notes.md", "x", 0).then(() => true, () => false);
    if (wrote) throw new Error("the window wrote a file that is not a plan");

    // The library, through the shell.
    const recent = await api.recent();
    steps.library = { folders: recent.length };
    if (!recent.some((path) => path === home.folder || home.folder.endsWith(path.split("/").pop() ?? "?"))) throw new Error("the open folder is not in the list of folders");

    // A lesson: the window's own opening words, then talk with mathematics and a diagram.
    await actions.continueLesson();
    await until("the first reply", idle);
    const message = "Self-check. Inline $a^b$ and a diagram:\n\n```mermaid\ngraph LR\n  A[rate] --> B[reciprocal]\n```";
    await actions.talk(message);
    await until("the reply to be on screen", () => idle() && actions.session().live?.learner.includes("Self-check") === true);
    const reply = [...(actions.session().live?.done ?? [])].join("\n");
    steps.reply = reply.slice(0, 120);
    if (!reply.includes("Self-check")) throw new Error(`the reply was not the stand-in's echo: ${reply.slice(0, 80)}`);

    await until("the page to draw the reply", () => document.querySelector('[data-role="tutor"] .prose') !== null, 5000);
    await wait(1500);
    steps.math = document.querySelectorAll('[data-role="tutor"] .katex').length;
    steps.diagram = document.querySelectorAll('[data-role="tutor"] .diagram svg').length;
    if (steps.math === 0) throw new Error("the formula did not draw");
    if (steps.diagram === 0) throw new Error("the diagram did not draw");
    steps.saidMath = document.querySelectorAll('[data-role="said"] .katex').length;
    if (steps.saidMath === 0) throw new Error("the mathematics in the learner's own line did not draw");

    await actions.talk("PERMISSION");
    await until("the permission card", () => actions.session().cards.length > 0);
    steps.card = document.querySelector('[data-role="permission"] .card-title')?.textContent ?? null;
    if (!steps.card) throw new Error("the permission card was not drawn");
    await api.decide(actions.session().cards[0].id, "allow_once");
    await until("the reply after the permission", () => actions.session().cards.length === 0 && idle());

    const rail = await api.rail();
    const plan = await api.plan();
    steps.rail = { goal: rail.goal, steps: rail.steps.length, plan: plan.path };
    actions.openDoc("record");
    await wait(400);
    steps.record = document.querySelector('[data-doc="record"]') !== null;
    if (!steps.record) throw new Error("the record did not open");
    actions.openDoc("lesson");

    // Closing the lesson keeps it for the next sitting, and it resumes.
    await actions.stop();
    const after = await api.home();
    const lessons = await api.lessons();
    steps.afterStop = { can_resume: after.can_resume, lessons: lessons.length, current: lessons.some((l) => l.current) };
    if (!after.can_resume) throw new Error("the lesson was not kept for the next sitting");
    if (!lessons.some((l) => l.current)) throw new Error("the kept lesson is not listed");
    const current = lessons.find((l) => l.current)!;
    const turns = await api.lessonTurns(current.session);
    steps.turns = turns.length;
    if (turns.length < 2) throw new Error("the lesson's turns were not kept");
    await actions.continueLesson();
    await until("the reply after resuming", () => idle() && (actions.session().live?.learner.startsWith("I'm back") ?? false));
    steps.resumed = true;
  } catch (problem) {
    failure = String(problem);
  }
  const errors = [...window.__mgErrors];
  await api.selftestReport({ ok: failure === null && errors.length === 0, failure, errors, steps });
}
