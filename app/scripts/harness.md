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
- `session`
- `smooth {ms, scroll_by?, type_every?}`
- `document`
- `attach {path | paths}`
- `scroll {target?, to?, by?}`
- `menu`
- `menu_click {path}`
- `frame {width?, height?, x?, y?, on_top?}`
- `quit`

## `data-drive` names

- Tab bar: `home`, `tab-<n>`, `close-tab-<n>`, `new-session`, `plan-toggle`, `flip`, `settings`
- Home: `start`, `workspace-<name>`, `open-folder`
- A workspace's page: `home`, `new-session`, `add-material`, `read-<paper>`, `paper-<paper>`
- ⌘K: `layer`, `command-home`, `command-switch`, `command-new-session`, `command-close-session`, `command-settings`, `command-plan`, `switch-<workspace>`
- A session: `composer`, `start-session`, `open-task`, `allow`, `start-again`
- A session's line, before the lesson and in it: `stuck` and `more`, and `stop` while the tutor writes. Under Stuck: `hint`, `help`, `dont-know` (greyed before a lesson has begun). Under More: `send-talk` (typing, with a task open), `attach`, `symbols`, `model`
- The model's list, from either line: `model-row` (with `data-value`), `model-effort` (with `data-level`), `model-older`, `model-current`
- The panel: `side-plan`, `plan-<item id>`, `doc-<title>`, `close-doc-<title>`, `open-document`, `open-<title>`, `zoom-in`, `zoom-out`, `zoom-fit`, `share-document`, `panel-expand`, `panel-grip`, `doc-pages`, `doc-flow`
- Settings: `colour-<kind>-<light|dark>`, `glass` (the slider for the window's ground, -10 to 10)
