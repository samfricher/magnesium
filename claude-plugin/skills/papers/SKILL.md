---
name: papers
description: Durable plans of timed practice papers in a folder set up with Magnesium (it has a .magnesium directory). Use for /magnesium:papers, when the learner names an assessment or a chapter set to study for ("I have a midterm Monday", "build me practice papers for chapters 6 and 7"), returns a paper for marking ("mark my Paper 1"), or uses a plan verb (teach me TAG, repair TAG, cold redo, new mock, re-time today, rebuild). Ordinary coding or repository work does not start it.
---

# Magnesium papers

Open the nearest initialized workspace `.magnesium/INSTRUCTIONS.md` with the Read tool, not a shell command. Load `context(action:"instructions",host:"claude",stage:"papers")` in build mode and for the verbs new mock and rebuild, and `stage:"papers_loop"` for every other verb. Follow that stage; it holds the policy. Load the same call with `operation:"tool_recipes"`. Then, before you read materials or templates, start or resume the Magnesium session for the target, because native capture expires within minutes. Reload after context loss or a bundle change.

In this session, the prompt notice usually repeats the teaching rule and can say that the previous reply had 200 words or more or had no task. The rule is for teaching replies. A build, a marking report or a plan change runs as long as this stage needs and needs no task, so ignore that feedback after one.

## Choose the target and the mode

The target slug is short, given or derived from the request: `midterm-1`, `test-1`, `final`, `ch6-7`, `computability`. If the request names no target and `Study/` holds one plan, use that plan. If it holds several, match the paper or tag the learner names, and ask only when that does not decide it.

Start the Magnesium session with a goal that names the target slug, such as "midterm-1 practice papers". `/magnesium:help` finds the plan's session by that slug.

- `Study/<target>/plan.md` does not exist: build mode.
- `Study/<target>/plan.md` exists: run mode. Read `plan.md`, `gap-log.md` and `graph.yaml` and prepare current memory before you act. Do not rebuild.

In `plan.md`, the When cell starts with the date in ISO form, such as `2026-10-21 (Wed) 12:30 to 14:00`. `magnesium digest` reads the date from there. Each session line under Sessions starts `If it is <days> at <HH:MM>, then`, with a 24-hour time, as the template shows. `magnesium digest` reads the days and the time from there.

## Loop verbs

Accept any phrasing of: mark my PAPER, teach me TAG, repair TAG, cold redo, new mock, re-time today, rebuild. Only "rebuild" starts the plan over.

## Templates

When Claude Code loads this skill, it prints "Base directory for this skill: <path>". The templates are in `<path>/templates/`. `magnesium init` rewrites that directory and installs its files with mode 0600. The templates directory is outside the study folder. Magnesium's folder settings allow the Read tool there without a permission prompt, but the shell asks the learner each time. So read each template with the Read tool and write each filled copy with the Write tool, which also makes any missing folders. Do not use `ls`, `cat` or `cp` on the templates directory, also not inside a longer shell command. The table names every template, so you do not need to list the directory. Do not edit the originals.

| Template | Copy to |
|---|---|
| `plan.md` | `Study/<target>/plan.md` |
| `gap-log.md` | `Study/<target>/gap-log.md` |
| `graph.yaml` | `Study/<target>/graph.yaml` |
| `drills.md` | `Study/<target>/drills.md` |
| `past-papers-README.md` | `Study/<target>/past-papers/README.md` |
| `paper.md` | `Study/<target>/papers/<Name>.md`, one per Markdown paper; its key is `<Name>-KEY.md` |
| `preamble.tex`, `build.sh` | `Study/<target>/papers/src/`, for LaTeX papers; one `<Name>-body.tex` per paper |

Name papers Diagnostic, Paper1-Easy, Paper2-Medium, MockA, MockB and Stretch1, as the ramp needs them. Run the LaTeX build as `bash Study/<target>/papers/src/build.sh`, never as `./build.sh`, because the copied script keeps mode 0600 and is not executable. The script writes `<Name>.pdf` and `<Name>-KEY.pdf` into `papers/` and exits non-zero when a build fails.

## Upgrade an old gap log

A `gap-log.md` without a `Next redo` column comes from an older template, in which one cold redo closed a row. Upgrade it before you act on it, in this order:

1. In the Gaps table, rename the `Cold-redo date` column to `Next redo`, so each row keeps its date. If there is no such column, add `Next redo` with empty cells. Put an empty `Passes` column before it, and add `Conf` after `Type`, with an empty cell in every row.
2. Clear Next redo on closed rows, and keep them closed. Write their Status as exactly `closed`, so that a row closed in other words, such as `Closed ✓` or `done`, does not read as an unfixed gap. An open row keeps its old date, so a row whose date has passed is due today. An open row with no date gets today's date only when its Repair done is filled. An unrepaired row waits for its repair.
3. Read the `gap-log.md` template, and replace the old file's text above `## Tags` with the template's text.
4. In `plan.md`, replace steps 1, 4 and 5 of the Method with the template's steps.
5. Dismiss the plan's old review suggestions for gap tags. Next redo is now each row's schedule.
6. Update the papers not yet marked, as step 4 of the next list says.

Log the upgrade as one line under Changes in `plan.md`.

A study folder comes from before confidence letters when `gap-log.md` has no `Conf:` line above `## Tags`, or when the Method in `plan.md` or a paper not yet marked does not ask for S, U or G. Look for these even when papers-loop has already added the `Conf` column, because that step adds only the column. Upgrade the folder before you act on it, in this order:

1. If the Gaps table has no `Conf` column, add one after `Type`, with an empty cell in every row. A row with a missing cell reads the cells after it under the wrong headers.
2. If `gap-log.md` has no `Conf:` line, read the `gap-log.md` template, and replace the old file's text above `## Tags` with the template's text.
3. If the Method does not ask for S, U or G, replace steps 1, 4 and 5 of the Method in `plan.md` with the template's steps.
4. Update each paper not yet marked, so the learner sees the S, U and G line. A paper is marked when Scores has its row with a real score, not the template's `<score>` placeholder. In a Markdown paper, replace the paragraph under the title line that asks for a `?` with the `paper.md` template's paragraph. For LaTeX papers, replace the line of `\paperheader` in `papers/src/preamble.tex` that asks for a ? with the `preamble.tex` template's line, and run the build again.

Log the upgrade as one line under Changes in `plan.md`.

## Optional tools

Write LaTeX papers only when tectonic is available. Otherwise write Markdown or HTML papers, which also work. The study folder's `.magnesium/config.toml` records each answer in a `[tools]` table, as `tectonic = "installed"` or `tectonic = "declined"`. Read it with the Read tool before you choose a format.

If there is no answer and the course needs math notation, check `command -v tectonic` once. If tectonic is absent, ask: "May I install tectonic? It lets me build your practice papers as PDFs with proper math notation. Without it I write them in Markdown or HTML, which also works." On yes, install it the way this operating system installs software, then record `"installed"`. On no, record `"declined"` and write Markdown or HTML. With the Edit tool, add the key to the `[tools]` table, or append the table at the end of the file if there is none. Keys below a table header belong to that table, so a table placed higher would capture `schema_version` and `workspace_id`. Each study folder keeps its own answer, so do not ask again in this folder after `"declined"` unless the learner raises it.

If `heic = "declined"` and the learner sends iPhone photos in HEIC format, ask for PDF or JPEG instead.
