//! Read-only digest of a study folder and its learner record for `/magnesium:help`.
//!
//! Code gathers the facts and does the date arithmetic; the help skill only
//! renders them. Nothing here writes to the folder or the learner store.
use crate::learner::store::LearnerStore;
use crate::workspace::{Workspace, reject_symlink_components};
use chrono::{Datelike, Local, NaiveDate, NaiveTime, TimeZone, Weekday, WeekdaySet};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::process::ExitCode;

const MAX_LISTED_FILES: usize = 60;
const MAX_SCANNED_ENTRIES: usize = 2000;
const MAX_TEXT_CHARS: usize = 300;
const MAX_RECENT_SESSIONS: usize = 3;
const MAX_READY_PURPOSES: usize = 3;
const MAX_PLAN_BYTES: u64 = 256 * 1024;
const SKIPPED_NAMES: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];
const DAY_NAMES: [(&str, Weekday); 7] = [
    ("monday", Weekday::Mon),
    ("tuesday", Weekday::Tue),
    ("wednesday", Weekday::Wed),
    ("thursday", Weekday::Thu),
    ("friday", Weekday::Fri),
    ("saturday", Weekday::Sat),
    ("sunday", Weekday::Sun),
];
const WEEKDAYS: WeekdaySet = WeekdaySet::from_array([
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
]);
const WEEKEND: WeekdaySet = WeekdaySet::from_array([Weekday::Sat, Weekday::Sun]);

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
            let gaps = read_small(&dir.join("gap-log.md")).unwrap_or_default();
            Some(plan_digest(&target, &plan, &gaps, today, sessions))
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
    let unfixed = unfixed_gaps(&gap_table);
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
    let planned_sessions_today = planned_sessions_today(section(plan, "Sessions"), today);
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
        "planned_sessions_today": planned_sessions_today,
        "session": session,
    })
}

fn planned_sessions_today(text: &str, today: NaiveDate) -> Vec<String> {
    let mut times = without_comments(text)
        .lines()
        .filter_map(planned_session)
        .filter(|(days, _)| days.contains(today.weekday()))
        .map(|(_, time)| time.format("%H:%M").to_string())
        .collect::<Vec<_>>();
    times.sort();
    times.dedup();
    times
}

fn without_comments(text: &str) -> String {
    let mut kept = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        kept.push_str(&rest[..start]);
        rest = rest[start..]
            .split_once("-->")
            .map_or("", |(_, after)| after);
    }
    kept + rest
}

fn planned_session(line: &str) -> Option<(WeekdaySet, NaiveTime)> {
    let line = line
        .replace(['*', '_'], "")
        .replace(['–', '—'], "-")
        .to_lowercase();
    let line = line.trim().trim_start_matches('-').trim_start();
    let cue = ["if it is ", "if it's ", "if it’s "]
        .iter()
        .find_map(|start| line.strip_prefix(start))?;
    let (cue, _) = cue.split_once(" then ")?;
    let cue = cue.trim_end_matches(',');
    let (days, time) = match cue.rsplit_once(" at ") {
        Some((days, time)) => (day_set(days)?, time),
        None => (WeekdaySet::ALL, cue),
    };
    Some((days, start_time(time.trim())?))
}

fn start_time(text: &str) -> Option<NaiveTime> {
    match text.split_once(" to ").or_else(|| text.split_once('-')) {
        Some((start, end)) => clock(end.trim()).and(clock(start.trim())),
        None => clock(text),
    }
}

fn clock(text: &str) -> Option<NaiveTime> {
    let (hour, minute) = text.split_once(':')?;
    let digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
    if hour.is_empty() || hour.len() > 2 || minute.len() != 2 || !digits(hour) || !digits(minute) {
        return None;
    }
    NaiveTime::from_hms_opt(hour.parse().ok()?, minute.parse().ok()?, 0)
}

fn day_set(phrase: &str) -> Option<WeekdaySet> {
    let mut days = WeekdaySet::EMPTY;
    for item in phrase
        .split([',', '/', '&'])
        .flat_map(|part| part.split(" and "))
        .flat_map(|part| part.split(" or "))
    {
        let item = item.trim();
        let item = ["every ", "each "]
            .iter()
            .find_map(|start| item.strip_prefix(start))
            .unwrap_or(item);
        days = days.union(match item {
            "" => WeekdaySet::EMPTY,
            "day" | "any day" | "daily" | "everyday" => WeekdaySet::ALL,
            "weekday" | "weekdays" | "a weekday" => WEEKDAYS,
            "weekend" | "weekends" | "the weekend" => WEEKEND,
            _ => match item
                .split_once(" to ")
                .or_else(|| item.split_once(" through "))
                .or_else(|| item.split_once('-'))
            {
                Some((first, last)) => day_range(weekday(first.trim())?, weekday(last.trim())?),
                None => WeekdaySet::single(weekday(item)?),
            },
        });
    }
    (!days.is_empty()).then_some(days)
}

fn weekday(word: &str) -> Option<Weekday> {
    let named = |word: &str| {
        DAY_NAMES
            .iter()
            .find(|(name, _)| word.len() >= 3 && name.starts_with(word))
            .map(|(_, day)| *day)
    };
    named(word).or_else(|| named(word.strip_suffix('s')?))
}

fn day_range(first: Weekday, last: Weekday) -> WeekdaySet {
    std::iter::successors(Some(first), |day| (*day != last).then(|| day.succ())).collect()
}

struct Gap<'a> {
    paper: Option<&'a str>,
    question: Option<&'a str>,
    next_redo: Option<NaiveDate>,
}

fn unfixed_gaps(table: &Table) -> Vec<Gap<'_>> {
    table
        .rows
        .iter()
        .filter_map(|row| {
            let cell = |name| table.cell(row, name).filter(|c| !is_placeholder(c));
            cell("Date")?;
            if cell("Status").is_some_and(says_closed) {
                return None;
            }
            Some(Gap {
                paper: cell("Paper"),
                question: cell("Q"),
                next_redo: cell("Next redo").and_then(first_date),
            })
        })
        .collect()
}

fn says_closed(status: &str) -> bool {
    let status = status.replace(['*', '_'], "").trim().to_lowercase();
    status == "closed" || status == "3 of 3"
}

/// Body of the first `## ` heading whose text contains `name`, up to the next `## `.
fn section<'a>(text: &'a str, name: &str) -> &'a str {
    let mut start = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if let Some(heading) = line.strip_prefix("## ") {
            if let Some(start) = start {
                return &text[start..offset];
            }
            if heading.contains(name) {
                start = Some(offset + line.len());
            }
        }
        offset += line.len();
    }
    start.map_or("", |s| &text[s..])
}

/// The first Markdown table in `text`. Columns are found by header name,
/// because the agent may add, drop or reorder columns in its copy of a template.
struct Table {
    header: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    fn parse(text: &str) -> Self {
        let mut lines = text
            .lines()
            .map(str::trim)
            .skip_while(|line| !line.starts_with('|'))
            .take_while(|line| line.starts_with('|'))
            .map(|line| {
                line.trim_matches('|')
                    .split('|')
                    .map(|c| c.trim().to_owned())
                    .collect::<Vec<_>>()
            })
            .filter(|cells| {
                !cells
                    .iter()
                    .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
            });
        let header = lines.next().unwrap_or_default();
        Self {
            header,
            rows: lines.collect(),
        }
    }

    /// The cell in `row` under the header `name`, ignoring case.
    fn cell<'a>(&self, row: &'a [String], name: &str) -> Option<&'a str> {
        let index = self
            .header
            .iter()
            .position(|h| h.eq_ignore_ascii_case(name))?;
        row.get(index).map(String::as_str)
    }
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
    }

    fn sessions_on(lines: &str, day: Weekday) -> Value {
        let plan = format!("{PLAN}\n## 14. Sessions\n\n{lines}\n");
        let today = NaiveDate::from_isoywd_opt(2026, 43, day).unwrap();
        plan_digest("midterm-1", &plan, GAPS, today, &[])["planned_sessions_today"].clone()
    }

    const WEEK: [Weekday; 7] = [
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
        Weekday::Sun,
    ];

    #[test]
    fn planned_sessions_today_gives_the_start_times_of_the_lines_that_fall_today() {
        let lines = "- If it is Monday to Friday at 08:00, then I do the due cold redos at my desk before class.\n\
            - If it is Tuesday and Thursday at 18:30, then I sit the next block at the library.\n\
            - If it is Tuesday at 08:00, then I bring the gap log.\n\
            - If I miss the morning, then I do them after dinner.\n";
        assert_eq!(sessions_on(lines, Weekday::Tue), json!(["08:00", "18:30"]));
        assert_eq!(sessions_on(lines, Weekday::Wed), json!(["08:00"]));
        assert_eq!(sessions_on(lines, Weekday::Sat), json!([]));
    }

    #[test]
    fn a_sessions_line_falls_on_each_day_its_days_name() {
        for (days, falls) in [
            ("Monday to Friday", "MTWTF.."),
            ("Mon-Fri", "MTWTF.."),
            ("Mon–Fri", "MTWTF.."),
            ("Friday to Monday", "M...FSS"),
            ("Monday, Wednesday and Friday", "M.W.F.."),
            ("Monday, Wednesday, and Friday", "M.W.F.."),
            ("Tuesday or Thursday", ".T.T..."),
            ("Tue and Thurs", ".T.T..."),
            ("Saturdays", ".....S."),
            ("every Sunday", "......S"),
            ("weekdays", "MTWTF.."),
            ("a weekday", "MTWTF.."),
            ("the weekend", ".....SS"),
            ("Saturday or Sunday", ".....SS"),
            ("every day", "MTWTFSS"),
            ("Monday through Friday", "MTWTF.."),
            ("Mon, Weds and Fri", "M.W.F.."),
            ("each weekday", "MTWTF.."),
            ("everyday", "MTWTFSS"),
        ] {
            let line = format!("- **If it is {days} at 7:15, then** I do the due cold redos.\n");
            for (day, mark) in WEEK.iter().zip(falls.chars()) {
                let expected = if mark == '.' {
                    json!([])
                } else {
                    json!(["07:15"])
                };
                assert_eq!(sessions_on(&line, *day), expected, "{days} on {day}");
            }
        }
        for day in WEEK {
            for line in [
                "If it's 06:45, then I do the due cold redos.\n",
                "If it’s 06:45, then I do the due cold redos.\n",
            ] {
                assert_eq!(
                    sessions_on(line, day),
                    json!(["06:45"]),
                    "a line without days falls every day: {line}"
                );
            }
        }
    }

    #[test]
    fn a_time_range_gives_its_start_time() {
        let lines = "- If it is Tuesday at 07:00 to 08:00, then I do the due cold redos.\n\
            - If it is Tuesday at 12:30-13:30, then I sit a block at the library.\n\
            - If it is Tuesday at 18:00–19:00, then I read the gap log.\n\
            - If it is Tuesday at 8:05, then I bring the key.\n";
        assert_eq!(
            sessions_on(lines, Weekday::Tue),
            json!(["07:00", "08:05", "12:30", "18:00"])
        );
    }

    #[test]
    fn a_session_line_inside_an_html_comment_is_not_read() {
        let lines = "<!-- Old times:\n\
            - If it is Monday at 06:00, then I do the due cold redos.\n\
            -->\n\
            - <!-- If it is Monday at 05:00, then I run. --> If it is Tuesday at 08:00, then I do the due cold redos.\n";
        assert_eq!(sessions_on(lines, Weekday::Mon), json!([]));
        assert_eq!(sessions_on(lines, Weekday::Tue), json!(["08:00"]));
    }

    const UNREADABLE_SESSIONS: &str = "- If it is Monday to Friday except Wednesday at 08:00, then I do the due cold redos.\n\
        - If it is a weekday at 8am, then I do the due cold redos.\n\
        - If it is Monday at 7:30 pm, then I do the due cold redos.\n\
        - If it is Monday at 25:00, then I do the due cold redos.\n\
        - If it is Funday at 08:00, then I do the due cold redos.\n\
        - If it is Monday or when I can at 08:00, then I do the due cold redos.\n\
        - If it is 08:00 on Mondays, then I do the due cold redos.\n\
        - If it is Monday at 08:00 before class, then I do the due cold redos.\n\
        - If it is Monday to Friday at 08:00 I do the due cold redos.\n\
        - Every weekday at 08:00, I do the due cold redos.\n\
        - If I miss Monday at 08:00, then I do them after dinner.\n\
        - If it is <days> at <HH:MM>, then I <what I do> <where>.\n\
        - If it is T at 08:00, then I do the due cold redos.\n\
        - If it is Monday at 8:5, then I do the due cold redos.\n\
        - If it is Monday at 08:5, then I do the due cold redos.\n\
        - If it is Monday at 008:00, then I do the due cold redos.\n\
        - If it is Monday at 08:00 to dinner, then I do the due cold redos.\n";

    #[test]
    fn a_sessions_line_the_code_cannot_read_gives_no_session() {
        for day in WEEK {
            assert_eq!(sessions_on(UNREADABLE_SESSIONS, day), json!([]), "{day}");
        }
    }

    #[test]
    fn an_if_then_line_outside_sessions_is_not_a_session() {
        let checklist =
            "\n## 11. Assessment-day checklist\n\n- If it is 10:00, then I move on to Q6.\n";
        let plan = format!("{PLAN}{checklist}");
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        assert_eq!(
            plan_digest("midterm-1", &plan, GAPS, today, &[])["planned_sessions_today"],
            json!([])
        );
        assert_eq!(
            sessions_on(
                &format!("If it is 08:00, then I do the due cold redos.\n{checklist}"),
                Weekday::Tue
            ),
            json!(["08:00"])
        );
    }

    #[test]
    fn a_missing_or_unreadable_sessions_section_leaves_the_rest_of_the_digest() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let without = plan_digest("midterm-1", PLAN, GAPS, today, &[]);
        assert_eq!(without["planned_sessions_today"], json!([]));
        assert_eq!(without["unfixed_gaps"], 2);
        let unreadable = format!("{PLAN}\n## 14. Sessions\n\n{UNREADABLE_SESSIONS}");
        assert_eq!(
            plan_digest("midterm-1", &unreadable, GAPS, today, &[]),
            without
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

    #[test]
    fn a_closed_row_missing_its_conf_cell_counts_as_unfixed() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let full = four_gap_rows_in_columns(&[
            "Date",
            "Paper",
            "Q",
            "Type",
            "Conf",
            "Passes",
            "Next redo",
            "Status",
        ]);
        let short = full.replace("| K | G | ", "| K | ");
        assert_ne!(short, full);
        assert_eq!(
            plan_digest("midterm-1", PLAN, &full, today, &[])["unfixed_gaps"],
            3
        );
        let value = plan_digest("midterm-1", PLAN, &short, today, &[]);
        assert_eq!(value["unfixed_gaps"], 4);
        assert_eq!(value["redos_due"], 2);
    }

    #[test]
    fn an_open_due_row_missing_its_conf_cell_drops_out_of_redos_due() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let full = four_gap_rows_in_columns(&[
            "Date",
            "Paper",
            "Q",
            "Type",
            "Conf",
            "Passes",
            "Next redo",
            "Status",
        ]);
        let short = full.replace("| A | S | ", "| A | ");
        assert_ne!(short, full);
        let value = plan_digest("midterm-1", PLAN, &short, today, &[]);
        assert_eq!(value["unfixed_gaps"], 3);
        assert_eq!(value["redos_due"], 1);
        assert_eq!(
            value["first_due"],
            json!({"paper": "Diagnostic", "question": "Q2", "more_due": 0})
        );
    }

    #[test]
    fn rows_with_conf_cells_under_a_header_without_conf_report_a_closed_row_as_first_due() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let full = four_gap_rows_in_columns(&[
            "Date",
            "Paper",
            "Q",
            "Type",
            "Conf",
            "Passes",
            "Next redo",
            "Status",
        ]);
        let headless = full.replace("| Type | Conf |", "| Type |");
        assert_ne!(headless, full);
        let value = plan_digest("midterm-1", PLAN, &headless, today, &[]);
        assert_eq!(value["unfixed_gaps"], 4);
        assert_eq!(value["redos_due"], 3);
        assert_eq!(
            value["first_due"],
            json!({"paper": "Diagnostic", "question": "Q4", "more_due": 2})
        );
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
