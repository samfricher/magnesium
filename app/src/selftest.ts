// A check the window runs on itself when started with MAGNESIUM_APP_SELFTEST.
//
// It drives the real page inside the real window, against whatever agent
// program the environment names. scripts/selftest.sh names the stand-in, so
// the check starts no model. It proves the parts that only exist once the
// window is running: the calls between the page and the shell, the content
// policy, and that mathematics and a diagram draw.

import type { Actions } from "./App";
import { tutorText, type SessionState } from "./lesson";
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

    const message = "Self-check. Inline $a^b$ and a diagram:\n\n```mermaid\ngraph LR\n  A[rate] --> B[reciprocal]\n```";
    await actions.continueLesson(false);
    await until("the first reply", idle);
    await actions.say(message);
    await until("the reply to be on screen", () => session().earlier.length >= 1 && idle());
    const reply = tutorText(session().live!);
    steps.reply = reply.slice(0, 120);
    if (!reply.includes("Self-check")) throw new Error(`the reply was not the stand-in's echo: ${reply.slice(0, 80)}`);

    await until("the page to draw the reply", () => document.querySelector(".tutor .md") !== null, 5000);
    await wait(1500);
    steps.math = document.querySelectorAll(".tutor .katex").length;
    steps.diagram = document.querySelectorAll(".tutor .diagram svg").length;
    if (steps.math === 0) throw new Error("the formula did not draw");
    if (steps.diagram === 0) throw new Error("the diagram did not draw");

    await actions.say("PERMISSION");
    await until("the permission card", () => session().cards.length > 0);
    steps.card = document.querySelector(".ask .tq")?.textContent ?? null;
    await api.decide(session().cards[0].id, "allow_once");
    await until("the reply after the permission", () => session().cards.length === 0 && idle());

    const rail = await api.rail();
    steps.rail = { goal: rail.goal, steps: rail.steps.length };
    const progress = await api.progress(null);
    steps.progress = { confirmed: progress.confirmed.length, message: progress.message };

    await actions.openProgress(null);
    await wait(300);
    steps.progressScreen = document.querySelector(".progress") !== null;

    await api.stop("self-check stop line");
    const after = await api.home();
    steps.afterStop = { can_resume: after.can_resume };
    if (!after.can_resume) throw new Error("the lesson was not kept for the next sitting");
  } catch (problem) {
    failure = String(problem);
  }
  const errors = [...window.__mgErrors];
  await api.selftestReport({ ok: failure === null && errors.length === 0, failure, errors, steps });
}
