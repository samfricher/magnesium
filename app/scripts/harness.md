# Driving the window

The ops and `data-drive` names a harness uses on a window started with `MAGNESIUM_APP_DRIVE=<dir>`. Send an op with `node scripts/drive.mjs <dir> '<op json>'`; each is answered `{ok, error?, ...}`.

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
- `session`: the session in front, with `mark_said` (what the live region tells a screen reader of the marks that arrived since the learner last sent, `""` when none has) and `pass_days` (each mark's rings: the `day` it earned and whether it is `arriving`, filling as it comes, whether it is `hung` in the margin or starts the mark's line, and its `left` edge in the window, which is never below 0)
- `smooth {ms, scroll_by?, type_every?}`
- `steady {text, back?, every?}`: write `text` on the line a key at a time, then take `back` characters off, and say how far the line moved on screen frame to frame (`largestMove`, `lowest` under the view's edge), how often the task's copy over the line came or went, and `from_foot`
- `caret {at?}`: where the caret is on the line, put at `at` first
- `watch {ms}`: each text the copy of the open task pinned over the line showed, in order, and for how long
- `document`
- `attach {path | paths}`
- `scroll {target?, to?, by?}`
- `menu`
- `menu_click {path}`
- `frame {width?, height?, x?, y?, on_top?}`
- `quit`

## `data-drive` names

- Tab bar: `home`, `tab-<n>`, `close-tab-<n>`, `new-session`, `plan-toggle`, `flip`, `settings` (only while Settings has no tab; once it has one, click its `tab-<n>`)
- Home: `start`, `workspace-<name>`, `open-folder`
- A workspace's page: `home`, `next` (the plan's item in hand, when the plan has one), `new-session` ("Something else…", or the page's first row when there is no item), `material` (opens the folded material), `add-material` (once the material is open, or when there is none), `read-<paper>`, `paper-<paper>`; while a paper runs, `sitting-hand-in`, `sitting-open`, `sitting-end` (asks first, in place), then `sitting-keep` or `sitting-end-now`, and `sitting-left` (the minutes left, or over)
- ⌘K: `layer`, `command-home`, `command-switch`, `command-new-session`, `command-close-session`, `command-settings`, `command-plan`, `switch-<workspace>`
- A session: `composer`, `start-session`, `open-task`, `allow`, `allow-always`, `start-again`, `sign-in`, `pick-model`
- A session with a task open and the tutor not writing: `look-back` (under the heading: "the lesson so far" while the page is the task alone, "only the task" while the lesson is open), and `look-back-top` (the same, at the top edge once the heading has scrolled away). The `session` op says which as `page`: `"folded"`, `"open"` or null
- A session's line: `more`, and in a lesson `stuck` too (not shown before a lesson has begun), and `stop` while the tutor writes. Stuck's row, which opens in the line above the words: `hint`, `lost`, `dont-know` (greyed while the tutor writes). Under More: `send-talk` (typing, with a task open), `attach`, `symbols`, `model`
- The model's list, from either line: `model-row` (with `data-value`), `model-effort` (with `data-level`), `model-older`, `model-current`
- The panel: `side-plan`, `plan-<item id>`, `doc-<title>`, `close-doc-<title>`, `open-document`, `open-<title>`, `zoom-in`, `zoom-out`, `zoom-fit`, `share-document`, `panel-expand`, `panel-grip`, `doc-pages`, `doc-flow`
- Settings: `colour-<kind>-<light|dark>`, `glass` (the slider for the window's ground, -10 to 10)
