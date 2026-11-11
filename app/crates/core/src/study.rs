//! What the entrance and the progress page show, built from the runtime's
//! digest and the learner's own plan files.
//!
//! Progress counts one thing: an answer the learner reproduced alone on a
//! later day. Minutes, sessions, streaks and answers given with help are never
//! counted, so nothing here reads them into a total.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::gaplog::{self, Gap, GapLog};
use crate::peek::{self, Check, Step};
use crate::runtime;
use crate::store::{PaperSitting, Store};

#[derive(Clone, Debug, Serialize)]
pub struct Exam {
    pub target: String,
    pub title: String,
    pub date: Option<String>,
    /// The date as "Tue 17 Nov".
    pub day: Option<String>,
    pub days_left: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Last {
    pub days_ago: Option<i64>,
    pub goal: String,
    /// The line the learner saved when they pressed Stop in the window. None
    /// when they have studied since, in a terminal or anywhere else.
    pub stopped_at: Option<String>,
    /// What the tutor's last checkpoint says comes next.
    pub next_action: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Paper {
    pub target: String,
    pub name: String,
    /// The paper's path inside the study root, which can be a parent of the
    /// folder the window opened.
    pub path: String,
    pub minutes: Option<u32>,
    pub sat: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HomeState {
    /// A folder with a record or a plan.
    Ready,
    /// A folder set up for Magnesium with nothing in it yet.
    Empty,
    /// A folder Magnesium has not been set up in.
    NotSetUp,
    /// The `magnesium` program is missing.
    NoRuntime,
    /// An earlier Magnesium set this folder up. The window asks once before
    /// it upgrades the folder.
    NeedsUpgrade,
    /// The runtime could not read the folder.
    Unreadable,
}

#[derive(Clone, Debug, Serialize)]
pub struct Home {
    pub folder: String,
    pub name: String,
    pub state: HomeState,
    pub message: Option<String>,
    pub exam: Option<Exam>,
    pub last: Option<Last>,
    /// None when the window cannot tell how many are due (see `peek`).
    pub checks_due: Option<u32>,
    pub redos_due: u32,
    pub first_redo: Option<String>,
    pub papers: Vec<Paper>,
    pub can_resume: bool,
    pub sitting: Option<PaperSitting>,
    pub block_minutes: u32,
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

pub fn day_label(date: NaiveDate) -> String {
    date.format("%a %-d %b").to_string()
}

fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

fn exam_of(plan: &Value) -> Option<Exam> {
    let target = text(&plan["target"])?;
    let date = text(&plan["date"]);
    Some(Exam {
        title: text(&plan["title"]).unwrap_or_else(|| target.clone()),
        day: date
            .as_deref()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
            .map(day_label),
        date,
        days_left: plan["days_left"].as_i64(),
        target,
    })
}

/// The plan whose date is nearest and not past. A plan with no date is a
/// topic, not an exam, and is chosen only when no dated plan is left.
fn nearest_plan(plans: &[Value]) -> Option<&Value> {
    plans
        .iter()
        .filter(|plan| plan["days_left"].as_i64().is_some_and(|days| days >= 0))
        .min_by_key(|plan| plan["days_left"].as_i64())
        .or_else(|| plans.iter().find(|plan| plan["days_left"].is_null()))
        .or_else(|| plans.last())
}

/// The root the runtime resolved for this folder, which may be a parent.
fn root_of(digest: &Value, folder: &Path) -> PathBuf {
    text(&digest["study_folder"])
        .map(PathBuf::from)
        .filter(|root| root.is_dir())
        .unwrap_or_else(|| folder.to_path_buf())
}

/// Whether the runtime says an earlier version set the folder up. Reading the
/// record then would upgrade it, so the digest holds no record.
fn upgrade_needed(digest: &Value) -> bool {
    digest["upgrade_needed"].as_bool() == Some(true)
}

/// Whether the folder needs an upgrade before a lesson can start in it.
pub fn needs_upgrade(folder: &Path) -> bool {
    runtime::digest(folder).is_ok_and(|digest| upgrade_needed(&digest))
}

/// The study root for a folder: the folder itself, or the parent the runtime
/// resolved it to.
pub fn root(folder: &Path) -> PathBuf {
    runtime::digest(folder)
        .map(|digest| root_of(&digest, folder))
        .unwrap_or_else(|_| folder.to_path_buf())
}

const KEY_WORDS: [&str; 9] = [
    "key",
    "solution",
    "answer",
    "marking",
    "markscheme",
    "mark-scheme",
    "memo",
    "rubric",
    "soln",
];

/// Whether a file is an answer key by its name. The window never opens one.
pub fn looks_like_a_key(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    KEY_WORDS.iter().any(|word| name.contains(word))
        || path.components().any(|part| {
            let part = part.as_os_str().to_string_lossy().to_ascii_lowercase();
            part == "keys" || part == "solutions" || part == "answers"
        })
}

fn papers(root: &Path, target: &str, log: &GapLog) -> Vec<Paper> {
    let plan =
        fs::read_to_string(root.join("Study").join(target).join("plan.md")).unwrap_or_default();
    let facts = gaplog::read_plan(&plan);
    let dir = root.join("Study").join(target).join("papers");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut papers: Vec<Paper> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
                && !looks_like_a_key(path)
        })
        .filter_map(|path| {
            let name = path.file_stem()?.to_string_lossy().into_owned();
            let same = |other: &str| {
                let plain = |t: &str| {
                    t.chars()
                        .filter(|c| c.is_ascii_alphanumeric())
                        .collect::<String>()
                        .to_ascii_lowercase()
                };
                plain(other) == plain(&name)
            };
            Some(Paper {
                target: target.to_owned(),
                minutes: facts
                    .paper_minutes
                    .iter()
                    .find(|(paper, _)| same(paper))
                    .map(|(_, minutes)| *minutes)
                    .or(facts.minutes),
                sat: log.scores.iter().any(|score| same(&score.paper)),
                path: format!(
                    "Study/{target}/papers/{}",
                    path.file_name()?.to_string_lossy()
                ),
                name,
            })
        })
        .collect();
    papers.sort_by(|a, b| a.sat.cmp(&b.sat).then_with(|| a.name.cmp(&b.name)));
    papers
}

/// Whether the gap log on disk says what the learner record says. A runtime
/// that writes the Gaps table itself marks the plan `gap_log_from: record`
/// and counts the record's unfixed and due rows. The table can still differ:
/// a hand edit is kept until the runtime next writes it. Then the file must
/// not be shown, or a row could read as confirmed that the record has not
/// confirmed. An older runtime gives no counts, and its file is the record.
fn matches_record(plan: &Value, log: &GapLog, today: NaiveDate) -> bool {
    if plan["gap_log_from"].as_str() != Some("record") {
        return true;
    }
    let unfixed = log.gaps.iter().filter(|gap| !gap.closed);
    let due = unfixed
        .clone()
        .filter(|gap| gap.next_redo.is_some_and(|date| date <= today))
        .count();
    log.unreadable == 0
        && plan["unfixed_gaps"].as_u64() == Some(unfixed.count() as u64)
        && plan["redos_due"].as_u64() == Some(due as u64)
}

fn gap_log(root: &Path, target: &str) -> GapLog {
    fs::read_to_string(root.join("Study").join(target).join("gap-log.md"))
        .map(|text| gaplog::read_gap_log(&text))
        .unwrap_or_default()
}

pub fn home(folder: &Path, store: &Store) -> Home {
    let name = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.to_string_lossy().into_owned());
    let remembered = store.folder(folder);
    let mut home = Home {
        folder: folder.to_string_lossy().into_owned(),
        name,
        state: HomeState::Ready,
        message: None,
        exam: None,
        last: None,
        checks_due: Some(0),
        redos_due: 0,
        first_redo: None,
        papers: Vec::new(),
        can_resume: remembered.session_id.is_some(),
        sitting: remembered.paper.clone(),
        block_minutes: store.block_minutes(),
    };
    if runtime::program().is_none() {
        home.state = HomeState::NoRuntime;
        home.message = Some(
            "The magnesium program is not installed, or the window cannot find it. Install Magnesium, then open the window again."
                .into(),
        );
        return home;
    }
    let digest = match runtime::digest(folder) {
        Ok(digest) => digest,
        Err(error) => {
            home.state = HomeState::Unreadable;
            home.message = Some(format!("Magnesium could not read this folder: {error}."));
            return home;
        }
    };
    if digest["active"].as_bool() != Some(true) {
        home.state = HomeState::NotSetUp;
        return home;
    }
    if upgrade_needed(&digest) {
        home.state = HomeState::NeedsUpgrade;
        home.message = Some(
            "An earlier version of Magnesium set this folder up. Upgrade it to study here with this version. Magnesium keeps a copy of your record in .magnesium/backups before it changes it."
                .into(),
        );
        return home;
    }
    let root = root_of(&digest, folder);
    let plans = digest["plans"].as_array().cloned().unwrap_or_default();
    let sessions = digest["record"]["sessions"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let plan = nearest_plan(&plans);
    home.exam = plan.and_then(exam_of);
    home.redos_due = plans
        .iter()
        .filter_map(|p| p["redos_due"].as_u64())
        .sum::<u64>() as u32;
    home.first_redo = plans
        .iter()
        .filter(|p| p["redos_due"].as_u64().is_some_and(|due| due > 0))
        .find_map(|p| {
            let first = &p["first_due"];
            let parts: Vec<String> = [text(&first["paper"]), text(&first["question"])]
                .into_iter()
                .flatten()
                .collect();
            (!parts.is_empty()).then(|| parts.join(" "))
        });
    home.checks_due = peek::checks_due(&root, today());

    // The most recently changed lesson is where the learner stopped.
    home.last = sessions.first().and_then(|session| {
        // A stop line saved before the lesson last changed is out of date.
        let stopped_day = remembered
            .stopped_ms
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|utc| utc.with_timezone(&Local).date_naive());
        let last_active = text(&session["last_active"])
            .and_then(|day| NaiveDate::parse_from_str(&day, "%Y-%m-%d").ok());
        let current = match (stopped_day, last_active) {
            (Some(stopped), Some(active)) => stopped >= active,
            _ => true,
        };
        Some(Last {
            goal: text(&session["goal"])?,
            days_ago: session["days_ago"].as_i64(),
            stopped_at: remembered
                .stopped_at
                .clone()
                .filter(|line| current && !line.trim().is_empty()),
            next_action: text(&session["next_action"])
                .or_else(|| text(&session["current_problem"])),
        })
    });
    if let Some(exam) = &home.exam {
        home.papers = papers(&root, &exam.target, &gap_log(&root, &exam.target));
    }
    if sessions.is_empty() && plans.is_empty() {
        home.state = HomeState::Empty;
    }
    home
}

/// Which of the folders the learner has opened to show first: the one whose
/// next exam is nearest. With no exam ahead in any of them, the folder opened
/// most recently. The learner should not have to work out what to open.
pub fn folder_to_open(recent: &[String]) -> Option<String> {
    let nearest = recent
        .iter()
        .take(8)
        .filter_map(|folder| {
            let digest = runtime::digest(Path::new(folder)).ok()?;
            let plans = digest["plans"].as_array()?;
            let days = plans
                .iter()
                .filter_map(|plan| plan["days_left"].as_i64())
                .filter(|days| *days >= 0)
                .min()?;
            Some((days, folder))
        })
        .min_by_key(|(days, _)| *days)
        .map(|(_, folder)| folder.clone());
    nearest.or_else(|| recent.first().cloned())
}

/// One row of proof. `dots` holds three marks: 2 is a pass that counts, 1 is a
/// pass made before a later failure, 0 is a pass still to make.
#[derive(Clone, Debug, Serialize)]
pub struct Row {
    pub dots: [u8; 3],
    pub name: String,
    pub what: String,
    pub note: String,
    /// Sort key: the day the next proof is due.
    #[serde(skip)]
    order: Option<NaiveDate>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Progress {
    pub targets: Vec<Exam>,
    pub exam: Option<Exam>,
    pub confirmed: Vec<Row>,
    pub in_progress: Vec<Row>,
    pub untested: Vec<Row>,
    pub scores: Vec<gaplog::Score>,
    /// Answers given with help. They are listed and never counted.
    pub helped: Vec<String>,
    pub helped_count: usize,
    pub unreadable: usize,
    /// The quick checks are left out because the window cannot tell their
    /// passes (see `peek`).
    pub checks_unknown: bool,
    pub view: String,
    pub message: Option<String>,
}

fn dots(passes: u8, earlier: u8) -> [u8; 3] {
    let mut dots = [0u8; 3];
    let passes = passes.min(3) as usize;
    let earlier = (earlier as usize).min(3 - passes);
    dots[..passes].fill(2);
    dots[passes..passes + earlier].fill(1);
    dots
}

fn due_note(next: Option<NaiveDate>, today: NaiveDate, otherwise: &str) -> String {
    match next {
        Some(date) if date < today => format!("due since {}", day_label(date)),
        Some(date) if date == today => "due today".to_owned(),
        Some(date) => format!("next {}", day_label(date)),
        None => otherwise.to_owned(),
    }
}

fn gap_row(gap: &Gap, today: NaiveDate) -> Row {
    let name = if gap.tag.is_empty() {
        format!("{} {}", gap.paper, gap.question).trim().to_owned()
    } else {
        gap.tag.clone()
    };
    let what = match gap.failed_on {
        Some(date) if !gap.closed => {
            let restart = format!("found before the exam, restarted {}", day_label(date));
            if gap.wrong.is_empty() {
                restart
            } else {
                format!("{} · {restart}", gap.wrong)
            }
        }
        _ => gap.wrong.clone(),
    };
    let note = if gap.closed {
        gap.last_pass.map_or_else(
            || "confirmed".to_owned(),
            |date| format!("confirmed {}", day_label(date)),
        )
    } else {
        due_note(
            gap.next_redo,
            today,
            if gap.repaired { "–" } else { "repair first" },
        )
    };
    Row {
        dots: dots(gap.passes, gap.earlier_passes),
        name,
        what,
        note,
        order: gap.next_redo,
    }
}

fn check_row(check: &Check, today: NaiveDate) -> Row {
    Row {
        dots: dots(check.passes, 0),
        name: check.label.clone(),
        what: check.last_miss.clone().unwrap_or_default(),
        note: if check.closed {
            check.last_pass.map_or_else(
                || "confirmed".to_owned(),
                |date| format!("confirmed {}", day_label(date)),
            )
        } else {
            due_note(check.due, today, "–")
        },
        order: check.due,
    }
}

pub fn progress(folder: &Path, target: Option<&str>, store: &Store) -> Progress {
    let mut progress = Progress {
        view: store.progress_view(),
        ..Progress::default()
    };
    let digest = match runtime::digest(folder) {
        Ok(digest) if upgrade_needed(&digest) => {
            progress.message = Some("An earlier version of Magnesium set this folder up. Upgrade it from the start screen to see your progress.".into());
            return progress;
        }
        Ok(digest) if digest["active"].as_bool() == Some(true) => digest,
        Ok(_) => {
            progress.message = Some("Nothing is proved yet. Progress fills in after your first marked paper or quick check.".into());
            return progress;
        }
        Err(error) => {
            progress.message = Some(format!("Magnesium could not read this folder: {error}."));
            return progress;
        }
    };
    let root = root_of(&digest, folder);
    let plans = digest["plans"].as_array().cloned().unwrap_or_default();
    progress.targets = plans.iter().filter_map(exam_of).collect();
    let plan = target
        .and_then(|wanted| plans.iter().find(|p| p["target"].as_str() == Some(wanted)))
        .or_else(|| nearest_plan(&plans));
    progress.exam = plan.and_then(exam_of);
    let today = today();

    let mut rows: Vec<(bool, Row)> = Vec::new();
    if let (Some(exam), Some(plan)) = (&progress.exam, plan) {
        let log = gap_log(&root, &exam.target);
        if !matches_record(plan, &log, today) {
            progress.message = Some(format!(
                "The gap log for {} does not match your record, most likely because it was edited by hand. Ask the tutor to fix your plan files, then look again.",
                exam.title
            ));
            return progress;
        }
        progress.unreadable = log.unreadable;
        progress.scores = log.scores.clone();
        rows.extend(log.gaps.iter().map(|gap| (gap.closed, gap_row(gap, today))));
        let graph = fs::read_to_string(root.join("Study").join(&exam.target).join("graph.yaml"))
            .unwrap_or_default();
        let plain = |t: &str| t.trim().to_ascii_lowercase();
        progress.untested = gaplog::read_tags(&graph)
            .into_iter()
            .filter(|(tag, _)| log.gaps.iter().all(|gap| plain(&gap.tag) != plain(tag)))
            .map(|(tag, covers)| Row {
                dots: [0; 3],
                name: tag,
                what: covers,
                note: "–".into(),
                order: None,
            })
            .collect();
    }
    // Quick checks from chat belong to the folder, not to one exam.
    match peek::checks(&root) {
        Some(checks) => rows.extend(
            checks
                .iter()
                .map(|check| (check.closed, check_row(check, today))),
        ),
        None => progress.checks_unknown = true,
    }
    for (closed, row) in rows {
        if closed {
            progress.confirmed.push(row);
        } else {
            progress.in_progress.push(row);
        }
    }
    progress
        .in_progress
        .sort_by_key(|row| (row.order.is_none(), row.order));

    if let Some(attempts) = runtime::attempts(folder) {
        let helped: Vec<&Value> = attempts
            .iter()
            .filter(|a| a["help"].as_str().is_some_and(|help| help != "none"))
            .collect();
        progress.helped_count = helped.len();
        for attempt in helped {
            let Some(label) = text(&attempt["label"]).or_else(|| text(&attempt["question"])) else {
                continue;
            };
            if !progress.helped.contains(&label) && progress.helped.len() < 4 {
                progress.helped.push(label);
            }
        }
    }
    if progress.exam.is_none() && progress.confirmed.is_empty() && progress.in_progress.is_empty() {
        progress.message = Some("Nothing is proved yet. Progress fills in after your first marked paper or quick check.".into());
    }
    progress
}

/// What the rail shows beside a lesson.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Rail {
    pub goal: Option<String>,
    pub steps: Vec<Step>,
    pub next_action: Option<String>,
}

pub fn rail(folder: &Path) -> Rail {
    let Ok(digest) = runtime::digest(folder) else {
        return Rail::default();
    };
    if digest["active"].as_bool() != Some(true) {
        return Rail::default();
    }
    let root = root_of(&digest, folder);
    let session = &digest["record"]["sessions"][0];
    Rail {
        goal: text(&session["goal"]),
        steps: peek::goal_steps(&root),
        next_action: text(&session["next_action"]).or_else(|| text(&session["current_problem"])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_nearest_exam_that_is_not_past_is_chosen() {
        let plans = vec![
            json!({"target": "old", "days_left": -3}),
            json!({"target": "final", "days_left": 30}),
            json!({"target": "midterm-1", "days_left": 7, "title": "Midterm 1", "date": "2026-11-17"}),
            json!({"target": "topic", "days_left": null}),
        ];
        let exam = nearest_plan(&plans).and_then(exam_of).unwrap();
        assert_eq!(exam.target, "midterm-1");
        assert_eq!(exam.day.as_deref(), Some("Tue 17 Nov"));
    }

    #[test]
    fn a_folder_of_topics_only_still_has_a_plan() {
        let plans = vec![
            json!({"target": "old", "days_left": -3}),
            json!({"target": "topic", "days_left": null}),
        ];
        assert_eq!(nearest_plan(&plans).unwrap()["target"], "topic");
    }

    #[test]
    fn dots_show_counted_passes_then_history() {
        assert_eq!(dots(2, 0), [2, 2, 0]);
        assert_eq!(dots(0, 2), [1, 1, 0]);
        assert_eq!(dots(1, 3), [2, 1, 1]);
        assert_eq!(dots(3, 2), [2, 2, 2]);
    }

    #[test]
    fn answer_keys_are_recognised_by_name() {
        assert!(looks_like_a_key(Path::new("Study/m/papers/PaperB-key.pdf")));
        assert!(looks_like_a_key(Path::new("Study/m/keys/PaperB.pdf")));
        assert!(looks_like_a_key(Path::new("A3 Solutions.pdf")));
        assert!(!looks_like_a_key(Path::new("Study/m/papers/PaperB.pdf")));
    }

    #[test]
    fn a_failed_redo_reads_as_found_before_the_exam() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let gap = Gap {
            paper: "Diagnostic".into(),
            question: "Q3".into(),
            tag: "slope and Km".into(),
            wrong: "read the slope as Km".into(),
            confidence: "S".into(),
            passes: 0,
            earlier_passes: 2,
            failed_on: NaiveDate::from_ymd_opt(2026, 11, 8),
            last_pass: None,
            next_redo: NaiveDate::from_ymd_opt(2026, 11, 10),
            closed: false,
            repaired: true,
        };
        let row = gap_row(&gap, today);
        assert_eq!(row.dots, [1, 1, 0]);
        assert!(row
            .what
            .ends_with("found before the exam, restarted Sun 8 Nov"));
        assert_eq!(row.note, "next Tue 10 Nov");
    }
}
