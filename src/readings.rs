use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rusqlite::Connection;
use serde_json::{Value, json};

use crate::learner::label_key;
use crate::practice::Item;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Reading {
    Untested,
    Open,
    Conditional,
    Sound,
}

impl Reading {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Untested => "untested",
            Self::Open => "open",
            Self::Conditional => "conditional",
            Self::Sound => "sound",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TagReading {
    pub(crate) tag: String,
    pub(crate) reading: Reading,
    pub(crate) why: String,
}

#[derive(Clone, Debug)]
pub(crate) struct TagAttempt {
    pub(crate) label_key: String,
    pub(crate) right: bool,
    pub(crate) partly: bool,
    pub(crate) alone: bool,
    pub(crate) cold: bool,
    pub(crate) slow: bool,
    pub(crate) place: String,
}

pub(crate) fn load_attempts(conn: &Connection) -> rusqlite::Result<Vec<TagAttempt>> {
    let mut statement = conn.prepare(
        "SELECT label_key, mark, help, cold, slow, paper, paper_question, question
         FROM attempts ORDER BY answered_at_ms, attempt_no",
    )?;
    statement
        .query_map([], |r| {
            let mark: String = r.get(1)?;
            let paper: Option<String> = r.get(5)?;
            let part: Option<String> = r.get(6)?;
            let question: String = r.get(7)?;
            Ok(TagAttempt {
                label_key: r.get(0)?,
                right: mark == "right",
                partly: mark == "partly",
                alone: r.get::<_, String>(2)? == "none",
                cold: r.get(3)?,
                slow: r.get(4)?,
                place: match (paper, part) {
                    (Some(paper), Some(part)) => format!("{paper} Q{part}"),
                    _ => format!("\"{}\"", question.chars().take(60).collect::<String>()),
                },
            })
        })?
        .collect()
}

pub(crate) fn load_imported(
    conn: &Connection,
) -> rusqlite::Result<BTreeMap<String, (String, String)>> {
    let mut statement = conn.prepare("SELECT topic_key, reading, phrase FROM imported_readings")?;
    statement
        .query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
        .collect()
}

pub(crate) struct PlanInputs<'a> {
    pub(crate) plan: &'a str,
    pub(crate) scoped: &'a BTreeSet<String>,
    pub(crate) items: &'a [Item],
    pub(crate) attempts: &'a [TagAttempt],
    pub(crate) imported: &'a BTreeMap<String, (String, String)>,
}

pub(crate) fn read_plan(inputs: &PlanInputs<'_>) -> Vec<TagReading> {
    let PlanInputs {
        plan,
        scoped,
        items,
        attempts,
        imported,
    } = *inputs;
    let rows: Vec<&Item> = items
        .iter()
        .filter(|item| item.facts.plan.as_deref() == Some(plan))
        .collect();
    let mut order: Vec<String> = Vec::new();
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    for row in &rows {
        let key = label_key(&row.facts.label);
        if !names.contains_key(&key) {
            names.insert(key.clone(), row.facts.label.clone());
            order.push(key);
        }
    }
    for key in scoped {
        if !names.contains_key(key)
            && (attempts.iter().any(|a| a.label_key == *key) || imported.contains_key(key))
        {
            names.insert(key.clone(), key.clone());
            order.push(key.clone());
        }
    }
    let name = |key: &str| names.get(key).cloned().unwrap_or_else(|| key.to_owned());
    let open_row = |key: &str| -> Option<&Item> {
        rows.iter().copied().find(|row| {
            label_key(&row.facts.label) == key && !row.state.closed() && row.state.passes == 0
        })
    };
    let own = |key: &str| -> Vec<&TagAttempt> {
        attempts.iter().filter(|a| a.label_key == key).collect()
    };
    let cold_pass = |a: &&TagAttempt| a.right && a.alone && a.cold && !a.slow;
    let passed: BTreeMap<String, String> = order
        .iter()
        .filter_map(|key| {
            own(key)
                .into_iter()
                .find(cold_pass)
                .map(|a| (key.clone(), a.place.clone()))
        })
        .collect();
    order
        .iter()
        .map(|key| {
            let (reading, why) = (|| {
                if let Some(row) = open_row(key) {
                    let state = if row.state.next_on.is_none() {
                        "waiting for its repair".to_owned()
                    } else {
                        format!(
                            "redo due {}",
                            row.state.next_on.map(|d| d.to_string()).unwrap_or_default()
                        )
                    };
                    return (Reading::Open, format!("{} open, {state}", row.name()));
                }
                if let Some(row) = rows.iter().find(|row| {
                    label_key(&row.facts.label) == *key
                        && !row.state.closed()
                        && row.state.passes > 0
                }) {
                    return (
                        Reading::Conditional,
                        format!("{} at {}", row.name(), row.state.status()),
                    );
                }
                let closed = rows
                    .iter()
                    .find(|row| label_key(&row.facts.label) == *key && row.state.closed());
                let sound = passed
                    .get(key)
                    .map(|place| format!("cold in {place}"))
                    .or_else(|| closed.map(|row| format!("{} closed", row.name())))
                    .or_else(|| {
                        imported
                            .get(key)
                            .filter(|(reading, _)| reading == "sound")
                            .map(|(_, phrase)| format!("memory before the upgrade: {phrase}"))
                    });
                if let Some(why) = sound {
                    return (Reading::Sound, why);
                }
                let own = own(key);
                let weak = own
                    .iter()
                    .find(|a| a.right && a.slow)
                    .map(|a| format!("passed only slow, in {}", a.place))
                    .or_else(|| {
                        own.iter()
                            .find(|a| a.right)
                            .map(|a| format!("passed only with help or not cold, in {}", a.place))
                    })
                    .or_else(|| {
                        own.iter()
                            .find(|a| a.partly)
                            .map(|a| format!("partly right in {}", a.place))
                    })
                    .or_else(|| {
                        imported
                            .get(key)
                            .filter(|(reading, _)| reading == "conditional")
                            .map(|(_, phrase)| format!("memory before the upgrade: {phrase}"))
                    });
                match weak {
                    Some(why) => (Reading::Conditional, why),
                    None => (Reading::Untested, String::new()),
                }
            })();
            TagReading {
                tag: name(key),
                reading,
                why,
            }
        })
        .collect()
}

pub(crate) fn plan_json(
    plan: &str,
    readings: &[TagReading],
    help: &BTreeMap<String, &'static str>,
) -> Value {
    let mut value = json!({"plan": plan});
    for reading in [Reading::Open, Reading::Conditional, Reading::Sound] {
        let tags = readings
            .iter()
            .filter(|r| r.reading == reading)
            .map(|r| json!({"tag": r.tag, "why": r.why}))
            .collect::<Vec<_>>();
        if !tags.is_empty() {
            value[reading.as_str()] = json!(tags);
        }
    }
    let untested = readings
        .iter()
        .filter(|r| r.reading == Reading::Untested)
        .map(|r| r.tag.clone())
        .collect::<Vec<_>>();
    if !untested.is_empty() {
        value["untested"] = json!(untested);
    }
    let levels = readings
        .iter()
        .filter_map(|r| {
            help.get(&label_key(&r.tag))
                .map(|level| (r.tag.clone(), json!(level)))
        })
        .collect::<serde_json::Map<_, _>>();
    if !levels.is_empty() {
        value["help_levels"] = Value::Object(levels);
    }
    value
}

pub(crate) fn readings_json(conn: &Connection, root: &Path, plans: &[String]) -> Option<Value> {
    if plans.is_empty() {
        return None;
    }
    let items = crate::practice::snapshot(conn, root).ok()?;
    let attempts = load_attempts(conn).ok()?;
    let imported = load_imported(conn).ok()?;
    let help = crate::learner::help::all_levels(conn).ok()?;
    let values = plans
        .iter()
        .map(|plan| {
            let scoped = crate::course_map::scoped_ids(root, plan);
            let readings = read_plan(&PlanInputs {
                plan,
                scoped: &scoped,
                items: &items,
                attempts: &attempts,
                imported: &imported,
            });
            plan_json(plan, &readings, &help)
        })
        .collect::<Vec<_>>();
    Some(json!({
        "plans": values,
        "note": "Magnesium works out each label's reading from the gap log and the stored attempts each time, and keeps each kind's help level: the example step last given when the learner had no starting move. Use these; never write a reading or a step into a memory. What was never asked is on the course map: course_map read states it per item.",
    }))
}

pub(crate) fn plan_tag_keys(root: &Path, plan: &str, items: &[Item]) -> BTreeSet<String> {
    let mut keys: BTreeSet<String> = crate::course_map::scoped_ids(root, plan);
    keys.extend(
        items
            .iter()
            .filter(|item| item.facts.plan.as_deref() == Some(plan))
            .map(|item| label_key(&item.facts.label)),
    );
    keys
}

fn request_attempts(request: &Value) -> Vec<&Value> {
    request["observations"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|o| {
            o.get("attempt")
                .filter(|a| a.is_object())
                .into_iter()
                .chain(o["attempts"].as_array().into_iter().flatten())
        })
        .collect()
}

pub(crate) fn help_levels_json(conn: &Connection, request: &Value) -> Option<Value> {
    use crate::learner::help::{Level, known_kinds, stored};
    let mut kinds: Vec<(String, bool)> = Vec::new();
    for attempt in request_attempts(request) {
        let Some(kind) = attempt["kind"].as_str() else {
            continue;
        };
        let key = label_key(kind);
        let stuck = attempt["no_start"].as_bool().unwrap_or(false);
        match kinds.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = stuck,
            None => kinds.push((key, stuck)),
        }
    }
    if kinds.is_empty() {
        return None;
    }
    let levels = kinds
        .iter()
        .filter_map(|(key, stuck)| {
            let stored = stored(conn, key).ok()??;
            let mut value = json!({
                "kind": stored.kind,
                "level": Level::words(stored.level),
            });
            match (stuck, stored.level) {
                (true, Some(level)) => value["give_now"] = json!(level.give()),
                _ => value["if_stuck_next"] = json!(Level::forward(stored.level).give()),
            }
            Some(value)
        })
        .collect::<Vec<_>>();
    Some(json!({
        "kinds": levels,
        "known_kinds": known_kinds(conn).unwrap_or_default(),
        "note": "Magnesium keeps each kind's help level from the attempts: forward when the learner has no starting move on a new problem of the kind, back one step when a first try of the kind is not right without help. Give the help it names; record no example step in a memory. Reuse a known kind's name for the same method.",
    }))
}

fn plans_to_state(
    conn: &Connection,
    root: &Path,
    committed: Option<&Value>,
    practice_change: bool,
    paper_stage: bool,
    result: &Value,
) -> Vec<String> {
    let all = crate::learner::practice::plans(root);
    if paper_stage {
        return all;
    }
    let Some(request) = committed else {
        return Vec::new();
    };
    let mut chosen = BTreeSet::new();
    let operation = request["session"]["operation"].as_str();
    if matches!(operation, Some("start" | "resume")) {
        let goal = request["session"]["goal"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| {
                let id = request["session"]["session_id"].as_str().or_else(|| {
                    request["session"]["client_ref"]
                        .as_str()
                        .and_then(|name| result["references"][name].as_str())
                })?;
                conn.query_row(
                    "SELECT goal FROM learning_sessions WHERE session_id=?1",
                    [id],
                    |r| r.get::<_, String>(0),
                )
                .ok()
            });
        if let Some(goal) = goal {
            chosen.extend(
                all.iter()
                    .filter(|plan| crate::digest::names_target(&goal, plan, None))
                    .cloned(),
            );
        }
    }
    if practice_change && let Ok(items) = crate::practice::snapshot(conn, root) {
        let labels: BTreeSet<String> = request_attempts(request)
            .into_iter()
            .flat_map(|a| [a["label"].as_str(), a["kind"].as_str()])
            .flatten()
            .map(label_key)
            .collect();
        let named: BTreeSet<String> = request_attempts(request)
            .into_iter()
            .flat_map(|a| a["counts_for"].as_array().into_iter().flatten())
            .chain(
                request["practice"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|e| &e["item"]),
            )
            .filter_map(Value::as_str)
            .filter_map(|name| {
                name.rsplit_once('#')
                    .map(|(plan, _)| plan.trim().to_owned())
            })
            .collect();
        for plan in &all {
            if named.contains(plan)
                || !plan_tag_keys(root, plan, &items).is_disjoint(&labels)
                || !request["attempt_corrections"]
                    .as_array()
                    .is_none_or(Vec::is_empty)
            {
                chosen.insert(plan.clone());
            }
        }
    }
    chosen.into_iter().collect()
}

pub(crate) fn add_to_result(
    workspace: &crate::workspace::Workspace,
    committed: Option<&Value>,
    practice_change: bool,
    paper_stage: bool,
    result: &mut Value,
) {
    if committed.is_none() && !paper_stage {
        return;
    }
    let Ok(store) = crate::practice::open_store(workspace) else {
        return;
    };
    if let Some(request) = committed
        && let Some(levels) = help_levels_json(&store.conn, request)
    {
        result["help_levels"] = levels;
    }
    let plans = plans_to_state(
        &store.conn,
        workspace.root(),
        committed,
        practice_change,
        paper_stage,
        result,
    );
    if let Some(readings) = readings_json(&store.conn, workspace.root(), &plans) {
        result["readings"] = readings;
    }
    if let Some(request) = committed
        && let Some(answers) = earlier_answers(&store.conn, request, result)
    {
        result["earlier_answers"] = answers;
    }
}

const COMMON_WORDS: &[&str] = &[
    "about",
    "after",
    "again",
    "also",
    "answer",
    "basic",
    "basics",
    "before",
    "being",
    "between",
    "both",
    "calculate",
    "case",
    "cases",
    "check",
    "compute",
    "does",
    "each",
    "every",
    "example",
    "explain",
    "find",
    "first",
    "from",
    "give",
    "have",
    "help",
    "into",
    "just",
    "know",
    "learn",
    "learning",
    "lesson",
    "like",
    "make",
    "more",
    "most",
    "much",
    "need",
    "only",
    "other",
    "over",
    "practice",
    "practise",
    "problem",
    "problems",
    "question",
    "questions",
    "quick",
    "review",
    "same",
    "show",
    "solve",
    "some",
    "start",
    "step",
    "steps",
    "study",
    "such",
    "teach",
    "than",
    "that",
    "their",
    "them",
    "then",
    "there",
    "these",
    "they",
    "this",
    "through",
    "understand",
    "understanding",
    "using",
    "very",
    "want",
    "what",
    "when",
    "where",
    "which",
    "while",
    "will",
    "with",
    "without",
    "work",
    "working",
    "would",
    "write",
    "your",
];

fn earlier_answers(conn: &Connection, request: &Value, result: &Value) -> Option<Value> {
    if !matches!(
        request["session"]["operation"].as_str(),
        Some("start" | "resume")
    ) {
        return None;
    }
    let goal = request["session"]["goal"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            let id = request["session"]["session_id"].as_str()?;
            conn.query_row(
                "SELECT goal FROM learning_sessions WHERE session_id=?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .ok()
        })?;
    let words = |text: &str| -> BTreeSet<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|word| word.chars().count() >= 4)
            .map(str::to_lowercase)
            .filter(|word| !COMMON_WORDS.contains(&word.as_str()))
            .collect()
    };
    let wanted = words(&goal);
    if wanted.is_empty() {
        return None;
    }
    let own: BTreeSet<&str> = result["captured_event_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut statement = conn
        .prepare(
            "SELECT label, question, mark, help, answered_at_ms, response_event_id,
                    COALESCE(problem_kind, '') FROM attempts
             ORDER BY answered_at_ms DESC, attempt_no DESC LIMIT 400",
        )
        .ok()?;
    let rows = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
            ))
        })
        .ok()?;
    let answers: Vec<Value> = rows
        .filter_map(std::result::Result::ok)
        .filter(|(label, _, _, _, _, event, kind)| {
            !own.contains(event.as_str()) && !wanted.is_disjoint(&words(&format!("{label} {kind}")))
        })
        .take(8)
        .map(|(label, question, mark, help, at, _, _)| {
            json!({
                "label": label,
                "question": question,
                "mark": mark,
                "help": help,
                "day": crate::learner::practice::local_day(at).to_string(),
            })
        })
        .collect();
    (!answers.is_empty()).then_some(Value::Array(answers))
}

#[cfg(test)]
mod tests;
