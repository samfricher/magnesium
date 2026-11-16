//! A receipt: one line of evidence from the record under an answer that
//! earned a pass.
//!
//! It names the last miss on the skill before the answer (its day and the
//! label the record keeps for it) and how many calendar days the answer came
//! after the learner's previous answer on that skill. Both come from the
//! attempts table and the practice items. The tutor's words are never read.
//!
//! `magnesium progress --json` carries a receipt for each answer that earned
//! a pass the skill holds now, by the record's id for the answer, and the
//! study window shows its `text` under that answer. In a terminal, the
//! PostToolUse hook after the `learning_step` that recorded the answer shows
//! the same text. Both read the same JSON, so the two say the same thing.
use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::learner::practice::{EventKind, local_day};
use crate::practice::Item;

/// One marked answer, as a receipt needs it.
#[derive(Clone, Debug)]
pub(crate) struct Answer {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) mark: String,
    pub(crate) paper: Option<String>,
    pub(crate) day: NaiveDate,
}

/// What the record knows of a practice item beyond its answers.
#[derive(Clone, Debug, Default)]
pub(crate) struct ItemFacts {
    pub(crate) label: String,
    pub(crate) paper: Option<String>,
    /// For a gap-log row brought in from a file, which no answer in the
    /// record made: the day of the paper that lost the mark.
    pub(crate) brought_in_on: Option<NaiveDate>,
    /// The day of the last pass that row was brought in with.
    pub(crate) brought_in_pass_on: Option<NaiveDate>,
}

/// The miss a receipt names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Miss {
    pub(crate) day: NaiveDate,
    pub(crate) label: String,
    /// "wrong" or "partly", or "lost" for a gap-log row whose answer is not
    /// in the record.
    pub(crate) mark: String,
    pub(crate) paper: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Receipt {
    pub(crate) miss: Option<Miss>,
    /// The day of the learner's previous answer on the skill.
    pub(crate) previous: Option<NaiveDate>,
    /// Calendar days from that answer to this one.
    pub(crate) days: Option<i64>,
}

/// The same name with case, spaces, hyphens and underscores aside, as the
/// runtime matches a chat label to a course graph's tag
/// (`plan_tag_named` in `learner::update::observations`). An answer filed
/// under the graph's name and an earlier one filed under the tutor's own
/// spelling are then one skill here, so a receipt never claims a longer gap
/// than the learner had.
fn loose(label: &str) -> String {
    label
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The receipt for each answer in `wanted`. `links` names the practice items
/// each answer is an event of, and `items` what is known of each item.
pub(crate) fn receipts(
    answers: &[Answer],
    links: &BTreeMap<String, BTreeSet<String>>,
    items: &BTreeMap<String, ItemFacts>,
    wanted: &BTreeSet<String>,
) -> BTreeMap<String, Receipt> {
    let none = BTreeSet::new();
    let items_of = |id: &str| links.get(id).unwrap_or(&none);
    let mut out = BTreeMap::new();
    for (at, answer) in answers.iter().enumerate() {
        if !wanted.contains(&answer.id) {
            continue;
        }
        let own_items = items_of(&answer.id);
        let mut names: BTreeSet<String> = own_items
            .iter()
            .filter_map(|item| items.get(item))
            .map(|item| loose(&item.label))
            .collect();
        names.insert(loose(&answer.label));
        names.remove("");
        // A declined question is not an answer.
        let before: Vec<&Answer> = answers[..at]
            .iter()
            .filter(|other| other.mark != "declined")
            .filter(|other| {
                names.contains(&loose(&other.label)) || !items_of(&other.id).is_disjoint(own_items)
            })
            .collect();
        let mut previous = before.last().map(|other| other.day);
        let mut miss = before
            .iter()
            .rev()
            .find(|other| other.mark == "wrong" || other.mark == "partly")
            .map(|other| Miss {
                day: other.day,
                label: other.label.clone(),
                mark: other.mark.clone(),
                paper: other.paper.clone(),
            });
        // A row brought in from a gap-log file is a lost mark the record
        // holds with no answer of its own.
        for item in own_items.iter().filter_map(|item| items.get(item)) {
            for day in [item.brought_in_on, item.brought_in_pass_on]
                .into_iter()
                .flatten()
                .filter(|day| *day < answer.day)
            {
                previous = Some(previous.map_or(day, |known| known.max(day)));
            }
            if let Some(day) = item.brought_in_on.filter(|day| *day < answer.day)
                && miss.as_ref().is_none_or(|known| known.day < day)
            {
                miss = Some(Miss {
                    day,
                    label: item.label.clone(),
                    mark: "lost".to_owned(),
                    paper: item.paper.clone(),
                });
            }
        }
        let days = previous
            .map(|day| (answer.day - day).num_days())
            .filter(|days| *days > 0);
        if miss.is_none() && days.is_none() {
            continue;
        }
        out.insert(
            answer.id.clone(),
            Receipt {
                miss,
                previous,
                days,
            },
        );
    }
    out
}

/// "2026-11-10" as "Tue 10 Nov", as the study window writes a day.
fn short_day(day: NaiveDate) -> String {
    day.format("%a %-d %b").to_string()
}

/// The miss, as the record has it: "Not right on Tue 10 Nov: sign of b."
/// A paper's lost mark says the paper, as the gap log does.
fn miss_words(miss: &Miss) -> String {
    let day = short_day(miss.day);
    let label = miss.label.split_whitespace().collect::<Vec<_>>().join(" ");
    let paper = miss
        .paper
        .as_deref()
        .map(str::trim)
        .filter(|paper| !paper.is_empty());
    match (paper, miss.mark.as_str()) {
        (Some(paper), _) => format!("Lost marks on {paper}, {day}: {label}."),
        (None, "lost") => format!("Lost marks on {day}: {label}."),
        (None, "partly") => format!("Partly right on {day}: {label}."),
        _ => format!("Not right on {day}: {label}."),
    }
}

/// The days since the learner's previous answer on the skill, said as what
/// this answer held through.
fn held_words(days: i64) -> String {
    if days == 1 {
        "It held through the day since you last answered it.".to_owned()
    } else {
        format!("It held through the {days} days since you last answered it.")
    }
}

impl Receipt {
    /// The line the window and a terminal show.
    pub(crate) fn text(&self) -> String {
        let mut said: Vec<String> = Vec::new();
        if let Some(miss) = &self.miss {
            said.push(miss_words(miss));
        }
        if let Some(days) = self.days {
            said.push(held_words(days));
        }
        said.join(" ")
    }

    pub(crate) fn to_json(&self) -> Value {
        json!({
            "missed_on": self.miss.as_ref().map(|miss| miss.day.to_string()),
            "missed": self.miss.as_ref().map(|miss| miss.label.clone()),
            "missed_mark": self.miss.as_ref().map(|miss| miss.mark.clone()),
            "missed_paper": self.miss.as_ref().and_then(|miss| miss.paper.clone()),
            "last_answered_on": self.previous.map(|day| day.to_string()),
            "days": self.days,
            "text": self.text(),
        })
    }
}

fn load_answers(conn: &Connection) -> rusqlite::Result<Vec<Answer>> {
    let mut statement = conn.prepare(
        "SELECT attempt_id, label, mark, paper, answered_at_ms
         FROM attempts ORDER BY answered_at_ms, attempt_no",
    )?;
    statement
        .query_map([], |r| {
            Ok(Answer {
                id: r.get(0)?,
                label: r.get(1)?,
                mark: r.get(2)?,
                paper: r.get(3)?,
                day: local_day(r.get(4)?),
            })
        })?
        .collect()
}

/// The receipts for the answers `wanted`, from the record, as the JSON
/// `magnesium progress` prints under `pass_receipts`: by the answer's id.
pub(crate) fn of(conn: &Connection, items: &[Item], wanted: &BTreeSet<String>) -> Value {
    if wanted.is_empty() {
        return json!({});
    }
    let Ok(answers) = load_answers(conn) else {
        return json!({});
    };
    let mut links: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut facts: BTreeMap<String, ItemFacts> = BTreeMap::new();
    for item in items {
        let id = &item.facts.item_id;
        let mut made_by_an_answer = false;
        for event in &item.facts.events {
            if let Some(attempt) = &event.attempt {
                links
                    .entry(attempt.attempt_id.clone())
                    .or_default()
                    .insert(id.clone());
                made_by_an_answer |= event.kind == EventKind::Made;
            }
        }
        facts.insert(
            id.clone(),
            ItemFacts {
                label: item.facts.label.clone(),
                paper: item.facts.paper.clone(),
                brought_in_on: (!made_by_an_answer && item.facts.logged_on != NaiveDate::MIN)
                    .then_some(item.facts.logged_on),
                brought_in_pass_on: item.facts.base_last_pass_on,
            },
        );
    }
    let found = receipts(&answers, &links, &facts, wanted);
    Value::Object(
        found
            .into_iter()
            .map(|(id, receipt)| (id, receipt.to_json()))
            .collect(),
    )
}

/// The ids of the answers that earned the passes each skill holds now, in
/// what `magnesium progress --json` printed.
pub(crate) fn passing_answers(skills: &[Value]) -> BTreeSet<String> {
    skills
        .iter()
        .flat_map(|skill| skill["passed_by"].as_array().into_iter().flatten())
        .filter_map(|id| id.as_str().map(str::to_owned))
        .collect()
}

const PASS_WORDS: [&str; 3] = ["First", "Second", "Third"];

/// What a terminal shows after the `learning_step` that recorded `answers`:
/// for each that earned a pass, which of three days it was and its receipt,
/// from what `magnesium progress --json` printed. None when no answer
/// earned a pass with a receipt.
pub(crate) fn terminal_lines(progress: &Value, answers: &BTreeSet<String>) -> Option<String> {
    let plans = progress["plans"].as_array().into_iter().flatten();
    let skills = plans
        .flat_map(|plan| plan["skills"].as_array().into_iter().flatten())
        .chain(progress["other"]["skills"].as_array().into_iter().flatten());
    let mut lines: Vec<String> = Vec::new();
    let mut said: BTreeSet<&str> = BTreeSet::new();
    for skill in skills {
        for (at, id) in skill["passed_by"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let Some(id) = id.as_str().filter(|id| answers.contains(*id)) else {
                continue;
            };
            let Some(text) = progress["pass_receipts"][id]["text"].as_str() else {
                continue;
            };
            if !said.insert(id) {
                continue;
            }
            let pass = PASS_WORDS.get(at).copied().unwrap_or("Another");
            // A terminal has no answer just above the line, so it names the
            // skill unless the miss it names is that skill.
            let name = skill["skill"].as_str().unwrap_or_default().trim();
            let named = progress["pass_receipts"][id]["missed"]
                .as_str()
                .is_some_and(|missed| loose(missed) == loose(name));
            lines.push(if named || name.is_empty() {
                format!("{pass} of three days. {text}")
            } else {
                format!("{pass} of three days on {name}. {text}")
            });
        }
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// The answers a `learning_step` result names as recorded, under
/// `practice.attempts`. Claude Code hands an MCP tool's result to a hook as
/// its content, whose text holds the runtime's JSON, so strings are read as
/// JSON too.
pub(crate) fn recorded_answers(response: Option<&Value>) -> BTreeSet<String> {
    fn walk(value: &Value, depth: u8, found: &mut BTreeSet<String>) {
        if depth > 6 {
            return;
        }
        match value {
            Value::Object(fields) => {
                for attempt in fields
                    .get("practice")
                    .and_then(|practice| practice["attempts"].as_array())
                    .into_iter()
                    .flatten()
                {
                    if let Some(id) = attempt["attempt_id"].as_str() {
                        found.insert(id.to_owned());
                    }
                }
                for field in fields.values() {
                    walk(field, depth + 1, found);
                }
            }
            Value::Array(values) => {
                for value in values {
                    walk(value, depth + 1, found);
                }
            }
            Value::String(text) => {
                if let Ok(parsed) = serde_json::from_str::<Value>(text)
                    && !parsed.is_string()
                {
                    walk(&parsed, depth + 1, found);
                }
            }
            _ => {}
        }
    }
    let mut found = BTreeSet::new();
    if let Some(response) = response {
        walk(response, 0, &mut found);
    }
    found
}

/// The receipt lines for a terminal after a committed `learning_step`, or
/// none. The study window shows receipts from the record itself, under the
/// answer, so a lesson it started gets none here and nothing is said twice.
pub(crate) fn for_hook(
    workspace: &crate::workspace::Workspace,
    response: Option<&Value>,
) -> Option<String> {
    if crate::mcp::catalog::window_surface() {
        return None;
    }
    let answers = recorded_answers(response);
    if answers.is_empty() {
        return None;
    }
    let progress =
        crate::progress::progress(workspace, chrono::Local::now().timestamp_millis()).ok()?;
    terminal_lines(&progress, &answers)
}

#[cfg(test)]
mod tests;
