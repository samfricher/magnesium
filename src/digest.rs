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
                serde_json::to_string_pretty(&value).unwrap_or_default()
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
    drop(snapshot);
    let plans = plans(workspace.root(), today, &sessions);
    let recent = sessions
        .iter()
        .take(MAX_RECENT_SESSIONS)
        .map(|s| s.value.clone())
        .collect::<Vec<_>>();
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
fn plans(root: &Path, today: NaiveDate, sessions: &[Session]) -> Vec<Value> {
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

fn read_small(path: &Path) -> Option<String> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_PLAN_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
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
        "path": format!("Study/{target}/plan.md"),
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

fn gap_log_file(target: &str) -> String {
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
    /// with the header, or whose Next redo is not a date, is never guessed at.
    fn read(file: &str, table: &'a Table) -> Self {
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

fn says_closed(status: &str) -> bool {
    let status = status.replace(['*', '_'], "").trim().to_lowercase();
    status == "closed" || status == "3 of 3"
}

/// The body of a `## ` section and the 1-based line of the file it starts on.
struct Section<'a> {
    text: &'a str,
    first_line: usize,
}

/// Body of the first `## ` heading whose text contains `name`, up to the next `## `.
fn section<'a>(text: &'a str, name: &str) -> Section<'a> {
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

struct Row {
    line: usize,
    cells: Vec<String>,
}

/// The first Markdown table in a section. Columns are found by header name,
/// because the agent may add, drop or reorder columns in its copy of a template.
struct Table {
    header: Vec<String>,
    header_line: usize,
    rows: Vec<Row>,
}

impl Table {
    fn parse(section: Section) -> Self {
        let mut lines = section
            .text
            .lines()
            .enumerate()
            .map(|(index, line)| (section.first_line + index, line.trim()))
            .skip_while(|(_, line)| !line.starts_with('|'))
            .take_while(|(_, line)| line.starts_with('|'))
            .map(|(line, text)| Row {
                line,
                cells: split_cells(text),
            })
            .filter(|row| {
                !row.cells
                    .iter()
                    .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
            });
        let header = lines.next();
        Self {
            header_line: header.as_ref().map_or(0, |row| row.line),
            header: header.map(|row| row.cells).unwrap_or_default(),
            rows: lines.collect(),
        }
    }

    fn column(&self, name: &str) -> Option<usize> {
        self.header
            .iter()
            .position(|h| h.eq_ignore_ascii_case(name))
    }

    /// The cell in `row` under the header `name`, ignoring case.
    fn cell<'a>(&self, row: &'a Row, name: &str) -> Option<&'a str> {
        row.cells.get(self.column(name)?).map(String::as_str)
    }
}

/// The cells of one table line. One leading and one trailing pipe frame the
/// row, and `\|` is a pipe inside a cell, not a separator.
fn split_cells(line: &str) -> Vec<String> {
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

fn is_placeholder(cell: &str) -> bool {
    cell.is_empty() || cell.starts_with('<')
}

fn first_date(text: &str) -> Option<NaiveDate> {
    text.char_indices()
        .filter_map(|(i, _)| text.get(i..i + 10))
        .find_map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
}

fn names_target(goal: &str, target: &str, title: Option<&str>) -> bool {
    let goal = goal.to_lowercase();
    let spaced = target.replace(['-', '_'], " ").to_lowercase();
    goal.contains(&target.to_lowercase())
        || goal.contains(&spaced)
        || title.is_some_and(|t| !t.is_empty() && goal.contains(&t.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = "# Midterm 1: study plan\n\n## 1. Facts\n\n| Item | Value |\n|---|---|\n| Target | Midterm 1 |\n| When | 2026-10-23 18:00 |\n| Length | <minutes> |\n\n## 6. Blocks\n\n| # | Block | Purpose |\n|---|---|---|\n| 1 | Diagnostic | find gaps |\n| 2 | Cold redo | close gaps |\n\n## 7. Must know cold\n\n- Km\n";
    const GAPS: &str = "# Gap log\n\n## Gaps\n\n| # | Date | Paper | Status |\n|---|---|---|---|\n| 1 | 2026-10-14 | Diagnostic | open |\n| 2 | 2026-10-14 | Diagnostic | closed |\n| 3 | 2026-10-14 | Diagnostic | Open |\n\n## Scores\n\n| Paper | Date | Score | Time used |\n|---|---|---|---|\n| Diagnostic | 2026-10-14 | 14/30 | 40/45 min |\n| <PAPER> | <YYYY-MM-DD> | <score> | |\n";

    #[test]
    fn plan_digest_reads_facts_blocks_gaps_and_scores() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
        let value = plan_digest("midterm-1", PLAN, GAPS, today, &[]);
        assert_eq!(value["title"], "Midterm 1");
        assert_eq!(value["date"], "2026-10-23");
        assert_eq!(value["days_left"], 7);
        assert_eq!(value["blocks"], json!(["Diagnostic", "Cold redo"]));
        assert_eq!(value["unfixed_gaps"], 2);
        assert_eq!(
            value["scores"],
            json!([{"paper":"Diagnostic","date":"2026-10-14","score":"14/30"}])
        );
        assert_eq!(value["session"], Value::Null);
    }

    #[test]
    fn reshaped_tables_are_read_by_header_name() {
        let plan = "## 1. Facts\n\n| Item | Value |\n|---|---|\n| When | Wed Oct 21, 12:30 |\n\n## 6. Blocks\n\n| # | Day | Block | Status |\n|---|---|---|---|\n| 1 | Fri 16 | Diagnostic | done |\n| 2 | Sat 17 | Cold redo | |\n";
        let gaps = "## Gaps\n\n| # | Date | Paper | Status | Notes |\n|---|---|---|---|---|\n| 1 | 2026-10-15 | Diagnostic | open | teach first |\n| 2 | 2026-10-15 | Diagnostic | closed | |\n\n## Scores\n\n| Date | Paper | Time used | Score |\n|---|---|---|---|\n| 2026-10-15 | Diagnostic | 44/45 min | 18/30 |\n";
        let today = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
        let value = plan_digest("midterm-1", plan, gaps, today, &[]);
        assert_eq!(value["blocks"], json!(["Diagnostic", "Cold redo"]));
        assert_eq!(value["unfixed_gaps"], 1);
        assert_eq!(
            value["scores"],
            json!([{"paper":"Diagnostic","date":"2026-10-15","score":"18/30"}])
        );
        // The papers skill writes the When cell with its ISO date first; other forms give no date.
        assert_eq!(value["when"], "Wed Oct 21, 12:30");
        assert_eq!(value["date"], Value::Null);
        assert_eq!(value["days_left"], Value::Null);
    }

    #[test]
    fn template_placeholders_are_not_facts() {
        let template = include_str!("../claude-plugin/skills/papers/templates/plan.md");
        let gaps = include_str!("../claude-plugin/skills/papers/templates/gap-log.md");
        let today = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
        let value = plan_digest("final", template, gaps, today, &[]);
        assert_eq!(value["title"], Value::Null);
        assert_eq!(value["date"], Value::Null);
        assert_eq!(value["blocks"], json!([]));
        assert_eq!(value["unfixed_gaps"], 0);
        assert_eq!(value["redos_due"], 0);
        assert_eq!(value["first_due"], Value::Null);
        assert_eq!(value["scores"], json!([]));
        assert_eq!(value["planned_sessions_today"], json!([]));
        assert_eq!(value["unreadable"], json!([]));
    }

    #[test]
    fn a_missing_sessions_section_leaves_the_rest_of_the_digest() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let without = plan_digest("midterm-1", PLAN, GAPS, today, &[]);
        assert_eq!(without["planned_sessions_today"], json!([]));
        assert_eq!(without["unfixed_gaps"], 2);
        let plan_items = without["unreadable"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["file"] == "Study/midterm-1/plan.md")
            .count();
        assert_eq!(plan_items, 0);
    }

    #[test]
    fn sessions_lines_it_cannot_read_are_reported_not_treated_as_no_plan() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let mut without = plan_digest("midterm-1", PLAN, GAPS, today, &[]);
        without["unreadable"] = json!([]);
        let plan = format!(
            "{PLAN}\n## 14. Sessions\n\n- If it is Tuesday at 8am, then I do the due cold redos.\n"
        );
        let value = plan_digest("midterm-1", &plan, GAPS, today, &[]);
        let items = value["unreadable"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["file"] == "Study/midterm-1/plan.md")
            .collect::<Vec<_>>();
        assert_eq!(items.len(), 1, "{value}");
        assert_eq!(items[0]["section"], "Sessions");
        let line = plan.lines().position(|line| line.contains("8am")).unwrap() + 1;
        assert_eq!(items[0]["line"], line);
        let mut rest = value.clone();
        rest["unreadable"] = json!([]);
        assert_eq!(rest, without);
    }

    #[test]
    fn unreadable_gap_rows_and_sessions_lines_are_listed_together() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let gaps = "## Gaps\n\n| # | Date | Paper | Next redo | Status |\n|---|---|---|---|---|\n| 1 | 2026-10-14 | Diagnostic |\n";
        let plan = format!(
            "{PLAN}\n## 14. Sessions\n\n- If it is Tuesday at 8am, then I do the due cold redos.\n"
        );
        let value = plan_digest("midterm-1", &plan, gaps, today, &[]);
        let files = value["unreadable"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["file"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            files,
            ["Study/midterm-1/gap-log.md", "Study/midterm-1/plan.md"]
        );
    }

    #[test]
    fn a_gap_is_fixed_only_when_its_status_says_closed() {
        let gaps = "## Gaps\n\n| # | Date | Paper | Q | Next redo | Status |\n|---|---|---|---|---|---|\n\
            | 1 | 2026-10-14 | Diagnostic | Q1 | 2026-10-21 | open |\n\
            | 2 | 2026-10-14 | Diagnostic | Q2 | 2026-10-20 | Open |\n\
            | 3 | 2026-10-14 | Diagnostic | Q3 | 2026-10-19 | 1 of 3 |\n\
            | 4 | 2026-10-14 | Diagnostic | Q6 | | open (failed) |\n\
            | 5 | 2026-10-12 | Paper1-Easy | Q4 | 2026-10-16 | 1 of 3 passes |\n\
            | 6 | 2026-10-12 | Paper1-Easy | Q6 | 2026-10-18 | reopened |\n\
            | 7 | 2026-10-12 | Paper1-Easy | Q7 | 2026-10-25 | 0 of 3 |\n\
            | 8 | 2026-10-12 | Paper1-Easy | Q8 | 2026-10-17 | |\n\
            | 9 | 2026-10-08 | Diagnostic | Q5 | 2026-10-01 | closed |\n\
            | 10 | 2026-10-08 | Diagnostic | Q7 | 2026-10-01 | **Closed** |\n\
            | 11 | 2026-10-08 | Diagnostic | Q8 | 2026-10-01 | CLOSED |\n\
            | 12 | 2026-10-08 | Diagnostic | Q9 | 2026-10-01 | 3 of 3 |\n";
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let value = plan_digest("midterm-1", PLAN, gaps, today, &[]);
        assert_eq!(value["unfixed_gaps"], 8);
        assert_eq!(value["redos_due"], 5);
        assert_eq!(
            value["first_due"],
            json!({"paper": "Paper1-Easy", "question": "Q4", "more_due": 4})
        );
    }

    fn unfixed_for_status(status: &str) -> Value {
        let gaps = format!(
            "## Gaps\n\n| # | Date | Paper | Q | Next redo | Status |\n|---|---|---|---|---|---|\n| 1 | 2026-10-14 | Diagnostic | Q1 | 2026-10-19 | {status} |\n"
        );
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        plan_digest("midterm-1", PLAN, &gaps, today, &[])["unfixed_gaps"].clone()
    }

    #[test]
    fn only_an_exact_closed_or_3_of_3_status_closes_a_gap() {
        for status in [
            "closed",
            "Closed",
            "CLOSED",
            "**Closed**",
            "_closed_",
            "3 of 3",
            "**3 of 3**",
        ] {
            assert_eq!(
                unfixed_for_status(status),
                0,
                "{status:?} should close the gap"
            );
        }
    }

    #[test]
    fn every_other_status_keeps_a_gap_unfixed() {
        for status in [
            "",
            "open",
            "1 of 3",
            "2 of 3",
            "2/3",
            "1 of 3 passes",
            "0 of 3",
            "open (failed)",
            "open (repair pending)",
            "reopened",
            "not closed",
            "isn't closed",
            "~~closed~~",
            "~~closed~~ open",
            "~closed~",
            "<del>closed</del>",
            "closed, then failed 2026-10-24",
            "1 of 3 done",
            "2/3 done",
            "1 of 3 (repair done)",
            "2 of 3 (3 of 3 due 2026-10-25)",
            "Closed ✓",
            "closed by MockA Q2",
            "done",
            "3/3",
            "3 of 3.",
            "3 of 3 passes",
            "3 of 3 (closed)",
            "3 out of 3",
            "~~2 of 3~~ closed",
        ] {
            assert_eq!(
                unfixed_for_status(status),
                1,
                "{status:?} should leave the gap unfixed"
            );
        }
    }

    #[test]
    fn an_upgraded_old_log_keeps_its_overdue_row_due() {
        let gaps = "## Gaps\n\n| # | Date | Paper | Q | Tag | What went wrong | Type | Repair done | Passes | Next redo | Status |\n|---|---|---|---|---|---|---|---|---|---|---|\n\
            | 1 | 2026-10-01 | Diagnostic | Q1 | km | sign error | A | tighten | | 2026-10-03 | OPEN |\n\
            | 2 | 2026-10-01 | Diagnostic | Q2 | vmax | no units | N | tighten | | | closed |\n\
            | 3 | 2026-10-08 | MockA | Q4 | inhibition | wrong type | K | repair | | 2026-10-25 | open |\n";
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let value = plan_digest("final", PLAN, gaps, today, &[]);
        assert_eq!(value["unfixed_gaps"], 2);
        assert_eq!(value["redos_due"], 1);
        assert_eq!(
            value["first_due"],
            json!({"paper": "Diagnostic", "question": "Q1", "more_due": 0})
        );
    }

    #[test]
    fn redos_due_counts_unfixed_rows_whose_next_redo_has_come() {
        let gaps = "## Gaps\n\n| Status | # | Q | Paper | NEXT REDO | Date |\n|---|---|---|---|---|---|\n\
            | open | 1 | Q2 | Diagnostic | 2026-10-21 | 2026-10-14 |\n\
            | 1 of 3 | 2 | Q4 | Diagnostic | 2026-10-18 | 2026-10-14 |\n\
            | 2 of 3 | 3 | Q1 | Paper1-Easy | 2026-10-20 (Tue) | 2026-10-16 |\n\
            | closed | 4 | Q5 | Diagnostic | 2026-10-01 | 2026-10-14 |\n\
            | open | 5 | Q6 | Diagnostic | | 2026-10-14 |\n\
            | open | 6 | Q2 | MockA | 2026-10-18 | 2026-10-17 |\n";
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let value = plan_digest("midterm-1", PLAN, gaps, today, &[]);
        assert_eq!(value["unfixed_gaps"], 5);
        assert_eq!(value["redos_due"], 3);
        assert_eq!(
            value["first_due"],
            json!({"paper": "Diagnostic", "question": "Q4", "more_due": 2})
        );
    }

    #[test]
    fn a_conf_column_and_conf_counts_leave_the_digest_unchanged() {
        let with_conf = "## Gaps\n\n| # | Date | Paper | Q | Tag | What went wrong | Type | Conf | Repair done | Passes | Next redo | Status |\n|---|---|---|---|---|---|---|---|---|---|---|---|\n\
            | 1 | 2026-10-14 | Diagnostic | Q1 | km | sign error | A | S | tighten | | 2026-10-16 | open |\n\
            | 2 | 2026-10-14 | Diagnostic | Q2 | vmax | no units | N | | tighten | 2026-10-16 pass | 2026-10-19 | 1 of 3 |\n\
            | 3 | 2026-10-14 | Diagnostic | Q4 | inhibition | wrong type | K | G | repair | 2026-10-15 pass, 2026-10-18 pass, 2026-10-25 pass | | closed |\n\
            | 4 | 2026-10-14 | Diagnostic | Q5 | lb-plot | axes swapped | K | U | repair | 2026-10-17 fail | | open |\n\n\
            ## Scores\n\n| Paper | Date | Score | Time used | Notes |\n|---|---|---|---|---|\n\
            | Diagnostic | 2026-10-14 | 18/30 | 44/45 min | Q1 2/4; Conf right: S 9/12, U 4/6, G 1/2 |\n";
        let without_conf = "## Gaps\n\n| # | Date | Paper | Q | Tag | What went wrong | Type | Repair done | Passes | Next redo | Status |\n|---|---|---|---|---|---|---|---|---|---|---|\n\
            | 1 | 2026-10-14 | Diagnostic | Q1 | km | sign error | A | tighten | | 2026-10-16 | open |\n\
            | 2 | 2026-10-14 | Diagnostic | Q2 | vmax | no units | N | tighten | 2026-10-16 pass | 2026-10-19 | 1 of 3 |\n\
            | 3 | 2026-10-14 | Diagnostic | Q4 | inhibition | wrong type | K | repair | 2026-10-15 pass, 2026-10-18 pass, 2026-10-25 pass | | closed |\n\
            | 4 | 2026-10-14 | Diagnostic | Q5 | lb-plot | axes swapped | K | repair | 2026-10-17 fail | | open |\n\n\
            ## Scores\n\n| Paper | Date | Score | Time used | Notes |\n|---|---|---|---|---|\n\
            | Diagnostic | 2026-10-14 | 18/30 | 44/45 min | Q1 2/4 |\n";
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let value = plan_digest("midterm-1", PLAN, with_conf, today, &[]);
        assert_eq!(value["unfixed_gaps"], 3);
        assert_eq!(value["redos_due"], 2);
        assert_eq!(
            value["first_due"],
            json!({"paper": "Diagnostic", "question": "Q1", "more_due": 1})
        );
        assert_eq!(
            value["scores"],
            json!([{"paper":"Diagnostic","date":"2026-10-14","score":"18/30"}])
        );
        assert_eq!(
            value,
            plan_digest("midterm-1", PLAN, without_conf, today, &[])
        );
    }

    fn four_gap_rows_in_columns(columns: &[&str]) -> String {
        let names = ["Q", "Type", "Conf", "Passes", "Next redo", "Status"];
        let rows = [
            ["Q1", "A", "S", "", "2026-10-16", "open"],
            ["Q2", "N", "", "2026-10-16 pass", "2026-10-19", "1 of 3"],
            [
                "Q4",
                "K",
                "G",
                "2026-10-15 pass, 2026-10-18 pass, 2026-10-25 pass",
                "",
                "closed",
            ],
            ["Q5", "K", "U", "2026-10-17 fail", "", "open"],
        ];
        let line = |cells: Vec<&str>| format!("| {} |\n", cells.join(" | "));
        let mut text = String::from("## Gaps\n\n");
        text += &line(columns.to_vec());
        text += &line(columns.iter().map(|_| "---").collect());
        for row in rows {
            text += &line(
                columns
                    .iter()
                    .map(|column| match *column {
                        "Date" => "2026-10-14",
                        "Paper" => "Diagnostic",
                        name => row[names.iter().position(|n| *n == name).unwrap()],
                    })
                    .collect(),
            );
        }
        text
    }

    #[test]
    fn a_conf_column_in_any_position_leaves_the_digest_unchanged() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let without = [
            "Date",
            "Paper",
            "Q",
            "Type",
            "Passes",
            "Next redo",
            "Status",
        ];
        let expected = plan_digest(
            "midterm-1",
            PLAN,
            &four_gap_rows_in_columns(&without),
            today,
            &[],
        );
        assert_eq!(expected["unfixed_gaps"], 3);
        assert_eq!(expected["redos_due"], 2);
        assert_eq!(
            expected["first_due"],
            json!({"paper": "Diagnostic", "question": "Q1", "more_due": 1})
        );
        for at in 0..=without.len() {
            let mut columns = without.to_vec();
            columns.insert(at, "Conf");
            assert_eq!(
                plan_digest(
                    "midterm-1",
                    PLAN,
                    &four_gap_rows_in_columns(&columns),
                    today,
                    &[]
                ),
                expected,
                "Conf at column {at}"
            );
        }
    }

    const CONF_COLUMNS: [&str; 8] = [
        "Date",
        "Paper",
        "Q",
        "Type",
        "Conf",
        "Passes",
        "Next redo",
        "Status",
    ];

    fn unreadable_rows(value: &Value) -> Vec<(u64, &str)> {
        value["unreadable"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| {
                assert_eq!(item["file"], "Study/midterm-1/gap-log.md");
                assert_eq!(item["section"], "Gaps");
                (
                    item["line"].as_u64().unwrap(),
                    item["reason"].as_str().unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn a_closed_row_missing_its_conf_cell_is_reported_and_does_not_become_unfixed() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let full = four_gap_rows_in_columns(&CONF_COLUMNS);
        let short = full.replace("| K | G | ", "| K | ");
        assert_ne!(short, full);
        let value = plan_digest("midterm-1", PLAN, &full, today, &[]);
        assert_eq!(value["unfixed_gaps"], 3);
        assert_eq!(value["unreadable"], json!([]));
        let value = plan_digest("midterm-1", PLAN, &short, today, &[]);
        assert_eq!(value["unfixed_gaps"], 3);
        assert_eq!(value["redos_due"], 2);
        let rows = unreadable_rows(&value);
        assert_eq!(rows.len(), 1, "{value}");
        assert_eq!(rows[0].0, 7);
        assert!(
            rows[0].1.contains('7') && rows[0].1.contains('8'),
            "{}",
            rows[0].1
        );
    }

    #[test]
    fn an_open_due_row_missing_its_conf_cell_is_reported_not_silently_dropped() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let full = four_gap_rows_in_columns(&CONF_COLUMNS);
        let short = full.replace("| A | S | ", "| A | ");
        assert_ne!(short, full);
        let value = plan_digest("midterm-1", PLAN, &short, today, &[]);
        let rows = unreadable_rows(&value);
        assert_eq!(rows.len(), 1, "{value}");
        assert_eq!(rows[0].0, 5);
        assert_eq!(value["unfixed_gaps"], 2);
        assert_eq!(value["redos_due"], 1);
        assert_eq!(
            value["first_due"],
            json!({"paper": "Diagnostic", "question": "Q2", "more_due": 0})
        );
    }

    #[test]
    fn rows_with_conf_cells_under_a_header_without_conf_are_all_reported_and_none_counted() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let full = four_gap_rows_in_columns(&CONF_COLUMNS);
        let headless = full.replace("| Type | Conf |", "| Type |");
        assert_ne!(headless, full);
        let value = plan_digest("midterm-1", PLAN, &headless, today, &[]);
        let lines = unreadable_rows(&value)
            .into_iter()
            .map(|(line, _)| line)
            .collect::<Vec<_>>();
        assert_eq!(lines, [5, 6, 7, 8]);
        assert_eq!(value["unfixed_gaps"], 0);
        assert_eq!(value["redos_due"], 0);
        assert_eq!(value["first_due"], Value::Null);
    }

    #[test]
    fn an_old_gap_log_with_a_cold_redo_date_has_unfixed_gaps_and_nothing_due() {
        let gaps = "## Gaps\n\n| # | Date | Paper | Q | Tag | What went wrong | Type | Repair done | Cold-redo date | Status |\n|---|---|---|---|---|---|---|---|---|---|\n\
            | 1 | 2026-10-14 | Diagnostic | Q2 | km | sign error | A | tighten | 2026-10-15 | open |\n\
            | 2 | 2026-10-14 | Diagnostic | Q3 | vmax | no units | N | tighten | 2026-10-15 | closed |\n\
            | 3 | 2026-10-14 | Diagnostic | Q4 | lb-plot | axes swapped | K | repair | 2026-10-16 | open |\n";
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let value = plan_digest("midterm-1", PLAN, gaps, today, &[]);
        assert_eq!(value["unfixed_gaps"], 2);
        assert_eq!(value["redos_due"], 0);
        assert_eq!(value["first_due"], Value::Null);
    }

    const GAP_HEADER: &str = "# Gap log\n\n## Gaps\n\n| # | Date | Paper | Q | Passes | Next redo | Status |\n|---|---|---|---|---|---|---|\n";

    fn digest_of_rows(rows: &[&str]) -> Value {
        let gaps = format!("{GAP_HEADER}{}\n", rows.join("\n"));
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        plan_digest("midterm-1", PLAN, &gaps, today, &[])
    }

    fn unreadable_lines(value: &Value) -> Vec<(&str, u64)> {
        value["unreadable"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| {
                assert_eq!(item["file"], "Study/midterm-1/gap-log.md");
                (
                    item["section"].as_str().unwrap(),
                    item["line"].as_u64().unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn a_readable_gap_log_reports_nothing_unreadable() {
        let value = digest_of_rows(&[
            "| 1 | 2026-10-14 | Diagnostic | Q1 | | 2026-10-19 | open |",
            "| 2 | 2026-10-14 | Diagnostic | Q2 | | | open |",
        ]);
        assert_eq!(value["unreadable"], json!([]));
        assert_eq!(value["unfixed_gaps"], 2);
        assert_eq!(value["redos_due"], 1);
    }

    #[test]
    fn a_next_redo_that_is_not_a_date_is_reported_and_left_out_of_the_counts() {
        let value = digest_of_rows(&[
            "| 1 | 2026-10-14 | Diagnostic | Q1 | | soon | open |",
            "| 2 | 2026-10-14 | Diagnostic | Q2 | | 2026-10-19 | open |",
            "| 3 | 2026-10-14 | Diagnostic | Q3 | | 2026-10-4 | 1 of 3 |",
        ]);
        assert_eq!(unreadable_lines(&value), [("Gaps", 7), ("Gaps", 9)]);
        let reason = value["unreadable"][0]["reason"].as_str().unwrap();
        assert!(
            reason.contains("Next redo") && reason.contains("soon"),
            "{reason}"
        );
        assert_eq!(value["unfixed_gaps"], 1);
        assert_eq!(value["redos_due"], 1);
        assert_eq!(
            value["first_due"],
            json!({"paper": "Diagnostic", "question": "Q2", "more_due": 0})
        );
    }

    #[test]
    fn a_closed_row_is_not_read_for_its_next_redo() {
        let value = digest_of_rows(&["| 1 | 2026-10-14 | Diagnostic | Q1 | | n/a | closed |"]);
        assert_eq!(value["unreadable"], json!([]));
        assert_eq!(value["unfixed_gaps"], 0);
    }

    #[test]
    fn a_row_with_too_few_or_too_many_cells_is_reported_and_left_out_of_the_counts() {
        let value = digest_of_rows(&[
            "| 1 | 2026-10-14 | Diagnostic | Q1 | 2026-10-19 | open |",
            "| 2 | 2026-10-14 | Diagnostic | Q2 | | 2026-10-19 | open |",
            "| 3 | 2026-10-14 | Diagnostic | Q3 | | 2026-10-19 | open | extra |",
            "| 4 | 2026-10-14 | Diagnostic | Q4 | | 2026-10-19 | closed |",
        ]);
        assert_eq!(unreadable_lines(&value), [("Gaps", 7), ("Gaps", 9)]);
        let reason = value["unreadable"][0]["reason"].as_str().unwrap();
        assert!(reason.contains('6') && reason.contains('7'), "{reason}");
        assert_eq!(value["unfixed_gaps"], 1);
        assert_eq!(value["redos_due"], 1);
        assert_eq!(
            value["first_due"],
            json!({"paper": "Diagnostic", "question": "Q2", "more_due": 0})
        );
    }

    #[test]
    fn escaped_pipes_and_an_empty_last_cell_do_not_misalign_a_row() {
        let gaps = "## Gaps\n\n| # | Date | Q | Next redo | Status | Notes |\n|---|---|---|---|---|---|\n\
            | 1 | 2026-10-14 | Q1 | 2026-10-19 | open | a \\| b |\n\
            | 2 | 2026-10-14 | Q2 | 2026-10-19 | open | |\n\
            | 3 | 2026-10-14 | Q3 | 2026-10-19 | open |  |\n\
            | 4 | 2026-10-14 | Q4 | 2026-10-19 | open\n";
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let value = plan_digest("midterm-1", PLAN, gaps, today, &[]);
        assert_eq!(unreadable_lines(&value), [("Gaps", 8)]);
        assert_eq!(value["unfixed_gaps"], 3);
        assert_eq!(value["redos_due"], 3);
    }

    #[test]
    fn a_row_without_a_date_is_reported_unless_it_is_the_template_row() {
        let value = digest_of_rows(&[
            "| 1 | <YYYY-MM-DD> | <PAPER> | <Q> | | <empty until the repair> | open |",
            "| 2 | | Diagnostic | Q2 | | 2026-10-19 | open |",
        ]);
        assert_eq!(unreadable_lines(&value), [("Gaps", 8)]);
        assert_eq!(value["unfixed_gaps"], 0);
        assert_eq!(value["redos_due"], 0);
    }

    #[test]
    fn a_gaps_table_missing_a_column_the_digest_needs_is_reported_once() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let no_status = "# Gap log\n\n## Gaps\n\n| # | Date | Q | Next redo |\n|---|---|---|---|\n| 1 | 2026-10-14 | Q1 | 2026-10-19 |\n| 2 | 2026-10-14 | Q2 | 2026-10-19 |\n";
        let value = plan_digest("midterm-1", PLAN, no_status, today, &[]);
        assert_eq!(unreadable_lines(&value), [("Gaps", 5)]);
        assert!(
            value["unreadable"][0]["reason"]
                .as_str()
                .unwrap()
                .contains("Status")
        );
        assert_eq!(value["unfixed_gaps"], 0);
        assert_eq!(value["redos_due"], 0);

        let no_date = "## Gaps\n\n| # | Q | Next redo | Status |\n|---|---|---|---|\n| 1 | Q1 | 2026-10-19 | open |\n";
        let value = plan_digest("midterm-1", PLAN, no_date, today, &[]);
        assert!(
            value["unreadable"][0]["reason"]
                .as_str()
                .unwrap()
                .contains("Date")
        );
        assert_eq!(value["unfixed_gaps"], 0);
    }

    #[test]
    fn an_old_log_without_a_next_redo_column_is_reported_with_its_gaps_still_counted() {
        let gaps = "## Gaps\n\n| # | Date | Paper | Q | Cold-redo date | Status |\n|---|---|---|---|---|---|\n| 1 | 2026-10-14 | Diagnostic | Q1 | 2026-10-15 | open |\n";
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let value = plan_digest("midterm-1", PLAN, gaps, today, &[]);
        assert_eq!(unreadable_lines(&value), [("Gaps", 3)]);
        assert!(
            value["unreadable"][0]["reason"]
                .as_str()
                .unwrap()
                .contains("Next redo")
        );
        assert_eq!(value["unfixed_gaps"], 1);
        assert_eq!(value["redos_due"], 0);
    }

    #[test]
    fn a_gaps_table_with_no_rows_reports_nothing() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        for gaps in [
            "",
            "## Gaps\n\nNo gaps yet.\n",
            "## Gaps\n\n| # | Date |\n|---|---|\n",
        ] {
            let value = plan_digest("midterm-1", PLAN, gaps, today, &[]);
            assert_eq!(value["unreadable"], json!([]), "{gaps:?}");
        }
    }

    #[test]
    fn the_gap_log_template_does_not_invite_a_date_before_the_repair() {
        let template = include_str!("../claude-plugin/skills/papers/templates/gap-log.md");
        let table = Table::parse(section(template, "Gaps"));
        let row = &table.rows[0];
        for name in ["Passes", "Next redo"] {
            let cell = table.cell(row, name).unwrap();
            assert!(cell.starts_with("<empty until"), "{name}: {cell}");
        }
    }

    #[test]
    fn goals_match_a_target_by_slug_or_title() {
        assert!(names_target("Midterm 1 practice papers", "midterm-1", None));
        assert!(names_target("Study for ch6-7", "ch6-7", None));
        assert!(names_target(
            "BIO final review",
            "final",
            Some("Final exam")
        ));
        assert!(!names_target(
            "Week 4: enzyme kinetics",
            "midterm-1",
            Some("Midterm 1")
        ));
    }

    #[test]
    fn long_text_is_clipped_on_a_character_boundary() {
        let long = "é".repeat(MAX_TEXT_CHARS + 5);
        let clipped = clip(&long);
        assert!(clipped.ends_with("..."));
        assert_eq!(clipped.chars().count(), MAX_TEXT_CHARS + 3);
    }
}
