// A check the window runs on itself when started with MAGNESIUM_APP_SELFTEST.
//
// It drives the real page inside the real window, against whatever agent
// program the environment names. scripts/selftest.sh names the stand-in, so
// the check starts no model. It proves the parts that only exist once the
// window is running: the calls between the page and the shell, the content
// policy, and that mathematics and a diagram draw.

import type { Actions } from "./App";
import { tutorText, type SessionState } from "./lesson";
import { renderInline, renderMarkdown, renderPlain, withoutOpenMath } from "./render";
import type { Core } from "./types";

const wait = (ms: number) => new Promise((done) => window.setTimeout(done, ms));

async function until(what: string, test: () => boolean, limitMs = 30_000): Promise<void> {
  const started = Date.now();
  while (!test()) {
    if (Date.now() - started > limitMs) throw new Error(`timed out waiting for ${what}`);
    await wait(50);
  }
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

    // One line of text anywhere on the page draws its mathematics, and the
    // learner's own words keep every character that is not mathematics.
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

    // A task that runs over several blocks is one box, to the end of the reply.
    const task = document.createElement("div");
    task.innerHTML = renderMarkdown("An explanation.\n\n**Task.** Find the eigenvalues of\n\n$$A$$\n\nOne line.");
    const box = task.querySelector("div.task");
    steps.task = { boxes: task.querySelectorAll("div.task").length, blocks: box?.children.length ?? 0, before: task.firstElementChild?.tagName };
    if (!box || box.children.length !== 3 || task.firstElementChild?.tagName !== "P" || box.querySelectorAll(".katex-display").length !== 1) {
      throw new Error(`the task was not set apart as one box: ${task.innerHTML.slice(0, 200)}`);
    }

    // The library and the study days, through the shell. The folder that is
    // open is in the library, and nothing was studied before this check.
    const cards = await api.library();
    const days = await api.activity();
    steps.library = { cards: cards.length, first: cards[0]?.name, run: days.run, today: days.today };
    if (!cards.some((card) => card.folder === home.folder)) throw new Error("the open folder is not in the library");
    if (!/^\d{4}-\d{2}-\d{2}$/.test(days.today)) throw new Error("the core did not say what day it is");

    const message = "Self-check. Inline $a^b$ and a diagram:\n\n```mermaid\ngraph LR\n  A[rate] --> B[reciprocal]\n```";
    await actions.continueLesson(false);
    await until("the first reply", idle);
    await actions.say(message);
    await until("the reply to be on screen", () => session().earlier.length >= 1 && idle());
    const reply = tutorText(session().live!);
    steps.reply = reply.slice(0, 120);
    if (!reply.includes("Self-check")) throw new Error(`the reply was not the stand-in's echo: ${reply.slice(0, 80)}`);

    await until("the page to draw the reply", () => document.querySelector(".tutor .prose") !== null, 5000);
    await wait(1500);
    steps.math = document.querySelectorAll(".tutor .katex").length;
    steps.diagram = document.querySelectorAll(".tutor .diagram svg").length;
    if (steps.math === 0) throw new Error("the formula did not draw");
    if (steps.diagram === 0) throw new Error("the diagram did not draw");

    await actions.say("PERMISSION");
    await until("the permission card", () => session().cards.length > 0);
    steps.card = document.querySelector(".ask h3")?.textContent ?? null;
    await api.decide(session().cards[0].id, "allow_once");
    await until("the reply after the permission", () => session().cards.length === 0 && idle());

    // The learner's own line on screen draws its mathematics too.
    await actions.say("my answer is $x^2$");
    await until("the reply to the answer", () => idle() && session().live?.learner.includes("my answer") === true);
    steps.saidMath = document.querySelectorAll(".exchange.now .you .katex").length;
    if (steps.saidMath === 0) throw new Error("the mathematics in the learner's own line did not draw");

    const rail = await api.rail();
    steps.rail = { goal: rail.goal, steps: rail.steps.length };
    const progress = await api.progress(null);
    steps.progress = { confirmed: progress.confirmed.length, message: progress.message };

    await actions.openProgress(null);
    await wait(300);
    steps.progressScreen = document.querySelector('[data-screen="progress"]') !== null;

    // Ending the sitting from the page: the lesson is kept and the end
    // screen shows. A study day is a day with a marked answer, and the
    // stand-in tutor marks none. So with a runtime that says which days had
    // one, today is not a study day, however many messages were sent. With
    // an earlier runtime the window counts the answers sent from it.
    await actions.stop("self-check stop line");
    await until("the end of the sitting", () => document.querySelector('[data-screen="summary"]') !== null, 10_000);
    const after = await api.home();
    const studied = await api.activity();
    const fromRecord = !progress.level_unknown;
    steps.afterStop = { can_resume: after.can_resume, studied_today: studied.studied_today, run: studied.run, days_from_record: fromRecord };
    if (!after.can_resume) throw new Error("the lesson was not kept for the next sitting");
    if (fromRecord && (studied.studied_today || studied.run !== 0)) throw new Error("a day with no marked answer was counted as a study day");
    if (!fromRecord && (!studied.studied_today || studied.run !== 1)) throw new Error("today's answers were not counted as a study day");
  } catch (problem) {
    failure = String(problem);
  }
  const errors = [...window.__mgErrors];
  await api.selftestReport({ ok: failure === null && errors.length === 0, failure, errors, steps });
}
