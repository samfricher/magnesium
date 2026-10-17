-- Runtime schema for Magnesium learner-model/v2, fresh-store revision 2.
-- Initialize a NEW database only. Do not run against an old learner store.
PRAGMA foreign_keys = ON;

CREATE TABLE workspace_meta (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  workspace_id TEXT NOT NULL UNIQUE,
  storage_family TEXT NOT NULL CHECK (storage_family = 'learner-model/v2'),
  schema_version INTEGER NOT NULL CHECK (schema_version = 2),
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
