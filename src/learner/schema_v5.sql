DROP TABLE help_levels;

CREATE TABLE help_levels (
  kind_key TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  base_level TEXT CHECK (base_level IN ('full','partial','problem_only')),
  base_memory_id TEXT,
  level TEXT CHECK (level IN ('full','partial','problem_only')),
  basis_attempt_id TEXT REFERENCES attempts(attempt_id),
  updated_at_ms INTEGER NOT NULL
);

CREATE TABLE imported_readings (
  topic_key TEXT PRIMARY KEY,
  topic TEXT NOT NULL,
  reading TEXT NOT NULL CHECK (reading IN ('conditional','sound')),
  phrase TEXT NOT NULL,
  memory_id TEXT NOT NULL,
  imported_at_ms INTEGER NOT NULL
);

ALTER TABLE attempts ADD COLUMN kind_key TEXT;
ALTER TABLE attempts ADD COLUMN no_start INTEGER NOT NULL DEFAULT 0 CHECK (no_start IN (0,1));

CREATE INDEX attempt_kind ON attempts(kind_key, answered_at_ms, attempt_no) WHERE kind_key IS NOT NULL;
