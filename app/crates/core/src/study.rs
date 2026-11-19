use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::gaplog::{self, Gap, GapLog};
use crate::peek::{self, Check};
use crate::runtime;
use crate::runtime::Step;
use crate::store::{PaperSitting, Store};

#[derive(Clone, Debug, Serialize)]
pub struct Exam {
    pub target: String,
    pub title: String,
    pub date: Option<String>,
    pub day: Option<String>,
    pub days_left: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Last {
    pub days_ago: Option<i64>,
    pub goal: String,
    pub stopped_at: Option<String>,
    pub next_action: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Paper {
    pub target: String,
    pub name: String,
    pub path: String,
    pub minutes: Option<u32>,
    pub sat: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HomeState {
    Ready,
    Empty,
    NotSetUp,
    NoRuntime,
    NeedsUpgrade,
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
    pub checks_due: Option<u32>,
    pub checks_alone: Option<u32>,
    pub redos_due: u32,
    pub opening: Option<peek::Opening>,
    pub first_check: Option<String>,
    pub first_check_id: Option<String>,
    pub posed_check: Option<String>,
    pub can_count_today: Option<u32>,
    pub first_redo: Option<String>,
    pub papers: Vec<Paper>,
    pub can_resume: bool,
    pub sitting: Option<PaperSitting>,
    pub block_minutes: u32,
}

pub fn checks_alone(progress: &Value) -> Option<u32> {
    progress["due"]["checks_alone"]
        .as_u64()
        .map(|n| u32::try_from(n).unwrap_or(u32::MAX))
}

pub fn first_check(progress: &Value) -> Option<String> {
    text(&progress["first_check"]["question"])
}

pub fn can_count_today(progress: &Value, today: NaiveDate) -> Option<u32> {
    let plans = progress["plans"].as_array();
    let other = progress["other"]["skills"].as_array();
    if plans.is_none() && other.is_none() {
        return None;
    }
    let skills = plans
        .into_iter()
        .flatten()
        .filter_map(|plan| plan["skills"].as_array())
        .flatten()
        .chain(other.into_iter().flatten());
    let ready: std::collections::BTreeSet<&str> = skills
        .filter(|skill| skill["proven"].as_bool() != Some(true))
        .filter(|skill| day_of(&skill["next_on"]).is_some_and(|day| day <= today))
        .filter_map(|skill| skill["skill"].as_str())
        .collect();
    Some(ready.len() as u32)
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

pub(crate) fn nearest_plan(plans: &[Value]) -> Option<&Value> {
    plans
        .iter()
        .filter(|plan| plan["days_left"].as_i64().is_some_and(|days| days >= 0))
        .min_by_key(|plan| plan["days_left"].as_i64())
        .or_else(|| plans.iter().find(|plan| plan["days_left"].is_null()))
        .or_else(|| plans.last())
}

pub(crate) fn root_of(digest: &Value, folder: &Path) -> PathBuf {
    text(&digest["study_folder"])
        .map(PathBuf::from)
        .filter(|root| root.is_dir())
        .unwrap_or_else(|| folder.to_path_buf())
}

fn upgrade_needed(digest: &Value) -> bool {
    digest["upgrade_needed"].as_bool() == Some(true)
}

pub fn needs_upgrade(folder: &Path) -> bool {
    runtime::digest(folder).is_ok_and(|digest| upgrade_needed(&digest))
}

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

struct Folder {
    home: Home,
    digest: Option<Value>,
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
        checks_alone: None,
        redos_due: 0,
        opening: None,
        first_check: None,
        first_check_id: None,
        posed_check: None,
        can_count_today: None,
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
            home.message = Some(format!("Magnesium could not read this space: {error}."));
            return;
        }
    };
    if digest["active"].as_bool() != Some(true) {
        home.state = HomeState::NotSetUp;
        home.message = Some(
            "This space isn’t set up for Magnesium yet. Setting it up adds a hidden folder for the record of your answers and leaves your own files as they are."
                .into(),
        );
        return;
    }
    if upgrade_needed(&digest) {
        home.state = HomeState::NeedsUpgrade;
        home.message = Some(
            "This space’s record is in an older form. Upgrade it to study here. Magnesium keeps a copy of your record before it changes it."
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
    home.checks_alone = record.as_ref().and_then(checks_alone);
    home.checks_due = match &record {
        Some(record) => Some(record["due"]["checks"].as_u64().unwrap_or(0) as u32),
        None => peek::checks_due(&root, today()),
    };
    home.opening = peek::opening(
        &root,
        home.checks_due.unwrap_or(0),
        home.redos_due,
        Local::now().timestamp_millis(),
    );
    if home.opening.is_some_and(|opening| opening.checks > 0) {
        home.first_check = record.as_ref().and_then(first_check);
        home.first_check_id = home
            .first_check
            .as_ref()
            .and(record.as_ref())
            .and_then(|record| text(&record["first_check"]["id"]));
    }
    home.posed_check = record
        .as_ref()
        .and_then(|record| text(&record["posed_check"]["id"]));
    home.can_count_today = record
        .as_ref()
        .and_then(|record| can_count_today(record, today()));

    home.last = sessions.first().and_then(|session| {
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Proofs {
    pub earned: u32,
    pub total: u32,
    pub proven: u32,
    pub skills: u32,
    pub level: u32,
    pub to_next: u32,
    pub levelled: bool,
}

impl Proofs {
    fn from_record(progress: &Value) -> Option<Self> {
        let number = |name: &str| progress[name].as_u64().map(|n| n as u32);
        if progress.get("level").is_none() {
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

#[derive(Clone, Debug, Serialize)]
pub struct Card {
    pub folder: String,
    pub name: String,
    pub state: HomeState,
    pub exam: Option<Exam>,
    pub goal: Option<String>,
    pub days_ago: Option<i64>,
    pub proofs: Proofs,
    pub due: u32,
    pub sitting: Option<PaperSitting>,
}

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

fn shelf_order(card: &Card, opened: usize) -> (bool, i64, usize) {
    let days = card
        .exam
        .as_ref()
        .and_then(|exam| exam.days_left)
        .filter(|days| *days >= 0)
        .unwrap_or(i64::MAX);
    (card.sitting.is_none(), days, opened)
}

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

#[derive(Clone, Debug, Serialize)]
pub struct Row {
    pub dots: [u8; 3],
    pub name: String,
    pub what: String,
    pub note: String,
    pub due: bool,
    pub next_day: Option<String>,
    pub needs_repair: bool,
    pub last_answer: Option<String>,
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
    pub helped: Vec<String>,
    pub helped_count: usize,
    pub unreadable: usize,
    pub checks_unknown: bool,
    pub proofs: Proofs,
    pub level_unknown: bool,
    pub today: Option<Today>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Today {
    pub day: String,
    pub answered: u32,
    pub started: u32,
    pub proofs: u32,
    pub due_tomorrow: u32,
}

impl Today {
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
        needs_repair: !gap.closed && !gap.repaired,
        last_answer: None,
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
        needs_repair: false,
        last_answer: None,
        order: check.due,
    }
}

#[derive(Default)]
struct Board {
    confirmed: Vec<Row>,
    in_progress: Vec<Row>,
    untested: Vec<Row>,
    scores: Vec<gaplog::Score>,
    unreadable: usize,
    checks_unknown: bool,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Confirmed,
    InProgress,
    Untested,
}

fn day_of(value: &Value) -> Option<NaiveDate> {
    value.as_str().and_then(crate::activity::parse)
}

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
        needs_repair: !proven && skill["needs_repair"].as_bool() == Some(true),
        last_answer: text(&skill["last_answer"]),
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
                        needs_repair: false,
                        last_answer: None,
                        order: None,
                    };
                    (Place::Untested, row)
                }),
        );
    }
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
            progress.message = Some("An earlier version of Magnesium set this space up. Upgrade it from the space’s page to see your progress.".into());
            return progress;
        }
        Ok(digest) if digest["active"].as_bool() == Some(true) => digest,
        Ok(_) => {
            progress.message = Some("Nothing is proved yet. Progress fills in after your first marked paper or quick check.".into());
            return progress;
        }
        Err(error) => {
            progress.message = Some(format!("Magnesium could not read this space: {error}."));
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
        steps: crate::plan::goal_steps(&root),
        next_action: text(&session["next_action"]).or_else(|| text(&session["current_problem"])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn just_the_checks_is_offered_only_by_a_runtime_that_can_run_it() {
        assert_eq!(
            checks_alone(&serde_json::json!({"due": {"checks": 5, "checks_alone": 3}})),
            Some(3)
        );
        assert_eq!(
            checks_alone(&serde_json::json!({"due": {"checks": 5}})),
            None
        );
    }

    #[test]
    fn the_first_checks_question_is_read_without_anything_beside_it() {
        assert_eq!(
            first_check(&json!({"first_check": {"question": " What is b? ", "key": "-9"}})),
            Some("What is b?".into())
        );
        assert_eq!(first_check(&json!({"first_check": null})), None);
        assert_eq!(first_check(&json!({"due": {"checks": 1}})), None);
    }

    #[test]
    fn skills_count_today_when_their_next_pass_can_be_earned_today() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 17).unwrap();
        let printed = json!({
            "plans": [
                {"plan": "a", "skills": [
                    {"skill": "due today", "proven": false, "next_on": "2026-11-17"},
                    {"skill": "overdue", "proven": false, "next_on": "2026-11-10"},
                    {"skill": "tomorrow", "proven": false, "next_on": "2026-11-18"},
                    {"skill": "never answered", "proven": false, "next_on": null},
                    {"skill": "proven", "proven": true, "next_on": "2026-11-01"},
                ]},
                {"plan": "b", "from": "file"},
            ],
            "other": {"skills": [{"skill": "loose", "proven": false, "next_on": "2026-11-17"}]},
        });
        assert_eq!(can_count_today(&printed, today), Some(3));
        let twice = json!({
            "plans": [
                {"plan": "a", "skills": [{"skill": "pooled variance", "proven": false, "next_on": "2026-11-17"}]},
                {"plan": "b", "skills": [{"skill": "pooled variance", "proven": false, "next_on": "2026-11-17"}]},
            ],
            "other": {"skills": []},
        });
        assert_eq!(can_count_today(&twice, today), Some(1));
        assert_eq!(can_count_today(&json!({"active": true}), today), None);
    }

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

        let proofs = board.proofs.unwrap();
        assert_eq!((proofs.level, proofs.earned, proofs.total), (4, 6, 15));
        assert_eq!((proofs.proven, proofs.skills, proofs.to_next), (1, 5, 1));

        assert_eq!(names(&board.confirmed), ["km"]);
        assert_eq!(board.confirmed[0].note, "confirmed Fri 6 Nov");
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
        let board = record_board(&self::record(), dir.path(), None, today).unwrap();
        assert_eq!(board.proofs.unwrap().level, 4);
    }

    #[test]
    fn a_plan_the_runtime_has_not_read_in_is_read_from_its_file_with_no_level() {
        let dir = tempfile::tempdir().unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let record = record();
        assert!(record_board(&record, dir.path(), Some(&exam("final")), today).is_none());
        assert!(record_board(&record, dir.path(), Some(&exam("other")), today).is_none());

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

    #[test]
    fn only_the_runtime_says_a_skill_waits_for_its_repair() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let skill = |needs: bool, next: Option<&str>| serde_json::json!({"skill": "pooled standard error", "passes": 0, "needs_repair": needs, "next_on": next});
        let (_, waits) = skill_row(&skill(true, None), None, today);
        assert!(waits.needs_repair);
        assert_eq!(waits.next_day, None);
        let (_, dateless) = skill_row(&skill(false, None), None, today);
        assert!(!dateless.needs_repair);
        assert_eq!(dateless.next_day, None);
        let gap = |repaired: bool| Gap {
            paper: "Diagnostic".into(),
            question: "Q3".into(),
            tag: "slope and Km".into(),
            wrong: "read the slope as Km".into(),
            confidence: "S".into(),
            passes: 0,
            earlier_passes: 0,
            failed_on: None,
            last_pass: None,
            next_redo: None,
            closed: false,
            repaired,
        };
        assert!(!gap_row(&gap(true), today).needs_repair);
        assert!(gap_row(&gap(false), today).needs_repair);
    }
}
