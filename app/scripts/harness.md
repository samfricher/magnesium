# Driving the window

The ops and `data-drive` names a harness uses on a window started with `MAGNESIUM_APP_DRIVE=<dir>`. Send an op with `node scripts/drive.mjs <dir> '<op json>'`; each is answered `{ok, error?, ...}`.

To photograph one state without writing a script, `node scripts/look.mjs --at task click:stuck shot:stuck` builds what changed, opens the window on the made-up course, goes to a place (`home`, `workspace`, `session`, `task`, `marked`, `settings`), does each step and prints the photographs' paths. `--base <ref>` does the same in the window built from that commit, for before and after; `--keep` leaves the window open for more ops. A script of your own gets the same course, windows and photographs from `scripts/window.mjs`, as `scripts/pages.mjs` does.

## Ops

- `type {text, replace?}`
- `key {key, meta?, shift?, alt?, ctrl?}`
- `click {target, by?}`: a `data-drive` name, else exact visible text, else a substring of it; with `by: "drive"`, the name only
- `focus {target, by?}`
- `slide {target, value, by?}`: set a slider, as a drag to that value does
- `wait_idle {timeout_ms, quiet_ms?}`
- `wait_reply {timeout_ms}`
- `wait {ms}`
- `text`
- `perf`
- `scheme {scheme}`: `dark`, `light` or `system`
- `plan`
- `transcript`
- `ready`
- `errors`
- `session`: the session in front, with `heads` (the lesson's step heads drawn now, as a screen reader hears each: "Step 2 of 3: A 95% interval"), `top_count` (what the top edge counts, "2 of 3", or null), `mark_said` (what the live region tells a screen reader of the marks that arrived since the learner last sent, `""` when none has) and `pass_days` (each mark's rings: the `day` it earned and whether it is `arriving`, filling as it comes, whether it is `hung` in the margin or starts the mark's line, and its `left` edge in the window, which is never below 0)
- `smooth {ms, scroll_by?, type_every?}`
- `steady {text, back?, every?}`: write `text` on the line a key at a time, then take `back` characters off, and say how far the line moved on screen frame to frame (`largestMove`, `lowest` under the view's edge), how often the task's copy over the line came or went, and `from_foot`
- `still {ms, send?, alt?, past?}`: with `send`, press Return (⌥ with `alt`) first; with `past`, once the turn ends, scroll so the reply's top is just above the view, as a learner who read on; then for `ms`, every frame, where the last reply stands on screen and how many step heads are drawn. Says when the turn ended (`ended_at`), when a step it began was heard (`step_at`), when a head was drawn (`head_at`), the reply's top in the frames either side of `step_at`, and every frame after the turn ended in which the reply moved (`moves_after_end`)
- `caret {at?}`: where the caret is on the line, put at `at` first, and the words on it (`text`)
- `watch {ms}`: each text the copy of the open task pinned over the line showed, in order, and for how long
- `document`
- `attach {path | paths}`
- `scroll {target?, to?, by?}`
- `menu`: the menu bar as data; `replaced`, how many times the whole menu bar was put in place since the window opened; and `native`, the bar as macOS holds it (each menu's item titles in order, with their keys), which also has what macOS adds, such as the window list in Window
- `switch {target, ms?}`: click a tab or a row (as `click` names it), then every frame for `ms` (800 by default) read what the view in front shows: its words' length and its first lines with where each stands. Says when the view first differed from what it showed before the click (`answered_at`, in ms, or null), that `first` frame, and `changes_after_first`, each later frame that differed: whether a line `moved` (stands elsewhere, or lines came or went; words that change where a line stands are not a move), where the keyboard is (`focus`) and, when the words changed, where they part (`text`)
- `menu_click {path}`
- `frame {width?, height?, x?, y?, on_top?}`
- `quit`

## `data-drive` names

- Tab bar: `home` (the first tab showing Home or a workspace's page), `tab-<n>` (a session not begun is named "New session"), `close-tab-<n>` (on every tab; on the last one it closes the window), `new-tab` (+: Home in a new tab, or the tab already showing it; ⌘N, sent with the `key` op, starts a new session), `plan-toggle` ("Plan": the plan and the session's documents), `flip`, `settings` (only while Settings has no tab; once it has one, click its `tab-<n>`)
- Home: `start` (the step; with a question shown, "Start it instead" under Then), `something-else`, `workspace-<name>`, `open-folder`, `just-checks` (with a check due and no question shown), `home-answer` (the line under the first quick check's question, when Home shows one; Return there starts the checks)
- A workspace's page: `home`, `next` (the plan's item in hand, when the plan has one), `new-session` ("Something else…", or the step itself when there is no item), `show-sessions` (past six sessions: shows them all, then fewer), `show-repair` (a long list of skills to repair: shows the rest), `material` (shows the course's files, then hides them), `add-material`, `read-<paper>`, `paper-<paper>`; while a paper runs, `sitting-hand-in`, `sitting-open`, `sitting-end` (asks first, in place), then `sitting-keep` or `sitting-end-now`, and `sitting-left` (the minutes left, or over)
- ⌘K: `layer`, `command-home`, `command-switch`, `command-new-session`, `command-close-session`, `command-settings`, `command-plan` ("Show plan" or "Hide plan"), `switch-<workspace>`
- A session: `composer`, `start-session`, `open-task`, `allow`, `allow-always`, `start-again`, `sign-in`, `pick-model`
- A session that took an answer written on Home: while it waits for the tutor to pose the check, the held words carry `data-role="held"`; when the check posed could not be matched to Home's and the words were put on the line, the note in the lesson under the reply carries `data-role="other-check"`
- A session with a task open and the tutor not writing: `look-back` (under the heading: "Show the lesson" while the page is the task alone, "Hide the lesson" while the lesson is open), and `look-back-top` (the same, at the top edge once the heading has scrolled away). The `session` op says which as `page`: `"folded"`, `"open"` or null
- A session's line: `more`, and in a lesson `stuck` too (not shown before a lesson has begun), and `stop` while the tutor writes. Stuck's row, which opens in the line above the words: `hint`, `lost`, `dont-know` (greyed while the tutor writes). Under More: `send-talk` (typing, with a task open), `attach`, `symbols`, `model`
- The model's list, from either line: `model-row` (with `data-value`), `model-effort` (with `data-level`), `model-older`, `model-current`
- The panel: `side-plan`, `plan-<item id>`, `doc-<title>`, `close-doc-<title>`, `open-document`, `open-<title>`, `zoom-in`, `zoom-out`, `zoom-fit`, `share-document`, `panel-expand`, `panel-grip`, `doc-pages`, `doc-flow`
- Settings: `colour-<kind>-<light|dark>`, `glass` (the slider for the window's ground, -10 to 10)
