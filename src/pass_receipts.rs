use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::learner::label_key;
use crate::learner::practice::{EventKind, local_day};
use crate::practice::Item;

#[derive(Clone, Debug)]
pub(crate) struct Answer {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) mark: String,
    pub(crate) paper: Option<String>,
    pub(crate) day: NaiveDate,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ItemFacts {
    pub(crate) label: String,
    pub(crate) paper: Option<String>,
    pub(crate) from_a_paper: bool,
    pub(crate) brought_in_on: Option<NaiveDate>,
    pub(crate) brought_in_pass_on: Option<NaiveDate>,
    pub(crate) brought_in_redos: Vec<(NaiveDate, bool)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Miss {
    pub(crate) day: NaiveDate,
    pub(crate) label: String,
    pub(crate) mark: String,
    pub(crate) paper: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Receipt {
    pub(crate) miss: Option<Miss>,
    pub(crate) previous: Option<NaiveDate>,
    pub(crate) days: Option<i64>,
}

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
            .map(|item| label_key(&item.label))
            .collect();
        names.insert(label_key(&answer.label));
        let before: Vec<&Answer> = answers[..at]
            .iter()
            .filter(|other| other.mark != "declined")
            .filter(|other| {
                names.contains(&label_key(&other.label))
                    || !items_of(&other.id).is_disjoint(own_items)
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
        for item in own_items.iter().filter_map(|item| items.get(item)) {
            let redos = item.brought_in_redos.iter().copied();
            let days = [item.brought_in_on, item.brought_in_pass_on]
                .into_iter()
                .flatten()
                .chain(redos.clone().map(|(day, _)| day));
            for day in days.filter(|day| *day < answer.day) {
                previous = Some(previous.map_or(day, |known| known.max(day)));
            }
            let lost = item
                .brought_in_on
                .filter(|_| item.from_a_paper)
                .map(|day| (day, "lost", item.paper.clone()));
            let failed = redos
                .filter(|(_, failed)| *failed)
                .map(|(day, _)| (day, "failed", None));
            for (day, mark, paper) in lost.into_iter().chain(failed) {
                if day < answer.day && miss.as_ref().is_none_or(|known| known.day < day) {
                    miss = Some(Miss {
                        day,
                        label: item.label.clone(),
                        mark: mark.to_owned(),
                        paper,
                    });
                }
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

fn short_day(day: NaiveDate) -> String {
    day.format("%a %-d %b").to_string()
}

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

fn held_words(days: i64) -> String {
    if days == 1 {
        "It held through the day since you last answered it.".to_owned()
    } else {
        format!("It held through the {days} days since you last answered it.")
    }
}

impl Receipt {
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
                from_a_paper: item.facts.origin == crate::learner::practice::Origin::Paper,
                brought_in_on: (!made_by_an_answer && item.facts.logged_on != NaiveDate::MIN)
                    .then_some(item.facts.logged_on),
                brought_in_pass_on: item.facts.base_last_pass_on,
                brought_in_redos: redos_in(&item.facts.base_history),
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

fn redos_in(history: &str) -> Vec<(NaiveDate, bool)> {
    history
        .split([',', ';'])
        .filter_map(|entry| {
            let day = crate::digest::first_date(entry)?;
            Some((day, entry.to_lowercase().contains("fail")))
        })
        .collect()
}

pub(crate) fn passing_answers(skills: &[Value]) -> BTreeSet<String> {
    skills
        .iter()
        .flat_map(|skill| skill["passed_by"].as_array().into_iter().flatten())
        .filter_map(|id| id.as_str().map(str::to_owned))
        .collect()
}

const PASS_WORDS: [&str; 3] = ["First", "Second", "Third"];

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
            let name = skill["skill"].as_str().unwrap_or_default().trim();
            let named = progress["pass_receipts"][id]["missed"]
                .as_str()
                .is_some_and(|missed| label_key(missed) == label_key(name));
            lines.push(if named || name.is_empty() {
                format!("{pass} of three days. {text}")
            } else {
                format!("{pass} of three days on {name}. {text}")
            });
        }
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

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
