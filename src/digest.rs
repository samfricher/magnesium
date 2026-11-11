//! Read-only digest of a study folder and its learner record for `/magnesium:help`.
//!
//! Code gathers the facts and does the date arithmetic; the help skill only
//! renders them. Nothing here writes to the folder or the learner store.
//!
//! Input the digest cannot read is never skipped silently. Each plan's
//! `unreadable` lists it, with the file, section, line and reason, and the
//! counts leave it out.
use crate::learner::store::LearnerStore;
use crate::workspace::{Workspace, reject_symlink_components};
use chrono::{Local, NaiveDate, TimeZone};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::process::ExitCode;

mod sessions;

const MAX_LISTED_FILES: usize = 60;
const MAX_SCANNED_ENTRIES: usize = 2000;
const MAX_TEXT_CHARS: usize = 300;
const MAX_RECENT_SESSIONS: usize = 3;
const MAX_READY_PURPOSES: usize = 3;
const MAX_QUOTED_CHARS: usize = 40;
const MAX_PLAN_BYTES: u64 = 256 * 1024;
const SKIPPED_NAMES: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];

/// Which of the window's needs this runtime meets, as one number. The window
/// refuses a runtime that reports a lower number, or none. A version number
/// cannot tell: a build from an older commit can carry the same one. Raise it
/// when the window comes to need something new of the runtime.
///
/// 1: `progress --json` with `today_counts`.
/// 2: `sitting_redos`, the due paper redos as data, beside `sitting_checks`.
pub(crate) const WINDOW_FEATURES: u64 = 2;

/// Every digest says the number, in a study folder or outside one, so the
/// window can ask without a folder.
fn with_window_features(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("window_features".into(), json!(WINDOW_FEATURES));
    }
    value
}

pub(crate) fn print_digest() -> ExitCode {
    let result = (|| {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        match Workspace::discover(&cwd).map_err(|e| e.to_string())? {
            Some(workspace) => digest(&workspace, Local::now().timestamp_millis()),
            None => Ok(json!({"active":false,"reason":"workspace_not_initialized"})),
        }
    })();
    match result {
        Ok(value) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&with_window_features(value)).unwrap_or_default()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("magnesium: {e}");
            ExitCode::FAILURE
        }
    }
}

fn digest(workspace: &Workspace, now_ms: i64) -> Result<Value, String> {
    let today = local_date(now_ms).ok_or("the clock is out of range")?;
    let id = workspace.id().map_err(|e| e.to_string())?;
    reject_symlink_components(&workspace.store_path(), workspace.control())
        .map_err(|e| e.to_string())?;
    // Opening an earlier version's record upgrades it. The digest changes
    // nothing, so it says so and stops: `magnesium init
    // --upgrade-instructions` upgrades the folder, after keeping a copy.
    if crate::learner::store::needs_upgrade(&workspace.store_path()).map_err(|e| e.to_string())?
        || crate::workspace::instruction_bundle::earlier_bundle_active(workspace.control())
    {
        return Ok(json!({
            "active": true,
            "upgrade_needed": true,
            "today": today.to_string(),
            "study_folder": workspace.root().to_string_lossy(),
        }));
    }
    let store = LearnerStore::open(&workspace.store_path(), &id).map_err(|e| e.to_string())?;
    let snapshot = store
        .conn
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let sessions = sessions(&snapshot, today).map_err(|e| e.to_string())?;
    let reviews = reviews(&snapshot, now_ms, today).map_err(|e| e.to_string())?;
    let memories: i64 = snapshot
        .query_row(
            "SELECT count(*) FROM learner_memories WHERE lifecycle='active'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let plans = plans(workspace.root(), today, &sessions, &snapshot);
    let recent = sessions
        .iter()
        .take(MAX_RECENT_SESSIONS)
        .map(|s| s.value.clone())
        .collect::<Vec<_>>();
    drop(snapshot);
    Ok(json!({
        "active": true,
        "today": today.to_string(),
        "study_folder": workspace.root().to_string_lossy(),
        "files": files(workspace.root()),
        "plans": plans,
        "record": {
            "memories": memories,
            "sessions": recent,
            "reviews": reviews,
        },
    }))
}

fn local_date(ms: i64) -> Option<NaiveDate> {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| t.date_naive())
}

fn clip(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match text.char_indices().nth(MAX_TEXT_CHARS) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text,
    }
}

/// Visible files two levels below the study folder, as the help skill sees them.
fn files(root: &Path) -> Value {
    let mut listed = Vec::new();
    let mut count = 0usize;
    let mut scanned = 0usize;
    let mut stack = vec![(root.to_path_buf(), String::new(), 0usize)];
    while let Some((dir, prefix, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut names = entries
            .filter_map(Result::ok)
            .take(MAX_SCANNED_ENTRIES.saturating_sub(scanned))
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let kind = entry.file_type().ok()?;
                Some((name, kind.is_dir()))
            })
            .collect::<Vec<_>>();
        scanned += names.len();
        names.sort();
        let mut children = Vec::new();
        for (name, is_dir) in names {
            if name.starts_with('.') || (depth == 0 && SKIPPED_NAMES.contains(&name.as_str())) {
                continue;
            }
            let path = format!("{prefix}{name}");
            count += 1;
            if listed.len() < MAX_LISTED_FILES {
                listed.push(if is_dir {
                    format!("{path}/")
                } else {
                    path.clone()
                });
            }
            if is_dir && depth == 0 {
                children.push((dir.join(&name), format!("{path}/"), depth + 1));
            }
        }
        // Reversed, so the stack visits folders in name order.
        stack.extend(children.into_iter().rev());
    }
    listed.sort();
    json!({
        "count": count,
        "listed": listed,
        "truncated": count > listed.len() || scanned >= MAX_SCANNED_ENTRIES,
    })
}

struct Session {
    goal: String,
    value: Value,
}

/// Every session, most recently changed first, with its saved checkpoint.
fn sessions(conn: &Connection, today: NaiveDate) -> rusqlite::Result<Vec<Session>> {
    let mut statement = conn.prepare(
        "SELECT goal,status,checkpoint_json,updated_at_ms FROM learning_sessions ORDER BY updated_at_ms DESC, session_id LIMIT 50",
    )?;
    let rows = statement.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })?;
    rows.map(|row| {
        let (goal, status, checkpoint, updated_at_ms) = row?;
        let checkpoint: Value = serde_json::from_str(&checkpoint).unwrap_or(Value::Null);
        let field = |name: &str| {
            checkpoint[name]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(clip)
        };
        let last = local_date(updated_at_ms);
        Ok(Session {
            value: json!({
                "goal": clip(&goal),
                "status": status,
                "last_active": last.map(|d| d.to_string()),
                "days_ago": last.map(|d| (today - d).num_days()),
                "current_problem": field("current_problem"),
                "next_action": field("next_action"),
                "plan": field("plan_text"),
                "has_pending_task": !checkpoint["pending_task"].is_null(),
            }),
            goal,
        })
    })
    .collect()
}

/// Review suggestions for active memories. A suggestion whose date has come, or
/// that has no date, is ready: an offer to the learner, not a queue to clear.
fn reviews(conn: &Connection, now_ms: i64, today: NaiveDate) -> rusqlite::Result<Value> {
    let mut statement = conn.prepare(
        "SELECT r.purpose,r.due_at_ms FROM review_items r JOIN learner_memories m ON m.memory_id=r.memory_id \
         WHERE r.status='suggested' AND m.lifecycle='active' ORDER BY coalesce(r.due_at_ms,0), r.review_id",
    )?;
    let rows = statement
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let (ready, later): (Vec<_>, Vec<_>) = rows
        .into_iter()
        .partition(|(_, due_at)| due_at.is_none_or(|ms| ms <= now_ms));
    let next = later.first().and_then(|(_, ms)| ms.and_then(local_date));
    Ok(json!({
        "ready_purposes": ready.iter().take(MAX_READY_PURPOSES).map(|(p, _)| clip(p)).collect::<Vec<_>>(),
        "ready": ready.len(),
        "later": later.len(),
        "next_ready": next.map(|d| d.to_string()),
        "next_ready_in_days": next.map(|d| (d - today).num_days()),
    }))
}

/// Each `Study/<target>/plan.md` with its date, blocks and gap-log state.
/// The gap-log state of a plan the runtime has taken over comes from the
/// learner store, so an edit to its Markdown table changes nothing.
fn plans(root: &Path, today: NaiveDate, sessions: &[Session], store: &Connection) -> Vec<Value> {
    let study = root.join("Study");
    let Ok(entries) = fs::read_dir(&study) else {
        return Vec::new();
    };
    let mut targets = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect::<Vec<_>>();
    targets.sort();
    targets
        .into_iter()
        .filter_map(|target| {
            let dir = study.join(&target);
            let plan = read_small(&dir.join("plan.md"))?;
            let gap_path = dir.join("gap-log.md");
            let gaps = read_small(&gap_path);
            let mut value = plan_digest(
                &target,
                &plan,
                gaps.as_deref().unwrap_or_default(),
                today,
                sessions,
            );
            if let Some(recorded) = crate::practice::plan_gaps(store, root, &target, today) {
                for field in ["unfixed_gaps", "redos_due", "first_due"] {
                    value[field] = recorded[field].clone();
                }
                value["gap_log_from"] = json!("record");
                if let Some(list) = value["unreadable"].as_array_mut() {
                    list.retain(|item| item["file"] != json!(gap_log_file(&target)));
                }
                return Some(value);
            }
            value["gap_log_from"] = json!("file");
            if gaps.is_none() && fs::symlink_metadata(&gap_path).is_ok() {
                let unreadable = Unreadable::file(
                    gap_log_file(&target),
                    "the file is too large, not text, or not a regular file",
                );
                if let Some(list) = value["unreadable"].as_array_mut() {
                    list.push(unreadable.to_json());
                }
            }
            Some(value)
        })
        .collect()
}

pub(crate) fn read_small(path: &Path) -> Option<String> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_PLAN_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

/// The test date a plan states in its Facts table's When row, if any.
pub(crate) fn plan_test_date(root: &Path, plan: &str) -> Option<NaiveDate> {
    let text = read_small(&root.join("Study").join(plan).join("plan.md"))?;
    let facts = Table::parse(section(&text, "Facts"));
    facts
        .rows
        .iter()
        .find(|row| {
            facts
                .cell(row, "Item")
                .is_some_and(|c| c.eq_ignore_ascii_case("When"))
        })
        .and_then(|row| facts.cell(row, "Value"))
        .filter(|value| !is_placeholder(value))
        .and_then(first_date)
}

fn plan_digest(
    target: &str,
    plan: &str,
    gaps: &str,
    today: NaiveDate,
    sessions: &[Session],
) -> Value {
    let facts = Table::parse(section(plan, "Facts"));
    let fact = |name: &str| {
        facts
            .rows
            .iter()
            .find(|row| {
                facts
                    .cell(row, "Item")
                    .is_some_and(|c| c.eq_ignore_ascii_case(name))
            })
            .and_then(|row| facts.cell(row, "Value"))
            .filter(|value| !is_placeholder(value))
            .map(str::to_owned)
    };
    let title = fact("Target");
    let when = fact("When");
    let date = when.as_deref().and_then(first_date);
    let plan_blocks = Table::parse(section(plan, "Blocks"));
    let blocks = plan_blocks
        .rows
        .iter()
        .filter_map(|row| plan_blocks.cell(row, "Block"))
        .filter(|block| !is_placeholder(block))
        .collect::<Vec<_>>();
    let gap_table = Table::parse(section(gaps, "Gaps"));
    let GapLog {
        unfixed,
        unreadable,
    } = GapLog::read(&gap_log_file(target), &gap_table);
    let due = unfixed
        .iter()
        .filter(|gap| gap.next_redo.is_some_and(|date| date <= today))
        .collect::<Vec<_>>();
    let first_due = due.iter().min_by_key(|gap| gap.next_redo).map(
        |gap| json!({"paper": gap.paper, "question": gap.question, "more_due": due.len() - 1}),
    );
    let score_rows = Table::parse(section(gaps, "Scores"));
    let scores = score_rows
        .rows
        .iter()
        .filter_map(|row| {
            let paper = score_rows
                .cell(row, "Paper")
                .filter(|p| !is_placeholder(p))?;
            let score = score_rows
                .cell(row, "Score")
                .filter(|s| !is_placeholder(s))?;
            let date = score_rows.cell(row, "Date").filter(|d| !is_placeholder(d));
            Some(json!({"paper": paper, "date": date, "score": score}))
        })
        .collect::<Vec<_>>();
    let session = sessions
        .iter()
        .find(|s| names_target(&s.goal, target, title.as_deref()))
        .map(|s| s.value.clone());
    let planned = sessions::read(&plan_file(target), plan, today);
    json!({
        "target": target,
        "path": plan_file(target),
        "title": title,
        "when": when,
        "date": date.map(|d| d.to_string()),
        "days_left": date.map(|d| (d - today).num_days()),
        "blocks": blocks,
        "unfixed_gaps": unfixed.len(),
        "redos_due": due.len(),
        "first_due": first_due,
        "scores": scores,
        "planned_sessions_today": planned.today,
        "session": session,
        "unreadable": unreadable
            .iter()
            .chain(&planned.unreadable)
            .map(Unreadable::to_json)
            .collect::<Vec<_>>(),
    })
}

pub(crate) fn gap_log_file(target: &str) -> String {
    format!("Study/{target}/gap-log.md")
}

fn plan_file(target: &str) -> String {
    format!("Study/{target}/plan.md")
}

/// Something in a study file that the digest could not read. It is left out of
/// the counts and listed in the plan's `unreadable`, so the help skill can send
/// the tutor to fix it instead of showing numbers that silently skip it.
struct Unreadable {
    file: String,
    section: Option<&'static str>,
    line: Option<usize>,
    reason: String,
}

impl Unreadable {
    fn file(file: String, reason: &str) -> Self {
        Self {
            file,
            section: None,
            line: None,
            reason: reason.to_owned(),
        }
    }

    fn at(file: &str, section: &'static str, line: usize, reason: String) -> Self {
        Self {
            file: file.to_owned(),
            section: Some(section),
            line: Some(line),
            reason,
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "file": self.file,
            "section": self.section,
            "line": self.line,
            "reason": self.reason,
        })
    }
}

fn quoted(cell: &str) -> String {
    match cell.char_indices().nth(MAX_QUOTED_CHARS) {
        Some((end, _)) => format!("\"{}...\"", &cell[..end]),
        None => format!("\"{cell}\""),
    }
}

struct Gap<'a> {
    paper: Option<&'a str>,
    question: Option<&'a str>,
    next_redo: Option<NaiveDate>,
}

/// The gap-log rows the digest can trust, and a note for each it cannot.
struct GapLog<'a> {
    unfixed: Vec<Gap<'a>>,
    unreadable: Vec<Unreadable>,
}

impl<'a> GapLog<'a> {
    /// Rows that are not closed become gaps. A row whose cells do not line up
    /// with the header, or whose Next redo is not a date, is never guessed at,
    /// and neither is a row below a break in the table.
    fn read(file: &str, table: &'a Table) -> Self {
        let mut log = Self::read_rows(file, table);
        for &line in &table.later_rows {
            let reason = "the row comes after a break in the Gaps table, such as a blank line";
            log.unreadable
                .push(Unreadable::at(file, "Gaps", line, reason.to_owned()));
        }
        log
    }

    fn read_rows(file: &str, table: &'a Table) -> Self {
        let mut log = Self {
            unfixed: Vec::new(),
            unreadable: Vec::new(),
        };
        if table.rows.is_empty() {
            return log;
        }
        let note = |line, reason: String| Unreadable::at(file, "Gaps", line, reason);
        let mut cannot_count = false;
        for name in ["Date", "Status"] {
            if table.column(name).is_none() {
                cannot_count = true;
                let reason = format!("the Gaps table has no {name} column");
                log.unreadable.push(note(table.header_line, reason));
            }
        }
        if table.column("Next redo").is_none() {
            let reason = "the Gaps table has no Next redo column, so no redo can be due";
            log.unreadable
                .push(note(table.header_line, reason.to_owned()));
        }
        if cannot_count {
            return log;
        }
        for row in &table.rows {
            if row.cells.len() != table.header.len() {
                let reason = format!(
                    "the row has {} cells but the header has {} columns",
                    row.cells.len(),
                    table.header.len()
                );
                log.unreadable.push(note(row.line, reason));
                continue;
            }
            let cell = |name| table.cell(row, name).filter(|c| !is_placeholder(c));
            if cell("Date").is_none() {
                if cell("Paper").is_some() || cell("Q").is_some() {
                    log.unreadable
                        .push(note(row.line, "the row has no Date".to_owned()));
                }
                continue;
            }
            if cell("Status").is_some_and(says_closed) {
                continue;
            }
            let next_redo = match cell("Next redo") {
                None => None,
                Some(text) => {
                    let Some(date) = first_date(text) else {
                        let reason = format!(
                            "Next redo {} is not a date written as YYYY-MM-DD",
                            quoted(text)
                        );
                        log.unreadable.push(note(row.line, reason));
                        continue;
                    };
                    Some(date)
                }
            };
            log.unfixed.push(Gap {
                paper: cell("Paper"),
                question: cell("Q"),
                next_redo,
            });
        }
        log
    }
}

pub(crate) fn says_closed(status: &str) -> bool {
    let status = status.replace(['*', '_'], "").trim().to_lowercase();
    status == "closed" || status == "3 of 3"
}

/// The body of a `## ` section and the 1-based line of the file it starts on.
pub(crate) struct Section<'a> {
    pub(crate) text: &'a str,
    pub(crate) first_line: usize,
}

/// Body of the first `## ` heading whose text contains `name`, up to the next `## `.
pub(crate) fn section<'a>(text: &'a str, name: &str) -> Section<'a> {
    let mut start = None;
    let mut offset = 0;
    let mut first_line = 0;
    for (index, line) in text.split_inclusive('\n').enumerate() {
        if let Some(heading) = line.strip_prefix("## ") {
            if let Some(start) = start {
                return Section {
                    text: &text[start..offset],
                    first_line,
                };
            }
            if heading.contains(name) {
                start = Some(offset + line.len());
                first_line = index + 2;
            }
        }
        offset += line.len();
    }
    Section {
        text: start.map_or("", |s| &text[s..]),
        first_line,
    }
}

pub(crate) struct Row {
    pub(crate) line: usize,
    pub(crate) cells: Vec<String>,
}

/// The first Markdown table in a section. Columns are found by header name,
/// because the agent may add, drop or reorder columns in its copy of a template.
/// A line that is not a table row ends the table; `later_rows` holds the line
/// of each table row after that, so a reader can report what it did not read.
pub(crate) struct Table {
    pub(crate) header: Vec<String>,
    pub(crate) header_line: usize,
    pub(crate) rows: Vec<Row>,
    pub(crate) later_rows: Vec<usize>,
}

impl Table {
    pub(crate) fn parse(section: Section) -> Self {
        let mut lines = section
            .text
            .lines()
            .enumerate()
            .map(|(index, line)| (section.first_line + index, line.trim()))
            .skip_while(|(_, line)| !line.starts_with('|'));
        let mut rows = lines
            .by_ref()
            .take_while(|(_, line)| line.starts_with('|'))
            .map(|(line, text)| Row {
                line,
                cells: split_cells(text),
            })
            .filter(|row| !is_separator(&row.cells))
            .collect::<Vec<_>>()
            .into_iter();
        let later_rows = lines
            .filter(|(_, text)| text.starts_with('|') && !is_separator(&split_cells(text)))
            .map(|(line, _)| line)
            .collect();
        let header = rows.next();
        Self {
            header_line: header.as_ref().map_or(0, |row| row.line),
            header: header.map(|row| row.cells).unwrap_or_default(),
            rows: rows.collect(),
            later_rows,
        }
    }

    pub(crate) fn column(&self, name: &str) -> Option<usize> {
        self.header
            .iter()
            .position(|h| h.eq_ignore_ascii_case(name))
    }

    /// The cell in `row` under the header `name`, ignoring case.
    pub(crate) fn cell<'a>(&self, row: &'a Row, name: &str) -> Option<&'a str> {
        row.cells.get(self.column(name)?).map(String::as_str)
    }
}

/// The cells of one table line. One leading and one trailing pipe frame the
/// row, and `\|` is a pipe inside a cell, not a separator.
pub(crate) fn split_cells(line: &str) -> Vec<String> {
    let inner = line.strip_prefix('|').unwrap_or(line);
    let inner = match inner.strip_suffix('|') {
        Some(rest) if !rest.ends_with('\\') => rest,
        _ => inner,
    };
    let mut cells = vec![String::new()];
    let mut chars = inner.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek() == Some(&'|') {
            chars.next();
            cells.last_mut().expect("one cell").push('|');
        } else if ch == '|' {
            cells.push(String::new());
        } else {
            cells.last_mut().expect("one cell").push(ch);
        }
    }
    cells.iter().map(|cell| cell.trim().to_owned()).collect()
}

fn is_separator(cells: &[String]) -> bool {
    cells
        .iter()
        .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
}

pub(crate) fn is_placeholder(cell: &str) -> bool {
    cell.is_empty() || cell.starts_with('<')
}

pub(crate) fn first_date(text: &str) -> Option<NaiveDate> {
    text.char_indices()
        .filter_map(|(i, _)| text.get(i..i + 10))
        .find_map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
}

pub(crate) fn names_target(goal: &str, target: &str, title: Option<&str>) -> bool {
    let goal = goal.to_lowercase();
    let spaced = target.replace(['-', '_'], " ").to_lowercase();
    goal.contains(&target.to_lowercase())
        || goal.contains(&spaced)
        || title.is_some_and(|t| !t.is_empty() && goal.contains(&t.to_lowercase()))
}

#[cfg(test)]
mod tests;
