//! Practice items: what the learner got wrong, to be reproduced cold on later
//! days (#30). A quick check from chat and a gap-log row from a paper are the
//! same kind of thing.
//!
//! The tutor reports marks, repairs and next questions. The runtime works out
//! every pass, status and next date from the item's events and the attempts
//! they name, with [`derive`]. Nothing here stores a derived date, so a
//! correction to an attempt, or a new test date in the plan, changes what is
//! due at once.
use super::*;
use crate::learner::update::field_error;
use chrono::{Days, Local, NaiveDate, TimeZone};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Days from a pass to the next redo: after the first pass, after the second.
const SPACING_DAYS: [u64; 2] = [3, 7];
pub(crate) const PASSES_TO_CLOSE: i64 = 3;
const MAX_MISSES_KEPT: usize = 3;
/// Longer text is cut, not refused.
pub(crate) const TEXT_BYTES: usize = 1000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Origin {
    Chat,
    Paper,
}

/// One attempt as an item's events see it.
#[derive(Clone, Debug)]
pub(crate) struct AttemptFacts {
    pub(crate) attempt_id: String,
    pub(crate) mark: FirstTry,
    pub(crate) help: AttemptHelp,
    pub(crate) cold: bool,
    pub(crate) slow: bool,
    pub(crate) confidence: Option<Confidence>,
    pub(crate) paper: Option<String>,
    pub(crate) paper_question: Option<String>,
    /// The observation's description: for a miss, what went wrong.
    pub(crate) description: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EventKind {
    Made,
    Repair,
    Attempt,
}

#[derive(Clone, Debug)]
pub(crate) struct ItemEvent {
    pub(crate) event_no: i64,
    pub(crate) kind: EventKind,
    pub(crate) day: NaiveDate,
    pub(crate) attempt: Option<AttemptFacts>,
}

/// An item as stored, with its events in order.
#[derive(Clone, Debug)]
pub(crate) struct ItemFacts {
    pub(crate) item_id: String,
    pub(crate) origin: Origin,
    pub(crate) label: String,
    pub(crate) plan: Option<String>,
    pub(crate) row_no: Option<i64>,
    pub(crate) paper: Option<String>,
    pub(crate) paper_question: Option<String>,
    pub(crate) logged_on: NaiveDate,
    pub(crate) gap_type: Option<GapType>,
    pub(crate) what_went_wrong: String,
    pub(crate) repair: String,
    pub(crate) confidence: Option<Confidence>,
    pub(crate) next_question: Option<String>,
    pub(crate) answer_key: Option<String>,
    pub(crate) dropped: Option<String>,
    pub(crate) base_passes: i64,
    pub(crate) base_next_on: Option<NaiveDate>,
    pub(crate) base_last_pass_on: Option<NaiveDate>,
    pub(crate) base_history: String,
    pub(crate) base_misses: Vec<String>,
    pub(crate) created_at_ms: i64,
    pub(crate) events: Vec<ItemEvent>,
}

impl ItemFacts {
    /// How the tutor names it: "midterm-1#3" for a gap-log row, the label
    /// for a quick check.
    pub(crate) fn name(&self) -> String {
        match (&self.plan, self.row_no) {
            (Some(plan), Some(row)) => format!("{plan}#{row}"),
            _ => self.label.clone(),
        }
    }
}

/// What one event did to its item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Effect {
    Made,
    /// A repair dated the first redo.
    Scheduled,
    Passed,
    Closed,
    /// A due redo that was not a pass.
    Failed,
    /// A chat miss before the check was due starts it over.
    Restarted,
    /// Right with help on a due item: no pass, and for a quick check no fail.
    Helped,
    Declined,
    /// Not due yet, or not cold, or a second pass the same day: counts
    /// neither way.
    NoCount,
    /// A repair while redos are under way, or anything on a closed row.
    Ignored,
}

impl Effect {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Made => "made",
            Self::Scheduled => "scheduled",
            Self::Passed => "passed",
            Self::Closed => "closed",
            Self::Failed => "failed",
            Self::Restarted => "restarted",
            Self::Helped => "helped",
            Self::Declined => "declined",
            Self::NoCount => "no_count",
            Self::Ignored => "ignored",
        }
    }
}

/// An item as the runtime works it out.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ItemState {
    pub(crate) passes: i64,
    pub(crate) next_on: Option<NaiveDate>,
    /// The Passes cell: what was brought in, then each result since.
    pub(crate) history: Vec<String>,
    pub(crate) confidence: Option<Confidence>,
    pub(crate) misses: Vec<String>,
    /// Each event's effect, by event_no.
    pub(crate) effects: Vec<(i64, Effect)>,
    /// The passes the item held after each event, by event_no.
    pub(crate) passes_after: Vec<(i64, i64)>,
}

impl ItemState {
    pub(crate) fn closed(&self) -> bool {
        self.passes >= PASSES_TO_CLOSE
    }

    /// Exactly one of open, 1 of 3, 2 of 3 or closed.
    pub(crate) fn status(&self) -> &'static str {
        match self.passes {
            0 => "open",
            1 => "1 of 3",
            2 => "2 of 3",
            _ => "closed",
        }
    }

    pub(crate) fn due_on(&self, day: NaiveDate) -> bool {
        !self.closed() && self.next_on.is_some_and(|next| next <= day)
    }

    pub(crate) fn effect_of(&self, event_no: i64) -> Option<Effect> {
        self.effects
            .iter()
            .find(|(no, _)| *no == event_no)
            .map(|(_, effect)| *effect)
    }
}

pub(crate) fn local_day(ms: i64) -> NaiveDate {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map_or(NaiveDate::MIN, |time| time.date_naive())
}

fn days_after(day: NaiveDate, days: u64) -> NaiveDate {
    day.checked_add_days(Days::new(days)).unwrap_or(day)
}

/// The next redo after a pass on `day` that leaves `passes` passes. Three
/// cold passes on three different days, at least 3 and then 7 days apart.
/// When the test comes first, the gap shortens so each redo left gets its
/// own day before it; never below one day, so a row that runs out of days
/// stays short of closed. A test date already past shortens nothing.
fn after_pass(day: NaiveDate, passes: i64, test_on: Option<NaiveDate>) -> NaiveDate {
    let gap = SPACING_DAYS[usize::try_from(passes - 1).unwrap_or(0).min(1)];
    let natural = days_after(day, gap);
    let tomorrow = days_after(day, 1);
    let left = u64::try_from(PASSES_TO_CLOSE - passes).unwrap_or(0);
    match test_on
        .filter(|test| *test > day)
        .and_then(|test| test.checked_sub_days(Days::new(left)))
    {
        Some(latest) if latest < natural => latest.max(tomorrow),
        _ => natural,
    }
}

/// Work out an item's passes, status and next date from what is stored.
/// `test_on` is the plan's test date, for a paper row in a plan that states
/// one.
pub(crate) fn derive(item: &ItemFacts, test_on: Option<NaiveDate>) -> ItemState {
    let mut state = ItemState {
        passes: item.base_passes.clamp(0, PASSES_TO_CLOSE),
        next_on: item.base_next_on,
        history: (!item.base_history.trim().is_empty())
            .then(|| item.base_history.trim().to_owned())
            .into_iter()
            .collect(),
        confidence: item.confidence,
        misses: item.base_misses.clone(),
        effects: Vec::new(),
        passes_after: Vec::new(),
    };
    if state.closed() {
        state.next_on = None;
    }
    let mut last_pass = item.base_last_pass_on;
    let paper = item.origin == Origin::Paper;
    for event in &item.events {
        let day = event.day;
        let restart = |state: &mut ItemState, next: Option<NaiveDate>| {
            state.passes = 0;
            state.next_on = next;
            state.history.clear();
        };
        let effect = match (event.kind, &event.attempt) {
            (EventKind::Made, attempt) => {
                if let Some(attempt) = attempt
                    && matches!(attempt.mark, FirstTry::Wrong | FirstTry::Partly)
                    && !paper
                {
                    note_miss(&mut state.misses, &attempt.description);
                }
                // A quick check's miss was put right in the chat, so it is
                // asked for cold the next day. A paper row waits for its
                // repair.
                restart(&mut state, (!paper).then(|| days_after(day, 1)));
                Effect::Made
            }
            (EventKind::Repair, _) => {
                if state.closed() || state.passes > 0 {
                    Effect::Ignored
                } else {
                    state.next_on = Some(days_after(day, 1));
                    Effect::Scheduled
                }
            }
            (EventKind::Attempt, None) => Effect::Ignored,
            (EventKind::Attempt, Some(attempt)) => {
                let due = state.due_on(day);
                let passes = attempt.mark == FirstTry::Right
                    && attempt.help == AttemptHelp::None
                    && attempt.cold
                    && !(attempt.slow && item.gap_type == Some(GapType::S));
                if paper {
                    if state.closed() {
                        Effect::Ignored
                    } else if !due {
                        Effect::NoCount
                    } else if attempt.mark == FirstTry::Declined {
                        Effect::Declined
                    } else if passes {
                        pass(&mut state, &mut last_pass, item, attempt, day, test_on)
                    } else if attempt.mark == FirstTry::Right
                        && attempt.help == AttemptHelp::None
                        && !attempt.cold
                    {
                        Effect::NoCount
                    } else {
                        // A due redo that is not a pass fails the row. It
                        // waits for a new repair, and its Passes start again.
                        restart(&mut state, None);
                        state.history.push(format!("{day} fail"));
                        if attempt.confidence.is_some() {
                            state.confidence = attempt.confidence;
                        }
                        Effect::Failed
                    }
                } else {
                    match (attempt.mark, attempt.help) {
                        (FirstTry::Wrong | FirstTry::Partly, _) => {
                            note_miss(&mut state.misses, &attempt.description);
                            restart(&mut state, Some(days_after(day, 1)));
                            if due {
                                Effect::Failed
                            } else {
                                Effect::Restarted
                            }
                        }
                        // The solution's answer, not the learner's: asked
                        // for cold the next day, with no miss stored.
                        (FirstTry::Right, AttemptHelp::Solution) => {
                            restart(&mut state, Some(days_after(day, 1)));
                            Effect::Helped
                        }
                        // Right only with a hint or a step, before the check
                        // is due: it is asked for cold the next day, and the
                        // passes already earned stay. The help field is the
                        // least sure one a tutor sends, so the smallest help
                        // costs no pass. A proven skill stays proven.
                        (FirstTry::Right, AttemptHelp::Hint | AttemptHelp::Step) if !due => {
                            if !state.closed() {
                                state.next_on = Some(days_after(day, 1));
                            }
                            Effect::Helped
                        }
                        (FirstTry::Declined, AttemptHelp::Solution) => {
                            restart(&mut state, Some(days_after(day, 1)));
                            Effect::Declined
                        }
                        // "I don't know" on a task the tutor set brings the
                        // check to the next day, as a miss does, with no miss
                        // kept. A declined check that is due stays free.
                        (FirstTry::Declined, _) if !due && !state.closed() => {
                            restart(&mut state, Some(days_after(day, 1)));
                            Effect::Declined
                        }
                        (FirstTry::Declined, _) => Effect::Declined,
                        _ if !due => Effect::NoCount,
                        _ if passes => pass(&mut state, &mut last_pass, item, attempt, day, None),
                        (FirstTry::Right, AttemptHelp::None) => Effect::NoCount,
                        (FirstTry::Right, _) => Effect::Helped,
                    }
                }
            }
        };
        state.effects.push((event.event_no, effect));
        state.passes_after.push((event.event_no, state.passes));
    }
    state
}

fn pass(
    state: &mut ItemState,
    last_pass: &mut Option<NaiveDate>,
    item: &ItemFacts,
    attempt: &AttemptFacts,
    day: NaiveDate,
    test_on: Option<NaiveDate>,
) -> Effect {
    // Three passes on three different days.
    if *last_pass == Some(day) {
        return Effect::NoCount;
    }
    *last_pass = Some(day);
    state.passes += 1;
    let mut cell = format!("{day} pass");
    if let (Some(paper), Some(question)) = (&attempt.paper, &attempt.paper_question)
        && (Some(paper), Some(question)) != (item.paper.as_ref(), item.paper_question.as_ref())
    {
        cell.push_str(&format!(" ({paper} Q{question})"));
    }
    state.history.push(cell);
    if state.closed() {
        state.next_on = None;
        Effect::Closed
    } else {
        state.next_on = Some(after_pass(day, state.passes, test_on));
        Effect::Passed
    }
}

fn note_miss(misses: &mut Vec<String>, description: &str) {
    let miss = description.split_whitespace().collect::<Vec<_>>().join(" ");
    if miss.is_empty() {
        return;
    }
    misses.retain(|kept| kept != &miss);
    misses.push(miss);
    let excess = misses.len().saturating_sub(MAX_MISSES_KEPT);
    misses.drain(..excess);
}

fn day_column(text: Option<String>) -> Option<NaiveDate> {
    text.and_then(|text| NaiveDate::parse_from_str(&text, "%Y-%m-%d").ok())
}

fn gap_type(text: Option<String>) -> Option<GapType> {
    match text.as_deref() {
        Some("K") => Some(GapType::K),
        Some("S") => Some(GapType::S),
        Some("N") => Some(GapType::N),
        Some("A") => Some(GapType::A),
        Some("R") => Some(GapType::R),
        _ => None,
    }
}

pub(crate) fn confidence(text: Option<&str>) -> Option<Confidence> {
    match text {
        Some("sure") => Some(Confidence::Sure),
        Some("unsure") => Some(Confidence::Unsure),
        Some("guess") => Some(Confidence::Guess),
        _ => None,
    }
}

pub(crate) fn first_try(text: &str) -> FirstTry {
    match text {
        "right" => FirstTry::Right,
        "partly" => FirstTry::Partly,
        "declined" => FirstTry::Declined,
        _ => FirstTry::Wrong,
    }
}

pub(crate) fn help(text: &str) -> AttemptHelp {
    match text {
        "hint" => AttemptHelp::Hint,
        "step" => AttemptHelp::Step,
        "solution" => AttemptHelp::Solution,
        _ => AttemptHelp::None,
    }
}

const ITEM_COLUMNS: &str =
    "item_id,origin,label,plan,row_no,paper,paper_question,logged_on,gap_type,
    what_went_wrong,repair,confidence,next_question,answer_key,dropped,base_passes,base_next_on,
    base_last_pass_on,base_history,base_misses_json,created_at_ms";

/// Every item, with its events, in the order the gap log numbers them:
/// paper rows by plan and #, then quick checks oldest first. Dropped items
/// are included; callers leave them out.
pub(crate) fn load_items(conn: &Connection) -> Result<Vec<ItemFacts>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {ITEM_COLUMNS} FROM practice_items
         ORDER BY origin DESC, plan, row_no, created_at_ms, item_id"
    ))?;
    let mut items = statement
        .query_map([], |r| {
            let misses: String = r.get(19)?;
            Ok(ItemFacts {
                item_id: r.get(0)?,
                origin: if r.get::<_, String>(1)? == "paper" {
                    Origin::Paper
                } else {
                    Origin::Chat
                },
                label: r.get(2)?,
                plan: r.get(3)?,
                row_no: r.get(4)?,
                paper: r.get(5)?,
                paper_question: r.get(6)?,
                logged_on: day_column(r.get(7)?).unwrap_or(NaiveDate::MIN),
                gap_type: gap_type(r.get(8)?),
                what_went_wrong: r.get(9)?,
                repair: r.get(10)?,
                confidence: confidence(r.get::<_, Option<String>>(11)?.as_deref()),
                next_question: r.get(12)?,
                answer_key: r.get(13)?,
                dropped: r.get(14)?,
                base_passes: r.get(15)?,
                base_next_on: day_column(r.get(16)?),
                base_last_pass_on: day_column(r.get(17)?),
                base_history: r.get(18)?,
                base_misses: serde_json::from_str(&misses).unwrap_or_default(),
                created_at_ms: r.get(20)?,
                events: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut events = conn.prepare(
        "SELECT e.item_id, e.event_no, e.kind, e.at_ms, a.attempt_id, a.mark, a.help, a.cold,
                a.slow, a.confidence, a.paper, a.paper_question, o.description
         FROM practice_events e
         LEFT JOIN attempts a ON a.attempt_id = e.attempt_id
         LEFT JOIN observations o ON o.observation_id = a.observation_id
         ORDER BY e.item_id, e.at_ms, e.event_no",
    )?;
    let rows = events
        .query_map([], |r| {
            let attempt = match r.get::<_, Option<String>>(4)? {
                None => None,
                Some(attempt_id) => Some(AttemptFacts {
                    attempt_id,
                    mark: first_try(&r.get::<_, String>(5)?),
                    help: help(&r.get::<_, String>(6)?),
                    cold: r.get(7)?,
                    slow: r.get(8)?,
                    confidence: confidence(r.get::<_, Option<String>>(9)?.as_deref()),
                    paper: r.get(10)?,
                    paper_question: r.get(11)?,
                    description: r.get::<_, Option<String>>(12)?.unwrap_or_default(),
                }),
            };
            let kind = match r.get::<_, String>(2)?.as_str() {
                "made" => EventKind::Made,
                "repair" => EventKind::Repair,
                _ => EventKind::Attempt,
            };
            Ok((
                r.get::<_, String>(0)?,
                ItemEvent {
                    event_no: r.get(1)?,
                    kind,
                    day: local_day(r.get(3)?),
                    attempt,
                },
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let index: std::collections::HashMap<String, usize> = items
        .iter()
        .enumerate()
        .map(|(i, item)| (item.item_id.clone(), i))
        .collect();
    for (item_id, event) in rows {
        if let Some(&i) = index.get(&item_id) {
            items[i].events.push(event);
        }
    }
    Ok(items)
}

/// The study folder a store belongs to: the store is
/// `<study folder>/.magnesium/state.sqlite`.
pub(crate) fn study_root(conn: &Connection) -> Option<PathBuf> {
    let path = Path::new(conn.path()?);
    Some(path.parent()?.parent()?.to_path_buf())
}

/// The plans in a study folder: each folder under `Study/` with a `plan.md`.
pub(crate) fn plans(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join("Study")) else {
        return Vec::new();
    };
    let mut plans = entries
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .filter(|name| {
            std::fs::symlink_metadata(root.join("Study").join(name).join("plan.md"))
                .is_ok_and(|m| m.is_file())
        })
        .collect::<Vec<_>>();
    plans.sort();
    plans
}

/// The item a name refers to: an item ID, "<plan>#<n>" for a gap-log row,
/// or a quick check's label. Dropped items are not found.
pub(crate) fn resolve(conn: &Connection, name: &str) -> Result<Option<String>> {
    let name = name.trim();
    let found = |sql: &str, params: &[&dyn rusqlite::ToSql]| -> Result<Option<String>> {
        Ok(conn
            .query_row(sql, params, |r| r.get::<_, String>(0))
            .optional()?)
    };
    if let Some(id) = found(
        "SELECT item_id FROM practice_items WHERE item_id=?1 AND dropped IS NULL",
        &[&name],
    )? {
        return Ok(Some(id));
    }
    if let Some((plan, row)) = name.rsplit_once('#')
        && let Ok(row) = row.trim().parse::<i64>()
        && let Some(id) = found(
            "SELECT item_id FROM practice_items WHERE plan=?1 AND row_no=?2 AND dropped IS NULL",
            &[&plan.trim(), &row],
        )?
    {
        return Ok(Some(id));
    }
    found(
        "SELECT item_id FROM practice_items WHERE origin='chat' AND label_key=?1 AND dropped IS NULL",
        &[&label_key(name)],
    )
}

fn resolve_all(
    conn: &Connection,
    path: &str,
    names: &[String],
    host: &HostContext,
) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    for (index, name) in names.iter().enumerate() {
        let id = resolve(conn, name)?.ok_or_else(|| {
            field_error(
                &format!("{path}[{index}]"),
                format!(
                    "No gap-log row or quick check is named \"{}\". Name a row as its gap log does, such as \"midterm-1#3\", or a quick check by its label exactly",
                    name.chars().take(120).collect::<String>()
                ),
                host,
            )
        })?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn insert_event(
    tx: &Connection,
    item: &str,
    kind: &str,
    attempt: Option<&str>,
    (at, note, now): (i64, &str, i64),
) -> Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO practice_events(item_id,kind,attempt_id,at_ms,note,created_at_ms)
         VALUES(?1,?2,?3,?4,?5,?6)",
        params![item, kind, attempt, at, note, now],
    )?;
    Ok(())
}

fn set_next(
    tx: &Connection,
    item: &str,
    next: &NextQuestion,
    path: &str,
    notes: &mut Vec<String>,
) -> Result<()> {
    let question = limits::shorten(
        &format!("{path}.question"),
        next.question.trim(),
        TEXT_BYTES,
        notes,
    );
    let key = limits::shorten(
        &format!("{path}.answer_key"),
        next.answer_key.trim(),
        TEXT_BYTES,
        notes,
    );
    tx.execute(
        "UPDATE practice_items SET next_question=?2, answer_key=?3, updated_at_ms=?4 WHERE item_id=?1",
        params![item, question, key, store::now_ms()],
    )?;
    Ok(())
}

/// A stored attempt and where it came from, for [`record_attempt`].
pub(crate) struct NewAttempt<'a> {
    pub(crate) attempt_id: &'a str,
    pub(crate) attempt: &'a Attempt,
    pub(crate) path: &'a str,
    pub(crate) answered_at: i64,
    pub(crate) now: i64,
}

/// What a newly stored attempt does to the items: it makes a gap-log row
/// from `gap`, counts for the items `counts_for` names, or, in chat, makes or
/// answers the quick check on its label.
pub(crate) fn record_attempt(
    tx: &Connection,
    new: &NewAttempt<'_>,
    host: &HostContext,
    notes: &mut Vec<String>,
) -> Result<()> {
    let NewAttempt {
        attempt_id,
        attempt,
        path,
        answered_at,
        now,
    } = *new;
    let label = attempt
        .label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    // The same name the attempt was stored under.
    let label = crate::learner::update::observations::plan_tag_named(tx, &label).unwrap_or(label);
    let mut touched = Vec::new();
    for item in resolve_all(tx, &format!("{path}.counts_for"), &attempt.counts_for, host)? {
        insert_event(
            tx,
            &item,
            "attempt",
            Some(attempt_id),
            (answered_at, "", now),
        )?;
        touched.push(item);
    }
    if let Some(gap) = &attempt.gap {
        let paper = attempt
            .paper
            .as_ref()
            .expect("a gap was checked for a paper");
        let plan = paper_plan(tx, gap, &format!("{path}.gap.plan"), host)?;
        let row_no: Option<i64> = match &plan {
            None => None,
            Some(plan) => Some(tx.query_row(
                "SELECT COALESCE(MAX(row_no),0)+1 FROM practice_items WHERE plan=?1",
                [plan],
                |r| r.get(0),
            )?),
        };
        let id = store::new_id("g")?;
        let what = limits::shorten(
            &format!("{path}.gap.what_went_wrong"),
            gap.what_went_wrong.trim(),
            TEXT_BYTES,
            notes,
        );
        tx.execute(
            "INSERT INTO practice_items(item_id,origin,label,label_key,plan,row_no,paper,paper_question,
                logged_on,gap_type,what_went_wrong,confidence,created_at_ms,updated_at_ms)
             VALUES(?1,'paper',?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?12)",
            params![
                id,
                label,
                label_key(&label),
                plan,
                row_no,
                paper.name.trim(),
                paper.question.trim(),
                local_day(answered_at).to_string(),
                gap.gap_type,
                what,
                attempt.confidence,
                now
            ],
        )?;
        insert_event(tx, &id, "made", Some(attempt_id), (answered_at, "", now))?;
        touched.insert(0, id);
    } else if attempt.paper.is_none() && attempt.counts_for.is_empty() {
        // A chat answer makes or answers the quick check on its label.
        let existing: Option<String> = tx
            .query_row(
                "SELECT item_id FROM practice_items WHERE origin='chat' AND label_key=?1",
                [label_key(&label)],
                |r| r.get(0),
            )
            .optional()?;
        // A right answer reached with a hint or a step, and an "I don't
        // know", make a check for the next day, as a wrong first try does:
        // help that is never taken away would never show what the learner
        // can do alone, and a task never answered would never come back.
        let makes = matches!(
            attempt.first_try,
            FirstTry::Wrong | FirstTry::Partly | FirstTry::Declined
        ) || attempt.help == AttemptHelp::Solution
            && attempt.first_try == FirstTry::Right
            || attempt.first_try == FirstTry::Right
                && matches!(attempt.help, AttemptHelp::Hint | AttemptHelp::Step);
        // The first answer on a skill, right and alone, makes a check for
        // the next day too, so the skill is asked again and can earn its
        // passes. Only when the skill has no item, open or closed, and no
        // earlier answer, whose passes would be lost to the new check.
        let first_right = existing.is_none()
            && attempt.first_try == FirstTry::Right
            && attempt.help == AttemptHelp::None
            && !tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM practice_items WHERE label_key=?1 AND dropped IS NULL)
                     OR EXISTS(SELECT 1 FROM attempts WHERE label_key=?1 AND attempt_id<>?2)",
                params![label_key(&label), attempt_id],
                |r| r.get::<_, bool>(0),
            )?;
        let makes = makes || first_right;
        match existing {
            Some(item) => {
                // Taken out by the tutor: a new miss puts it back.
                if makes {
                    tx.execute(
                        "UPDATE practice_items SET dropped=NULL, label=?2, updated_at_ms=?3 WHERE item_id=?1",
                        params![item, label, now],
                    )?;
                }
                insert_event(
                    tx,
                    &item,
                    "attempt",
                    Some(attempt_id),
                    (answered_at, "", now),
                )?;
                touched.push(item);
            }
            None if makes => {
                let id = store::new_id("q")?;
                tx.execute(
                    "INSERT INTO practice_items(item_id,origin,label,label_key,logged_on,created_at_ms,updated_at_ms)
                     VALUES(?1,'chat',?2,?3,?4,?5,?5)",
                    params![id, label, label_key(&label), local_day(answered_at).to_string(), now],
                )?;
                insert_event(tx, &id, "made", Some(attempt_id), (answered_at, "", now))?;
                touched.push(id);
            }
            None => {}
        }
    }
    if let Some(next) = &attempt.next {
        let [item] = touched.as_slice() else {
            return Err(field_error(
                &format!("{path}.next"),
                if touched.is_empty() {
                    "next is the next question on the item this answer makes or redoes, and this answer touches none. Send it in practice, naming the item"
                } else {
                    "This answer touches several items, so next is ambiguous. Send each item's next question in practice, naming the item"
                },
                host,
            ));
        };
        set_next(tx, item, next, &format!("{path}.next"), notes)?;
    }
    Ok(())
}

/// The plan a new gap-log row goes in: the one named, or the study folder's
/// only plan. A study folder with no plan keeps the row in no gap log.
fn paper_plan(
    tx: &Connection,
    gap: &GapReport,
    path: &str,
    host: &HostContext,
) -> Result<Option<String>> {
    let plans = study_root(tx).map(|root| plans(&root)).unwrap_or_default();
    match gap.plan.as_deref().map(str::trim) {
        Some(plan) if plans.iter().any(|p| p == plan) => Ok(Some(plan.to_owned())),
        Some(plan) => Err(field_error(
            path,
            format!(
                "No plan is named \"{}\". The plans are the folders under Study/ with a plan.md: {}",
                plan.chars().take(120).collect::<String>(),
                if plans.is_empty() {
                    "there are none".to_owned()
                } else {
                    plans.join(", ")
                }
            ),
            host,
        )),
        None => match plans.as_slice() {
            [] => Ok(None),
            [only] => Ok(Some(only.clone())),
            several => Err(field_error(
                path,
                format!(
                    "This study folder has several plans, so say which gap log the row goes in: one of {}",
                    several.join(", ")
                ),
                host,
            )),
        },
    }
}

/// Repairs, next questions and corrections the tutor reports on items.
pub(crate) fn apply_edits(
    tx: &Connection,
    edits: &[PracticeEdit],
    now: i64,
    host: &HostContext,
    notes: &mut Vec<String>,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    for (index, edit) in edits.iter().enumerate() {
        let path = format!("practice[{index}]");
        let item = resolve(tx, &edit.item)?.ok_or_else(|| {
            field_error(
                &format!("{path}.item"),
                format!(
                    "No gap-log row or quick check is named \"{}\". Name a row as its gap log does, such as \"midterm-1#3\", or a quick check by its label exactly",
                    edit.item.chars().take(120).collect::<String>()
                ),
                host,
            )
        })?;
        if !seen.insert(item.clone()) {
            return Err(field_error(
                &format!("{path}.item"),
                "Each item at most once per request: put every change to it in one entry",
                host,
            ));
        }
        let origin: String = tx.query_row(
            "SELECT origin FROM practice_items WHERE item_id=?1",
            [&item],
            |r| r.get(0),
        )?;
        if edit.repaired.is_none()
            && edit.next.is_none()
            && edit.gap_type.is_none()
            && edit.what_went_wrong.is_none()
            && edit.dropped.is_none()
        {
            return Err(field_error(
                &path,
                "Give at least one of repaired, next, type, what_went_wrong or dropped",
                host,
            ));
        }
        if origin == "chat" && (edit.repaired.is_some() || edit.gap_type.is_some()) {
            return Err(field_error(
                &path,
                "A quick check has no repair or type: its miss was put right in the chat, and it is asked for cold the next day",
                host,
            ));
        }
        if let Some(repaired) = &edit.repaired {
            let what = limits::shorten(
                &format!("{path}.repaired"),
                repaired.trim(),
                TEXT_BYTES,
                notes,
            );
            if what.is_empty() {
                return Err(field_error(
                    &format!("{path}.repaired"),
                    "Say what the repair was, such as \"taught the slope; two checks\"",
                    host,
                ));
            }
            tx.execute(
                "UPDATE practice_items SET repair=?2, updated_at_ms=?3 WHERE item_id=?1",
                params![item, what, now],
            )?;
            insert_event(tx, &item, "repair", None, (now, &what, now))?;
        }
        if let Some(next) = &edit.next {
            set_next(tx, &item, next, &format!("{path}.next"), notes)?;
        }
        if let Some(gap_type) = edit.gap_type {
            tx.execute(
                "UPDATE practice_items SET gap_type=?2, updated_at_ms=?3 WHERE item_id=?1",
                params![item, gap_type, now],
            )?;
        }
        if let Some(what) = &edit.what_went_wrong {
            let what = limits::shorten(
                &format!("{path}.what_went_wrong"),
                what.trim(),
                TEXT_BYTES,
                notes,
            );
            tx.execute(
                "UPDATE practice_items SET what_went_wrong=?2, updated_at_ms=?3 WHERE item_id=?1",
                params![item, what, now],
            )?;
        }
        if let Some(reason) = &edit.dropped {
            let reason =
                limits::shorten(&format!("{path}.dropped"), reason.trim(), TEXT_BYTES, notes);
            if reason.is_empty() {
                return Err(field_error(
                    &format!("{path}.dropped"),
                    "Say why the item was logged in error",
                    host,
                ));
            }
            tx.execute(
                "UPDATE practice_items SET dropped=?2, updated_at_ms=?3 WHERE item_id=?1",
                params![item, reason, now],
            )?;
        }
    }
    Ok(())
}

/// Correct stored attempts. The row as it was is kept in
/// attempt_corrections; what each item makes of it is worked out again.
pub(crate) fn apply_corrections(
    tx: &Connection,
    corrections: &[AttemptCorrection],
    now: i64,
    host: &HostContext,
    notes: &mut Vec<String>,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    for (index, correction) in corrections.iter().enumerate() {
        let path = format!("attempt_corrections[{index}]");
        let id = correction.attempt_id.trim();
        let before = tx
            .query_row(
                "SELECT json_object('mark',mark,'marks',marks,'out_of',out_of,'help',help,
                    'cold',cold,'slow',slow,'kind',problem_kind,'no_start',no_start,'counts_for',
                    (SELECT json_group_array(item_id) FROM practice_events
                     WHERE attempt_id=?1 AND kind='attempt'))
                 FROM attempts WHERE attempt_id=?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| {
                field_error(
                    &format!("{path}.attempt_id"),
                    "No stored attempt has this ID. Find it in practice.attempts of the result that stored it, or with magnesium attempts --json",
                    host,
                )
            })?;
        if !seen.insert(id.to_owned()) {
            return Err(field_error(
                &format!("{path}.attempt_id"),
                "Each attempt at most once per request",
                host,
            ));
        }
        let reason = correction.reason.trim();
        if reason.is_empty() {
            return Err(field_error(
                &format!("{path}.reason"),
                "Say why, such as \"the learner showed Q3b used the right method\"",
                host,
            ));
        }
        let (mark, marks, out_of, help_text, cold, slow, answered_at): (
            String,
            Option<f64>,
            Option<f64>,
            String,
            bool,
            bool,
            i64,
        ) = tx.query_row(
            "SELECT mark,marks,out_of,help,cold,slow,answered_at_ms FROM attempts WHERE attempt_id=?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )?;
        let marks = match (correction.marks, out_of) {
            (None, _) => marks,
            (Some(_), None) => {
                return Err(field_error(
                    &format!("{path}.marks"),
                    "Only a paper answer has marks; correct first_try instead",
                    host,
                ));
            }
            (Some(new), Some(out_of)) => {
                if !(new.is_finite() && (0.0..=out_of).contains(&new)) {
                    return Err(field_error(
                        &format!("{path}.marks"),
                        format!("marks runs from 0 to {out_of}"),
                        host,
                    ));
                }
                Some(new)
            }
        };
        let mut mark = correction.first_try.unwrap_or_else(|| first_try(&mark));
        if let (Some(marks), Some(out_of)) = (marks, out_of) {
            let follows = if marks == out_of {
                FirstTry::Right
            } else if marks == 0.0 {
                FirstTry::Wrong
            } else {
                FirstTry::Partly
            };
            if correction.first_try.is_some_and(|given| given != follows) {
                return Err(field_error(
                    &format!("{path}.first_try"),
                    format!(
                        "{marks} of {out_of} marks is {}: right is full marks, wrong is none, partly is between",
                        follows.as_str()
                    ),
                    host,
                ));
            }
            mark = follows;
        }
        let help = correction.help.unwrap_or_else(|| help(&help_text));
        let cold = correction.cold.unwrap_or(cold && help == AttemptHelp::None);
        if cold && help != AttemptHelp::None {
            return Err(field_error(
                &format!("{path}.cold"),
                "A helped answer is never cold: cold is true only with help none",
                host,
            ));
        }
        let slow = correction.slow.unwrap_or(slow);
        let (old_kind, old_no_start): (Option<String>, bool) = tx.query_row(
            "SELECT problem_kind, no_start FROM attempts WHERE attempt_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let kind = match correction.kind.as_deref().map(str::trim) {
            None => old_kind.clone(),
            Some("") => None,
            Some(kind) => {
                crate::learner::update::check_kind(&format!("{path}.kind"), kind, host)?;
                Some(crate::learner::help::kind_text(kind))
            }
        };
        let no_start = correction.no_start.unwrap_or(old_no_start) && kind.is_some();
        if correction.no_start == Some(true) && kind.is_none() {
            return Err(field_error(
                &format!("{path}.kind"),
                "no_start needs the kind of problem: give kind too",
                host,
            ));
        }
        if no_start && !matches!(mark, FirstTry::Declined | FirstTry::Wrong) {
            return Err(field_error(
                &format!("{path}.first_try"),
                "A learner with no starting move has no answer: first_try is declined or wrong",
                host,
            ));
        }
        tx.execute(
            "INSERT INTO attempt_corrections(attempt_id,before_json,reason,created_at_ms) VALUES(?1,?2,?3,?4)",
            params![
                id,
                before,
                limits::shorten(&format!("{path}.reason"), reason, TEXT_BYTES, notes),
                now
            ],
        )?;
        tx.execute(
            "UPDATE attempts SET mark=?2, marks=?3, help=?4, cold=?5, slow=?6,
                problem_kind=?7, kind_key=?8, no_start=?9 WHERE attempt_id=?1",
            params![
                id,
                mark,
                marks,
                help,
                cold,
                slow,
                kind,
                kind.as_deref().map(label_key),
                no_start
            ],
        )?;
        // The ladder of each kind it was or is now on is worked out again.
        for key in [old_kind.as_deref(), kind.as_deref()]
            .into_iter()
            .flatten()
            .map(label_key)
            .collect::<BTreeSet<_>>()
        {
            crate::learner::help::refresh(tx, &key, now)?;
        }
        if let Some(names) = &correction.counts_for {
            if names.len() > limits::COUNTS_FOR_PER_ATTEMPT {
                return Err(field_error(
                    &format!("{path}.counts_for"),
                    "At most 12 items",
                    host,
                ));
            }
            let items = resolve_all(tx, &format!("{path}.counts_for"), names, host)?;
            // The link to the quick check on its own label stays.
            tx.execute(
                "DELETE FROM practice_events WHERE attempt_id=?1 AND kind='attempt'
                   AND item_id NOT IN (SELECT p.item_id FROM practice_items p JOIN attempts a
                     ON a.attempt_id=?1 AND p.origin='chat' AND p.label_key=a.label_key)",
                [id],
            )?;
            for item in items {
                insert_event(tx, &item, "attempt", Some(id), (answered_at, "", now))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
