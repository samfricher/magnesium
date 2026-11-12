// What each key summons: the same layer, filled with one kind of thing.
//
//   ⌘K  everything, or words to say to the tutor
//   ⌘O  courses          ⌘P  the plan, and a line to change it
//   ⌘E  material, papers ⌘Y  your work: the record's marks
//   ⌘,  settings         ⌘/  help with the task

import { useEffect, useMemo, useState } from "react";

import { HELP } from "../lesson";
import { FACES, INKS, MEASURES, SCHEMES, SIZES, VOICES, faceCss, keyName, type Prefs } from "../prefs";
import { sourceRef, type SourceRef } from "../sources";
import type { About, Card, Core, Engine, EngineChoice, Home, Mark, Materials, Paper, Payment, Plan, Progress, Setup } from "../types";
import { displayName, longDay, shortDay, type PlanDiff } from "../words";
import { Layer, type Row } from "./Layer";
import { Line } from "./Rich";

export type LayerKind = "commands" | "courses" | "plan" | "material" | "work" | "settings" | "help";

type Acts = {
  start: () => void;
  /** Start without the quick checks and paper redos that are due: they stay due. */
  startSkip: () => void;
  setBlock: (minutes: number) => void;
  editConfig: () => void;
  /** Close the layer and say this to the tutor, as talk; at rest it starts a lesson with these words. */
  say: (text: string) => void;
  sayKeep: (text: string) => void;
  help: (words: string) => void;
  stop: () => void;
  interrupt: () => void;
  newLesson: () => void;
  switchCourse: (path: string) => void;
  pickFolder: () => void;
  newFolder: () => void;
  findFolders: () => void;
  forgetFolder: (path: string) => void;
  chooseMaterial: () => void;
  paste: () => void;
  openFile: (path: string) => void;
  sitPaper: (paper: Paper) => void;
  handIn: () => void;
  endPaper: () => void;
  chooseEngine: (engine: Engine) => void;
  choosePayment: (payment: Payment) => void;
  signIn: () => void;
  openSource: (source: SourceRef) => void;
};

type Props = {
  kind: LayerKind;
  api: Core | null;
  home: Home | null;
  plan: Plan | null;
  changed: PlanDiff | null;
  cards: Card[] | null;
  setup: Setup | null;
  about: About | null;
  engine: EngineChoice | null;
  prefs: Prefs;
  inLesson: boolean;
  lessonOpen: boolean;
  taskOpen: boolean;
  busy: boolean;
  sources: { text: string; href: string }[];
  onClose: () => void;
  open: (kind: LayerKind) => void;
  prefsChange: (change: Partial<Prefs>) => void;
  act: Acts;
};

const STATUS_MARK: Record<string, string> = { done: "✓", doing: "●", todo: "○" };
const MOVED_WORDS: Record<string, string> = { up: "moved up", down: "moved down", new: "new", day: "day moved" };
const ENGINE_NAMES: Record<Engine, string> = { plugin: "Claude Code", direct: "Magnesium" };

function stateWords(card: Card): string | null {
  if (card.sitting) return "a paper is running";
  switch (card.state) {
    case "needs_upgrade":
      return "needs an upgrade";
    case "not_set_up":
    case "empty":
      return "nothing started yet";
    case "no_runtime":
    case "unreadable":
      return "cannot be read now";
    default:
      return null;
  }
}

/** A mark in a few words, for the record of past work. */
function markShort(mark: Mark): string {
  const help = mark.help === "hint" ? ", with a hint" : mark.help === "step" ? ", with a step shown" : mark.help === "solution" ? ", after the solution" : "";
  switch (mark.mark) {
    case "right":
      return help ? `right${help}` : mark.cold ? "right, on your own" : "right";
    case "partly":
      return `partly right${help}`;
    case "wrong":
      return "not right yet";
    case "declined":
      return "left for later";
    default:
      return mark.mark;
  }
}

function useMaterials(api: Core | null, wanted: boolean): Materials | null | "none" {
  const [found, setFound] = useState<Materials | null | "none">(null);
  useEffect(() => {
    if (!api || !wanted) return;
    let alive = true;
    api.materialList().then(
      (got) => alive && setFound(got),
      () => alive && setFound("none"),
    );
    return () => {
      alive = false;
    };
  }, [api, wanted]);
  return found;
}

function useWork(api: Core | null, wanted: boolean): { marks: Mark[] | null; progress: Progress | null; read: boolean } {
  const [state, setState] = useState<{ marks: Mark[] | null; progress: Progress | null; read: boolean }>({ marks: null, progress: null, read: false });
  useEffect(() => {
    if (!api || !wanted) return;
    let alive = true;
    void Promise.all([api.marks().catch(() => null), api.progress(null).catch(() => null)]).then(([marks, progress]) => {
      if (alive) setState({ marks, progress, read: true });
    });
    return () => {
      alive = false;
    };
  }, [api, wanted]);
  return state;
}

export function Layers(props: Props) {
  const { kind, home, plan, act } = props;
  const materials = useMaterials(props.api, kind === "material" || kind === "commands");
  const work = useWork(props.api, kind === "work");

  const planRows = useMemo((): Row[] => {
    const rows: Row[] = [];
    if (!plan) return [{ id: "reading", kind: "note", label: "Reading the plan…", text: "" }];
    const changed = props.changed;
    if (plan.title || plan.exam_date) {
      rows.push({
        id: "exam",
        kind: "thing",
        label: plan.exam_date ? `${plan.title ?? "The exam"}, ${longDay(plan.exam_date)}` : (plan.title ?? "The plan"),
        text: plan.title ?? "",
        hint: changed?.examFrom ? `moved from ${shortDay(changed.examFrom)}` : undefined,
        changed: Boolean(changed?.examFrom),
        alt: plan.path ? () => act.openFile(plan.path!) : undefined,
      });
    }
    // The block in hand: the one the plan marks doing, else the first not done.
    const current = plan.items.find((item) => item.status === "doing")?.id ?? plan.items.find((item) => item.status !== "done")?.id;
    for (const item of plan.items) {
      const moved = changed?.moved[item.id];
      rows.push({
        id: `item-${item.id}`,
        kind: "thing",
        mark: item.id === current ? "●" : (STATUS_MARK[item.status] ?? "○"),
        label: <Line text={item.title} className={item.status === "done" ? "done" : undefined} />,
        text: item.title,
        // Each block's day, as the plan has it, and what moved in this sitting.
        hint: [moved ? MOVED_WORDS[moved] : null, item.day ? shortDay(item.day) : null].filter(Boolean).join(", ") || (item.note ? <Line text={item.note} /> : undefined),
        changed: Boolean(moved),
        drive: `plan-${item.id}`,
        run: item.status === "done" ? undefined : () => act.say(props.inLesson ? `Can we work on ${item.title} next?` : `Let's work on ${item.title}.`),
        alt: plan.path ? () => act.openFile(plan.path!) : undefined,
      });
    }
    if (plan.goal && plan.steps.length > 0) {
      rows.push({ id: "goal", kind: "note", label: <Line text={plan.goal} />, text: plan.goal });
      plan.steps.forEach((step, at) =>
        rows.push({ id: `step-${at}`, kind: "do", mark: STATUS_MARK[step.status] ?? "○", label: <Line text={step.step} className={step.status === "done" ? "done" : undefined} />, text: step.step }),
      );
    }
    if (plan.items.length === 0 && !plan.exam_date && !(plan.goal && plan.steps.length > 0)) {
      rows.push({ id: "none", kind: "note", label: "This course has no plan yet. Say what it is for, above, and the tutor writes one.", text: "" });
    }
    return rows;
  }, [plan, props.changed, props.inLesson, act]);

  if (kind === "plan") {
    return (
      <Layer
        name="The plan"
        mode="say"
        placeholder="Tell the tutor what changed"
        rows={planRows}
        onSay={(text) => act.say(text)}
        keys={`↩ tell the tutor   ↑ ↓ ↩ work on an item${plan?.path ? "   ⌘↩ open the file" : ""}   esc close`}
        onClose={props.onClose}
      />
    );
  }

  if (kind === "courses") {
    const rows: Row[] = [];
    if (props.cards === null) rows.push({ id: "reading", kind: "note", label: "Reading your courses…", text: "" });
    for (const card of props.cards ?? []) {
      const words = stateWords(card);
      rows.push({
        id: card.folder,
        kind: "thing",
        label: displayName(card.name),
        text: `${card.name} ${card.goal ?? ""}`,
        hint: card.folder === home?.folder ? "open now" : words ? words : card.goal ? <Line text={card.goal} /> : undefined,
        drive: `course-${displayName(card.name)}`,
        run: () => act.switchCourse(card.folder),
        remove: () => act.forgetFolder(card.folder),
      });
    }
    rows.push(
      { id: "pick", label: "Open a folder…", text: "open folder choose", run: act.pickFolder },
      { id: "new", label: "Make a new folder…", text: "new folder make course", run: act.newFolder },
      { id: "find", label: "Find folders on this Mac", text: "find search folders", run: act.findFolders },
    );
    return <Layer name="Courses" mode="filter" placeholder="A course" rows={rows} keys="↩ open   ⌘⌫ take off the list (the folder stays)   esc close" onClose={props.onClose} />;
  }

  if (kind === "material") {
    const rows: Row[] = [];
    if (materials === null) rows.push({ id: "reading", kind: "note", label: "Reading the material folder…", text: "" });
    else if (materials !== "none") {
      for (const name of materials.names) rows.push({ id: `f-${name}`, kind: "file", label: name, text: name, run: () => act.openFile(`Study/materials/${name}`) });
      const more = materials.files - materials.names.length;
      if (more > 0) rows.push({ id: "more", kind: "note", label: `and ${more} more in Study/materials`, text: "" });
      if (materials.keys > 0) rows.push({ id: "keys", kind: "note", label: `${materials.keys === 1 ? "One answer key is" : `${materials.keys} answer keys are`} kept out of sight.`, text: "" });
      if (materials.files === 0) rows.push({ id: "empty", kind: "note", label: "Nothing here yet. Drop notes, slides or past papers on the window.", text: "" });
    }
    if (home?.sitting) {
      rows.push({ id: "handin", label: `Hand in ${home.sitting.name}…`, text: "hand in paper", run: act.handIn });
      rows.push({ id: "endpaper", label: "End the paper without handing in", text: "end paper", run: act.endPaper });
    } else {
      for (const paper of home?.papers ?? []) {
        rows.push({
          id: `p-${paper.path}`,
          label: `Sit ${paper.name}`,
          text: `paper sit ${paper.name}`,
          hint: paper.sat ? "sat before" : paper.minutes ? `${paper.minutes} minutes` : undefined,
          drive: `paper-${paper.name}`,
          run: () => act.sitPaper(paper),
        });
      }
    }
    rows.push(
      { id: "add", label: "Add files…", text: "add files material", run: act.chooseMaterial },
      { id: "paste", label: "Paste text…", text: "paste text", run: act.paste },
    );
    return <Layer name="Material" mode="filter" placeholder="A file or a paper" rows={rows} keys="↩ open   esc close" onClose={props.onClose} />;
  }

  if (kind === "work") {
    const rows: Row[] = [];
    if (!work.read) rows.push({ id: "reading", kind: "note", label: "Reading the record…", text: "" });
    else if (work.marks === null) rows.push({ id: "none", kind: "note", label: "This runtime keeps no record of marked answers the window can read.", text: "" });
    else {
      const bySkill = new Map<string, Mark[]>();
      for (const mark of [...work.marks].reverse()) {
        if (mark.paper) continue;
        const label = mark.label.trim() || "Without a skill named";
        bySkill.set(label, [...(bySkill.get(label) ?? []), mark]);
      }
      for (const [label, marks] of bySkill) {
        const said = marks
          .slice(0, 2)
          .map((mark) => `${shortDay(mark.at)}, ${markShort(mark)}`)
          .join("; ");
        rows.push({ id: `s-${label}`, kind: "thing", label: <Line text={label} />, text: label, hint: said, run: () => act.say(`Can we go over ${label}?`) });
      }
      for (const score of work.progress?.scores ?? []) {
        rows.push({ id: `sc-${score.paper}-${score.date}`, kind: "thing", label: score.paper, text: score.paper, hint: `${score.score}, ${shortDay(score.date)}` });
      }
      if (rows.length === 0) rows.push({ id: "empty", kind: "note", label: "Nothing marked yet. Each answer the record marks is kept here.", text: "" });
    }
    return <Layer name="Your work" mode="filter" placeholder="A skill" rows={rows} keys="↩ go over it with the tutor   esc close" onClose={props.onClose} />;
  }

  if (kind === "help") {
    const rows: Row[] = props.taskOpen
      ? [
          { id: "hint", label: "A hint", text: "hint", drive: "hint", run: () => act.help(HELP.hint) },
          { id: "lost", label: "I'm lost", text: "lost", drive: "lost", run: () => act.help(HELP.lost) },
          { id: "unknown", label: "I don't know", text: "don't know", drive: "dont-know", run: () => act.help(HELP.unknown) },
        ]
      : [{ id: "none", kind: "note", label: "No task is open. Ask the tutor anything on the conversation's line.", text: "" }];
    return <Layer name="Help with the task" mode="none" numbered rows={rows} keys="1 2 3 or ↩   ⌥1 ⌥2 ⌥3 in your answer: sure, unsure, a guess   esc close" onClose={props.onClose} />;
  }

  if (kind === "settings") {
    const { prefs, prefsChange, engine, setup } = props;
    const custom = (value: string, known: [string, string][]) => !known.some(([name]) => name === value.toLowerCase());
    const rows: Row[] = [
      {
        id: "face",
        label: "Reading face",
        text: "font face reading",
        choices: [
          ...FACES.map(([value, label]) => ({ label, chosen: prefs.face.toLowerCase() === value, face: faceCss(value), pick: () => prefsChange({ face: value }) })),
          ...(custom(prefs.face, FACES) ? [{ label: prefs.face, chosen: true, face: faceCss(prefs.face), pick: () => {} }] : []),
        ],
      },
      {
        id: "voice",
        label: "Your words",
        text: "font your words mono",
        choices: [
          ...VOICES.map(([value, label]) => ({ label, chosen: prefs.voice.toLowerCase() === value, pick: () => prefsChange({ voice: value }) })),
          ...(custom(prefs.voice, VOICES) ? [{ label: prefs.voice, chosen: true, face: faceCss(prefs.voice), pick: () => {} }] : []),
        ],
      },
      { id: "size", label: "Text size", text: "size text", choices: [...new Set([...SIZES, prefs.size])].sort((x, y) => x - y).map((value) => ({ label: String(value), chosen: prefs.size === value, pick: () => prefsChange({ size: value }) })) },
      { id: "measure", label: "Line length", text: "width measure line length", choices: MEASURES.map(([value, label]) => ({ label, chosen: prefs.measure === value, pick: () => prefsChange({ measure: value }) })) },
      { id: "scheme", label: "Scheme", text: "dark light scheme theme", choices: SCHEMES.map(([value, label]) => ({ label, chosen: prefs.scheme === value, pick: () => prefsChange({ scheme: value }) })) },
      {
        id: "ink",
        label: "Ink",
        text: "ink colour caret",
        choices: Object.keys(INKS).map((value) => ({ label: value, chosen: prefs.ink === value, pick: () => prefsChange({ ink: value, inkDark: value }) })),
      },
      { id: "hints", label: "Lines that name keys", text: "hints keys lines", choices: [true, false].map((value) => ({ label: value ? "Shown" : "Hidden", chosen: prefs.hints === value, pick: () => prefsChange({ hints: value }) })) },
      { id: "text", label: "Edit as text…", text: "edit settings as text config file font colour keys bindings", hint: "any font, colour or key", drive: "settings-text", run: act.editConfig },
    ];
    if (props.home) {
      const minutes = props.home.block_minutes;
      rows.push({
        id: "block",
        label: "A paper's time",
        text: "paper time minutes length block",
        choices: [25, 50, 90].map((value) => ({ label: `${value} min`, chosen: minutes === value, pick: () => act.setBlock(value) })),
      });
    }
    if (engine) {
      rows.push(
        props.lessonOpen
          ? { id: "engine", label: "Lessons run on", text: "engine", hint: `${ENGINE_NAMES[engine.chosen]}, until this lesson ends` }
          : {
              id: "engine",
              label: "Lessons run on",
              text: "engine lessons run on claude code magnesium",
              choices: (["plugin", "direct"] as Engine[]).map((value) => ({ label: ENGINE_NAMES[value], chosen: engine.chosen === value, pick: () => act.chooseEngine(value) })),
            },
      );
    }
    if (setup) {
      rows.push({
        id: "payment",
        label: "Paid with",
        text: "payment plan api key",
        choices: (["plan", "api_key"] as Payment[]).map((value) => ({ label: value === "plan" ? "Claude plan" : "API key", chosen: setup.payment === value, pick: () => act.choosePayment(value) })),
      });
      const account = setup.account;
      if (!account.signed_in) rows.push({ id: "signin", label: `Sign in to ${account.name}`, text: "sign in", run: act.signIn });
      else if (account.problem) rows.push({ id: "problem", kind: "note", label: account.problem, text: "" });
    }
    if (props.about) {
      const built = props.about.commit ? `${props.about.version.split("+")[0]}, built from ${props.about.commit.slice(0, 7)}` : `${props.about.version.split("+")[0]}, built from source`;
      rows.push({ id: "about", kind: "note", label: `Magnesium ${built}`, text: "" });
    }
    return <Layer name="Settings" mode="none" rows={rows} keys="↑ ↓ a setting   ← → change it   kept on this Mac   esc close" onClose={props.onClose} />;
  }

  // ⌘K: everything, and words for the tutor.
  const k = (action: keyof Prefs["keys"]) => keyName(props.prefs.keys[action]);
  const rows: Row[] = [];
  const ready = home?.state === "ready" || home?.state === "empty" || home?.state === "not_set_up";
  if (!props.inLesson && home && ready && !home.sitting) rows.push({ id: "start", label: "Start", text: "start continue carry on lesson", hint: "↩", run: act.start });
  if (!props.inLesson && home?.state === "ready" && !home.sitting && (home.checks_due ?? 0) + home.redos_due > 0) {
    rows.push({ id: "skip", label: "Start without the quick checks", text: "start skip checks redos due", hint: "they stay for another day", run: act.startSkip });
  }
  if (props.inLesson && props.busy) rows.push({ id: "interrupt", label: "Stop the reply", text: "stop reply interrupt", hint: k("stop"), run: act.interrupt });
  if (props.inLesson && props.taskOpen) rows.push({ id: "help", label: "Help with the task", text: "help hint lost don't know", hint: k("help"), run: () => props.open("help") });
  rows.push(
    { id: "plan", label: "The plan", text: "plan exam date midterm items", hint: k("plan"), run: () => props.open("plan") },
    { id: "courses", label: "Courses", text: "courses library folders switch", hint: k("courses"), run: () => props.open("courses") },
    { id: "material", label: "Material and papers", text: "material files papers slides notes", hint: k("material"), run: () => props.open("material") },
    { id: "work", label: "Your work", text: "work record marks history past answers progress", hint: k("work"), run: () => props.open("work") },
    { id: "settings", label: "Settings", text: "settings preferences font size scheme dark light ink keys engine payment", hint: k("settings"), run: () => props.open("settings") },
  );
  if (props.inLesson) rows.push({ id: "stop", label: "Stop here for now", text: "stop end finish quit lesson sitting", run: act.stop });
  if (!props.inLesson && home) rows.push({ id: "new", label: "A new lesson…", text: "new lesson goal", hint: k("new"), run: act.newLesson });
  for (const source of props.sources) {
    const ref = sourceRef(source.href);
    if (ref) rows.push({ id: `src-${source.href}`, label: `Open ${source.text}`, text: `source open ${source.text}`, run: () => act.openSource(ref) });
  }
  for (const item of plan?.items ?? []) {
    if (item.status === "done") continue;
    rows.push({ id: `w-${item.id}`, label: <Line text={`Work on ${item.title}`} />, text: `work on ${item.title}`, run: () => act.say(props.inLesson ? `Can we work on ${item.title} next?` : `Let's work on ${item.title}.`) });
  }
  if (plan?.path) rows.push({ id: "planfile", label: "Open the plan file", text: `plan file open markdown ${plan.path}`, hint: plan.path, run: () => act.openFile(plan.path!) });
  if (home && ready && !home.sitting) {
    rows.push(
      { id: "addfiles", label: "Add files to the material…", text: "add files material upload", run: act.chooseMaterial },
      { id: "paste", label: "Paste text as material…", text: "paste text material", run: act.paste },
    );
    for (const paper of home.papers) rows.push({ id: `sit-${paper.path}`, label: `Sit ${paper.name}`, text: `sit paper timed ${paper.name}`, hint: paper.minutes ? `${paper.minutes} minutes` : undefined, run: () => act.sitPaper(paper) });
  }
  if (home?.sitting) rows.push({ id: "handin", label: `Hand in ${home.sitting.name}…`, text: "hand in paper", run: act.handIn });
  if (materials && materials !== "none") {
    for (const name of materials.names.slice(0, 8)) rows.push({ id: `m-${name}`, kind: "file", label: `Open ${name}`, text: `open ${name}`, run: () => act.openFile(`Study/materials/${name}`) });
  }
  if (props.engine && !props.lessonOpen) {
    const other: Engine = props.engine.chosen === "plugin" ? "direct" : "plugin";
    rows.push({ id: "engine", label: `Run lessons on ${ENGINE_NAMES[other]}`, text: "engine lessons run on claude code magnesium", hint: `now ${ENGINE_NAMES[props.engine.chosen]}`, run: () => act.chooseEngine(other) });
  }
  if (props.setup) {
    const other: Payment = props.setup.payment === "plan" ? "api_key" : "plan";
    rows.push({ id: "pay", label: other === "plan" ? "Pay with your Claude plan" : "Pay with an API key", text: "payment plan api key pay", run: () => act.choosePayment(other) });
    if (!props.setup.account.signed_in) rows.push({ id: "signin", label: `Sign in to ${props.setup.account.name}`, text: "sign in account", run: act.signIn });
  }
  for (const card of props.cards ?? []) {
    if (card.folder === home?.folder) continue;
    rows.push({ id: `c-${card.folder}`, label: `Open ${displayName(card.name)}`, text: `open course ${card.name}`, run: () => act.switchCourse(card.folder) });
  }
  rows.push(
    { id: "pick", label: "Open a folder…", text: "open folder choose", run: act.pickFolder },
    { id: "newfolder", label: "Make a new folder…", text: "new folder make course", run: act.newFolder },
    { id: "find", label: "Find folders on this Mac", text: "find search folders", run: act.findFolders },
  );
  if (home) rows.push({ id: "forget", label: `Take ${displayName(home.name)} off the list`, text: "remove forget take off", hint: "the folder stays", run: () => act.forgetFolder(home.folder) });
  // Every setting's values, one Return away once typed for: "mono", "dark", "iowan", "size 19".
  const setting = (id: string, label: string, text: string, change: Partial<Prefs>, chosen: boolean) =>
    rows.push({ id: `set-${id}`, label, text, hint: chosen ? "now" : "setting", onlyFound: true, run: () => (props.prefsChange(change), props.onClose()) });
  const p = props.prefs;
  FACES.forEach(([value, label]) => setting(`face-${value}`, `Reading face: ${label}`, `reading face font ${value} ${label}`, { face: value }, p.face === value));
  VOICES.forEach(([value, label]) => setting(`voice-${value}`, `Your words: ${label}`, `your words font ${value} ${label}`, { voice: value }, p.voice === value));
  SCHEMES.forEach(([value, label]) => setting(`scheme-${value}`, `Scheme: ${label}`, `scheme theme ${value} ${label}`, { scheme: value }, p.scheme === value));
  [13, 15, 17, 19, 21, 24].forEach((size) => setting(`size-${size}`, `Text size ${size}`, `text size ${size}`, { size }, p.size === size));
  MEASURES.forEach(([value, label]) => setting(`measure-${value}`, `Line length: ${label}, ${value} characters`, `line length width measure ${label} ${value}`, { measure: value }, p.measure === value));
  Object.keys(INKS).forEach((ink) => setting(`ink-${ink}`, `Ink: ${ink}`, `ink colour color ${ink}`, { ink, inkDark: ink }, p.ink === ink));
  setting("hints-off", "Hide the lines that name keys", "hints keys lines hide off quiet", { hints: false }, !p.hints);
  setting("hints-on", "Show the lines that name keys", "hints keys lines show on", { hints: true }, p.hints);
  rows.push({ id: "config", label: "Edit settings as text…", text: "settings text config font colour keys bindings preferences", hint: "any font, colour or key", run: act.editConfig });
  rows.push({ id: "keys", kind: "note", label: "↩ start or send   Esc steps back   ⌘= ⌘− text size   ⌘U photos of your work", text: "keys shortcuts keyboard help" });
  return (
    <Layer
      name="Everything"
      mode="filter"
      placeholder="Find anything, or say it to the tutor"
      rows={rows}
      fallback={(text) => ({ id: "say", label: `Say to the tutor: “${text}”`, text, drive: "say", run: () => act.say(text) })}
      keys="↩ do it   type to find, or to say it to the tutor   esc close"
      onClose={props.onClose}
    />
  );
}
