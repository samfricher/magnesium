//! Help levels: the teaching policy's ladder for a kind of problem (#31).
//!
//! The first time the learner has no starting move on a kind of problem they
//! get a full worked example, the next time a partly worked one, then the
//! problem alone. Help goes back one step when they do not solve a problem of
//! that kind alone. The tutor judges the kind and whether the learner could
//! start, and reports both on the attempt. The runtime keeps the count: each
//! change replays the kind's attempts, in order, from the step a memory stated
//! before schema 5, and stores the result in `help_levels`.
use super::*;
use rusqlite::{Connection, OptionalExtension, params};

/// One step of the ladder: the help given the last time the learner had no
/// starting move on the kind.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Level {
    Full,
    Partial,
    ProblemOnly,
}

impl Level {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Partial => "partial",
            Self::ProblemOnly => "problem_only",
        }
    }

    pub(crate) fn from_db(text: &str) -> Option<Self> {
        match text {
            "full" => Some(Self::Full),
            "partial" => Some(Self::Partial),
            "problem_only" => Some(Self::ProblemOnly),
            _ => None,
        }
    }

    /// As the teaching policy and the tool result write it.
    pub(crate) fn words(level: Option<Self>) -> &'static str {
        match level {
            None => "none yet",
            Some(Self::Full) => "full",
            Some(Self::Partial) => "partly worked",
            Some(Self::ProblemOnly) => "problem only",
        }
    }

    /// The learner is stuck on a new problem of the kind: give one step less
    /// help than last time.
    pub(crate) fn forward(level: Option<Self>) -> Self {
        match level {
            None => Self::Full,
            Some(Self::Full) => Self::Partial,
            Some(Self::Partial | Self::ProblemOnly) => Self::ProblemOnly,
        }
    }

    /// The learner did not solve a problem of the kind alone: one step back,
    /// so "partly worked" becomes "full" and "full" is removed.
    pub(crate) fn back(level: Option<Self>) -> Option<Self> {
        match level {
            None | Some(Self::Full) => None,
            Some(Self::Partial) => Some(Self::Full),
            Some(Self::ProblemOnly) => Some(Self::Partial),
        }
    }

    /// The help this step gives.
    pub(crate) fn give(self) -> &'static str {
        match self {
            Self::Full => {
                "a full worked example: work a similar problem with the same structure and new details, then give a changed version to solve alone"
            }
            Self::Partial => {
                "a partly worked example whose missing steps the learner fills in, then a full problem to solve alone"
            }
            Self::ProblemOnly => "the problem only, with no example",
        }
    }
}

/// One attempt of a kind, as the ladder sees it.
#[derive(Clone, Debug)]
pub(crate) struct Step {
    pub(crate) attempt_id: String,
    pub(crate) no_start: bool,
    pub(crate) mark: FirstTry,
    pub(crate) help: AttemptHelp,
}

/// What a step does to the ladder, if anything.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Move {
    Forward,
    Back,
}

pub(crate) fn move_of(step: &Step) -> Option<Move> {
    if step.no_start {
        return Some(Move::Forward);
    }
    match (step.mark, step.help) {
        // A decline is not a try, and a right answer alone holds the step.
        (FirstTry::Declined, _) | (FirstTry::Right, AttemptHelp::None) => None,
        _ => Some(Move::Back),
    }
}

/// The level after these steps, from `base`, and the attempt that last moved
/// it.
pub(crate) fn replay(base: Option<Level>, steps: &[Step]) -> (Option<Level>, Option<String>) {
    let mut level = base;
    let mut basis = None;
    for step in steps {
        let moved = match move_of(step) {
            None => continue,
            Some(Move::Forward) => Some(Level::forward(level)),
            Some(Move::Back) => Level::back(level),
        };
        if moved != level {
            basis = Some(step.attempt_id.clone());
        }
        level = moved;
    }
    (level, basis)
}

/// The kind as stored: its words with spacing made single.
pub(crate) fn kind_text(kind: &str) -> String {
    kind.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Work out one kind's level again from its base and every attempt of it,
/// and store it. Called in the update's transaction after an attempt of the
/// kind is stored or corrected.
pub(crate) fn refresh(tx: &Connection, kind_key: &str, now: i64) -> Result<()> {
    let row: Option<(String, Option<String>)> = tx
        .query_row(
            "SELECT kind, base_level FROM help_levels WHERE kind_key=?1",
            [kind_key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let mut statement = tx.prepare(
        "SELECT attempt_id, no_start, mark, help, problem_kind FROM attempts
         WHERE kind_key=?1 ORDER BY answered_at_ms, attempt_no",
    )?;
    let mut latest_kind = None;
    let steps = statement
        .query_map([kind_key], |r| {
            Ok((
                Step {
                    attempt_id: r.get(0)?,
                    no_start: r.get(1)?,
                    mark: practice::first_try(&r.get::<_, String>(2)?),
                    help: practice::help(&r.get::<_, String>(3)?),
                },
                r.get::<_, Option<String>>(4)?,
            ))
        })?
        .map(|row| {
            row.map(|(step, kind)| {
                if kind.is_some() {
                    latest_kind = kind;
                }
                step
            })
        })
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let base = row
        .as_ref()
        .and_then(|(_, base)| base.as_deref().and_then(Level::from_db));
    let (level, basis) = replay(base, &steps);
    let kind = latest_kind
        .or_else(|| row.map(|(kind, _)| kind))
        .unwrap_or_else(|| kind_key.to_owned());
    tx.execute(
        "INSERT INTO help_levels(kind_key, kind, level, basis_attempt_id, updated_at_ms)
         VALUES(?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(kind_key) DO UPDATE SET kind=?2, level=?3, basis_attempt_id=?4, updated_at_ms=?5",
        params![kind_key, kind, level.map(Level::as_db), basis, now],
    )?;
    Ok(())
}

/// A kind's level as stored.
#[derive(Clone, Debug)]
pub(crate) struct Stored {
    pub(crate) kind: String,
    pub(crate) level: Option<Level>,
}

pub(crate) fn stored(conn: &Connection, kind_key: &str) -> Result<Option<Stored>> {
    Ok(conn
        .query_row(
            "SELECT kind, level FROM help_levels WHERE kind_key=?1",
            [kind_key],
            |r| {
                Ok(Stored {
                    kind: r.get(0)?,
                    level: r
                        .get::<_, Option<String>>(1)?
                        .as_deref()
                        .and_then(Level::from_db),
                })
            },
        )
        .optional()?)
}

/// Each kind's level in words, by kind key, for the kinds that have one.
pub(crate) fn all_levels(
    conn: &Connection,
) -> Result<std::collections::BTreeMap<String, &'static str>> {
    let mut statement =
        conn.prepare("SELECT kind_key, level FROM help_levels WHERE level IS NOT NULL")?;
    Ok(statement
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .filter_map(|(key, level)| {
            Level::from_db(&level).map(|level| (key, Level::words(Some(level))))
        })
        .collect())
}

/// Every kind the store knows, in the words last used, for reuse.
pub(crate) fn known_kinds(conn: &Connection) -> Result<Vec<String>> {
    let mut statement = conn.prepare("SELECT kind FROM help_levels ORDER BY kind_key")?;
    Ok(statement
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Read once, when the store is upgraded to schema 5: each "example step
/// (KIND): …" and each topic reading the active memories state in words. A
/// kind stated in two memories keeps the step with less help, as the
/// state-and-evidence contract merged them.
pub(crate) fn import_phrases(conn: &Connection, now: i64) -> Result<()> {
    let mut statement = conn.prepare(
        "SELECT m.memory_id, m.state_text,
                (SELECT json_group_array(topic) FROM memory_topics t WHERE t.memory_id=m.memory_id)
         FROM learner_memories m WHERE m.lifecycle='active' ORDER BY m.memory_no",
    )?;
    let memories = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (memory_id, text, topics) in memories {
        for (kind, level) in example_steps(&text) {
            let key = label_key(&kind);
            let earlier: Option<String> = conn
                .query_row(
                    "SELECT base_level FROM help_levels WHERE kind_key=?1",
                    [&key],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            if earlier
                .as_deref()
                .and_then(Level::from_db)
                .is_some_and(|earlier| earlier >= level)
            {
                continue;
            }
            conn.execute(
                "INSERT INTO help_levels(kind_key, kind, base_level, base_memory_id, level, updated_at_ms)
                 VALUES(?1, ?2, ?3, ?4, ?3, ?5)
                 ON CONFLICT(kind_key) DO UPDATE SET base_level=?3, base_memory_id=?4, level=?3, updated_at_ms=?5",
                params![key, kind_text(&kind), level.as_db(), memory_id, now],
            )?;
        }
        let Some((reading, phrase)) = reading_phrase(&text) else {
            continue;
        };
        let topics: Vec<String> = serde_json::from_str(&topics).unwrap_or_default();
        for topic in topics {
            conn.execute(
                "INSERT OR IGNORE INTO imported_readings(topic_key, topic, reading, phrase, memory_id, imported_at_ms)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![label_key(&topic), topic, reading, phrase, memory_id, now],
            )?;
        }
    }
    Ok(())
}

/// Each "example step (KIND): full" or "example step (KIND): partly worked"
/// in a memory's text.
pub(crate) fn example_steps(text: &str) -> Vec<(String, Level)> {
    const MARK: &str = "example step (";
    let lower = text.to_lowercase();
    let mut found = Vec::new();
    let mut from = 0;
    // Lowercasing can change byte lengths, so search the lowered text and
    // read the kind from it too; a kind's case does not matter to its key.
    while let Some(at) = lower[from..].find(MARK) {
        let start = from + at + MARK.len();
        from = start;
        let mut depth = 1;
        let Some(end) = lower[start..].char_indices().find_map(|(i, c)| {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            (depth == 0).then_some(start + i)
        }) else {
            break;
        };
        let kind = lower[start..end].trim();
        let rest = lower[end + 1..].trim_start();
        let Some(rest) = rest.strip_prefix(':') else {
            continue;
        };
        let rest = rest.trim_start().trim_start_matches(['"', '\'', '*']);
        let level = if rest.starts_with("partly worked") || rest.starts_with("partial") {
            Level::Partial
        } else if rest.starts_with("problem only") || rest.starts_with("problem_only") {
            Level::ProblemOnly
        } else if rest.starts_with("full") {
            Level::Full
        } else {
            continue;
        };
        if !kind.is_empty() && kind.len() <= limits::ATTEMPT_LABEL_BYTES {
            found.push((kind.to_owned(), level));
        }
    }
    found
}

/// The first topic reading a memory states, such as "conditional: needs
/// TAG, which is open" or "sound from MockA Q3": a clause that starts with
/// the word and goes on as a reading does. Only sound and conditional count.
pub(crate) fn reading_phrase(text: &str) -> Option<(&'static str, String)> {
    for clause in text.split(['\n', ';', '.']) {
        let clause = clause.trim().trim_start_matches(['-', '*', ' ']);
        let lower = clause.to_lowercase();
        for reading in ["sound", "conditional", "open", "untested"] {
            let Some(rest) = lower.strip_prefix(reading) else {
                continue;
            };
            let reads = rest.is_empty()
                || [":", ",", " (", " —", " -", " from", " inferred", " now"]
                    .iter()
                    .any(|next| rest.starts_with(next));
            if !reads {
                continue;
            }
            return match reading {
                "sound" | "conditional" => {
                    Some((reading, clause.chars().take(200).collect::<String>()))
                }
                // The first stated reading is open or untested: nothing to keep.
                _ => None,
            };
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(no_start: bool, mark: FirstTry, help: AttemptHelp) -> Step {
        Step {
            attempt_id: format!("a-{no_start}-{}-{}", mark.as_str(), help.as_str()),
            no_start,
            mark,
            help,
        }
    }

    #[test]
    fn stuck_twice_gives_full_then_partly_worked_then_the_problem_only() {
        let stuck = step(true, FirstTry::Declined, AttemptHelp::None);
        let alone = step(false, FirstTry::Right, AttemptHelp::None);
        let (level, _) = replay(None, std::slice::from_ref(&stuck));
        assert_eq!(level, Some(Level::Full));
        let (level, _) = replay(None, &[stuck.clone(), alone.clone(), stuck.clone()]);
        assert_eq!(level, Some(Level::Partial));
        let (level, _) = replay(None, &[stuck.clone(), stuck.clone(), stuck.clone(), stuck]);
        assert_eq!(level, Some(Level::ProblemOnly));
    }

    #[test]
    fn help_goes_back_one_step_when_a_problem_is_not_solved_alone() {
        let stuck = step(true, FirstTry::Declined, AttemptHelp::None);
        let failed = step(false, FirstTry::Wrong, AttemptHelp::None);
        let hinted = step(false, FirstTry::Right, AttemptHelp::Hint);
        // Partly worked becomes full, and full is removed.
        let (level, basis) = replay(None, &[stuck.clone(), stuck.clone(), failed.clone()]);
        assert_eq!(level, Some(Level::Full));
        assert_eq!(basis.as_deref(), Some(failed.attempt_id.as_str()));
        let (level, _) = replay(None, &[stuck.clone(), hinted]);
        assert_eq!(level, None);
        // Nothing to step back from.
        assert_eq!(replay(None, std::slice::from_ref(&failed)).0, None);
        // A decline is not a try.
        let declined = step(false, FirstTry::Declined, AttemptHelp::Solution);
        assert_eq!(
            replay(Some(Level::Partial), &[declined]).0,
            Some(Level::Partial)
        );
        // From a step a memory stated before.
        assert_eq!(
            replay(Some(Level::Partial), &[stuck]).0,
            Some(Level::ProblemOnly)
        );
        assert_eq!(
            replay(Some(Level::ProblemOnly), &[failed]).0,
            Some(Level::Partial)
        );
    }

    #[test]
    fn example_steps_are_read_from_memory_text_whatever_their_case() {
        let text = "Applies E = sum x P(x) after help; example step (discrete expected value): full.\nExample Step (Bayes (two events)): partly worked; example step (x): maybe";
        assert_eq!(
            example_steps(text),
            vec![
                ("discrete expected value".to_owned(), Level::Full),
                ("bayes (two events)".to_owned(), Level::Partial),
            ]
        );
        assert!(example_steps("no step here").is_empty());
    }

    #[test]
    fn a_reading_is_read_only_where_a_clause_states_it() {
        assert_eq!(
            reading_phrase("conditional: needs chain-rule, which is open; example step (k): full"),
            Some((
                "conditional",
                "conditional: needs chain-rule, which is open".to_owned()
            ))
        );
        assert_eq!(
            reading_phrase("Week 2 work. Sound from MockA Q3 part b").map(|r| r.0),
            Some("sound")
        );
        // Words that only start like a reading are not one.
        assert_eq!(
            reading_phrase("Conditional probability with two dice"),
            None
        );
        assert_eq!(reading_phrase("Sound reasoning on the first case"), None);
        assert_eq!(reading_phrase("open: row midterm-1#2"), None);
        assert_eq!(reading_phrase("untested"), None);
    }
}
