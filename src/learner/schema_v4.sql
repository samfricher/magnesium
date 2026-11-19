DROP TABLE practice_items;

CREATE TABLE practice_items (
  item_id TEXT PRIMARY KEY,
  origin TEXT NOT NULL CHECK (origin IN ('chat','paper')),
  label TEXT NOT NULL,
  label_key TEXT NOT NULL,
  plan TEXT,
  row_no INTEGER CHECK (row_no >= 1),
  paper TEXT,
  paper_question TEXT,
  logged_on TEXT NOT NULL,
  gap_type TEXT CHECK (gap_type IN ('K','S','N','A','R')),
  what_went_wrong TEXT NOT NULL DEFAULT '',
  repair TEXT NOT NULL DEFAULT '',
  confidence TEXT CHECK (confidence IN ('sure','unsure','guess')),
  next_question TEXT,
  answer_key TEXT,
  dropped TEXT,
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

CREATE TABLE gap_logs (
  plan TEXT PRIMARY KEY,
  imported_at_ms INTEGER NOT NULL,
  imported_rows INTEGER NOT NULL CHECK (imported_rows >= 0),
  rendered_sha256 TEXT,
  updated_at_ms INTEGER NOT NULL
);

CREATE TABLE attempt_corrections (
  correction_no INTEGER PRIMARY KEY,
  attempt_id TEXT NOT NULL REFERENCES attempts(attempt_id),
  before_json TEXT NOT NULL CHECK (json_valid(before_json)),
  reason TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL
);

ALTER TABLE attempts ADD COLUMN slow INTEGER NOT NULL DEFAULT 0 CHECK (slow IN (0,1));

CREATE UNIQUE INDEX practice_chat_label ON practice_items(label_key) WHERE origin = 'chat';
CREATE INDEX practice_item_label ON practice_items(label_key);
CREATE UNIQUE INDEX practice_event_attempt ON practice_events(item_id, attempt_id) WHERE attempt_id IS NOT NULL;
CREATE INDEX practice_event_item ON practice_events(item_id, at_ms, event_no);
CREATE INDEX practice_event_by_attempt ON practice_events(attempt_id);
