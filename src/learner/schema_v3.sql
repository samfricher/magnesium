CREATE TABLE attempts (
  attempt_no INTEGER PRIMARY KEY,
  attempt_id TEXT NOT NULL UNIQUE,
  observation_id TEXT NOT NULL REFERENCES observations(observation_id),
  response_event_id TEXT NOT NULL REFERENCES learning_events(event_id),
  session_id TEXT REFERENCES learning_sessions(session_id),
  question TEXT NOT NULL,
  paper TEXT,
  paper_question TEXT,
  label TEXT NOT NULL,
  label_key TEXT NOT NULL,
  mark TEXT NOT NULL CHECK (mark IN ('right','partly','wrong','declined')),
  marks REAL,
  out_of REAL,
  help TEXT NOT NULL CHECK (help IN ('none','hint','step','solution')),
  confidence TEXT CHECK (confidence IN ('sure','unsure','guess')),
  cold INTEGER NOT NULL CHECK (cold IN (0,1)),
  answered_at_ms INTEGER NOT NULL,
  seconds INTEGER CHECK (seconds >= 0),
  item_id TEXT REFERENCES practice_items(item_id),
  problem_kind TEXT,
  created_at_ms INTEGER NOT NULL,
  CHECK (cold = 0 OR help = 'none'),
  CHECK ((paper IS NULL) = (paper_question IS NULL)),
  CHECK ((marks IS NULL) = (out_of IS NULL)),
  CHECK (marks IS NULL OR (out_of > 0 AND marks >= 0 AND marks <= out_of))
);

CREATE TABLE practice_items (
  item_id TEXT PRIMARY KEY,
  origin TEXT NOT NULL CHECK (origin IN ('chat','paper')),
  label TEXT NOT NULL,
  label_key TEXT NOT NULL,
  gap_log_path TEXT,
  paper TEXT,
  paper_question TEXT,
  gap_type TEXT CHECK (gap_type IN ('K','S','N','A','R')),
  status TEXT NOT NULL CHECK (status IN ('open','1 of 3','2 of 3','closed')),
  repaired_on TEXT,
  next_due_on TEXT,
  next_question TEXT,
  answer_key TEXT,
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
);

CREATE TABLE help_levels (
  kind_key TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  level TEXT NOT NULL CHECK (level IN ('full','partial','problem_only')),
  basis_attempt_id TEXT REFERENCES attempts(attempt_id),
  updated_at_ms INTEGER NOT NULL
);

CREATE TABLE anchors (
  anchor_id TEXT PRIMARY KEY,
  topic_key TEXT NOT NULL,
  skill TEXT NOT NULL,
  basis_attempt_id TEXT NOT NULL REFERENCES attempts(attempt_id),
  created_at_ms INTEGER NOT NULL
);

CREATE INDEX attempt_label ON attempts(label_key,answered_at_ms);
CREATE INDEX attempt_observation ON attempts(observation_id);
CREATE INDEX attempt_answered ON attempts(answered_at_ms,attempt_no);
CREATE INDEX practice_item_due ON practice_items(status,next_due_on);
CREATE INDEX practice_item_label ON practice_items(label_key);
CREATE INDEX anchor_topic ON anchors(topic_key);
