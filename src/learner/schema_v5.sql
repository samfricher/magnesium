-- Tables and columns schema 5 adds. A fresh store runs this after
-- schema_v4.sql; the upgrade from schema 2, 3 or 4 runs it once, inside its
-- transaction, and then reads the old phrases out of the memories (#31).

-- Schema 3 made help_levels before anything wrote to it, so it is empty in
-- every store. Schema 5 makes it again in the shape #31 needs.
DROP TABLE help_levels;

-- The help level for each kind of problem: the last step of the teaching
-- policy's ladder that was given (a full worked example, a partly worked one,
-- or the problem alone). The runtime keeps it: it starts from base_level and
-- moves it by each attempt of the kind, in order, after every change. NULL
-- means no step yet, so the next time the learner has no starting move they
-- get a full worked example.
CREATE TABLE help_levels (
  kind_key TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  -- The step a memory recorded in words, "example step (KIND): …", read
  -- once when the store was upgraded to schema 5.
  base_level TEXT CHECK (base_level IN ('full','partial','problem_only')),
  base_memory_id TEXT,
  level TEXT CHECK (level IN ('full','partial','problem_only')),
  -- The attempt that last moved it, if one did.
  basis_attempt_id TEXT REFERENCES attempts(attempt_id),
  updated_at_ms INTEGER NOT NULL
);

-- A topic's reading as a memory stated it in words before schema 5, such as
-- "sound from MockA Q3", read once at the upgrade. Readings are worked out
-- from the gap log, the attempts and the course graph; these stand in for the
-- answers that were marked before attempts were stored. Only sound and
-- conditional are kept, since an open reading comes from the gap-log rows.
CREATE TABLE imported_readings (
  topic_key TEXT PRIMARY KEY,
  topic TEXT NOT NULL,
  reading TEXT NOT NULL CHECK (reading IN ('conditional','sound')),
  phrase TEXT NOT NULL,
  memory_id TEXT NOT NULL,
  imported_at_ms INTEGER NOT NULL
);

-- The kind of problem an attempt was on, as a key (attempts.problem_kind from
-- schema 3 holds it as written), and whether the learner had no starting move
-- on it.
ALTER TABLE attempts ADD COLUMN kind_key TEXT;
ALTER TABLE attempts ADD COLUMN no_start INTEGER NOT NULL DEFAULT 0 CHECK (no_start IN (0,1));

CREATE INDEX attempt_kind ON attempts(kind_key, answered_at_ms, attempt_no) WHERE kind_key IS NOT NULL;
