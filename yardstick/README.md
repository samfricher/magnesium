# Yardstick

The yardstick measures Magnesium with five checks. It runs on the code as it is today, so every later change can be judged by the same numbers.

It reaches Magnesium only the way a learner's tools do: the `magnesium` command line, the hooks and tools Claude Code calls, and Claude Code itself with the plugin. All of that sits in one file, `adapter.py`. A later version of Magnesium that records a lesson turn in one call can get a second adapter there, and the checks stay the same.

## Run it

```bash
python3 yardstick/run.py <check> --fixture <fixture folder> --binary <path to magnesium> [--plugin <plugin folder>] [--out <results folder>]
```

Each check prints a short table and writes a JSON file to `--out`. Scratch study folders go in a `runs` folder beside it, or in `--work`. Nothing runs in the fixture folder itself: every check works on a copy.

| Check | What it asks | Needs a model |
|---|---|---|
| `coverage` | For each thing the test asked: is it in the materials, did the plan list it, was it taught, and did Magnesium say it was never taught? | no |
| `evidence` | Before the test, what does the record say about each thing the test asked? | no |
| `turn-cost` | How many tool calls per learner message, and how much text comes back per call? | no |
| `scenes` | Seven short lessons from real sessions, three runs each. Does the tutor do the right thing? | yes |
| `plan-build` | Count what a study plan holds against the course's own lists, or have Magnesium build one and count that. | for judged counts and the live build |

Examples, from the repository root:

```bash
python3 yardstick/run.py coverage --fixture yardstick/dummy/course --binary target/release/magnesium
python3 yardstick/run.py turn-cost --transcript ~/.claude/projects/<folder>/<lesson>.jsonl --binary target/release/magnesium
python3 yardstick/run.py turn-cost --host window --fixture <fixture> --binary target/release/magnesium
python3 yardstick/run.py turn-cost --mode scripted --fixture yardstick/dummy/course --binary target/release/magnesium
python3 yardstick/run.py scenes --fixture yardstick/dummy/course --binary target/release/magnesium --plugin claude-plugin
python3 yardstick/run.py plan-build --fixture yardstick/dummy/course --binary target/release/magnesium --judge
python3 yardstick/run.py plan-build --mode live --fixture yardstick/dummy/goal --binary target/release/magnesium --plugin claude-plugin
```

## What each check prints

**Coverage.** One row per item the test asked. "Taught before" is written by hand in the fixture, with its evidence. The other columns are worked out. Today Magnesium has no course map, so it flags nothing, and the check says so. It prints "Links: none" where a later version will report how items use one another.

**Evidence.** One row per item: how many answers were marked before the test, how many were right with no help and cold (today's nearest thing to answering from a blank page), how many had help, the last date, and Magnesium's own words for the item from `magnesium progress --json`. The check says which answer fields it read.

**Turn cost.** From a lesson transcript, one row per day: learner messages, Magnesium tool calls, record calls (`learning_step`) per message, and the characters that came back per call. It gives the characters as the tutor received them and also pretty-printed, because the earlier count of 7,000 to 9,000 was made on pretty-printed text. In `--mode scripted` it drives an invented ten-message lesson through the real binary, as the tutor's instructions say a lesson should go, and prints the same numbers.

**Scenes.** Each scene starts a fresh copy of the dummy course, sets up what came before through Magnesium's own tools, then plays the learner's messages to Claude Code with the plugin, started the way the study window starts it. A scene passes when every rule for it passes. A rule is mechanical when a fact settles it, such as the open task that `magnesium lesson --json` reports. Otherwise a second call to the same model judges it with a fixed rubric, and the table says "judged". The check also counts, roughly, requests for an explanation, stalls and detours per section, and replies that set no task, for comparison with real lessons later. `--rejudge <result file>` applies the current rules to saved runs without new lessons, for when a rubric changes.

**Plan build.** The scorer reads a plan folder against the fixture's lists. Mechanical counts: listed exercises that a timed or section block poses itself (not ones another problem is said to carry), listed exercises with a row of their own, listed exercises mentioned anywhere, slide ideas and added items with a practice item, and sections in lecture order in the schedule.

With `--judge` it also marks the plan's schedule block by block, by the codebook in `plan-blocks.md`: for each block, whether it was already done, which sections it works, what kind of work it opens with (a paper the tutor wrote, the instructor's own paper, teaching first, a test first, marking, work picked by lost marks, redos), which listed exercises it sets or carries, and whether it holds a problem the learner has not seen. The marking runs three times and the majority is kept. The shape is added up from those marks: sections worked, sections in lecture order, sections that open with a test, sections with an unseen problem, listed exercises set or carried, whether a paper the tutor wrote comes before any section, whether the instructor's own paper is sat, and how many blocks are picked by lost marks. The table shows where the three runs differed. When the fixture names a `plan_key` folder, a hand-marked key for a plan is shown beside the judged shape, with the agreement field by field and every block where they differ. One more judged call says whether ideas with no question of their own name the problem that carries them, and whether every cut is named by its content.

`--mode live` starts Magnesium on a copy of the fixture's folder, without the fixture's own plan, asks for a plan as a learner would, records minutes, tokens and the first searches, and scores the result, always with the judge. `--builds N` runs N separate builds, `--parallel` at a time, and adds a table of their shapes side by side. `--settle-index` lets Magnesium's source index finish before Claude Code starts, so a build can be judged apart from how fast the folder is read. `--planning own-run` is the later design, where planning runs outside the lesson. It is not built yet, and the check says so.

## Live runs

Live checks start Claude Code with an empty environment: a scratch home, the learner's user name, and a path with the measured `magnesium` first. The scratch home links the macOS keychain folder, so Claude Code finds its existing sign-in, and the link is removed when the run ends. If it reports that it is signed out, the check stops and prints what it said. It never signs in. The default model is `claude-sonnet-5-5` at medium effort. A Fable model is refused.

Magnesium has no way to set its clock. A scene that needs "yesterday" sends the earlier answers as messages dated the day before the live chat, which Magnesium labels as reported by the tutor. Magnesium opens a new sitting only three hours after the last learner message it stored, and stores the time it received a message, not the time the message was dated. The two scenes that start a sitting (due checks, and three days to a test) therefore set that stored time, in the scratch copy's `.magnesium/sittings.sqlite`, to their last seeded message. This is the one place the yardstick writes Magnesium's own files, and it lives in the adapter for today's Magnesium. `--settle-index` reads the source index's own tables, without writing, to see when indexing has finished; that is also in the adapter. If Magnesium still states no due checks, the run is shown as not counted. A plan build for a test that is already past states the time to the test in hours instead of a date.

## Fixtures

A fixture is a folder:

```text
fixture.json   name, as_of, test {label, at}, folder, target, attempts, asked, transcripts
asked.json     what the test asked, one item each: material, terms, plan_refs, record_tags, taught_before_test, evidence
attempts.json  the output of `magnesium attempts --json` on a copy of the record
folder/        the study folder as it stood on as_of
plans/         plans to score, one folder each
plans-key/     a hand-marked key for a plan, one <plan>.json each, named by fixture.json's plan_key
lists/         exercises.json, slide-ideas.json, added-items.json, order.json
```

A fixture can also hold `record.json` (a scripted record that `evidence` replays to get Magnesium's labels), `lesson-script.json` (for scripted turn cost) and `scenes.json`.

Two invented fixtures are committed:

- `dummy/course`: a small probability course with slides, a weekly exercise list, one old test, a plan in today's shape, the plan an outside agent would have written, a scripted record and an invented two-day lesson transcript.
- `dummy/goal`: one textbook chapter and a goal, with no course, no instructor, no test and no past paper.

Fixtures made from a real learner's record stay out of the repository.

## Tests

```bash
python3 ci/check.py python
```

runs the yardstick's own tests, on small invented inputs, with no binary and no model.
