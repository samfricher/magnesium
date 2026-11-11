//! The study folder's side of practice items (#30).
//!
//! The learner store holds every item, and [`crate::learner::practice`] works
//! out what is due. This module reads each plan's test date, brings a gap log
//! the runtime has not seen into the store once, and writes each plan's
//! `gap-log.md` Gaps table from the store. The tutor reads that table and no
//! longer edits it. An edit made there by hand is noticed, saved and reported,
//! never silently lost.
use std::collections::BTreeMap;
use std::path::Path;

use chrono::{Local, NaiveDate};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::digest::{
    Table, first_date, gap_log_file, is_placeholder, plan_test_date, read_small, says_closed,
    section,
};
use crate::learner::practice::{Effect, ItemFacts, ItemState, Origin, derive, load_items, plans};
use crate::learner::store::LearnerStore;
use crate::workspace::{Workspace, reject_symlink_components};

/// At most this many due rows per plan are spelled out in a sitting opener.
const MAX_LISTED_ROWS: usize = 3;
const MAX_TODO: usize = 12;

/// An item with its state worked out.
#[derive(Clone, Debug)]
pub(crate) struct Item {
    pub(crate) facts: ItemFacts,
    pub(crate) state: ItemState,
}

impl Item {
    pub(crate) fn name(&self) -> String {
        self.facts.name()
    }

    /// Whether a sitting takes it after the other due checks: a right first
    /// answer made it, and no miss, decline or help has touched it since.
    /// The repair of a miss matters more.
    pub(crate) fn waits_behind_misses(&self) -> bool {
        self.facts.made_by_a_right_answer()
            && !self.state.effects.iter().any(|(_, effect)| {
                matches!(
                    effect,
                    Effect::Failed | Effect::Restarted | Effect::Helped | Effect::Declined
                )
            })
    }
}

pub(crate) fn open_store(workspace: &Workspace) -> Result<LearnerStore, String> {
    let id = workspace.id().map_err(|e| e.to_string())?;
    reject_symlink_components(&workspace.store_path(), workspace.control())
        .map_err(|e| e.to_string())?;
    LearnerStore::open(&workspace.store_path(), &id).map_err(|e| e.to_string())
}

/// Every item that is not dropped, worked out with its plan's test date.
pub(crate) fn snapshot(conn: &Connection, root: &Path) -> Result<Vec<Item>, String> {
    let facts = load_items(conn).map_err(|e| e.to_string())?;
    let mut dates: BTreeMap<String, Option<NaiveDate>> = BTreeMap::new();
    Ok(facts
        .into_iter()
        .filter(|item| item.dropped.is_none())
        .map(|facts| {
            let test_on = facts.plan.as_ref().and_then(|plan| {
                *dates
                    .entry(plan.clone())
                    .or_insert_with(|| plan_test_date(root, plan))
            });
            let state = derive(&facts, test_on);
            Item { facts, state }
        })
        .collect())
}

/// Quick checks due on `day`, in the order a sitting takes them: first the
/// checks a miss, a decline or help made or touched, then those a right
/// first answer made, each group the longest due first.
pub(crate) fn due_checks(items: &[Item], day: NaiveDate) -> Vec<&Item> {
    let mut due = items
        .iter()
        .filter(|item| item.facts.origin == Origin::Chat && item.state.due_on(day))
        .collect::<Vec<_>>();
    due.sort_by_key(|item| {
        (
            item.waits_behind_misses(),
            item.state.next_on,
            item.facts.created_at_ms,
        )
    });
    due
}

/// Paper redos due on `day` in one plan's gap log.
#[derive(Clone, Debug)]
pub(crate) struct DueRedos {
    pub(crate) gap_log: String,
    pub(crate) due: u64,
    pub(crate) first: Option<String>,
    pub(crate) rows: Vec<DueRow>,
}

#[derive(Clone, Debug)]
pub(crate) struct DueRow {
    pub(crate) name: String,
    pub(crate) tag: String,
    pub(crate) question: Option<String>,
    pub(crate) key: Option<String>,
}

/// Each plan's paper redos due on `day`, the longest due first. Rows in no
/// plan are listed under no gap log.
pub(crate) fn due_redos(items: &[Item], day: NaiveDate) -> Vec<DueRedos> {
    let mut by_plan: BTreeMap<Option<String>, Vec<&Item>> = BTreeMap::new();
    for item in items {
        if item.facts.origin == Origin::Paper && item.state.due_on(day) {
            by_plan
                .entry(item.facts.plan.clone())
                .or_default()
                .push(item);
        }
    }
    by_plan
        .into_iter()
        .map(|(plan, mut rows)| {
            rows.sort_by_key(|item| (item.state.next_on, item.facts.row_no));
            let first = rows.first().map(|item| {
                [item.facts.paper.clone(), item.facts.paper_question.clone()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" Q")
            });
            DueRedos {
                gap_log: plan
                    .as_deref()
                    .map_or_else(|| "paper rows in no plan".to_owned(), gap_log_file),
                due: rows.len() as u64,
                first: first.filter(|first| !first.is_empty()),
                rows: rows
                    .iter()
                    .take(MAX_LISTED_ROWS)
                    .map(|item| DueRow {
                        name: item.name(),
                        tag: item.facts.label.clone(),
                        question: item.facts.next_question.clone(),
                        key: item.facts.answer_key.clone(),
                    })
                    .collect(),
            }
        })
        .collect()
}

/// Open quick checks' labels, soonest due first, so a later attempt on the
/// same skill can reuse one exactly.
pub(crate) fn open_labels(items: &[Item]) -> Vec<String> {
    let mut open = items
        .iter()
        .filter(|item| item.facts.origin == Origin::Chat && !item.state.closed())
        .collect::<Vec<_>>();
    open.sort_by_key(|item| (item.state.next_on, item.facts.created_at_ms));
    open.into_iter()
        .take(12)
        .map(|item| item.facts.label.clone())
        .collect()
}

/// What each attempt of a committed update did, by attempt, for the tool
/// result and the sitting: (attempt ID, observation client_ref, [(item,
/// effect)]).
pub(crate) struct AttemptEffects {
    pub(crate) attempt_id: String,
    pub(crate) client_ref: String,
    pub(crate) effects: Vec<(String, &'static str)>,
}

pub(crate) fn attempt_effects(
    conn: &Connection,
    items: &[Item],
    request: &Value,
    result: &Value,
) -> Vec<AttemptEffects> {
    let mut out = Vec::new();
    let observations = request["observations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for observation in observations {
        let Some(client_ref) = observation["client_ref"].as_str() else {
            continue;
        };
        let Some(observation_id) = result["references"][client_ref].as_str() else {
            continue;
        };
        let Ok(mut statement) = conn
            .prepare("SELECT attempt_id FROM attempts WHERE observation_id=?1 ORDER BY attempt_no")
        else {
            continue;
        };
        let attempts = statement
            .query_map([observation_id], |r| r.get::<_, String>(0))
            .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
            .unwrap_or_default();
        for attempt_id in attempts {
            let effects = items
                .iter()
                .flat_map(|item| {
                    item.facts
                        .events
                        .iter()
                        .filter(|event| {
                            event
                                .attempt
                                .as_ref()
                                .is_some_and(|a| a.attempt_id == attempt_id)
                        })
                        .filter_map(|event| item.state.effect_of(event.event_no))
                        .map(|effect| (item.facts.item_id.clone(), effect.as_str()))
                        .collect::<Vec<_>>()
                })
                .collect();
            out.push(AttemptEffects {
                attempt_id,
                client_ref: client_ref.to_owned(),
                effects,
            });
        }
    }
    out
}

/// The `practice` field of a committed update's result: each attempt's ID
/// and what it did to each item, and what each touched item still needs.
pub(crate) fn update_summary(
    items: &[Item],
    effects: &[AttemptEffects],
    request: &Value,
    conn: &Connection,
) -> Option<Value> {
    let by_id: BTreeMap<&str, &Item> = items
        .iter()
        .map(|item| (item.facts.item_id.as_str(), item))
        .collect();
    let mut touched: Vec<&Item> = Vec::new();
    fn touch<'a>(by_id: &BTreeMap<&str, &'a Item>, id: &str, touched: &mut Vec<&'a Item>) {
        if let Some(found) = by_id.get(id)
            && !touched.iter().any(|t| t.facts.item_id == id)
        {
            touched.push(found);
        }
    }
    let attempts = effects
        .iter()
        .map(|attempt| {
            json!({
                "attempt_id": attempt.attempt_id,
                "observation": attempt.client_ref,
                "items": attempt.effects.iter().filter_map(|(id, effect)| {
                    touch(&by_id, id, &mut touched);
                    let item = by_id.get(id.as_str())?;
                    Some(json!({"item": item.name(), "effect": effect}))
                }).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    for edit in request["practice"].as_array().into_iter().flatten() {
        if let Some(name) = edit["item"].as_str()
            && let Ok(Some(id)) = crate::learner::practice::resolve(conn, name)
        {
            touch(&by_id, &id, &mut touched);
        }
    }
    for correction in request["attempt_corrections"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let Some(attempt) = correction["attempt_id"].as_str() else {
            continue;
        };
        for item in items {
            if item.facts.events.iter().any(|event| {
                event
                    .attempt
                    .as_ref()
                    .is_some_and(|a| a.attempt_id == attempt.trim())
            }) {
                touch(&by_id, &item.facts.item_id, &mut touched);
            }
        }
    }
    if attempts.is_empty() && touched.is_empty() {
        return None;
    }
    let mut todo = Vec::new();
    let states = touched
        .iter()
        .map(|item| {
            let awaiting_repair = item.facts.origin == Origin::Paper
                && !item.state.closed()
                && item.state.next_on.is_none();
            if awaiting_repair {
                todo.push(format!(
                    "{}: when its repair is done, report practice {{item: \"{}\", repaired: what was done}}; the first redo falls due the next day.",
                    item.name(),
                    item.name()
                ));
            }
            if !item.state.closed() && item.facts.next_question.is_none() {
                todo.push(format!(
                    "{}: no next question stored; send practice {{item: \"{}\", next: {{question, answer_key}}}}, a new problem that asks for a result and never names or points at the skill, tag, method, form of answer, step or earlier mistake (\"Solve …\", not \"Solve … by <method>\"), and to which each earlier miss gives a different answer from the right one.",
                    item.name(),
                    item.name()
                ));
            }
            json!({
                "item": item.name(),
                "status": item.state.status(),
                "next_redo": item.state.next_on.map(|d| d.to_string()),
                "gap_log": item.facts.plan.as_deref().map(gap_log_file),
            })
        })
        .collect::<Vec<_>>();
    todo.truncate(MAX_TODO);
    let mut value = json!({"attempts": attempts, "items": states});
    if !todo.is_empty() {
        value["todo"] = json!(todo);
    }
    Some(value)
}

/// What bringing gap logs in and writing them did, for a tool result.
#[derive(Debug, Default)]
pub(crate) struct GapLogReport {
    pub(crate) imported: Vec<String>,
    pub(crate) written: Vec<String>,
    pub(crate) not_imported: Vec<Value>,
    pub(crate) edited: Vec<Value>,
}

impl GapLogReport {
    pub(crate) fn merge(&mut self, other: Self) {
        for (mine, theirs) in [
            (&mut self.imported, other.imported),
            (&mut self.written, other.written),
        ] {
            for file in theirs {
                if !mine.contains(&file) {
                    mine.push(file);
                }
            }
        }
        self.not_imported.extend(other.not_imported);
        self.edited.extend(other.edited);
    }

    pub(crate) fn to_json(&self) -> Option<Value> {
        if self.imported.is_empty()
            && self.written.is_empty()
            && self.not_imported.is_empty()
            && self.edited.is_empty()
        {
            return None;
        }
        let mut value = json!({});
        if !self.imported.is_empty() {
            value["imported"] = json!(self.imported);
        }
        if !self.written.is_empty() {
            value["written"] = json!(self.written);
            value["note"] = json!(
                "Magnesium wrote the Gaps table of each gap log listed from the learner record. Read it again before quoting it; do not edit that table."
            );
        }
        if !self.not_imported.is_empty() {
            value["not_imported"] = json!(self.not_imported);
            value["not_imported_note"] = json!(
                "These gap logs have rows Magnesium cannot read, so it has not taken them over and none of their rows can fall due. Correct each row at its line, keeping the learner's content; the next paper stage load brings the log in."
            );
        }
        if !self.edited.is_empty() {
            value["edited"] = json!(self.edited);
            value["edited_note"] = json!(
                "Someone edited the Gaps table by hand. The edited file is saved where shown, and the table was written again from the record. Ask the learner what they meant to change, and record it: a mark as an attempt correction, a repair or a row's description in practice."
            );
        }
        Some(value)
    }
}

/// Bring in each plan's gap log the runtime has not taken over, then write
/// the Gaps table of every plan it has. A plan whose log cannot be read is
/// left alone. Nothing here fails the call that triggered it.
pub(crate) fn sync_gap_logs(workspace: &Workspace, today: NaiveDate) -> GapLogReport {
    let mut report = GapLogReport::default();
    let Ok(store) = open_store(workspace) else {
        return report;
    };
    let root = workspace.root();
    for plan in plans(root) {
        let taken: bool = store
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM gap_logs WHERE plan=?1)",
                [&plan],
                |r| r.get(0),
            )
            .unwrap_or(true);
        if taken {
            continue;
        }
        match import(workspace, &store.conn, &plan, today) {
            Ok(true) => report.imported.push(gap_log_file(&plan)),
            Ok(false) => {}
            Err(reasons) => report.not_imported.push(json!({
                "file": gap_log_file(&plan),
                "unreadable": reasons,
            })),
        }
    }
    let Ok(items) = snapshot(&store.conn, root) else {
        return report;
    };
    let taken = {
        let Ok(mut statement) = store
            .conn
            .prepare("SELECT plan, rendered_sha256 FROM gap_logs ORDER BY plan")
        else {
            return report;
        };
        statement
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })
            .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
            .unwrap_or_default()
    };
    let plans = plans(root);
    for (plan, last) in taken {
        if !plans.contains(&plan) {
            continue;
        }
        let rows = items
            .iter()
            .filter(|item| item.facts.plan.as_deref() == Some(plan.as_str()))
            .collect::<Vec<_>>();
        let _ = write_gap_log(workspace, &store.conn, &plan, &rows, last, &mut report);
    }
    report
}

fn sha256(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn backup(workspace: &Workspace, plan: &str, kind: &str, text: &str) -> std::io::Result<String> {
    let backups =
        workspace.private_dir(&Path::new(crate::learner::store::BACKUP_DIR).join("gap-logs"))?;
    let stamp = Local::now().format("%Y%m%dT%H%M%S%3f");
    let path = backups.join(format!("{plan}-{kind}{stamp}.md"));
    crate::workspace::write_private_atomic(&path, text.as_bytes())?;
    Ok(path
        .strip_prefix(workspace.root())
        .unwrap_or(&path)
        .to_string_lossy()
        .into_owned())
}

/// One row of an old gap log, as it will be stored.
struct ImportedRow {
    row_no: Option<i64>,
    logged_on: NaiveDate,
    paper: Option<String>,
    question: Option<String>,
    tag: String,
    what: String,
    gap_type: Option<&'static str>,
    confidence: Option<&'static str>,
    repair: String,
    history: String,
    passes: i64,
    next_on: Option<NaiveDate>,
    last_pass_on: Option<NaiveDate>,
}

const KNOWN_COLUMNS: [&str; 13] = [
    "#",
    "Date",
    "Paper",
    "Q",
    "Tag",
    "What went wrong",
    "Type",
    "Conf",
    "Repair done",
    "Passes",
    "Next redo",
    "Cold-redo date",
    "Status",
];

/// The rows of a plan's gap log, or why it cannot be read. A log from any
/// template is read: one with a Cold-redo date column, or with no Next redo
/// column at all. An open row whose repair is written down and that has no
/// date is due `today`, as the conversion of #26 had it.
fn read_rows(file: &str, text: &str, today: NaiveDate) -> Result<Vec<ImportedRow>, Vec<Value>> {
    let table = Table::parse(section(text, "Gaps"));
    let mut reasons = Vec::new();
    let reason = |line: usize, reason: String| json!({"file": file, "section": "Gaps", "line": line, "reason": reason});
    for &line in &table.later_rows {
        reasons.push(reason(
            line,
            "the row comes after a break in the Gaps table, such as a blank line".into(),
        ));
    }
    if table.rows.is_empty() {
        return if reasons.is_empty() {
            Ok(Vec::new())
        } else {
            Err(reasons)
        };
    }
    for name in ["Date", "Status"] {
        if table.column(name).is_none() {
            reasons.push(reason(
                table.header_line,
                format!("the Gaps table has no {name} column"),
            ));
        }
    }
    if !reasons.is_empty() {
        return Err(reasons);
    }
    let next_column = table
        .column("Next redo")
        .or_else(|| table.column("Cold-redo date"));
    let mut rows = Vec::new();
    for row in &table.rows {
        if row.cells.len() != table.header.len() {
            reasons.push(reason(
                row.line,
                format!(
                    "the row has {} cells but the header has {} columns",
                    row.cells.len(),
                    table.header.len()
                ),
            ));
            continue;
        }
        let cell = |name: &str| {
            table
                .cell(row, name)
                .filter(|c| !is_placeholder(c))
                .map(str::to_owned)
        };
        let Some(logged_on) = cell("Date").as_deref().and_then(first_date) else {
            // The template's row has only placeholders, a number and a status.
            let content = table.header.iter().enumerate().any(|(index, header)| {
                !["#", "Status"]
                    .iter()
                    .any(|h| h.eq_ignore_ascii_case(header))
                    && row.cells.get(index).is_some_and(|c| !is_placeholder(c))
            });
            if content {
                reasons.push(reason(
                    row.line,
                    "the row has no Date written as YYYY-MM-DD".into(),
                ));
            }
            continue;
        };
        let status = cell("Status").unwrap_or_default();
        let status_key = status.replace(['*', '_'], "").trim().to_lowercase();
        let passes = if says_closed(&status) {
            3
        } else if status_key == "2 of 3" {
            2
        } else if status_key == "1 of 3" {
            1
        } else {
            0
        };
        let next_text = next_column
            .and_then(|i| row.cells.get(i))
            .filter(|c| !is_placeholder(c));
        let next_on = match next_text {
            Some(text) if passes < 3 => match first_date(text) {
                Some(date) => Some(date),
                None => {
                    reasons.push(reason(
                        row.line,
                        format!(
                            "Next redo \"{}\" is not a date written as YYYY-MM-DD",
                            text.chars().take(40).collect::<String>()
                        ),
                    ));
                    continue;
                }
            },
            _ => None,
        };
        let repair = cell("Repair done").unwrap_or_default();
        // An older log had no Next redo: a repaired open row there is due now.
        // A row part-way through its redos with no date is due now too, or
        // it could never fall due again.
        let next_on = match next_on {
            None if passes < 3 && table.column("Next redo").is_none() && !repair.is_empty() => {
                Some(today)
            }
            None if (1..3).contains(&passes) => Some(today),
            other => other,
        };
        let history = cell("Passes").unwrap_or_default();
        let last_pass_on = history
            .split([',', ';'])
            .filter(|part| part.to_lowercase().contains("pass"))
            .filter_map(first_date)
            .max();
        let type_cell = cell("Type").unwrap_or_default();
        let gap_type = match type_cell
            .trim()
            .chars()
            .next()
            .map(|c| c.to_ascii_uppercase())
        {
            Some('K') => Some("K"),
            Some('S') => Some("S"),
            Some('N') => Some("N"),
            Some('A') => Some("A"),
            Some('R') => Some("R"),
            _ => None,
        };
        let mut what = cell("What went wrong").unwrap_or_default();
        // Keep whatever the runtime has no column for, so nothing is lost.
        let mut extra = Vec::new();
        if gap_type.is_none() && !type_cell.is_empty() {
            extra.push(format!("Type: {type_cell}"));
        }
        if passes == 0 && !status_key.is_empty() && status_key != "open" {
            extra.push(format!("Status: {status}"));
        }
        for (index, header) in table.header.iter().enumerate() {
            if !KNOWN_COLUMNS.iter().any(|k| k.eq_ignore_ascii_case(header))
                && let Some(value) = row.cells.get(index).filter(|c| !is_placeholder(c))
            {
                extra.push(format!("{header}: {value}"));
            }
        }
        if !extra.is_empty() {
            what = format!("{what} ({})", extra.join("; ")).trim().to_owned();
        }
        let confidence = match cell("Conf").and_then(|c| c.trim().chars().next()) {
            Some('S' | 's') => Some("sure"),
            Some('U' | 'u') => Some("unsure"),
            Some('G' | 'g') => Some("guess"),
            _ => None,
        };
        rows.push(ImportedRow {
            row_no: cell("#")
                .and_then(|n| n.trim().parse::<i64>().ok())
                .filter(|n| *n >= 1),
            logged_on,
            paper: cell("Paper"),
            question: cell("Q"),
            tag: cell("Tag").unwrap_or_default(),
            what,
            gap_type,
            confidence,
            repair,
            history,
            passes,
            next_on,
            last_pass_on,
        });
    }
    if reasons.is_empty() {
        Ok(rows)
    } else {
        Err(reasons)
    }
}

/// Bring one plan's gap log into the store, once. Ok(true) when it had a
/// file; the original is kept in `.magnesium/backups/gap-logs/`.
fn import(
    workspace: &Workspace,
    conn: &Connection,
    plan: &str,
    today: NaiveDate,
) -> Result<bool, Vec<Value>> {
    let file = gap_log_file(plan);
    let path = workspace.root().join(&file);
    let text = match read_small(&path) {
        Some(text) => Some(text),
        None if std::fs::symlink_metadata(&path).is_ok() => {
            return Err(vec![
                json!({"file": file, "reason": "the file is too large, not text, or not a regular file"}),
            ]);
        }
        None => None,
    };
    let rows = match &text {
        Some(text) => read_rows(&file, text, today)?,
        None => Vec::new(),
    };
    let storage = |e: rusqlite::Error| vec![json!({"file": file, "reason": e.to_string()})];
    // With no file, the plan is taken over only once it has a row to write,
    // so a gap log made later is still brought in.
    if text.is_none() {
        let has_rows: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM practice_items WHERE plan=?1)",
                [plan],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if !has_rows {
            return Ok(false);
        }
    }
    if let Some(text) = &text {
        backup(workspace, plan, "", text)
            .map_err(|e| vec![json!({"file": file, "reason": e.to_string()})])?;
    }
    conn.execute_batch("BEGIN IMMEDIATE").map_err(storage)?;
    let written = (|| -> rusqlite::Result<bool> {
        let taken: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM gap_logs WHERE plan=?1)",
            [plan],
            |r| r.get(0),
        )?;
        if taken {
            return Ok(false);
        }
        let now = crate::learner::store::now_ms();
        let mut used: Vec<i64> = {
            let mut statement = conn.prepare("SELECT row_no FROM practice_items WHERE plan=?1")?;
            statement
                .query_map([plan], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        for row in &rows {
            let row_no = match row.row_no {
                Some(n) if !used.contains(&n) => n,
                _ => {
                    used.iter()
                        .copied()
                        .max()
                        .unwrap_or(0)
                        .max(rows.iter().filter_map(|r| r.row_no).max().unwrap_or(0))
                        + 1
                }
            };
            used.push(row_no);
            let id = crate::learner::store::new_id("g")
                .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;
            conn.execute(
                "INSERT INTO practice_items(item_id,origin,label,label_key,plan,row_no,paper,
                    paper_question,logged_on,gap_type,what_went_wrong,repair,confidence,
                    base_passes,base_next_on,base_last_pass_on,base_history,created_at_ms,updated_at_ms)
                 VALUES(?1,'paper',?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?17)",
                params![
                    id,
                    row.tag,
                    crate::learner::label_key(&row.tag),
                    plan,
                    row_no,
                    row.paper,
                    row.question,
                    row.logged_on.to_string(),
                    row.gap_type,
                    row.what,
                    row.repair,
                    row.confidence,
                    row.passes,
                    (row.passes < 3).then_some(row.next_on).flatten().map(|d| d.to_string()),
                    row.last_pass_on.map(|d| d.to_string()),
                    row.history,
                    now
                ],
            )?;
        }
        // The import keeps its own copy, so the table as brought in counts
        // as the last one seen, and the first write makes no second copy.
        let seen = text
            .as_deref()
            .and_then(|text| gaps_range(text).map(|(start, end)| sha256(&text[start..end])));
        conn.execute(
            "INSERT INTO gap_logs(plan,imported_at_ms,imported_rows,rendered_sha256,updated_at_ms) VALUES(?1,?2,?3,?4,?2)",
            params![plan, now, rows.len() as i64, seen],
        )?;
        Ok(true)
    })();
    match written {
        Ok(taken) => {
            conn.execute_batch("COMMIT").map_err(storage)?;
            Ok(taken && text.is_some())
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(storage(error))
        }
    }
}

fn cell(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "\\|")
}

fn conf_letter(item: &Item) -> &'static str {
    match item.state.confidence {
        Some(crate::learner::Confidence::Sure) => "S",
        Some(crate::learner::Confidence::Unsure) => "U",
        Some(crate::learner::Confidence::Guess) => "G",
        None => "",
    }
}

/// The Gaps section, from its heading to the line before the next one.
pub(crate) fn render_section(plan: &str, rows: &[&Item]) -> String {
    let mut text = format!(
        "## Gaps\n\nMagnesium writes this table from the learner record after each change, so an edit here is not kept: tell the tutor what is wrong instead. In learning_step a row is named as {plan}#1 for row 1.\n\n| # | Date | Paper | Q | Tag | What went wrong | Type | Conf | Repair done | Passes | Next redo | Status |\n|---|---|---|---|---|---|---|---|---|---|---|---|\n"
    );
    for item in rows {
        let facts = &item.facts;
        let cells = [
            facts.row_no.map(|n| n.to_string()).unwrap_or_default(),
            facts.logged_on.to_string(),
            facts.paper.clone().unwrap_or_default(),
            facts.paper_question.clone().unwrap_or_default(),
            facts.label.clone(),
            facts.what_went_wrong.clone(),
            facts
                .gap_type
                .map(|t| t.as_str().to_owned())
                .unwrap_or_default(),
            conf_letter(item).to_owned(),
            facts.repair.clone(),
            item.state.history.join(", "),
            item.state
                .next_on
                .map(|d| d.to_string())
                .unwrap_or_default(),
            item.state.status().to_owned(),
        ];
        text.push_str("| ");
        text.push_str(
            &cells
                .iter()
                .map(|c| cell(c))
                .collect::<Vec<_>>()
                .join(" | "),
        );
        text.push_str(" |\n");
    }
    text.push('\n');
    text
}

/// The byte range of the Gaps section: its heading line through the line
/// before the next `## ` heading, or the end.
fn gaps_range(text: &str) -> Option<(usize, usize)> {
    let mut start = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if let Some(heading) = line.strip_prefix("## ") {
            if let Some(start) = start {
                return Some((start, offset));
            }
            if heading.contains("Gaps") {
                start = Some(offset);
            }
        }
        offset += line.len();
    }
    start.map(|start| (start, text.len()))
}

/// The file with its Gaps section replaced by `section`. Without one, the
/// section goes before Scores, or at the end.
fn with_section(text: &str, new: &str) -> String {
    if let Some((start, end)) = gaps_range(text) {
        return format!("{}{new}{}", &text[..start], &text[end..]);
    }
    let scores = text
        .split_inclusive('\n')
        .scan(0, |offset, line| {
            let at = *offset;
            *offset += line.len();
            Some((at, line))
        })
        .find(|(_, line)| line.starts_with("## ") && line.contains("Scores"))
        .map(|(at, _)| at);
    match scores {
        Some(at) => format!("{}{new}{}", &text[..at], &text[at..]),
        None => {
            let mut out = text.to_owned();
            if !out.is_empty() && !out.ends_with("\n\n") {
                out.push_str(if out.ends_with('\n') { "\n" } else { "\n\n" });
            }
            out.push_str(new);
            out
        }
    }
}

fn write_gap_log(
    workspace: &Workspace,
    conn: &Connection,
    plan: &str,
    rows: &[&Item],
    last: Option<String>,
    report: &mut GapLogReport,
) -> std::io::Result<()> {
    let file = gap_log_file(plan);
    let path = workspace.root().join(&file);
    reject_symlink_components(&path, workspace.root())?;
    // A file that exists but cannot be read is never written over.
    let current = match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => match read_small(&path) {
            Some(text) => Some(text),
            None => {
                report.not_imported.push(json!({"file": file, "reason": "the file is too large or not text, so Magnesium did not write its Gaps table"}));
                return Ok(());
            }
        },
        Ok(_) => return Ok(()),
        Err(_) => None,
    };
    // A plan with no rows and no file needs no file yet.
    if current.is_none() && rows.is_empty() {
        return Ok(());
    }
    let new_section = render_section(plan, rows);
    let text = current.clone().unwrap_or_else(|| {
        format!(
            "# Gap log: {plan}\n\n## Gaps\n\n## Scores\n\n| Paper | Date | Score | Time used | Notes |\n|---|---|---|---|---|\n"
        )
    });
    let old_section = gaps_range(&text).map(|(start, end)| &text[start..end]);
    if old_section == Some(new_section.as_str()) {
        if last.as_deref() != Some(sha256(&new_section).as_str()) {
            remember(conn, plan, &new_section);
        }
        return Ok(());
    }
    match (&last, old_section) {
        (Some(last), Some(old)) if sha256(old) != *last => {
            let saved = backup(workspace, plan, "edited-", &text)?;
            report.edited.push(json!({"file": file, "saved_to": saved}));
        }
        // The first write over a file keeps a copy of it as it was.
        (None, _) if current.is_some() => {
            backup(workspace, plan, "before-first-write-", &text)?;
        }
        _ => {}
    }
    let updated = with_section(&text, &new_section);
    crate::workspace::write_user_owned_atomic(&path, updated.as_bytes())?;
    remember(conn, plan, &new_section);
    report.written.push(file);
    Ok(())
}

fn remember(conn: &Connection, plan: &str, section: &str) {
    let _ = conn.execute(
        "UPDATE gap_logs SET rendered_sha256=?2, updated_at_ms=?3 WHERE plan=?1",
        params![plan, sha256(section), crate::learner::store::now_ms()],
    );
}

/// The digest's view of a plan the runtime has taken over: its unfixed rows,
/// those due, and the first due, from the store.
pub(crate) fn plan_gaps(
    conn: &Connection,
    root: &Path,
    plan: &str,
    today: NaiveDate,
) -> Option<Value> {
    let taken: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM gap_logs WHERE plan=?1)",
            [plan],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten()
        .unwrap_or(false);
    if !taken {
        return None;
    }
    let items = snapshot(conn, root).ok()?;
    let rows = items
        .iter()
        .filter(|item| item.facts.plan.as_deref() == Some(plan) && !item.state.closed())
        .collect::<Vec<_>>();
    let mut due = rows
        .iter()
        .filter(|item| item.state.due_on(today))
        .collect::<Vec<_>>();
    due.sort_by_key(|item| (item.state.next_on, item.facts.row_no));
    let first_due = due.first().map(|item| {
        json!({"paper": item.facts.paper, "question": item.facts.paper_question, "more_due": due.len() - 1})
    });
    Some(json!({
        "unfixed_gaps": rows.len(),
        "redos_due": due.len(),
        "first_due": first_due,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_old_gap_log_is_read_with_its_rows_and_dates() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let old = "# Gap log\n\n## Gaps\n\n| # | Date | Paper | Q | Tag | What went wrong | Type | Repair done | Cold-redo date | Status | Notes |\n|---|---|---|---|---|---|---|---|---|---|---|\n| 1 | 2026-10-20 | Diagnostic | 3 | km | slope | K | taught the slope | 2026-10-25 | open | |\n| 2 | 2026-10-20 | Diagnostic | 4 | vmax | read a\\|b | R | reread | | open | ask again |\n| 3 | 2026-10-20 | Diagnostic | 5 | km | units | N | | | open | |\n| 4 | 2026-10-20 | Diagnostic | 6 | km | slip | A | fixed | | closed | |\n";
        let rows = read_rows("Study/x/gap-log.md", old, today).unwrap();
        assert_eq!(rows.len(), 4);
        let due = |i: usize| rows[i].next_on.map(|d| d.to_string());
        assert_eq!(due(0).as_deref(), Some("2026-10-25"));
        assert_eq!(
            due(1).as_deref(),
            Some("2026-11-09"),
            "repaired, no date: due now"
        );
        assert_eq!(due(2), None, "still waiting for its repair");
        assert_eq!((rows[3].passes, due(3)), (3, None));
        assert_eq!(rows[1].what, "read a|b (Notes: ask again)");
        assert_eq!(rows[1].gap_type, Some("R"));
    }

    #[test]
    fn a_row_the_runtime_cannot_read_stops_the_import() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let log = "## Gaps\n\n| # | Date | Next redo | Status |\n|---|---|---|---|\n| 1 | 2026-10-20 | soon | open |\n| 2 | 2026-10-20 | | open | extra |\n";
        let reasons = read_rows("f", log, today).err().unwrap();
        assert_eq!(reasons.len(), 2, "{reasons:?}");
    }

    #[test]
    fn the_gaps_section_is_replaced_and_the_rest_kept() {
        let text = "# Gap log\n\nIntro.\n\n## Gaps\n\nold table\n\n## Scores\n\n| Paper |\n";
        let new = with_section(text, "## Gaps\n\nNEW\n\n");
        assert_eq!(
            new,
            "# Gap log\n\nIntro.\n\n## Gaps\n\nNEW\n\n## Scores\n\n| Paper |\n"
        );
        let without = "# Gap log\n\n## Scores\n\nx\n";
        assert_eq!(
            with_section(without, "## Gaps\n\nNEW\n\n"),
            "# Gap log\n\n## Gaps\n\nNEW\n\n## Scores\n\nx\n"
        );
        assert_eq!(with_section("# Log", "## Gaps\n"), "# Log\n\n## Gaps\n");
    }
}
