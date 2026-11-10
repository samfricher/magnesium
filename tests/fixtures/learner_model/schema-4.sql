-- The learner store at schema 4, exactly as the main branch made it before
-- schema 5: schema.sql, then schema_v3.sql, then schema_v4.sql.
-- Runtime schema for Magnesium learner-model/v2, fresh-store revision 4.
-- Initialize a NEW database only. Do not run against an old learner store.
-- The tables schemas 3 and 4 added are in schema_v3.sql and schema_v4.sql,
-- which a fresh store runs after this file and the upgrade runs on their own.
PRAGMA foreign_keys = ON;

CREATE TABLE workspace_meta (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  workspace_id TEXT NOT NULL UNIQUE,
  storage_family TEXT NOT NULL CHECK (storage_family = 'learner-model/v2'),
  schema_version INTEGER NOT NULL CHECK (schema_version = 4),
  memory_generation INTEGER NOT NULL DEFAULT 0,
  observation_generation INTEGER NOT NULL DEFAULT 0,
  event_generation INTEGER NOT NULL DEFAULT 0,
  review_generation INTEGER NOT NULL DEFAULT 0,
  memory_fts_generation INTEGER NOT NULL DEFAULT 0,
  observation_fts_generation INTEGER NOT NULL DEFAULT 0,
  created_at_ms INTEGER NOT NULL
);
CREATE TABLE learning_sessions (
  session_id TEXT PRIMARY KEY,
  goal TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('active','paused','completed')),
  checkpoint_json TEXT NOT NULL CHECK (json_valid(checkpoint_json)),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
);
CREATE TABLE host_bindings (
  host_kind TEXT NOT NULL,
  host_thread_id TEXT NOT NULL,
  session_id TEXT REFERENCES learning_sessions(session_id),
  learning_active INTEGER NOT NULL DEFAULT 0 CHECK (learning_active IN (0,1)),
  recording_enabled INTEGER NOT NULL DEFAULT 1 CHECK (recording_enabled IN (0,1)),
  PRIMARY KEY (host_kind,host_thread_id)
);
CREATE TABLE learning_events (
  event_no INTEGER PRIMARY KEY,
  event_id TEXT NOT NULL UNIQUE,
  session_id TEXT REFERENCES learning_sessions(session_id),
  host_kind TEXT NOT NULL,
  host_thread_id TEXT NOT NULL,
  external_event_key TEXT NOT NULL,
  speaker TEXT NOT NULL CHECK (speaker IN ('learner','tutor','tool','source')),
  kind TEXT NOT NULL,
  origin TEXT NOT NULL CHECK (origin IN ('host_observed','model_reported')),
  text TEXT NOT NULL,
  metadata_json TEXT NOT NULL CHECK (json_valid(metadata_json)),
  occurred_at_ms INTEGER,
  captured_at_ms INTEGER NOT NULL,
  processing TEXT NOT NULL CHECK (processing IN ('pending','processed','no_change','not_applicable')),
  processed_request_id TEXT,
  UNIQUE(host_kind,host_thread_id,external_event_key,kind)
);
CREATE TABLE observations (
  observation_no INTEGER PRIMARY KEY,
  observation_id TEXT NOT NULL UNIQUE,
  session_id TEXT REFERENCES learning_sessions(session_id),
  kind TEXT NOT NULL,
  description TEXT NOT NULL,
  conditions_text TEXT NOT NULL DEFAULT '',
  assistance_text TEXT NOT NULL DEFAULT '',
  source_refs_json TEXT NOT NULL CHECK (json_valid(source_refs_json)),
  observed_at_ms INTEGER,
  created_at_ms INTEGER NOT NULL
);
CREATE TABLE observation_events (
  observation_id TEXT NOT NULL REFERENCES observations(observation_id),
  event_id TEXT NOT NULL REFERENCES learning_events(event_id),
  role TEXT NOT NULL,
  excerpts_json TEXT NOT NULL CHECK (json_valid(excerpts_json)),
  PRIMARY KEY(observation_id,event_id,role)
);
CREATE TABLE learner_memories (
  memory_no INTEGER PRIMARY KEY,
  memory_id TEXT NOT NULL UNIQUE,
  title TEXT NOT NULL,
  scope TEXT NOT NULL,
  summary TEXT NOT NULL,
  state_text TEXT NOT NULL,
  retrieval_cues_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(retrieval_cues_json)),
  review_note TEXT,
  current_basis_json TEXT NOT NULL CHECK (json_valid(current_basis_json)),
  replacement_ids_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(replacement_ids_json)),
  lifecycle TEXT NOT NULL CHECK (lifecycle IN ('active','retired')),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  last_observed_at_ms INTEGER,
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
);
CREATE TABLE memory_topics (
  memory_id TEXT NOT NULL REFERENCES learner_memories(memory_id),
  topic TEXT NOT NULL,
  PRIMARY KEY(memory_id,topic)
);
CREATE TABLE memory_observations (
  memory_id TEXT NOT NULL REFERENCES learner_memories(memory_id),
  observation_id TEXT NOT NULL REFERENCES observations(observation_id),
  PRIMARY KEY(memory_id,observation_id)
);
CREATE TABLE memory_changes (
  memory_id TEXT NOT NULL REFERENCES learner_memories(memory_id),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  change_kind TEXT NOT NULL CHECK (change_kind IN ('new_observation','reinterpretation','editorial','flag','retire')),
  snapshot_json TEXT NOT NULL CHECK (json_valid(snapshot_json)),
  reason TEXT NOT NULL,
  change_basis_json TEXT NOT NULL CHECK (json_valid(change_basis_json)),
  created_at_ms INTEGER NOT NULL,
  PRIMARY KEY(memory_id,revision)
);
CREATE TABLE operation_receipts (
  request_id TEXT PRIMARY KEY,
  request_sha256 TEXT NOT NULL,
  result_json TEXT NOT NULL CHECK (json_valid(result_json)),
  created_at_ms INTEGER NOT NULL
);
CREATE TABLE review_items (
  review_id TEXT PRIMARY KEY,
  memory_id TEXT NOT NULL REFERENCES learner_memories(memory_id),
  memory_revision INTEGER NOT NULL,
  purpose TEXT NOT NULL,
  reason TEXT NOT NULL,
  due_at_ms INTEGER,
  status TEXT NOT NULL CHECK (status IN ('suggested','done','dismissed')),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  updated_at_ms INTEGER NOT NULL
);
CREATE TABLE context_receipts (
  trace_id TEXT PRIMARY KEY,
  host_kind TEXT NOT NULL,
  host_thread_id TEXT NOT NULL,
  session_id TEXT REFERENCES learning_sessions(session_id),
  tool_action TEXT NOT NULL,
  request_json TEXT NOT NULL CHECK (json_valid(request_json)),
  response_json TEXT NOT NULL CHECK (json_valid(response_json)),
  created_at_ms INTEGER NOT NULL
);

-- Contentful derived indexes. The central writer keeps one row per canonical
-- memory/observation using memory_no/observation_no as the corresponding rowid.
CREATE VIRTUAL TABLE memory_fts USING fts5(
  memory_id UNINDEXED,title,scope,topics,summary,state_text,review_note,retrieval_cues,
  tokenize='unicode61'
);
CREATE VIRTUAL TABLE observation_fts USING fts5(
  observation_id UNINDEXED,description,conditions_text,assistance_text,
  tokenize='unicode61'
);
CREATE INDEX memory_active_page ON learner_memories(lifecycle,memory_no);
CREATE INDEX topic_directory ON memory_topics(topic,memory_id);
CREATE INDEX observation_session ON observations(session_id,observation_no);
CREATE INDEX observation_event_lookup ON observation_events(event_id);
CREATE INDEX memory_observation_lookup ON memory_observations(observation_id);
CREATE INDEX event_thread ON learning_events(host_kind,host_thread_id,event_no);
CREATE INDEX event_pending ON learning_events(processing,event_no);
CREATE INDEX review_due ON review_items(status,due_at_ms);
CREATE INDEX context_thread ON context_receipts(host_kind,host_thread_id,created_at_ms);
-- Tables added by schema 3. A fresh store runs this after schema.sql; the
-- upgrade from schema 2 runs it once, inside its transaction.

-- One row per answer the tutor marked, in chat or on a paper (#29). The
-- observation is the evidence and the learner event holds their own words or
-- photo; this row holds the facts code counts. Help given before the answer is
-- stored as given, and a helped answer is never cold.
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
  -- Filled by later work: the practice item this answer redoes (#30) and the
  -- kind of problem whose help level it moves (#31).
  item_id TEXT REFERENCES practice_items(item_id),
  problem_kind TEXT,
  created_at_ms INTEGER NOT NULL,
  CHECK (cold = 0 OR help = 'none'),
  CHECK ((paper IS NULL) = (paper_question IS NULL)),
  CHECK ((marks IS NULL) = (out_of IS NULL)),
  CHECK (marks IS NULL OR (out_of > 0 AND marks >= 0 AND marks <= out_of))
);

-- Something the learner got wrong, to be reproduced cold on later days: a
-- quick check from chat or a gap row from a paper (#30). The runtime works out
-- its passes, status and next date from its attempts. Empty until #30.
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

-- The help level for each kind of problem, moved by the teaching policy's
-- ladder from the attempts (#31). Empty until #31.
CREATE TABLE help_levels (
  kind_key TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  level TEXT NOT NULL CHECK (level IN ('full','partial','problem_only')),
  basis_attempt_id TEXT REFERENCES attempts(attempt_id),
  updated_at_ms INTEGER NOT NULL
);

-- A skill the learner's own work has shown, on a topic (#31, if anchors move
-- out of memory text there). Empty until then.
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
