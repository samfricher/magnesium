# The combined window

Issue #71. Three concepts were tried on a real course (#67 one surface, #68 the workspace editor, #69 the logbook); each felt like one course alone. This window keeps the part of each that worked and puts them in three levels, with tabs.

## Three levels, in tabs

- **Home** is what the window opens on. It is a plain list. First the most recent session, named by its goal, with its workspace and when it was last opened: it has the focus, so Return carries it on. Then every workspace (study folder) in the library, most recently opened first, with when. Nothing else: no levels, counts, due items or countdowns. Before this Mac can run a lesson, the one step it needs comes first, in the words #67 used.
- **A workspace** is a study folder's page, opened in the Home tab (‹ Home goes back). A visible **New session** sits at the top, over its sessions (a name and when last opened). Under them is what used to sit behind ⌘K: the folder's material (open, add files, paste text), its papers (sit one, hand it in, end it), the record of marked answers, and the folder itself (take it off the list, upgrade it, set it up).
- **A session** is the one-surface lesson of #67, in a tab of its own. It is the same as before: the reading face, the one column at its measure, the `›` line, the answer written inside the task, the record's marks, the plan card under a reply and the foot band. Two quiet things are new. A clip at the end of the line takes photos and PDFs. Under the answer line is a row of help: Hint, I'm lost, I don't know, Photo, Symbols. An earlier session opens with its saved turns and no tutor running. Writing in it continues it; Return on its empty line carries it on.
- **Settings** is a page in its own tab.

The tab bar is #68's: one quiet row beside the traffic lights, which the window is dragged by. Home is always first, then the open sessions, then Settings. ⌘1–9 pick a tab, ⌘W closes one, and ⇧⌘[ and ⇧⌘] step through them. At its right end are the plan's switch (in a session) and Settings.

The window holds one tutor's program at a time. The session tab that holds it gets the tutor's events. Writing in another tab moves the program there, after any reply under way has finished, and the first session continues from its saved turns when it is written to again. Reads of any folder never stop a lesson in another one: the core's reads now take a folder (`home_at`, `plan_at`, `lessons`, `lesson_turns` and the rest).

## The plan beside the lesson

From #69's agenda. The panel shows the exam and its date, then the plan's items as a task list (✓ done, ● the one in hand, ○ to do) with each block's day, then this session's steps. When the tutor changes the plan, the panel changes in place: a row that moved says *moved up*, *moved down*, *new* or *day moved*, and a moved exam says *moved from* its old date. B's plan card under the reply stays as it was. It shows no file, path or Markdown, and nothing in it edits the plan: the plan changes when the learner asks the tutor. It is shown and hidden by its switch, View > Show Plan, ⌘P or ⌘K, and the window remembers which. Hidden, the session is the surface alone.

## Settings

Sections with real controls. **Reading**: the face of the tutor's words and of your own (the built-in choices, or any face on this Mac by name, checked as you type), size, line width, and the lines that name keys. **Appearance**: like the Mac, light or dark; the ink (four swatches, or any colour for light and for dark); the ground and the text. **Keys**: every command, rebound by pressing the new combination; the menu bar follows. **Lessons**: the engine, how lessons are paid for, a paper's time, and the magnesium program. **About**: the version. Choices are kept in the window's own storage on this Mac. Nothing is shown as a file, and "Edit settings as text" is gone.

## Every command three ways

⌘K holds only the window's own commands: Home, Switch workspace, New session, Close session, Settings, and Show or hide the plan. Every command is also a visible control and an item in the menu bar (Magnesium, File, Edit, View, Go, Session, Window, Help). A chosen item sends the page the same command its control runs. The menu's keys are the learner's own keys.

## Files in a session

The clip, File > Attach… (⌘U), a file dropped on the session, and an image pasted into the line all add a file to the line. It shows there by its name and can be taken off, and it goes with the next message. Nobody is asked what it is. A file sent with an answer, or while a task is open, is the learner's working and is handed in with it. Any other goes as a file for the tutor to read, and the message says plainly that it may be working or material. The tutor (`prompts/tutor.md`) asks in one line when it cannot tell. Other kinds of file dropped on a session, and anything dropped on a workspace's page, are course material.

## No raw files

No path, settings file or Markdown is shown as interface anywhere: on the setup steps, in the core's messages (a path is said by its file's name, the material folder as "the course material"), in a permission card, under a message that took a file, or in the plan. File names and source names are fine.

## What stays true

A mark or a proof on screen comes only from the runtime's record (`marked`, `marks()`), never from the tutor's words. Talk is never marked, and the tutor can change the plan. Everything stays local: the window makes no network request, and lessons run on the learner's own Claude Code. The record's format and the rules for a proof are unchanged.

## The window's own state

The tabs, which one is in front, the workspace page in the Home tab and the plan's switch are kept in `state.json` as a new `window` field. When each folder and each session was last opened, a session's name and its engine are kept as new fields beside each folder. All of them are additive: an older build reads the file and skips them, and this build keeps any field it does not know. Beside each kept turn, in a file of its own next to the turns, the window keeps the ids of the record's rows that appeared for that message, so a reopened session shows each mark under the answer it was shown on. Turns kept before that match the record's rows by the minute, as they did. Drafts, the line's place and each session's scroll are kept per tab in the page's storage. The window reopens as it was left. After three hours away (the runtime's gap for a new sitting) it opens on Home, with the tabs still there, so starting stays one key.

## For the harness

`ready()` is called once Home (or the tab left in front) has its content. The `data-drive` names are: `start` (Home's first row), `composer`, `answer` and `talk` as before, `home`, `new-session`, `settings`, `plan-toggle`, `attach`, `tab-<n>`, and under the answer line `hint`, `lost`, `dont-know`, `photo`, `symbols`. Two ops read and use the menu bar, which the page cannot reach: `menu` and `menu_click {path}`. `errors` reports what went wrong on the page; `session` says where the session in front stands (its line, its open task, whether its tutor is running) and what is attached to its line; `attach {path | paths}` puts files on that line as a drop would; `type {text, replace}` with `replace` puts the words in place of all that is in the field. All of them do nothing unless `MAGNESIUM_APP_DRIVE` is set.
