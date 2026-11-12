//! `magnesium progress`: each skill's passes, the level they add up to, what
//! is due, and the days with a marked answer.
//!
//! Read-only, like `magnesium digest`. It never upgrades a folder: for a
//! folder an earlier version set up it says so and prints no record.
//!
//! A pass is one answer the learner gave right, alone and cold, on its own
//! day. Three passes prove a skill. Every skill can earn them:
//!
//! - A skill with a practice item (a gap-log row from a paper, or a quick
//!   check from chat) has the item's passes, as the runtime works them out
//!   from the item's events. With several items it is as proved as the
//!   weakest.
//! - A skill with no item was never got wrong. Its passes are the days with
//!   a right, unaided, cold, in-time answer labelled with it, counted since
//!   its last wrong or partly right answer.
//!
//! A study window reads this in place of the Markdown gap log and the
//! runtime's own tables, so what it shows is what the record holds.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::ExitCode;

use chrono::{Local, NaiveDate};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use crate::digest::gap_log_file;
use crate::learner::label_key;
use crate::learner::practice::{Effect, EventKind, Origin, PASSES_TO_CLOSE, local_day, plans};
use crate::learner::store::LearnerStore;
use crate::practice::{Item, snapshot};
use crate::readings::{
    PlanInputs, TagReading, load_attempts, load_imported, read_graph, read_plan,
};
use crate::workspace::{Workspace, reject_symlink_components};

/// The highest level, reached when every skill is proven.
const TOP_LEVEL: u64 = 10;
/// How many days the text form lists.
const DAYS_IN_TEXT: usize = 14;

/// One marked answer, as progress sees it.
#[derive(Clone, Debug)]
struct Answer {
    attempt_id: String,
    key: String,
    label: String,
    day: NaiveDate,
    /// Right, unaided, cold and in time.
    clean: bool,
    /// Wrong or partly right.
    miss: bool,
    declined: bool,
}

fn load_answers(conn: &Connection) -> rusqlite::Result<Vec<Answer>> {
    let mut statement = conn.prepare(
        "SELECT label_key, label, mark, help, cold, slow, answered_at_ms, attempt_id
         FROM attempts ORDER BY answered_at_ms, attempt_no",
    )?;
    statement
        .query_map([], |r| {
            let mark: String = r.get(2)?;
            let alone = r.get::<_, String>(3)? == "none";
            let cold: bool = r.get(4)?;
            let slow: bool = r.get(5)?;
            Ok(Answer {
                attempt_id: r.get(7)?,
                key: r.get(0)?,
                label: r.get(1)?,
                day: local_day(r.get(6)?),
                clean: mark == "right" && alone && cold && !slow,
                miss: mark == "wrong" || mark == "partly",
                declined: mark == "declined",
            })
        })?
        .collect()
}

/// One skill and how far it is proved.
#[derive(Clone, Debug, Default, PartialEq)]
struct Skill {
    name: String,
    /// 0 to 3.
    passes: i64,
    /// Passes made before the last miss. They are history and do not count.
    earlier_passes: i64,
    /// The most passes the skill has held at one time, 0 to 3. A plan's
    /// level counts these, so a miss never lowers it.
    best_passes: i64,
    /// The plan's reading of the tag, and why. A skill in no plan has none.
    reading: Option<&'static str>,
    why: String,
    due: bool,
    next_on: Option<NaiveDate>,
    last_pass_on: Option<NaiveDate>,
    /// The day of the miss that started its passes again.
    restarted_on: Option<NaiveDate>,
    /// A gap-log row that waits for its repair before it can be redone.
    needs_repair: bool,
    what_went_wrong: String,
    /// Its gap-log rows, as the tutor names them: "midterm-1#3".
    rows: Vec<String>,
    quick_check: bool,
}

impl Skill {
    fn proven(&self) -> bool {
        self.passes >= PASSES_TO_CLOSE
    }

    fn to_json(&self) -> Value {
        let day = |day: Option<NaiveDate>| day.map(|day| day.to_string());
        json!({
            "skill": self.name,
            "passes": self.passes,
            "earlier_passes": self.earlier_passes,
            "best_passes": self.best_passes,
            "proven": self.proven(),
            "reading": self.reading,
            "why": self.why,
            "due": self.due,
            "next_on": day(self.next_on),
            "last_pass_on": day(self.last_pass_on),
            "restarted_on": day(self.restarted_on),
            "needs_repair": self.needs_repair,
            "what_went_wrong": self.what_went_wrong,
            "rows": self.rows,
            "quick_check": self.quick_check,
        })
    }
}

/// What one item's events say beyond its passes: the passes made before its
/// last miss, the day of that miss, and the day of its last pass.
fn item_history(item: &Item) -> (i64, Option<NaiveDate>, Option<NaiveDate>) {
    let day_of = |event_no: i64| {
        item.facts
            .events
            .iter()
            .find(|event| event.event_no == event_no)
            .map(|event| event.day)
    };
    let mut running = item.facts.base_passes.clamp(0, PASSES_TO_CLOSE);
    let mut earlier = 0;
    let mut restarted_on = None;
    let mut last_pass_on = item.facts.base_last_pass_on;
    for (event_no, effect) in &item.state.effects {
        match effect {
            Effect::Passed | Effect::Closed => {
                running += 1;
                last_pass_on = day_of(*event_no).or(last_pass_on);
            }
            Effect::Failed | Effect::Restarted => {
                earlier = running;
                running = 0;
                restarted_on = day_of(*event_no);
            }
            // The item was made again, by a new miss with no redo between.
            Effect::Made => running = 0,
            _ => {}
        }
    }
    (
        earlier.clamp(0, PASSES_TO_CLOSE),
        restarted_on,
        last_pass_on,
    )
}

/// The most passes a skill with items has held at one time. Its passes at
/// any moment are its weakest item's, among the items made by then. So the
/// items' events are put in one order, by day and then as they were stored,
/// and the fewest passes is taken after each one.
fn most_passes_held(items: &[&Item]) -> i64 {
    let mut steps: Vec<(NaiveDate, i64, usize, i64)> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let day_of = |event_no: i64| {
            item.facts
                .events
                .iter()
                .find(|event| event.event_no == event_no)
                .map_or(item.facts.logged_on, |event| event.day)
        };
        // What it was brought in with, before any event of its own.
        let first_day = item
            .facts
            .events
            .first()
            .map_or(item.facts.logged_on, |event| event.day);
        steps.push((first_day, i64::MIN, index, item.facts.base_passes));
        for (event_no, passes) in &item.state.passes_after {
            steps.push((day_of(*event_no), *event_no, index, *passes));
        }
    }
    steps.sort_by_key(|(day, event_no, _, _)| (*day, *event_no));
    let mut held: Vec<Option<i64>> = vec![None; items.len()];
    let mut most = 0;
    for (_, _, index, passes) in steps {
        held[index] = Some(passes.clamp(0, PASSES_TO_CLOSE));
        let fewest = held.iter().flatten().min().copied().unwrap_or(0);
        most = most.max(fewest);
    }
    most
}

/// A skill with practice items is as proved as its weakest item. `before`
/// are its answers from before its first item was made, when its passes
/// were counted from its answers.
fn from_items(name: &str, items: &[&Item], before: &[&Answer], today: NaiveDate) -> Skill {
    let weakest = items
        .iter()
        .min_by_key(|item| item.state.passes)
        .expect("a skill built from items has one");
    let (earlier_passes, restarted_on, last_pass_on) = item_history(weakest);
    let open = || items.iter().filter(|item| !item.state.closed());
    let what_went_wrong = items
        .iter()
        .map(|item| item.facts.what_went_wrong.trim())
        .find(|text| !text.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            items
                .iter()
                .find_map(|item| item.state.misses.last().cloned())
        })
        .unwrap_or_default();
    Skill {
        name: name.to_owned(),
        passes: weakest.state.passes.clamp(0, PASSES_TO_CLOSE),
        earlier_passes: if weakest.state.passes == 0 {
            earlier_passes
        } else {
            0
        },
        due: items.iter().any(|item| item.state.due_on(today)),
        next_on: open().filter_map(|item| item.state.next_on).min(),
        last_pass_on,
        restarted_on: restarted_on.filter(|_| weakest.state.passes == 0),
        needs_repair: open().any(|item| {
            item.facts.origin == Origin::Paper
                && item.state.passes == 0
                && item.state.next_on.is_none()
        }),
        what_went_wrong,
        rows: items
            .iter()
            .filter(|item| item.facts.origin == Origin::Paper)
            .map(|item| item.name())
            .collect(),
        quick_check: items.iter().any(|item| item.facts.origin == Origin::Chat),
        best_passes: most_passes_held(items)
            .max(from_answers(name, before).best_passes)
            .max(weakest.state.passes.clamp(0, PASSES_TO_CLOSE)),
        ..Skill::default()
    }
}

/// A skill with no practice item was never got wrong on a paper or in a
/// check. Its passes are the days with a clean answer since its last miss.
fn from_answers(name: &str, answers: &[&Answer]) -> Skill {
    let mut days: BTreeSet<NaiveDate> = BTreeSet::new();
    let mut earlier = 0;
    let mut most = 0;
    let mut restarted_on = None;
    for answer in answers {
        if answer.miss {
            earlier = days.len().min(PASSES_TO_CLOSE as usize) as i64;
            restarted_on = Some(answer.day);
            days.clear();
        } else if answer.clean {
            days.insert(answer.day);
            most = most.max(days.len().min(PASSES_TO_CLOSE as usize) as i64);
        }
    }
    let passes = days.len().min(PASSES_TO_CLOSE as usize) as i64;
    Skill {
        name: name.to_owned(),
        passes,
        best_passes: most,
        earlier_passes: if passes == 0 { earlier } else { 0 },
        last_pass_on: days.last().copied(),
        restarted_on: restarted_on.filter(|_| passes == 0),
        ..Skill::default()
    }
}

/// The level a share of passes stands for, from 1 to 10, and the passes
/// still to earn before the next level. It moves only when a pass is earned.
fn level(passes: u64, of: u64) -> (u64, u64) {
    let passes = passes.min(of);
    let steps = TOP_LEVEL - 1;
    let level = 1 + (passes * steps).checked_div(of).unwrap_or(0);
    let to_next = if of == 0 || level >= TOP_LEVEL {
        0
    } else {
        (level * of).div_ceil(steps) - passes
    };
    (level, to_next)
}

/// A set of skills' passes, proven skills and level. `passes` is what the
/// skills hold now. The level counts each skill's most passes held at one
/// time, `level_passes`, so a failed check never lowers it.
fn total<'a>(skills: impl Iterator<Item = &'a Skill>) -> Value {
    let (mut passes, mut held, mut count, mut proven) = (0u64, 0u64, 0u64, 0u64);
    for skill in skills {
        let now = skill.passes.clamp(0, PASSES_TO_CLOSE);
        passes += now as u64;
        held += skill.best_passes.clamp(now, PASSES_TO_CLOSE) as u64;
        count += 1;
        proven += u64::from(skill.proven());
    }
    let of = count * PASSES_TO_CLOSE as u64;
    let (level, to_next) = level(held, of);
    json!({
        "passes": passes,
        "level_passes": held,
        "of": of,
        "proven": proven,
        "skills": count,
        "level": level,
        "to_next": to_next,
    })
}

/// Skills in no plan: how many are proven and how many are in progress. A
/// list of names that grows with every lesson is no fixed whole, so it gets
/// no level.
fn counts<'a>(skills: impl Iterator<Item = &'a Skill>) -> Value {
    let (mut count, mut proven) = (0u64, 0u64);
    for skill in skills {
        count += 1;
        proven += u64::from(skill.proven());
    }
    json!({
        "skills": count,
        "proven": proven,
        "in_progress": count - proven,
    })
}

/// The skill a key names, from its items when it has any, else its answers.
fn skill_of(name: &str, key: &str, items: &[&Item], answers: &[Answer], today: NaiveDate) -> Skill {
    let own: Vec<&Answer> = answers.iter().filter(|a| a.key == key).collect();
    if items.is_empty() {
        return from_answers(name, &own);
    }
    // The answer that made its first item, and the answers before it.
    let made_by: BTreeSet<&str> = items
        .iter()
        .flat_map(|item| &item.facts.events)
        .filter(|event| event.kind == EventKind::Made)
        .filter_map(|event| event.attempt.as_ref())
        .map(|attempt| attempt.attempt_id.as_str())
        .collect();
    let before = own
        .iter()
        .position(|answer| made_by.contains(answer.attempt_id.as_str()))
        .map_or(&own[..0], |first| &own[..first]);
    from_items(name, items, before, today)
}

struct Record<'a> {
    root: &'a Path,
    today: NaiveDate,
    items: &'a [Item],
    answers: &'a [Answer],
}

/// One plan's skills: every tag of its graph and every label of its rows.
/// None when the plan's gap log is still a file the runtime has not taken
/// over, so its rows are not in the record yet.
fn plan_skills(conn: &Connection, record: &Record<'_>, plan: &str) -> Option<Vec<(String, Skill)>> {
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
    let on_disk = std::fs::symlink_metadata(record.root.join(gap_log_file(plan))).is_ok();
    if !taken && on_disk {
        return None;
    }
    let graph = read_graph(record.root, plan);
    let attempts = load_attempts(conn).ok()?;
    let imported = load_imported(conn).ok()?;
    let readings: Vec<TagReading> = read_plan(&PlanInputs {
        plan,
        graph: graph.as_ref().ok(),
        items: record.items,
        attempts: &attempts,
        imported: &imported,
    });
    Some(
        readings
            .into_iter()
            .map(|reading| {
                let key = label_key(&reading.tag);
                let own: Vec<&Item> = record
                    .items
                    .iter()
                    .filter(|item| label_key(&item.facts.label) == key)
                    .filter(|item| match item.facts.origin {
                        Origin::Paper => item.facts.plan.as_deref() == Some(plan),
                        Origin::Chat => true,
                    })
                    .collect();
                let mut skill = skill_of(&reading.tag, &key, &own, record.answers, record.today);
                skill.reading = Some(reading.reading.as_str());
                skill.why = reading.why;
                (key, skill)
            })
            .collect(),
    )
}

/// The skills that belong to no plan: quick checks, paper rows in no plan,
/// and every other label an answer was marked on.
fn other_skills(record: &Record<'_>, in_a_plan: &BTreeSet<String>) -> Vec<Skill> {
    let mut names: Vec<(String, String)> = Vec::new();
    let mut seen: BTreeSet<String> = in_a_plan.clone();
    let mut add = |label: &str| {
        let key = label_key(label);
        if seen.insert(key.clone()) {
            names.push((key, label.to_owned()));
        }
    };
    for item in record.items {
        if item.facts.origin == Origin::Chat || item.facts.plan.is_none() {
            add(&item.facts.label);
        }
    }
    for answer in record.answers {
        add(&answer.label);
    }
    names
        .into_iter()
        .map(|(key, name)| {
            let own: Vec<&Item> = record
                .items
                .iter()
                .filter(|item| label_key(&item.facts.label) == key)
                .filter(|item| item.facts.origin == Origin::Chat || item.facts.plan.is_none())
                .collect();
            skill_of(&name, &key, &own, record.answers, record.today)
        })
        .collect()
}

/// The days with at least one answer the learner gave, oldest first. A
/// declined question is not an answer.
fn answered_days(answers: &[Answer]) -> Vec<String> {
    answers
        .iter()
        .filter(|answer| !answer.declined)
        .map(|answer| answer.day)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|day| day.to_string())
        .collect()
}

/// The days on which a skill with no item earned its passes: each new day
/// with a clean answer, since its last miss, up to three.
fn answer_pass_days(answers: &[&Answer]) -> Vec<NaiveDate> {
    let mut days: BTreeSet<NaiveDate> = BTreeSet::new();
    let mut earned = Vec::new();
    for answer in answers {
        if answer.miss {
            days.clear();
        } else if answer.clean && days.insert(answer.day) && days.len() <= PASSES_TO_CLOSE as usize
        {
            earned.push(answer.day);
        }
    }
    earned
}

/// What the learner did today, over the whole study folder: answers given
/// (a declined question is not one), skills first answered, passes earned,
/// and checks and redos that first fall due tomorrow.
fn today_counts(items: &[Item], answers: &[Answer], today: NaiveDate) -> Value {
    let given: Vec<&Answer> = answers.iter().filter(|answer| !answer.declined).collect();
    let answered = given.iter().filter(|answer| answer.day == today).count();
    let mut first_day: BTreeMap<&str, NaiveDate> = BTreeMap::new();
    for answer in &given {
        first_day.entry(answer.key.as_str()).or_insert(answer.day);
    }
    let started = first_day.values().filter(|day| **day == today).count();
    let on_items: BTreeSet<String> = items
        .iter()
        .map(|item| label_key(&item.facts.label))
        .collect();
    let item_passes: usize = items
        .iter()
        .map(|item| {
            item.state
                .effects
                .iter()
                .filter(|(_, effect)| matches!(effect, Effect::Passed | Effect::Closed))
                .filter(|(event_no, _)| {
                    item.facts
                        .events
                        .iter()
                        .any(|event| event.event_no == *event_no && event.day == today)
                })
                .count()
        })
        .sum();
    let answer_passes: usize = first_day
        .keys()
        .filter(|key| !on_items.contains(**key))
        .map(|key| {
            let own: Vec<&Answer> = answers.iter().filter(|a| a.key == *key).collect();
            answer_pass_days(&own)
                .into_iter()
                .filter(|day| *day == today)
                .count()
        })
        .sum();
    let tomorrow = today.succ_opt();
    let due_tomorrow = items
        .iter()
        .filter(|item| {
            !item.state.closed() && item.state.next_on.is_some() && item.state.next_on == tomorrow
        })
        .count();
    json!({
        "answered": answered,
        "started": started,
        "passes": item_passes + answer_passes,
        "due_tomorrow": due_tomorrow,
    })
}

fn progress(workspace: &Workspace, now_ms: i64) -> Result<Value, String> {
    let today = local_day(now_ms);
    let id = workspace.id().map_err(|e| e.to_string())?;
    reject_symlink_components(&workspace.store_path(), workspace.control())
        .map_err(|e| e.to_string())?;
    // Opening an earlier version's record upgrades it. This command changes
    // nothing, so it says so and stops, as the digest does.
    if crate::learner::store::needs_upgrade(&workspace.store_path()).map_err(|e| e.to_string())?
        || crate::workspace::instruction_bundle::earlier_bundle_active(workspace.control())
    {
        return Ok(json!({
            "active": true,
            "upgrade_needed": true,
            "today": today.to_string(),
        }));
    }
    let store = LearnerStore::open(&workspace.store_path(), &id).map_err(|e| e.to_string())?;
    let snapshot_of = store
        .conn
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let items = snapshot(&snapshot_of, workspace.root())?;
    let answers = load_answers(&snapshot_of).map_err(|e| e.to_string())?;
    let record = Record {
        root: workspace.root(),
        today,
        items: &items,
        answers: &answers,
    };
    let mut in_a_plan: BTreeSet<String> = BTreeSet::new();
    let mut by_plan: BTreeMap<String, Option<Vec<Skill>>> = BTreeMap::new();
    for plan in plans(workspace.root()) {
        let skills = plan_skills(&snapshot_of, &record, &plan).map(|skills| {
            skills
                .into_iter()
                .map(|(key, skill)| {
                    in_a_plan.insert(key);
                    skill
                })
                .collect()
        });
        by_plan.insert(plan, skills);
    }
    let other = other_skills(&record, &in_a_plan);
    drop(snapshot_of);

    let plans_json: Vec<Value> = by_plan
        .iter()
        .map(|(plan, skills)| match skills {
            // Its rows are in a file the runtime has not read into the
            // record, so nothing true can be said about its skills here.
            None => json!({"plan": plan, "from": "file"}),
            Some(skills) => json!({
                "plan": plan,
                "from": "record",
                "skills": skills.iter().map(Skill::to_json).collect::<Vec<_>>(),
                // A plan's graph is a fixed whole, so its level counts its
                // own skills only, and cannot fall when the learner starts
                // something new.
                "progress": total(skills.iter()),
            }),
        })
        .collect();
    let due = |origin: Origin| {
        items
            .iter()
            .filter(|item| item.facts.origin == origin && item.state.due_on(today))
            .count()
    };
    Ok(json!({
        "active": true,
        "today": today.to_string(),
        "plans": plans_json,
        "other": {
            "skills": other.iter().map(Skill::to_json).collect::<Vec<_>>(),
            "progress": counts(other.iter()),
        },
        "due": {"redos": due(Origin::Paper), "checks": due(Origin::Chat)},
        "days": answered_days(&answers),
        "today_counts": today_counts(&items, &answers, today),
    }))
}

fn dots(skill: &Value) -> String {
    let passes = skill["passes"].as_u64().unwrap_or(0).min(3) as usize;
    format!("{}{}", "●".repeat(passes), "○".repeat(3 - passes))
}

fn skill_line(skill: &Value) -> String {
    let mut line = format!(
        "  {}  {}",
        dots(skill),
        skill["skill"].as_str().unwrap_or("")
    );
    let note = if skill["proven"] == true {
        Some("proven".to_owned())
    } else if skill["due"] == true {
        Some("due".to_owned())
    } else if skill["needs_repair"] == true {
        Some("waits for its repair".to_owned())
    } else {
        skill["next_on"].as_str().map(|day| format!("next {day}"))
    };
    if let Some(note) = note {
        line.push_str(&format!(" · {note}"));
    }
    line.push('\n');
    line
}

fn progress_line(progress: &Value) -> String {
    format!(
        "level {} of {TOP_LEVEL} · {} of {} passes · {} of {} skills proven",
        progress["level"],
        progress["passes"],
        progress["of"],
        progress["proven"],
        progress["skills"]
    )
}

/// The same facts for a person reading a terminal.
fn render(value: &Value) -> String {
    if value["upgrade_needed"] == true {
        return "An earlier version of Magnesium set this folder up. Run `magnesium init --upgrade-instructions` to upgrade it, then ask again.\n".into();
    }
    let mut text = String::new();
    for plan in value["plans"].as_array().into_iter().flatten() {
        let name = plan["plan"].as_str().unwrap_or("");
        if plan["from"] == "file" {
            text.push_str(&format!(
                "{name}: its gap log is not in the record yet. The tutor brings it in at the next marked answer.\n"
            ));
            continue;
        }
        text.push_str(&format!("{name}: {}\n", progress_line(&plan["progress"])));
        for skill in plan["skills"].as_array().into_iter().flatten() {
            text.push_str(&skill_line(skill));
        }
    }
    let other = value["other"]["skills"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if !other.is_empty() {
        let counts = &value["other"]["progress"];
        text.push_str(&format!(
            "In no plan: {} proven, {} in progress\n",
            counts["proven"], counts["in_progress"]
        ));
        for skill in &other {
            text.push_str(&skill_line(skill));
        }
    }
    if text.is_empty() {
        text.push_str("No answers recorded yet.\n");
    }
    let days: Vec<&str> = value["days"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !days.is_empty() {
        let shown = &days[days.len().saturating_sub(DAYS_IN_TEXT)..];
        text.push_str(&format!(
            "Days with a marked answer: {} in all. The latest: {}\n",
            days.len(),
            shown.join(", ")
        ));
    }
    text
}

pub(crate) fn print_progress(as_json: bool) -> ExitCode {
    let result = (|| {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        match Workspace::discover(&cwd).map_err(|e| e.to_string())? {
            Some(workspace) => progress(&workspace, Local::now().timestamp_millis()),
            None => Ok(json!({"active": false, "reason": "workspace_not_initialized"})),
        }
    })();
    match result {
        Ok(value) if as_json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_default()
            );
            ExitCode::SUCCESS
        }
        Ok(value) if value["active"] == true => {
            print!("{}", render(&value));
            ExitCode::SUCCESS
        }
        Ok(_) => {
            eprintln!("magnesium: this folder is not a Magnesium study folder");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("magnesium: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    fn answer(on: &str, mark: &str) -> Answer {
        Answer {
            attempt_id: format!("{on}-{mark}"),
            key: "units".into(),
            label: "units".into(),
            day: day(on),
            clean: mark == "clean",
            miss: mark == "wrong" || mark == "partly",
            declined: mark == "declined",
        }
    }

    fn skill(answers: &[Answer]) -> Skill {
        from_answers("units", &answers.iter().collect::<Vec<_>>())
    }

    #[test]
    fn a_skill_never_got_wrong_earns_a_pass_for_each_day_with_a_clean_answer() {
        let one = skill(&[answer("2026-11-02", "clean")]);
        assert_eq!((one.passes, one.proven()), (1, false));
        assert_eq!(one.last_pass_on, Some(day("2026-11-02")));

        // Two clean answers on one day are one pass.
        let same_day = skill(&[answer("2026-11-02", "clean"), answer("2026-11-02", "clean")]);
        assert_eq!(same_day.passes, 1);

        // Three days prove it, and a fourth adds nothing.
        let proven = skill(&[
            answer("2026-11-02", "clean"),
            answer("2026-11-05", "clean"),
            answer("2026-11-09", "clean"),
            answer("2026-11-12", "clean"),
        ]);
        assert_eq!((proven.passes, proven.proven()), (3, true));
    }

    #[test]
    fn an_answer_with_help_or_not_cold_is_no_pass_and_a_miss_starts_the_passes_again() {
        // Right, but with a hint or not cold: stored as neither clean nor a miss.
        let helped = skill(&[answer("2026-11-02", "helped")]);
        assert_eq!(helped.passes, 0);
        assert_eq!(helped.restarted_on, None);

        let missed = skill(&[
            answer("2026-11-02", "clean"),
            answer("2026-11-05", "clean"),
            answer("2026-11-08", "partly"),
        ]);
        assert_eq!((missed.passes, missed.earlier_passes), (0, 2));
        assert_eq!(missed.restarted_on, Some(day("2026-11-08")));

        // A clean answer after the miss counts, and the earlier ones stay history.
        let back = skill(&[
            answer("2026-11-02", "clean"),
            answer("2026-11-08", "wrong"),
            answer("2026-11-09", "clean"),
        ]);
        assert_eq!((back.passes, back.earlier_passes), (1, 0));
        assert_eq!(back.restarted_on, None);
    }

    #[test]
    fn a_level_moves_only_when_a_pass_is_earned() {
        // 12 skills, 36 passes: a level every 4 passes, the tenth when all are earned.
        assert_eq!(level(0, 36), (1, 4));
        assert_eq!(level(3, 36), (1, 1));
        assert_eq!(level(4, 36), (2, 4));
        assert_eq!(level(35, 36), (9, 1));
        assert_eq!(level(36, 36), (10, 0));
        // No skill yet: the first level, with nothing to earn.
        assert_eq!(level(0, 0), (1, 0));
    }

    #[test]
    fn every_skill_counts_toward_the_level_so_the_top_level_can_be_reached() {
        // Ten skills that were missed and are proven, and twenty that were
        // never missed and were answered right on three days.
        let proven = Skill {
            passes: 3,
            ..Skill::default()
        };
        let all: Vec<Skill> = vec![proven; 30];
        let whole = total(all.iter());
        assert_eq!(whole["level"], 10);
        assert_eq!(whole["proven"], 30);
        // With the twenty still untested, the level says so.
        let untested = Skill::default();
        let part: Vec<Skill> = all[..10]
            .iter()
            .cloned()
            .chain(std::iter::repeat_n(untested, 20))
            .collect();
        let partly = total(part.iter());
        assert_eq!(
            (partly["passes"].as_u64(), partly["of"].as_u64()),
            (Some(30), Some(90))
        );
        assert_eq!(partly["level"], 4);
    }

    /// A quick check on "units", made by a miss on `made_on`, with a cold
    /// answer on each of `answers`: right when true, wrong when false.
    fn check(made_on: &str, answers: &[(&str, bool)]) -> Item {
        use crate::learner::FirstTry;
        use crate::learner::practice::{AttemptFacts, EventKind, ItemEvent, ItemFacts, derive};
        let attempt = |id: String, right: bool| AttemptFacts {
            attempt_id: id,
            mark: if right {
                FirstTry::Right
            } else {
                FirstTry::Wrong
            },
            help: crate::learner::AttemptHelp::None,
            cold: true,
            slow: false,
            confidence: None,
            paper: None,
            paper_question: None,
            description: String::new(),
        };
        let mut events = vec![ItemEvent {
            event_no: 1,
            kind: EventKind::Made,
            day: day(made_on),
            attempt: Some(attempt(format!("{made_on}-wrong"), false)),
        }];
        for (n, (on, right)) in answers.iter().enumerate() {
            let no = i64::try_from(n).unwrap() + 2;
            events.push(ItemEvent {
                event_no: no,
                kind: EventKind::Attempt,
                day: day(on),
                attempt: Some(attempt(format!("a{no}"), *right)),
            });
        }
        let facts = ItemFacts {
            item_id: "q".into(),
            origin: Origin::Chat,
            label: "units".into(),
            plan: None,
            row_no: None,
            paper: None,
            paper_question: None,
            logged_on: day(made_on),
            gap_type: None,
            what_went_wrong: String::new(),
            repair: String::new(),
            confidence: None,
            next_question: None,
            answer_key: None,
            dropped: None,
            base_passes: 0,
            base_next_on: None,
            base_last_pass_on: None,
            base_history: String::new(),
            base_misses: Vec::new(),
            created_at_ms: 0,
            events,
        };
        let state = derive(&facts, None);
        Item { facts, state }
    }

    #[test]
    fn a_failed_check_leaves_the_level_where_the_passes_took_it() {
        // Two passes on a check, then a due answer that is wrong.
        let item = check(
            "2026-11-02",
            &[
                ("2026-11-03", true),
                ("2026-11-06", true),
                ("2026-11-13", false),
            ],
        );
        let failed = from_items("units", &[&item], &[], day("2026-11-13"));
        // The dots tell the truth: the passes start again.
        assert_eq!((failed.passes, failed.earlier_passes), (0, 2));
        let whole = total(std::iter::once(&failed));
        assert_eq!(whole["passes"], 0, "{whole}");
        assert_eq!(whole["proven"], 0, "{whole}");
        // The level stays where two passes of three took it.
        assert_eq!(
            (whole["level"].as_u64(), whole["to_next"].as_u64()),
            (Some(7), Some(1)),
            "{whole}"
        );
    }

    #[test]
    fn a_miss_that_makes_a_check_keeps_the_passes_the_answers_held_in_the_level() {
        // Two days of clean answers, then a miss that makes a check.
        let answers = [
            answer("2026-11-02", "clean"),
            answer("2026-11-05", "clean"),
            answer("2026-11-08", "wrong"),
        ];
        let item = check("2026-11-08", &[]);
        let skill = skill_of("units", "units", &[&item], &answers, day("2026-11-08"));
        assert_eq!((skill.passes, skill.best_passes), (0, 2));
        // A second item on the skill, made after the first earned a pass,
        // starts at none: the skill now holds none, and the level holds.
        let first = check("2026-11-02", &[("2026-11-03", true)]);
        let mut second = check("2026-11-04", &[]);
        second.facts.item_id = "q2".into();
        for event in &mut second.facts.events {
            event.event_no += 10;
        }
        second.state = crate::learner::practice::derive(&second.facts, None);
        let both = from_items("units", &[&first, &second], &[], day("2026-11-04"));
        assert_eq!((both.passes, both.best_passes), (0, 1));
    }

    #[test]
    fn a_miss_after_clean_answers_leaves_the_level_where_they_took_it() {
        let missed = skill(&[
            answer("2026-11-02", "clean"),
            answer("2026-11-05", "clean"),
            answer("2026-11-08", "wrong"),
        ]);
        assert_eq!(missed.passes, 0);
        let whole = total(std::iter::once(&missed));
        assert_eq!(
            (whole["passes"].as_u64(), whole["level"].as_u64()),
            (Some(0), Some(7)),
            "{whole}"
        );
    }

    #[test]
    fn a_skill_with_no_item_earns_a_pass_on_each_new_clean_day_up_to_three() {
        let answers = [
            answer("2026-11-02", "clean"),
            answer("2026-11-02", "clean"),
            answer("2026-11-03", "wrong"),
            answer("2026-11-04", "clean"),
            answer("2026-11-05", "helped"),
            answer("2026-11-06", "clean"),
            answer("2026-11-07", "clean"),
            answer("2026-11-08", "clean"),
        ];
        let days = answer_pass_days(&answers.iter().collect::<Vec<_>>());
        assert_eq!(
            days,
            [
                day("2026-11-02"),
                day("2026-11-04"),
                day("2026-11-06"),
                day("2026-11-07")
            ]
        );
    }

    #[test]
    fn a_day_counts_when_the_learner_answered_and_a_declined_question_does_not() {
        let days = answered_days(&[
            answer("2026-11-02", "wrong"),
            answer("2026-11-02", "clean"),
            answer("2026-11-03", "declined"),
            answer("2026-11-05", "helped"),
        ]);
        assert_eq!(days, ["2026-11-02", "2026-11-05"]);
    }

    #[test]
    fn the_text_form_lists_each_skill_with_its_passes() {
        let value = json!({
            "active": true,
            "plans": [
                {"plan": "midterm-1", "from": "record",
                 "progress": {"passes": 4, "of": 9, "proven": 1, "skills": 3, "level": 5, "to_next": 1},
                 "skills": [
                    {"skill": "slope and Km", "passes": 1, "proven": false, "due": true},
                    {"skill": "rate equation", "passes": 3, "proven": true, "due": false},
                    {"skill": "Hill coefficient", "passes": 0, "proven": false, "due": false, "needs_repair": true},
                 ]},
                {"plan": "final", "from": "file"},
            ],
            "other": {"skills": [], "progress": {}},
            "days": ["2026-11-08", "2026-11-09"],
        });
        assert_eq!(
            render(&value),
            "midterm-1: level 5 of 10 · 4 of 9 passes · 1 of 3 skills proven\n  \
             ●○○  slope and Km · due\n  \
             ●●●  rate equation · proven\n  \
             ○○○  Hill coefficient · waits for its repair\n\
             final: its gap log is not in the record yet. The tutor brings it in at the next marked answer.\n\
             Days with a marked answer: 2 in all. The latest: 2026-11-08, 2026-11-09\n"
        );
    }
}
