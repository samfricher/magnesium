//! Each topic's reading in a plan, worked out by the runtime (#31).
//!
//! The papers loop reads every gap tag as untested, open, conditional or
//! sound. The tutor used to work the reading out at every marking and write it
//! in words into the tag's memory, where it could drift from the gap log. Now
//! the runtime works it out each time it is asked, from the plan's gap-log
//! rows, the stored attempts on the tag and the plan's `graph.yaml`, with
//! today's definitions, and the tool results state it.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rusqlite::Connection;
use serde_json::{Value, json};

use crate::digest::read_small;
use crate::learner::label_key;
use crate::practice::Item;

/// A course graph's tags and the edges a reading follows, keyed by tag key.
#[derive(Debug, Default)]
pub(crate) struct Graph {
    /// In the order the file lists them.
    pub(crate) order: Vec<String>,
    pub(crate) tags: BTreeMap<String, GraphTag>,
}

#[derive(Debug, Default)]
pub(crate) struct GraphTag {
    pub(crate) name: String,
    pub(crate) needs: Vec<String>,
    pub(crate) key_need: Vec<String>,
    pub(crate) contains: Vec<String>,
}

const LIST_FIELDS: [&str; 4] = ["needs", "key_need", "contains", "touches"];

/// Read the papers skill's `graph.yaml`: a `tags:` map of tag to fields,
/// with each list in flow form (`[a, b]`) or block form (`- a`). It reads only
/// what a reading needs and refuses a shape it cannot be sure of.
pub(crate) fn parse_graph(text: &str) -> Result<Graph, String> {
    let mut graph = Graph::default();
    let mut in_tags = false;
    let mut tag_indent: Option<usize> = None;
    let mut current: Option<String> = None;
    let mut block: Option<(String, usize)> = None;
    for (index, raw) in text.lines().enumerate() {
        let line_no = index + 1;
        let line = strip_comment(raw);
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with('\t') {
            return Err(format!("line {line_no}: indented with a tab"));
        }
        let indent = line.len() - line.trim_start().len();
        let body = line.trim();
        if indent == 0 {
            in_tags = body == "tags:";
            current = None;
            block = None;
            continue;
        }
        if !in_tags {
            continue;
        }
        let tag_indent = *tag_indent.get_or_insert(indent);
        if indent < tag_indent {
            return Err(format!("line {line_no}: indented less than the tags"));
        }
        if indent == tag_indent {
            let Some(name) = body.strip_suffix(':') else {
                return Err(format!(
                    "line {line_no}: a tag is its name and a colon, with its fields below"
                ));
            };
            let name = unquote(name);
            if name.is_empty() {
                return Err(format!("line {line_no}: a tag with no name"));
            }
            let key = label_key(&name);
            if graph.tags.contains_key(&key) {
                return Err(format!("line {line_no}: the tag {name} is listed twice"));
            }
            graph.order.push(key.clone());
            graph.tags.insert(
                key.clone(),
                GraphTag {
                    name,
                    ..GraphTag::default()
                },
            );
            current = Some(key);
            block = None;
            continue;
        }
        let Some(tag) = current.as_ref().and_then(|key| graph.tags.get_mut(key)) else {
            return Err(format!("line {line_no}: a field before any tag"));
        };
        if let Some(item) = body.strip_prefix("- ").or((body == "-").then_some("")) {
            match &block {
                // YAML lets a block list's dashes sit at its key's indent.
                Some((field, field_indent)) if indent >= *field_indent => {
                    let item = unquote(item);
                    if !item.is_empty()
                        && let Some(list) = list(tag, field)
                    {
                        list.push(label_key(&item));
                    }
                    continue;
                }
                _ => return Err(format!("line {line_no}: a list item under no list field")),
            }
        }
        let Some((field, value)) = body.split_once(':') else {
            return Err(format!(
                "line {line_no}: expected a field such as needs: [TAG]"
            ));
        };
        let field = field.trim();
        let value = value.trim();
        block = None;
        // A field with its value below may hold a block list. The items of
        // one a reading does not use are skipped, as `list` gives no list.
        if value.is_empty() {
            block = Some((field.to_owned(), indent));
            continue;
        }
        if !LIST_FIELDS.contains(&field) {
            continue;
        }
        let Some(inner) = value
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        else {
            return Err(format!(
                "line {line_no}: {field} is a list, such as [TAG] or [] for none"
            ));
        };
        let items = inner
            .split(',')
            .map(unquote)
            .filter(|item| !item.is_empty())
            .map(|item| label_key(&item))
            .collect::<Vec<_>>();
        if let Some(list) = list(tag, field) {
            list.extend(items);
        }
    }
    if graph.tags.is_empty() {
        return Err("no tags: section with a tag in it".into());
    }
    Ok(graph)
}

/// The list a field fills. A touches edge gives a reading nothing.
fn list<'a>(tag: &'a mut GraphTag, field: &str) -> Option<&'a mut Vec<String>> {
    match field {
        "needs" => Some(&mut tag.needs),
        "key_need" => Some(&mut tag.key_need),
        "contains" => Some(&mut tag.contains),
        _ => None,
    }
}

/// A line without its comment: a `#` at the start of the text or after a
/// space, outside quotes.
fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    let mut previous = ' ';
    for (at, c) in line.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(open), _) if c == open => quote = None,
            (None, '#') if previous.is_whitespace() => return &line[..at],
            _ => {}
        }
        previous = c;
    }
    line
}

fn unquote(text: &str) -> String {
    let text = text.trim();
    text.strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .or_else(|| text.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')))
        .unwrap_or(text)
        .trim()
        .to_owned()
}

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
    /// Why, in a few words, such as "midterm-1#3 open, waiting for its
    /// repair" or "cold in MockA Q3b".
    pub(crate) why: String,
}

/// One stored attempt on a tag, as a reading sees it.
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

/// Every stored attempt, with where it was answered.
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

/// A reading a memory stated in words before schema 5: (reading, phrase).
pub(crate) fn load_imported(
    conn: &Connection,
) -> rusqlite::Result<BTreeMap<String, (String, String)>> {
    let mut statement = conn.prepare("SELECT topic_key, reading, phrase FROM imported_readings")?;
    statement
        .query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
        .collect()
}

/// What a plan's readings were worked out from.
pub(crate) struct PlanInputs<'a> {
    pub(crate) plan: &'a str,
    pub(crate) graph: Option<&'a Graph>,
    pub(crate) items: &'a [Item],
    pub(crate) attempts: &'a [TagAttempt],
    pub(crate) imported: &'a BTreeMap<String, (String, String)>,
}

/// Each tag's reading, by the papers loop's definitions:
/// - open: a gap-log row at open, not yet passed since its last repair;
/// - conditional: a row at 1 of 3 or 2 of 3, a key_need tag open, a tag in
///   needs open under a cold pass, or passed only slow, partly, with help or
///   not cold;
/// - sound: passed cold, unaided, in full and in time, directly or through
///   contains, with nothing in needs open;
/// - untested: no evidence yet.
pub(crate) fn read_plan(inputs: &PlanInputs<'_>) -> Vec<TagReading> {
    let PlanInputs {
        plan,
        graph,
        items,
        attempts,
        imported,
    } = *inputs;
    let rows: Vec<&Item> = items
        .iter()
        .filter(|item| item.facts.plan.as_deref() == Some(plan))
        .collect();
    let mut order: Vec<String> = graph.map(|g| g.order.clone()).unwrap_or_default();
    let mut names: BTreeMap<String, String> = graph
        .map(|g| {
            g.tags
                .iter()
                .map(|(key, tag)| (key.clone(), tag.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    for row in &rows {
        let key = label_key(&row.facts.label);
        if !names.contains_key(&key) {
            names.insert(key.clone(), row.facts.label.clone());
            order.push(key);
        }
    }
    let edges = |key: &str, pick: fn(&GraphTag) -> &Vec<String>| -> Vec<String> {
        graph
            .and_then(|g| g.tags.get(key))
            .map(|tag| pick(tag).clone())
            .unwrap_or_default()
    };
    let name = |key: &str| names.get(key).cloned().unwrap_or_else(|| key.to_owned());
    // Open comes from the rows alone, so the needs checks below never recurse.
    let open_row = |key: &str| -> Option<&Item> {
        rows.iter().copied().find(|row| {
            label_key(&row.facts.label) == key && !row.state.closed() && row.state.passes == 0
        })
    };
    let own = |key: &str| -> Vec<&TagAttempt> {
        attempts.iter().filter(|a| a.label_key == key).collect()
    };
    let cold_pass = |a: &&TagAttempt| a.right && a.alone && a.cold && !a.slow;
    // Tags with a cold, unaided, full, in-time pass of their own.
    let passed: BTreeMap<String, String> = order
        .iter()
        .filter_map(|key| {
            own(key)
                .into_iter()
                .find(cold_pass)
                .map(|a| (key.clone(), a.place.clone()))
        })
        .collect();
    // Through contains: one edge from any passing tag, or any number of edges
    // to a tag with no attempt of its own.
    let mut reached: BTreeMap<String, String> = BTreeMap::new();
    for from in passed.keys() {
        let mut seen = BTreeSet::new();
        let mut frontier: Vec<(String, usize)> = edges(from, |t| &t.contains)
            .into_iter()
            .map(|k| (k, 1))
            .collect();
        while let Some((key, depth)) = frontier.pop() {
            if !seen.insert(key.clone()) || key == *from {
                continue;
            }
            if depth == 1 || own(&key).is_empty() {
                reached.entry(key.clone()).or_insert_with(|| {
                    if depth == 1 {
                        format!("from {} via contains", name(from))
                    } else {
                        format!("inferred from {} via contains", name(from))
                    }
                });
            }
            for next in edges(&key, |t| &t.contains) {
                frontier.push((next, depth + 1));
            }
        }
    }
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
                let open_in = |field: fn(&GraphTag) -> &Vec<String>| {
                    edges(key, field)
                        .into_iter()
                        .find(|need| open_row(need).is_some())
                };
                if let Some(need) = open_in(|t| &t.key_need) {
                    return (
                        Reading::Conditional,
                        format!("needs {}, which is open", name(&need)),
                    );
                }
                let closed = rows
                    .iter()
                    .find(|row| label_key(&row.facts.label) == *key && row.state.closed());
                let sound = passed
                    .get(key)
                    .map(|place| format!("cold in {place}"))
                    .or_else(|| closed.map(|row| format!("{} closed", row.name())))
                    .or_else(|| reached.get(key).cloned())
                    .or_else(|| {
                        imported
                            .get(key)
                            .filter(|(reading, _)| reading == "sound")
                            .map(|(_, phrase)| format!("memory before the upgrade: {phrase}"))
                    });
                if let Some(why) = sound {
                    if let Some(need) = open_in(|t| &t.needs) {
                        return (
                            Reading::Conditional,
                            format!("needs {}, which is open", name(&need)),
                        );
                    }
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

/// A plan's readings as a tool result states them: the tags under each
/// reading, each with why, and the untested tags by name only.
pub(crate) fn plan_json(
    plan: &str,
    graph: &Result<Graph, String>,
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
    if let Err(problem) = graph {
        value["graph"] = json!(format!(
            "{}: {problem}. Readings use the gap log and attempts only, with no needs, key_need or contains, until it is fixed",
            graph_file(plan)
        ));
    }
    value
}

pub(crate) fn graph_file(plan: &str) -> String {
    format!("Study/{plan}/graph.yaml")
}

/// A plan's graph, or why it cannot be used.
pub(crate) fn read_graph(root: &Path, plan: &str) -> Result<Graph, String> {
    let path = root.join("Study").join(plan).join("graph.yaml");
    if std::fs::symlink_metadata(&path).is_err() {
        return Err("missing".into());
    }
    let text = read_small(&path).ok_or("too large, not text, or not a regular file")?;
    parse_graph(&text)
}

/// The readings of these plans, for a tool result, or None when there are
/// none to state.
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
            let graph = read_graph(root, plan);
            let readings = read_plan(&PlanInputs {
                plan,
                graph: graph.as_ref().ok(),
                items: &items,
                attempts: &attempts,
                imported: &imported,
            });
            plan_json(plan, &graph, &readings, &help)
        })
        .collect::<Vec<_>>();
    Some(json!({
        "plans": values,
        "note": "Magnesium works out each tag's reading from the gap log, the stored attempts and graph.yaml each time, and keeps each tag's help level: the example step last given when the learner had no starting move. Use these; never write a reading or a step into a memory. A tag not listed has no row, attempt or graph entry yet: it is untested.",
    }))
}

/// The tags a plan's readings cover, as keys: its graph's tags and the
/// labels of its rows.
pub(crate) fn plan_tag_keys(root: &Path, plan: &str, items: &[Item]) -> BTreeSet<String> {
    let mut keys: BTreeSet<String> = read_graph(root, plan)
        .map(|graph| graph.order.into_iter().collect())
        .unwrap_or_default();
    keys.extend(
        items
            .iter()
            .filter(|item| item.facts.plan.as_deref() == Some(plan))
            .map(|item| label_key(&item.facts.label)),
    );
    keys
}

/// Each attempt of an update, from `attempt` and `attempts`.
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

/// The `help_levels` field of a committed update whose attempts name a kind
/// of problem: each kind's level now, and the help to give. When the last
/// attempt of a kind in the update had no starting move, the help is for
/// now; otherwise it is for the next time.
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

/// The plans whose readings a result states: every plan on a paper stage
/// load; the plans a committed update's practice touched; and the plan a
/// session started or resumed for names in its goal.
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
        // An attempt concerns a plan when its label or its kind is one of
        // the plan's tags.
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

/// Add what the runtime keeps for the tutor to a tool result: the help levels
/// of the kinds a committed update's attempts name, and the readings of the
/// plans it concerns. Reading only; none of this can fail the call.
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
}

#[cfg(test)]
mod tests;
