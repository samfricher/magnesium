//! What the library, the course page and the progress page show, built from
//! the runtime's digest and the learner's own plan files.
//!
//! Progress counts one thing: an answer the learner reproduced alone on a
//! later day. Each such answer is one proof, and three prove a skill. Minutes,
//! sessions and answers given with help are never counted, so nothing here
//! reads them into a total. The days the learner studied are kept apart, in
//! `activity`, and no proof depends on them.
//!
//! The runtime decides every proof and the level (`magnesium progress`). The
//! window draws them. With an earlier runtime that has no such command, the
//! window reads the gap log as before and shows no level.

use std::collections::{BTreeMap, BTreeSet};
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
pub(crate) fn nearest_plan(plans: &[Value]) -> Option<&Value> {
    plans
        .iter()
        .filter(|plan| plan["days_left"].as_i64().is_some_and(|days| days >= 0))
        .min_by_key(|plan| plan["days_left"].as_i64())
        .or_else(|| plans.iter().find(|plan| plan["days_left"].is_null()))
        .or_else(|| plans.last())
}

/// The root the runtime resolved for this folder, which may be a parent.
pub(crate) fn root_of(digest: &Value, folder: &Path) -> PathBuf {
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
    read_folder(folder, store).home
}

/// A folder as the runtime describes it: the course page, and what its two
/// read-only commands printed, so that no page asks for them twice.
struct Folder {
    home: Home,
    digest: Option<Value>,
    /// What `magnesium progress` printed. None with an earlier runtime.
    record: Option<Value>,
}

fn read_folder(folder: &Path, store: &Store) -> Folder {
    let mut read = Folder {
        home: blank_home(folder, store),
        digest: None,
        record: None,
    };
    fill_home(&mut read, folder, store);
    read
}

fn blank_home(folder: &Path, store: &Store) -> Home {
    let name = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.to_string_lossy().into_owned());
    let remembered = store.folder(folder);
    Home {
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
    }
}

fn fill_home(read: &mut Folder, folder: &Path, store: &Store) {
    let remembered = store.folder(folder);
    let home = &mut read.home;
    if runtime::program().is_none() {
        home.state = HomeState::NoRuntime;
        home.message = Some(
            "The magnesium program is not installed, or the window cannot find it. Install Magnesium, then open the window again."
                .into(),
        );
        return;
    }
    let digest = match runtime::digest(folder) {
        Ok(digest) => digest,
        Err(error) => {
            home.state = HomeState::Unreadable;
            home.message = Some(format!("Magnesium could not read this folder: {error}."));
            return;
        }
    };
    if digest["active"].as_bool() != Some(true) {
        home.state = HomeState::NotSetUp;
        return;
    }
    if upgrade_needed(&digest) {
        home.state = HomeState::NeedsUpgrade;
        home.message = Some(
            "An earlier version of Magnesium set this folder up. Upgrade it to study here with this version. Magnesium keeps a copy of your record in .magnesium/backups before it changes it."
                .into(),
        );
        return;
    }
    let root = root_of(&digest, folder);
    let record = runtime::progress(folder);
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
    // The runtime counts what is due. An earlier runtime leaves the window
    // to look for the quick checks itself, and it cannot always tell.
    home.checks_due = match &record {
        Some(record) => Some(record["due"]["checks"].as_u64().unwrap_or(0) as u32),
        None => peek::checks_due(&root, today()),
    };

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
    read.digest = Some(digest);
    read.record = record;
}

/// How far the proof has come in one course. One proof is one answer the
/// learner gave right, alone and cold, on its own day, and three prove a
/// skill. The runtime works every number here out, the level too. All are 0
/// when the runtime cannot say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Proofs {
    /// Proofs that count now.
    pub earned: u32,
    /// Three for each skill of the course.
    pub total: u32,
    /// Skills with all three proofs.
    pub proven: u32,
    pub skills: u32,
    /// 1 to 10. It moves only when a proof is earned.
    pub level: u32,
    /// Proofs still to earn before the next level. 0 at the top level.
    pub to_next: u32,
    /// Whether the course has a level. A folder with no plan has none: its
    /// skills are counted, as proven and in progress, and no level is given.
    pub levelled: bool,
}

impl Proofs {
    /// The numbers under `progress` in what `magnesium progress` printed.
    fn from_record(progress: &Value) -> Option<Self> {
        let number = |name: &str| progress[name].as_u64().map(|n| n as u32);
        if progress.get("level").is_none() {
            // Skills in no plan: counts, and no level.
            return Some(Self {
                proven: number("proven")?,
                skills: number("skills")?,
                ..Self::default()
            });
        }
        Some(Self {
            levelled: true,
            earned: number("passes")?,
            total: number("of")?,
            proven: number("proven")?,
            skills: number("skills")?,
            level: number("level")?,
            to_next: number("to_next")?,
        })
    }
}

/// One study folder in the library, read without opening it.
#[derive(Clone, Debug, Serialize)]
pub struct Card {
    pub folder: String,
    pub name: String,
    pub state: HomeState,
    pub exam: Option<Exam>,
    /// What the learner last worked on.
    pub goal: Option<String>,
    pub days_ago: Option<i64>,
    pub proofs: Proofs,
    /// Paper redos and quick checks due today or earlier.
    pub due: u32,
    /// A timed paper whose clock is running.
    pub sitting: Option<PaperSitting>,
}

/// What the library shows for one folder. It runs the runtime's two
/// read-only commands once each and reads the plan files. It never starts an
/// agent and never changes the folder.
pub fn card(folder: &Path, store: &Store) -> Card {
    let read = read_folder(folder, store);
    let home = &read.home;
    let mut card = Card {
        folder: home.folder.clone(),
        name: home.name.clone(),
        state: home.state,
        exam: home.exam.clone(),
        goal: home.last.as_ref().map(|last| last.goal.clone()),
        days_ago: home.last.as_ref().and_then(|last| last.days_ago),
        proofs: Proofs::default(),
        due: home.redos_due + home.checks_due.unwrap_or(0),
        sitting: home.sitting.clone(),
    };
    if let (HomeState::Ready, Some(digest)) = (home.state, &read.digest) {
        let root = root_of(digest, folder);
        let plans = digest["plans"].as_array().cloned().unwrap_or_default();
        if let Ok(board) = board(&root, nearest_plan(&plans), read.record.as_ref(), today()) {
            card.proofs = board.proofs.unwrap_or_default();
        }
    }
    card
}

/// The days on which the tutor marked an answer, in any of these folders, as
/// each folder's record says. None when no folder's runtime can say.
pub fn answered_days(folders: &[String]) -> Option<BTreeSet<NaiveDate>> {
    let records: Vec<Option<Value>> = std::thread::scope(|scope| {
        let readers: Vec<_> = folders
            .iter()
            .map(|folder| scope.spawn(move || runtime::progress(Path::new(folder))))
            .collect();
        readers
            .into_iter()
            .map(|reader| reader.join().ok().flatten())
            .collect()
    });
    if records.iter().all(Option::is_none) {
        return None;
    }
    Some(
        records
            .iter()
            .flatten()
            .flat_map(|record| record["days"].as_array().into_iter().flatten())
            .filter_map(|day| day.as_str().and_then(crate::activity::parse))
            .collect(),
    )
}

/// Where a card goes in the library: a running paper first, then the nearest
/// exam that is not past, then the order the folders were last opened in.
fn shelf_order(card: &Card, opened: usize) -> (bool, i64, usize) {
    let days = card
        .exam
        .as_ref()
        .and_then(|exam| exam.days_left)
        .filter(|days| *days >= 0)
        .unwrap_or(i64::MAX);
    (card.sitting.is_none(), days, opened)
}

/// Every folder in the library, most urgent first. Each folder is read on
/// its own thread, so the library takes as long as its slowest folder.
pub fn library(folders: &[String], store: &Store) -> Vec<Card> {
    let mut cards: Vec<(usize, Card)> = std::thread::scope(|scope| {
        let readers: Vec<_> = folders
            .iter()
            .enumerate()
            .map(|(opened, folder)| scope.spawn(move || (opened, card(Path::new(folder), store))))
            .collect();
        readers
            .into_iter()
            .filter_map(|reader| reader.join().ok())
            .collect()
    });
    cards.sort_by_key(|(opened, card)| shelf_order(card, *opened));
    cards.into_iter().map(|(_, card)| card).collect()
}

/// One row of proof. `dots` holds three marks: 2 is a pass that counts, 1 is a
/// pass made before a later failure, 0 is a pass still to make.
#[derive(Clone, Debug, Serialize)]
pub struct Row {
    pub dots: [u8; 3],
    pub name: String,
    pub what: String,
    pub note: String,
    /// Whether the next proof can be earned today.
    pub due: bool,
    /// The day the next proof can be earned, as "2026-11-12".
    pub next_day: Option<String>,
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
    pub proofs: Proofs,
    /// The runtime could not say how far the course is proved, so no level
    /// is shown: it is an earlier version, or it has not read this plan's
    /// gap log into the record yet.
    pub level_unknown: bool,
    /// What the learner did today in this folder. None when the runtime
    /// cannot say: it is an earlier version, or the folder needs an upgrade.
    pub today: Option<Today>,
    pub message: Option<String>,
}

/// What the learner did today in one study folder, as the runtime counts it
/// from the record.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Today {
    /// The day these counts are for, as "2026-11-11".
    pub day: String,
    /// Answers the tutor marked today.
    pub answered: u32,
    /// Skills answered for the first time today.
    pub started: u32,
    /// Proofs earned today. The runtime calls them passes.
    pub proofs: u32,
    /// Quick checks and redos that come due tomorrow.
    pub due_tomorrow: u32,
}

impl Today {
    /// `today` and `today_counts` in what `magnesium progress` printed.
    fn from_record(record: &Value) -> Option<Self> {
        let counts = record.get("today_counts")?;
        let number = |name: &str| counts[name].as_u64().map(|n| n as u32);
        Some(Self {
            day: record["today"].as_str()?.to_owned(),
            answered: number("answered")?,
            started: number("started")?,
            proofs: number("passes")?,
            due_tomorrow: number("due_tomorrow")?,
        })
    }
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

/// The day the next proof can be earned, for a skill that is not proven yet.
fn next_day(closed: bool, next: Option<NaiveDate>) -> Option<String> {
    next.filter(|_| !closed)
        .map(|date| date.format("%Y-%m-%d").to_string())
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
            if gap.repaired { "–" } else { "fix it first" },
        )
    };
    Row {
        dots: dots(gap.passes, gap.earlier_passes),
        name,
        what,
        note,
        due: !gap.closed && gap.next_redo.is_some_and(|date| date <= today),
        next_day: next_day(gap.closed, gap.next_redo),
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
        due: !check.closed && check.due.is_some_and(|date| date <= today),
        next_day: next_day(check.closed, check.due),
        order: check.due,
    }
}

/// The rows of proof for one plan, and the skills of its folder that are in
/// no plan.
#[derive(Default)]
struct Board {
    confirmed: Vec<Row>,
    in_progress: Vec<Row>,
    untested: Vec<Row>,
    scores: Vec<gaplog::Score>,
    unreadable: usize,
    checks_unknown: bool,
    /// The runtime's numbers. None when it could not give them.
    proofs: Option<Proofs>,
}

impl Board {
    fn place(&mut self, rows: Vec<(Place, Row)>) {
        for (place, row) in rows {
            match place {
                Place::Confirmed => self.confirmed.push(row),
                Place::InProgress => self.in_progress.push(row),
                Place::Untested => self.untested.push(row),
            }
        }
        self.in_progress
            .sort_by_key(|row| (!row.due, row.order.is_none(), row.order));
    }
}

/// Which list a row goes in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Confirmed,
    InProgress,
    Untested,
}

fn day_of(value: &Value) -> Option<NaiveDate> {
    value.as_str().and_then(crate::activity::parse)
}

/// One skill as `magnesium progress` printed it, as a row. `covers` is what
/// the course's graph says the skill covers, shown for a skill with nothing
/// else to say.
fn skill_row(skill: &Value, covers: Option<&str>, today: NaiveDate) -> (Place, Row) {
    let passes = skill["passes"].as_u64().unwrap_or(0).min(3) as u8;
    let earlier = skill["earlier_passes"].as_u64().unwrap_or(0).min(3) as u8;
    let proven = skill["proven"].as_bool() == Some(true);
    let next = day_of(&skill["next_on"]).filter(|_| !proven);
    let wrong = text(&skill["what_went_wrong"]);
    let restarted = day_of(&skill["restarted_on"])
        .filter(|_| !proven)
        .map(|day| format!("found before the exam, restarted {}", day_label(day)));
    let what = match (wrong, restarted) {
        (Some(wrong), Some(restarted)) => format!("{wrong} · {restarted}"),
        (Some(wrong), None) => wrong,
        (None, Some(restarted)) => restarted,
        // A skill that was never got wrong says where it was answered, or
        // what it covers.
        (None, None) => text(&skill["why"])
            .or_else(|| covers.map(str::to_owned))
            .unwrap_or_default(),
    };
    let note = if proven {
        day_of(&skill["last_pass_on"]).map_or_else(
            || "confirmed".to_owned(),
            |day| format!("confirmed {}", day_label(day)),
        )
    } else {
        let waits = skill["needs_repair"].as_bool() == Some(true);
        due_note(next, today, if waits { "fix it first" } else { "–" })
    };
    let place = if proven {
        Place::Confirmed
    } else if skill["reading"].as_str() == Some("untested") {
        Place::Untested
    } else {
        Place::InProgress
    };
    let row = Row {
        dots: dots(passes, earlier),
        name: text(&skill["skill"]).unwrap_or_default(),
        what,
        note,
        due: skill["due"].as_bool() == Some(true),
        next_day: next.map(|day| day.format("%Y-%m-%d").to_string()),
        order: next,
    };
    (place, row)
}

fn skill_rows(
    skills: &Value,
    covers: &BTreeMap<String, String>,
    today: NaiveDate,
) -> Vec<(Place, Row)> {
    let plain = |t: &str| t.trim().to_ascii_lowercase();
    skills
        .as_array()
        .into_iter()
        .flatten()
        .map(|skill| {
            let name = skill["skill"].as_str().unwrap_or_default();
            skill_row(skill, covers.get(&plain(name)).map(String::as_str), today)
        })
        .collect()
}

/// The rows as the runtime's record gives them: the plan's skills, then the
/// skills in no plan. None when the record cannot speak for this plan,
/// because its gap log is still a file the runtime has not read in.
fn record_board(
    record: &Value,
    root: &Path,
    exam: Option<&Exam>,
    today: NaiveDate,
) -> Option<Board> {
    let mut board = Board::default();
    let mut covers = BTreeMap::new();
    let progress = match exam {
        Some(exam) => {
            let plan = record["plans"]
                .as_array()?
                .iter()
                .find(|plan| plan["plan"].as_str() == Some(exam.target.as_str()))?;
            if plan["from"].as_str() != Some("record") {
                return None;
            }
            let graph =
                fs::read_to_string(root.join("Study").join(&exam.target).join("graph.yaml"))
                    .unwrap_or_default();
            covers = gaplog::read_tags(&graph)
                .into_iter()
                .map(|(tag, covers)| (tag.trim().to_ascii_lowercase(), covers))
                .filter(|(_, covers)| !covers.is_empty())
                .collect();
            // The papers' scores are the tutor's own table in the gap log.
            board.scores = gap_log(root, &exam.target).scores;
            board.place(skill_rows(&plan["skills"], &covers, today));
            &plan["progress"]
        }
        None => &record["other"]["progress"],
    };
    board.place(skill_rows(&record["other"]["skills"], &covers, today));
    board.proofs = Proofs::from_record(progress);
    Some(board)
}

/// Read the rows of proof. The runtime's record gives them when it can. With
/// an earlier runtime, or a plan whose gap log the runtime has not read in,
/// they come from the gap log on disk, and no level is given. An error is the
/// sentence to show in place of the rows: the gap log on disk does not say
/// what the learner record says.
fn board(
    root: &Path,
    plan: Option<&Value>,
    record: Option<&Value>,
    today: NaiveDate,
) -> Result<Board, String> {
    let exam = plan.and_then(exam_of);
    if let Some(board) = record.and_then(|record| record_board(record, root, exam.as_ref(), today))
    {
        return Ok(board);
    }
    let mut board = Board::default();
    let mut rows: Vec<(Place, Row)> = Vec::new();
    let place = |closed: bool| {
        if closed {
            Place::Confirmed
        } else {
            Place::InProgress
        }
    };
    if let (Some(exam), Some(plan)) = (&exam, plan) {
        let log = gap_log(root, &exam.target);
        if !matches_record(plan, &log, today) {
            return Err(format!(
                "The gap log for {} does not match your record, most likely because it was edited by hand. Ask the tutor to fix your plan files, then look again.",
                exam.title
            ));
        }
        board.unreadable = log.unreadable;
        board.scores = log.scores.clone();
        rows.extend(
            log.gaps
                .iter()
                .map(|gap| (place(gap.closed), gap_row(gap, today))),
        );
        let graph = fs::read_to_string(root.join("Study").join(&exam.target).join("graph.yaml"))
            .unwrap_or_default();
        let plain = |t: &str| t.trim().to_ascii_lowercase();
        rows.extend(
            gaplog::read_tags(&graph)
                .into_iter()
                .filter(|(tag, _)| log.gaps.iter().all(|gap| plain(&gap.tag) != plain(tag)))
                .map(|(tag, covers)| {
                    let row = Row {
                        dots: [0; 3],
                        name: tag,
                        what: covers,
                        note: "–".into(),
                        due: false,
                        next_day: None,
                        order: None,
                    };
                    (Place::Untested, row)
                }),
        );
    }
    // Skills in no plan belong to the folder, not to one exam. The record
    // gives them when there is one. An earlier runtime leaves the window to
    // look for the quick checks itself.
    match record {
        Some(record) => rows.extend(skill_rows(
            &record["other"]["skills"],
            &BTreeMap::new(),
            today,
        )),
        None => match peek::checks(root) {
            Some(checks) => rows.extend(
                checks
                    .iter()
                    .map(|check| (place(check.closed), check_row(check, today))),
            ),
            None => board.checks_unknown = true,
        },
    }
    board.place(rows);
    Ok(board)
}

pub fn progress(folder: &Path, target: Option<&str>) -> Progress {
    let mut progress = Progress::default();
    let digest = match runtime::digest(folder) {
        Ok(digest) if upgrade_needed(&digest) => {
            progress.message = Some("An earlier version of Magnesium set this folder up. Upgrade it from the course page to see your progress.".into());
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

    let record = runtime::progress(folder);
    progress.today = record.as_ref().and_then(Today::from_record);
    match board(&root, plan, record.as_ref(), today()) {
        Ok(board) => {
            progress.proofs = board.proofs.unwrap_or_default();
            progress.level_unknown = board.proofs.is_none();
            progress.confirmed = board.confirmed;
            progress.in_progress = board.in_progress;
            progress.untested = board.untested;
            progress.scores = board.scores;
            progress.unreadable = board.unreadable;
            progress.checks_unknown = board.checks_unknown;
        }
        Err(message) => {
            progress.message = Some(message);
            return progress;
        }
    }

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
    /// The task the record holds open, when the record keeps it where the
    /// window can read it (a lesson on Magnesium). None otherwise.
    pub task: Option<String>,
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
        task: peek::open_task(&root),
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

    /// What `magnesium progress` prints for a course with one plan: four
    /// skills in it, and a quick check from chat that is in no plan.
    fn record() -> Value {
        json!({
            "active": true,
            "today": "2026-11-10",
            "plans": [
                {"plan": "midterm-1", "from": "record",
                 "progress": {"passes": 6, "of": 15, "proven": 1, "skills": 5, "level": 4, "to_next": 1},
                 "skills": [
                    {"skill": "km", "passes": 3, "earlier_passes": 0, "proven": true, "reading": "sound",
                     "why": "midterm-1#1 closed", "due": false, "next_on": null, "last_pass_on": "2026-11-06",
                     "restarted_on": null, "needs_repair": false, "what_went_wrong": "read Km as the intercept",
                     "rows": ["midterm-1#1"], "quick_check": false},
                    {"skill": "slope", "passes": 0, "earlier_passes": 2, "proven": false, "reading": "open",
                     "why": "midterm-1#3 open", "due": true, "next_on": "2026-11-10", "last_pass_on": "2026-11-05",
                     "restarted_on": "2026-11-08", "needs_repair": false, "what_went_wrong": "lost the sign of the slope",
                     "rows": ["midterm-1#3"], "quick_check": false},
                    {"skill": "hill", "passes": 0, "earlier_passes": 0, "proven": false, "reading": "open",
                     "why": "midterm-1#4 open, waiting for its repair", "due": false, "next_on": null, "last_pass_on": null,
                     "restarted_on": null, "needs_repair": true, "what_went_wrong": "read n from the wrong axis",
                     "rows": ["midterm-1#4"], "quick_check": false},
                    // Never got wrong: answered right, alone and cold, on two days.
                    {"skill": "units", "passes": 2, "earlier_passes": 0, "proven": false, "reading": "sound",
                     "why": "cold in Paper A Q2", "due": false, "next_on": null, "last_pass_on": "2026-11-09",
                     "restarted_on": null, "needs_repair": false, "what_went_wrong": "", "rows": [], "quick_check": false},
                    {"skill": "ph", "passes": 0, "earlier_passes": 0, "proven": false, "reading": "untested",
                     "why": "", "due": false, "next_on": null, "last_pass_on": null, "restarted_on": null,
                     "needs_repair": false, "what_went_wrong": "", "rows": [], "quick_check": false},
                 ]},
                {"plan": "final", "from": "file"},
            ],
            "other": {
                "progress": {"passes": 1, "of": 3, "proven": 0, "skills": 1, "level": 4, "to_next": 1},
                "skills": [
                    {"skill": "sign of b", "passes": 1, "earlier_passes": 0, "proven": false, "reading": null,
                     "why": "", "due": false, "next_on": "2026-11-13", "last_pass_on": "2026-11-10",
                     "restarted_on": null, "needs_repair": false, "what_went_wrong": "dropped the minus",
                     "rows": [], "quick_check": true},
                ],
            },
            "due": {"redos": 1, "checks": 0},
            "days": ["2026-11-09", "2026-11-10"],
        })
    }

    #[test]
    fn todays_counts_are_the_runtimes_and_absent_from_an_earlier_one() {
        let mut printed = record();
        assert_eq!(
            Today::from_record(&printed),
            None,
            "an earlier runtime prints no counts"
        );
        printed["today_counts"] =
            json!({"answered": 6, "started": 2, "passes": 1, "due_tomorrow": 3});
        assert_eq!(
            Today::from_record(&printed),
            Some(Today {
                day: "2026-11-10".into(),
                answered: 6,
                started: 2,
                proofs: 1,
                due_tomorrow: 3,
            })
        );
        // A count it cannot read gives none, not a zero that is not true.
        printed["today_counts"]["answered"] = json!("six");
        assert_eq!(Today::from_record(&printed), None);
    }

    fn exam(target: &str) -> Exam {
        Exam {
            target: target.into(),
            title: target.into(),
            date: None,
            day: None,
            days_left: None,
        }
    }

    #[test]
    fn the_rows_and_the_level_are_the_runtimes_and_every_skill_has_a_row() {
        let dir = tempfile::tempdir().unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let board = record_board(&record(), dir.path(), Some(&exam("midterm-1")), today).unwrap();
        let names = |rows: &[Row]| rows.iter().map(|row| row.name.clone()).collect::<Vec<_>>();

        // The level is the runtime's, over every skill of the plan.
        let proofs = board.proofs.unwrap();
        assert_eq!((proofs.level, proofs.earned, proofs.total), (4, 6, 15));
        assert_eq!((proofs.proven, proofs.skills, proofs.to_next), (1, 5, 1));

        assert_eq!(names(&board.confirmed), ["km"]);
        assert_eq!(board.confirmed[0].note, "confirmed Fri 6 Nov");
        // What is due comes first. A skill never got wrong is in progress
        // once it has a proof, and a quick check from chat is here too.
        assert_eq!(
            names(&board.in_progress),
            ["slope", "sign of b", "hill", "units"]
        );
        assert_eq!(names(&board.untested), ["ph"]);
        assert!(!board.checks_unknown);

        let slope = &board.in_progress[0];
        assert!(slope.due);
        assert_eq!(
            slope.dots,
            [1, 1, 0],
            "the passes before the miss are history"
        );
        assert_eq!(slope.note, "due today");
        assert_eq!(
            slope.what,
            "lost the sign of the slope · found before the exam, restarted Sun 8 Nov"
        );
        let check = &board.in_progress[1];
        assert_eq!(
            (check.dots, check.note.as_str()),
            ([2, 0, 0], "next Fri 13 Nov")
        );
        assert_eq!(check.next_day.as_deref(), Some("2026-11-13"));
        assert_eq!(board.in_progress[2].note, "fix it first");
        let units = &board.in_progress[3];
        assert_eq!(
            (units.dots, units.what.as_str()),
            ([2, 2, 0], "cold in Paper A Q2")
        );
    }

    #[test]
    fn a_course_with_no_plan_shows_the_skills_in_no_plan_and_their_counts() {
        let dir = tempfile::tempdir().unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let mut record = record();
        record["other"]["progress"] = json!({"skills": 1, "proven": 0, "in_progress": 1});
        let board = record_board(&record, dir.path(), None, today).unwrap();
        assert_eq!(board.in_progress.len(), 1);
        assert_eq!(board.in_progress[0].name, "sign of b");
        let proofs = board.proofs.unwrap();
        assert!(!proofs.levelled, "{proofs:?}");
        assert_eq!((proofs.skills, proofs.proven, proofs.total), (1, 0, 0));
        // A runtime from before #48 gave the skills in no plan a level.
        let board = record_board(&self::record(), dir.path(), None, today).unwrap();
        assert_eq!(board.proofs.unwrap().level, 4);
    }

    #[test]
    fn a_plan_the_runtime_has_not_read_in_is_read_from_its_file_with_no_level() {
        let dir = tempfile::tempdir().unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let record = record();
        // The record cannot speak for this plan, or for one it does not list.
        assert!(record_board(&record, dir.path(), Some(&exam("final")), today).is_none());
        assert!(record_board(&record, dir.path(), Some(&exam("other")), today).is_none());

        // The gap log on disk is read instead. No level is given, and the
        // skills in no plan still come from the record.
        let study = dir.path().join("Study/final");
        fs::create_dir_all(&study).unwrap();
        fs::write(
            study.join("gap-log.md"),
            "## Gaps\n\n| # | Tag | What went wrong | Passes | Next redo | Status |\n|---|---|---|---|---|---|\n\
             | 1 | limits | divided by zero | 2026-11-08 pass | 2026-11-11 | 1 of 3 |\n",
        )
        .unwrap();
        let plan = json!({"target": "final", "title": "Final"});
        let board = board(dir.path(), Some(&plan), Some(&record), today).unwrap();
        assert!(board.proofs.is_none());
        let names: Vec<&str> = board
            .in_progress
            .iter()
            .map(|row| row.name.as_str())
            .collect();
        assert_eq!(names, ["limits", "sign of b"]);
        assert!(!board.checks_unknown);
    }

    #[test]
    fn the_library_puts_a_running_paper_first_then_the_nearest_exam() {
        let card = |days: Option<i64>, sitting: bool| Card {
            folder: String::new(),
            name: String::new(),
            state: HomeState::Ready,
            exam: Some(Exam {
                target: "exam".into(),
                title: "Exam".into(),
                date: None,
                day: None,
                days_left: days,
            }),
            goal: None,
            days_ago: None,
            proofs: Proofs::default(),
            due: 0,
            sitting: sitting.then(PaperSitting::default),
        };
        let mut cards = [
            (0, card(Some(20), false)),
            (1, card(None, false)),
            (2, card(Some(-2), false)),
            (3, card(Some(3), false)),
            (4, card(Some(40), true)),
        ];
        cards.sort_by_key(|(opened, card)| shelf_order(card, *opened));
        let order: Vec<usize> = cards.iter().map(|(opened, _)| *opened).collect();
        // The paper, the exam in 3 days, the one in 20, then no exam and a
        // past exam in the order they were opened.
        assert_eq!(order, [4, 3, 0, 1, 2]);
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
        assert!(!row.due);
        assert_eq!(row.next_day.as_deref(), Some("2026-11-10"));
        assert!(gap_row(&gap, NaiveDate::from_ymd_opt(2026, 11, 10).unwrap()).due);
        assert_eq!(row.dots, [1, 1, 0]);
        assert!(row
            .what
            .ends_with("found before the exam, restarted Sun 8 Nov"));
        assert_eq!(row.note, "next Tue 10 Nov");
    }
}
