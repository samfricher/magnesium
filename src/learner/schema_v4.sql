-- Tables and columns schema 4 adds. A fresh store runs this after
-- schema_v3.sql; the upgrade from schema 2 or 3 runs it once, inside its
-- transaction.

-- Schema 3 made practice_items before anything wrote to it, so it is empty in
-- every store. Schema 4 makes it again in the shape #30 needs. Its index goes
-- with it.
DROP TABLE practice_items;

-- Something the learner got wrong, to be reproduced cold on later days: a
-- quick check from chat, or a gap-log row from a paper (#30). Its passes,
-- status and next date are never stored. The runtime works them out from its
-- practice_events, the attempts they name and the plan's test date, so no
-- edit to a Markdown table changes what is due.
CREATE TABLE practice_items (
  item_id TEXT PRIMARY KEY,
  origin TEXT NOT NULL CHECK (origin IN ('chat','paper')),
  label TEXT NOT NULL,
  label_key TEXT NOT NULL,
  -- A paper row's plan, the folder name under Study/, and its # in that
  -- plan's gap log. A row with no plan is in no gap log.
  plan TEXT,
  row_no INTEGER CHECK (row_no >= 1),
  paper TEXT,
  paper_question TEXT,
  logged_on TEXT NOT NULL,
  gap_type TEXT CHECK (gap_type IN ('K','S','N','A','R')),
  what_went_wrong TEXT NOT NULL DEFAULT '',
  repair TEXT NOT NULL DEFAULT '',
  confidence TEXT CHECK (confidence IN ('sure','unsure','guess')),
  -- The next question to pose cold, and its key, which the tutor holds back
  -- until the learner has answered.
  next_question TEXT,
  answer_key TEXT,
  -- Why the tutor took the item out, when it was logged in error.
  dropped TEXT,
  -- An item brought in from a gap log or the earlier quick-check store
  -- starts from the state it had there.
  base_passes INTEGER NOT NULL DEFAULT 0 CHECK (base_passes BETWEEN 0 AND 3),
  base_next_on TEXT,
  base_last_pass_on TEXT,
  base_history TEXT NOT NULL DEFAULT '',
  base_misses_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(base_misses_json)),
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  CHECK ((plan IS NULL) = (row_no IS NULL)),
  CHECK (origin = 'paper' OR plan IS NULL),
  UNIQUE (plan, row_no)
);

-- What happened to an item, in order: made by an attempt, repaired by the
-- tutor, or answered again by an attempt. attempts.item_id from schema 3 is
-- not used: one answer can count for several items, so the links are here.
CREATE TABLE practice_events (
  event_no INTEGER PRIMARY KEY,
  item_id TEXT NOT NULL REFERENCES practice_items(item_id),
  kind TEXT NOT NULL CHECK (kind IN ('made','repair','attempt')),
  attempt_id TEXT REFERENCES attempts(attempt_id),
  at_ms INTEGER NOT NULL,
  note TEXT NOT NULL DEFAULT '',
  created_at_ms INTEGER NOT NULL,
  CHECK ((kind = 'repair') = (attempt_id IS NULL))
);

-- Each plan whose gap log the runtime has taken over: brought in once, then
-- written from the record. rendered_sha256 is the Gaps section as last
-- written, so an edit made by hand is noticed and saved, not lost.
CREATE TABLE gap_logs (
  plan TEXT PRIMARY KEY,
  imported_at_ms INTEGER NOT NULL,
  imported_rows INTEGER NOT NULL CHECK (imported_rows >= 0),
  rendered_sha256 TEXT,
  updated_at_ms INTEGER NOT NULL
);

-- An attempt the tutor corrected, as it was before, and why.
CREATE TABLE attempt_corrections (
  correction_no INTEGER PRIMARY KEY,
  attempt_id TEXT NOT NULL REFERENCES attempts(attempt_id),
  before_json TEXT NOT NULL CHECK (json_valid(before_json)),
  reason TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL
);

-- Right but over the time allowed: never a pass for a Type S row.
ALTER TABLE attempts ADD COLUMN slow INTEGER NOT NULL DEFAULT 0 CHECK (slow IN (0,1));

CREATE UNIQUE INDEX practice_chat_label ON practice_items(label_key) WHERE origin = 'chat';
CREATE INDEX practice_item_label ON practice_items(label_key);
CREATE UNIQUE INDEX practice_event_attempt ON practice_events(item_id, attempt_id) WHERE attempt_id IS NOT NULL;
CREATE INDEX practice_event_item ON practice_events(item_id, at_ms, event_no);
CREATE INDEX practice_event_by_attempt ON practice_events(attempt_id);
