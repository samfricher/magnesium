---
name: papers
description: Study plans built from the course's own files, and the practice papers and marking that run on them, in a folder set up with Magnesium (it has a .magnesium directory). Use for /magnesium:papers, when the learner names an assessment or a chapter set to study for ("I have a midterm Monday", "build me practice papers for chapters 6 and 7"), returns a paper for marking ("mark my Paper 1"), or uses a plan verb (teach me a section, repair an item, cold redo, new mock, re-time today, re-plan, fix my plan files, rebuild). Ordinary coding or repository work does not start it.
---

# Magnesium papers

Open the nearest initialized workspace `.magnesium/INSTRUCTIONS.md` with the Read tool, not a shell command. Load `context(action:"instructions",host:"claude",stage:"papers")` in build mode and for the verbs new mock, re-plan and rebuild, and `stage:"papers_loop"` for every other verb. Follow that stage; it holds the policy. Load the same call with `operation:"tool_recipes"`. Then, before you read materials or templates, start or resume the Magnesium session for the target, because native capture expires within minutes. Reload after context loss or a bundle change.

During this Magnesium session, the prompt notice usually repeats the teaching rule and can say that the previous reply had 200 words or more. The rule is for teaching replies. A marking report or a plan change runs as long as this stage needs and needs no task, so ignore that feedback after one.

When the learner says they are stopping for now (for today, they have to go, that is enough), send learning_step `{"schema":"learner-model/v2","action":"end_sitting"}` once, before your reply, and follow its note. Magnesium then shows the learner, under your reply, what their record says of today; never say it yourself, and set no task.

## Choose the target and the mode

The target slug is short, given or derived from the request: `midterm-1`, `test-1`, `final`, `ch6-7`, `computability`. If the request names no target and `Study/` holds one plan, use that plan. If it holds several, match the paper or tag the learner names, and ask only when that does not decide it.

Start the Magnesium session with a goal that names the target slug, such as "midterm-1 practice papers". `/magnesium:help` finds the plan's session by that slug.

- The course map has no target of that name (course_map read with the target says so): build mode. The planning run builds the map and the plan, as stage papers says.
- It has one: run mode. Read `plan.md` and `gap-log.md`, course_map read for the target, and prepare current memory before you act. Do not rebuild.

The runtime writes `Study/<target>/plan.md` and `map.md` from the course map, in the forms `magnesium digest` reads. Never edit them: a change goes through course_map write, or through the planning run.

## Loop verbs

Accept any phrasing of: mark my PAPER, teach me a section or item, repair an item, cold redo, new mock, re-time today, re-plan, fix my plan files, rebuild. Only "rebuild" starts the plan over.

## Templates

When Claude Code loads this skill, it prints "Base directory for this skill: <path>". The templates are in `<path>/templates/`. `magnesium init` rewrites that directory and installs its files with mode 0600. The templates directory is outside the study folder. Magnesium's folder settings allow the Read tool there without a permission prompt, but the shell asks the learner each time. So read each template with the Read tool and write each filled copy with the Write tool, which also makes any missing folders. Do not use `ls`, `cat` or `cp` on the templates directory, also not inside a longer shell command. The table names every template, so you do not need to list the directory. Do not edit the originals.

| Template | Copy to |
|---|---|
| `gap-log.md` | `Study/<target>/gap-log.md` |
| `drills.md` | `Study/<target>/drills.md` |
| `past-papers-README.md` | `Study/<target>/past-papers/README.md` |
| `paper.md` | `Study/<target>/papers/<Name>.md`, one per Markdown paper; its key is `<Name>-KEY.md` |
| `preamble.tex`, `build.sh` | `Study/<target>/papers/src/`, for LaTeX papers; one `<Name>-body.tex` per paper |

Name papers after the block that sets them, such as SectionC, MockA, MockB or Stretch1. Run the LaTeX build as `bash Study/<target>/papers/src/build.sh`, never as `./build.sh`, because the copied script keeps mode 0600 and is not executable. The script writes `<Name>.pdf` and `<Name>-KEY.pdf` into `papers/` and exits non-zero when a build fails.

LaTeX cannot break a line inside `\text{}`, `\mbox{}` or a display, so such a line can be set wider than its column. The script finds where each such line ends on the page. When it ends past the right edge, where every PDF viewer cuts it mid-word, or within a quarter inch of the edge, which a printer cuts off, the script prints `CUT LINE` with the line of `<Name>-body.tex` and does not write that PDF. When it cannot tell where the line ends, it prints `UNCHECKED LINE` and does not write the PDF either. Rewrite each line it names so it fits its column. In running text, end the math before the words, as in `$\{\langle G,k\rangle :$ $G$ has a vertex cover of size at most $k\}$`. Break a long display into lines with `multline*` or `aligned` and `\\`. In a table, shorten the cell or widen its column. Then run the build again until it prints neither. Never give the learner a paper or key with a `CUT LINE`, an `UNCHECKED LINE` or a failed build. A `wide line` note names a line that stays on the page, in the margin or inside the text, where it can cover what is beside it. Look at that page, and rewrite the line if it covers anything.

In a folder from before this check, `papers/src/build.sh` has no `CUT LINE` in it. Before you build there, write the `build.sh` template's text over that copy, and run the build once without a paper name, so it checks every paper.

## Upgrade an old gap log

Magnesium brings an older gap log's rows into the learner record when stage `papers_loop` loads, and writes its Gaps table from then on, as that stage says. If the text above `## Gaps` in `gap-log.md` differs from the `gap-log.md` template's, read the template and replace the old file's text above `## Gaps` with the template's. A folder whose plan was written by hand, with a `graph.yaml` beside it, gets a new plan from the planning run; the old files stay as they are until the learner removes them.

## Optional tools

Write LaTeX papers only when tectonic is available. Otherwise write Markdown or HTML papers, which also work. The study folder's `.magnesium/config.toml` records each answer in a `[tools]` table, as `tectonic = "installed"` or `tectonic = "declined"`. Read it with the Read tool before you choose a format.

If there is no answer and the course needs math notation, check `command -v tectonic` once. If tectonic is absent, ask: "May I install tectonic? It lets me build your practice papers as PDFs with proper math notation. Without it I write them in Markdown or HTML, which also works." On yes, install it the way this operating system installs software, then record `"installed"`. On no, record `"declined"` and write Markdown or HTML. With the Edit tool, add the key to the `[tools]` table, or append the table at the end of the file if there is none. Keys below a table header belong to that table, so a table placed higher would capture `schema_version` and `workspace_id`. Each study folder keeps its own answer, so do not ask again in this folder after `"declined"` unless the learner raises it.

If `heic = "declined"` and the learner sends iPhone photos in HEIC format, ask for PDF or JPEG instead.
