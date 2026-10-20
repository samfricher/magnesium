# Using Magnesium

Magnesium helps your usual assistant teach understanding you can use independently. It draws on your materials, current work, and prior learning to choose what to explain, practise, and revisit. Workspace-local memory preserves descriptions of your understanding and difficulties, backed by your original attempts, so the tutor can build on them across conversations.

## Set up a study folder

[The install guide](install.md) tells your coding agent how to build Magnesium and set up a study folder. To set up another, run `magnesium init /path/to/your/course`, or run `magnesium init` from that folder. Initialization creates private `.magnesium` state and installs the packaged teaching instructions; in an existing Git repository, it adds `.magnesium` to local exclusions. Your study materials stay in place. Subfolders share its memory unless you initialize a separate workspace inside one. Incompatible learner data is preserved and rejected without migration.

- **Codex:** initialization installs the bundled plugin when the Codex CLI is available. Launch `codex` from the folder and approve Magnesium hooks if prompted.
- **OpenCode:** initialization detects the installed CLI and installs its Magnesium plugin, skill and MCP configuration. Follow any manual-registration instructions printed by `magnesium init`, then launch `opencode` from the folder.
- **Claude Code:** initialization sets up Magnesium's Claude plugin for this folder only. The CLI and the desktop app read the same files.
  - It copies the plugin to Magnesium's per-user data directory.
  - It declares that copy as the `magnesium` plugin source in your Claude user settings (`~/.claude/settings.json`, or `$CLAUDE_CONFIG_DIR`). That declaration turns Magnesium on nowhere.
  - It turns the plugin on only in this folder's `.claude/settings.local.json`.
  - In that file, it also adds Magnesium's permission rules to `permissions.allow`. Magnesium then runs in this folder without permission prompts.
  - `mcp__plugin_magnesium_magnesium__*` allows Magnesium's tools. A `Read` rule allows reads of the copied plugin, where the skills keep their templates. `Skill(magnesium:...)` rules let Magnesium's skills start without a prompt.
  - `Bash(magnesium digest)` allows the read-only command that `/magnesium:help` runs.
  - Claude Code applies `ask` and `deny` rules before `allow` rules. Initialization does not change those two lists.
  - To be asked before Magnesium's tools run, put `mcp__plugin_magnesium_magnesium__*` under `permissions.ask`. To block them, put it under `permissions.deny`.
  - To turn Magnesium off in one folder, set `"magnesium@magnesium": false` in that file. Initialization keeps that choice.
  - Start a new Claude Code session in the folder afterwards. Keep `magnesium` on `PATH`.
  - After you upgrade Magnesium, run `magnesium init --upgrade-instructions` once in each study folder. That run also refreshes the copied plugin and adds any new permission rules.
- **After upgrading Magnesium:** run `magnesium init --upgrade-instructions` once in each study folder, then start a new session. It does everything `magnesium init` does, including the plugin refresh and permission rules above. It also switches the folder to the new teaching instructions. Until you switch it, a folder keeps the instructions it was set up with, and a newer `magnesium` serves only its own. So the assistant reports Magnesium's instructions as unavailable, and plain `magnesium init` stops with an error there. The switch happens only if the old instructions were not edited. It keeps the old files and leaves learner history untouched.
- **Claude Code `/clear`:** Magnesium's tool connection keeps the conversation it started with, so after `/clear` Magnesium refuses to read or record learner state until you start a new Claude Code session. Start a new session instead of using `/clear` during a lesson.
- **Claude desktop pastes:** the desktop app sends pasted text between hidden `<pasted_content id="…">` and `</pasted_content id="…">` lines. Magnesium records a Claude message without those tag lines, blank lines before your first line, or whitespace after your last, so a pasted message keeps your exact words, including indentation, whether the hook records it or the tutor echoes it to start or resume a lesson. Other hosts' messages are recorded exactly as sent.

## Start with a request

In Claude Code, `/magnesium:help` looks at the folder and your learner record and suggests what to do next, and `/magnesium:help papers` shows how practice papers work. It reads both through `magnesium digest`, which prints the facts it uses as JSON and changes nothing; you can run it yourself in the study folder.

Ordinary project work doesn't start a lesson. Ask explicitly:

- “Help me understand recursion using this repository.”
- “Practise conditional probability with me. One question at a time.”
- “Review what we covered last time, then pick up where we stopped.”
- “Explain the solution directly. No quiz today.”

Give the tutor your attempts, questions and constraints. It can use authorized local files and preserve a checkpoint. You don't need to maintain memories yourself.

If a new kind of question leaves you without a way to start, the tutor works through a similar problem, then gives you a changed version to solve yourself. If that kind of question stops you again, the tutor leaves some steps for you to fill in. From the third time, you get the problem alone. If you say you are confused, it first asks one question about how you see the problem, and uses your answer to choose what to explain.

If a memory is wrong, say so: “That wasn't a conceptual gap; I misread the question. Please correct the memory.” The tutor can inspect your original work and revise its interpretation while keeping the history.

## Practise with timed papers

In Claude Code, say what you are studying for: “I have a midterm Monday” or “Build me practice papers for chapters 6 and 7”. You can also run `/magnesium:papers`. The tutor asks only for what your message and folder do not show. It then writes a plan, papers with separate keys, drills and a gap log in `Study/<target>/`, for example `Study/midterm-1/`. The tutor also keeps a map of the course's skills in `graph.yaml` and uses it to place you and to choose repairs. LaTeX papers build with `tectonic`. The tutor asks before it installs tectonic; without it, the tutor writes the papers in Markdown or HTML.

Sit each paper on your own, timed, and write `?` where you would look something up. Send photos or a PDF of your answers and say “mark my Diagnostic”. The tutor marks against the key, logs each lost mark, repairs it and sets a cold redo for later. It keeps the plan file current and asks before it adds a lot of time. Later conversations continue from the plan. Say “rebuild” to start over.

## Control recording and keep a copy

“Pause” or “stop learning” stops further lesson capture. Resume explicitly when ready. Already recorded work may still be consolidated while recording remains enabled.

“Turn recording off” blocks new Magnesium learner-text records. “Turn recording on” and resuming a lesson are separate actions.

Magnesium stores learner data locally, but your assistant's model receives selected memories and source excerpts. Exclude files from Magnesium's source access with `.magnesium/magnesiumignore`. Recording-off doesn't erase earlier Magnesium data, exports or provider chat history, or stop ordinary assistant responses.

From the study folder, run `magnesium export ../study-export` for a readable copy, or add `--format zip` for an archive. Sources are references by default; `--include-source-copies` adds verified in-scope files. Keep exports private.
