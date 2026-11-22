use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use chrono::NaiveDate;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

use super::store::fail;

#[derive(Clone, Debug)]
pub(crate) struct File {
    pub(crate) code: String,
    pub(crate) path: String,
    pub(crate) sha: Option<String>,
    pub(crate) readable: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Item {
    pub(crate) id: String,
    pub(crate) file: Option<String>,
    pub(crate) parent: Option<String>,
    pub(crate) kind: String,
    pub(crate) origin: String,
    pub(crate) title: String,
    pub(crate) reference: Option<String>,
    pub(crate) note: Option<String>,
    pub(crate) start: Option<u32>,
    pub(crate) end: Option<u32>,
    pub(crate) sha: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Target {
    pub(crate) slug: String,
    pub(crate) label: String,
    pub(crate) at: Option<String>,
    pub(crate) hours: Option<f64>,
    pub(crate) length_minutes: Option<i64>,
    pub(crate) goal: Option<String>,
    pub(crate) aids: Option<String>,
    pub(crate) words: Value,
    pub(crate) sessions: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct ScopeRow {
    pub(crate) reference: String,
    pub(crate) state: String,
    pub(crate) reason: Option<String>,
    pub(crate) confirmed: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Block {
    pub(crate) n: i64,
    pub(crate) title: String,
    pub(crate) sections: Vec<String>,
    pub(crate) tasks: Vec<String>,
    pub(crate) minutes: Option<i64>,
    pub(crate) opens_with: Option<String>,
    pub(crate) note: Option<String>,
    pub(crate) done: Option<String>,
    pub(crate) spent: Option<i64>,
}

#[derive(Clone, Debug)]
pub(crate) struct Carry {
    pub(crate) idea: String,
    pub(crate) by: String,
    pub(crate) reserve: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Trace {
    pub(crate) question: String,
    pub(crate) source: String,
    pub(crate) changed: String,
}

#[derive(Default, Debug)]
pub(crate) struct Map {
    pub(crate) revision: i64,
    pub(crate) files: Vec<File>,
    pub(crate) items: Vec<Item>,
    pub(crate) index: BTreeMap<String, usize>,
    pub(crate) children: BTreeMap<String, Vec<String>>,
    pub(crate) uses: BTreeMap<String, Vec<String>>,
    pub(crate) used_by: BTreeMap<String, Vec<String>>,
    pub(crate) targets: Vec<Target>,
    pub(crate) scope: BTreeMap<String, Vec<ScopeRow>>,
    pub(crate) blocks: BTreeMap<String, Vec<Block>>,
    pub(crate) carried: BTreeMap<String, Vec<Carry>>,
    pub(crate) traces: Vec<Trace>,
}

fn strings(text: String) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(&text).unwrap_or_default()
}

impl Map {
    pub(crate) fn load(conn: &Connection) -> Result<Self, String> {
        let mut map = Map {
            revision: conn
                .query_row("SELECT revision FROM meta", [], |r| r.get(0))
                .map_err(fail)?,
            ..Map::default()
        };
        let mut statement = conn
            .prepare("SELECT code, path, content_sha256, readable FROM files ORDER BY path")
            .map_err(fail)?;
        map.files = statement
            .query_map([], |r| {
                Ok(File {
                    code: r.get(0)?,
                    path: r.get(1)?,
                    sha: r.get(2)?,
                    readable: r.get::<_, i64>(3)? == 1,
                })
            })
            .map_err(fail)?
            .collect::<Result<_, _>>()
            .map_err(fail)?;
        let mut statement = conn
            .prepare(
                "SELECT items.id, items.file, items.parent, items.kind, items.origin, items.title, items.note,
                        items.anchor_start, items.anchor_end, items.content_sha256, items.ref
                 FROM items LEFT JOIN files ON files.code = items.file
                 ORDER BY files.path IS NULL, files.path, items.position, items.id",
            )
            .map_err(fail)?;
        map.items = statement
            .query_map([], |r| {
                Ok(Item {
                    id: r.get(0)?,
                    file: r.get(1)?,
                    parent: r.get(2)?,
                    kind: r.get(3)?,
                    origin: r.get(4)?,
                    title: r.get(5)?,
                    note: r.get(6)?,
                    start: r.get(7)?,
                    end: r.get(8)?,
                    sha: r.get(9)?,
                    reference: r.get(10)?,
                })
            })
            .map_err(fail)?
            .collect::<Result<_, _>>()
            .map_err(fail)?;
        for (at, item) in map.items.iter().enumerate() {
            map.index.insert(item.id.clone(), at);
            if let Some(parent) = &item.parent {
                map.children
                    .entry(parent.clone())
                    .or_default()
                    .push(item.id.clone());
            }
        }
        let mut statement = conn
            .prepare("SELECT item, uses FROM uses ORDER BY item, uses")
            .map_err(fail)?;
        for row in statement
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(fail)?
        {
            let (item, used) = row.map_err(fail)?;
            map.used_by
                .entry(used.clone())
                .or_default()
                .push(item.clone());
            map.uses.entry(item).or_default().push(used);
        }
        let mut statement = conn
            .prepare("SELECT slug, label, at, hours, length_minutes, goal, aids, words, sessions FROM targets ORDER BY slug")
            .map_err(fail)?;
        map.targets = statement
            .query_map([], |r| {
                Ok(Target {
                    slug: r.get(0)?,
                    label: r.get(1)?,
                    at: r.get(2)?,
                    hours: r.get(3)?,
                    length_minutes: r.get(4)?,
                    goal: r.get(5)?,
                    aids: r.get(6)?,
                    words: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or(json!([])),
                    sessions: strings(r.get(8)?),
                })
            })
            .map_err(fail)?
            .collect::<Result<_, _>>()
            .map_err(fail)?;
        let mut statement = conn
            .prepare("SELECT target, ref, state, reason, confirmed FROM scope ORDER BY target, position, ref")
            .map_err(fail)?;
        for row in statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    ScopeRow {
                        reference: r.get(1)?,
                        state: r.get(2)?,
                        reason: r.get(3)?,
                        confirmed: r.get::<_, i64>(4)? == 1,
                    },
                ))
            })
            .map_err(fail)?
        {
            let (target, row) = row.map_err(fail)?;
            map.scope.entry(target).or_default().push(row);
        }
        let mut statement = conn
            .prepare("SELECT target, n, title, sections, tasks, minutes, opens_with, note, done, spent FROM blocks ORDER BY target, n")
            .map_err(fail)?;
        for row in statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    Block {
                        n: r.get(1)?,
                        title: r.get(2)?,
                        sections: strings(r.get(3)?),
                        tasks: strings(r.get(4)?),
                        minutes: r.get(5)?,
                        opens_with: r.get(6)?,
                        note: r.get(7)?,
                        done: r.get(8)?,
                        spent: r.get(9)?,
                    },
                ))
            })
            .map_err(fail)?
        {
            let (target, block) = row.map_err(fail)?;
            map.blocks.entry(target).or_default().push(block);
        }
        let mut statement = conn
            .prepare("SELECT target, idea, by_item, reserve FROM carried ORDER BY target, idea")
            .map_err(fail)?;
        for row in statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    Carry {
                        idea: r.get(1)?,
                        by: r.get(2)?,
                        reserve: r.get(3)?,
                    },
                ))
            })
            .map_err(fail)?
        {
            let (target, carry) = row.map_err(fail)?;
            map.carried.entry(target).or_default().push(carry);
        }
        let mut statement = conn
            .prepare("SELECT question, source, changed FROM traces ORDER BY question, source")
            .map_err(fail)?;
        map.traces = statement
            .query_map([], |r| {
                Ok(Trace {
                    question: r.get(0)?,
                    source: r.get(1)?,
                    changed: r.get(2)?,
                })
            })
            .map_err(fail)?
            .collect::<Result<_, _>>()
            .map_err(fail)?;
        Ok(map)
    }

    pub(crate) fn item(&self, id: &str) -> Option<&Item> {
        self.index.get(id).map(|&at| &self.items[at])
    }

    pub(crate) fn target(&self, slug: &str) -> Option<&Target> {
        self.targets.iter().find(|t| t.slug == slug)
    }

    pub(crate) fn file(&self, code: &str) -> Option<&File> {
        self.files.iter().find(|f| f.code == code)
    }

    pub(crate) fn file_by_ref(&self, reference: &str) -> Option<&File> {
        let reference = reference.trim().trim_start_matches("./");
        self.files
            .iter()
            .find(|f| f.code == reference.to_lowercase() || f.path == reference)
    }

    pub(crate) fn title(&self, id: &str) -> String {
        let Some(item) = self.item(id) else {
            return id.to_owned();
        };
        let word = match item.kind.as_str() {
            "exercise" => "Exercise",
            "question" => "Question",
            "example" => "Example",
            _ => return item.title.clone(),
        };
        let reference = item.reference.as_deref().unwrap_or("");
        if reference.is_empty()
            || !reference.chars().any(|c| c.is_ascii_digit())
            || item
                .title
                .to_lowercase()
                .contains(&reference.to_lowercase())
        {
            return item.title.clone();
        }
        format!("{word} {reference}: {}", item.title)
    }

    pub(crate) fn is_leaf(&self, id: &str) -> bool {
        self.children.get(id).is_none_or(Vec::is_empty)
    }

    fn ancestors(&self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut at = self.item(id).and_then(|i| i.parent.clone());
        while let Some(parent) = at {
            if out.contains(&parent) {
                break;
            }
            at = self.item(&parent).and_then(|i| i.parent.clone());
            out.push(parent);
        }
        out
    }

    pub(crate) fn top(&self, id: &str) -> String {
        self.ancestors(id)
            .last()
            .cloned()
            .unwrap_or_else(|| id.to_owned())
    }

    fn descendants(&self, id: &str, out: &mut Vec<String>) {
        for child in self.children.get(id).into_iter().flatten() {
            out.push(child.clone());
            self.descendants(child, out);
        }
    }

    fn state_for(&self, rows: &BTreeMap<&str, &ScopeRow>, item: &Item) -> Option<String> {
        if let Some(row) = rows.get(item.id.as_str()) {
            return Some(row.state.clone());
        }
        for parent in self.ancestors(&item.id) {
            if let Some(row) = rows.get(parent.as_str()) {
                return Some(row.state.clone());
            }
        }
        item.file
            .as_deref()
            .and_then(|file| rows.get(file))
            .map(|row| row.state.clone())
    }

    pub(crate) fn in_scope(&self, slug: &str) -> Scoped {
        let rows = self.scope.get(slug).cloned().unwrap_or_default();
        let by_ref: BTreeMap<&str, &ScopeRow> =
            rows.iter().map(|r| (r.reference.as_str(), r)).collect();
        let mut order: Vec<String> = Vec::new();
        let mut seen = BTreeSet::new();
        for row in rows.iter().filter(|r| r.state == "in") {
            let mut ids = Vec::new();
            if self.file(&row.reference).is_some() {
                ids.extend(
                    self.items
                        .iter()
                        .filter(|i| i.file.as_deref() == Some(row.reference.as_str()))
                        .map(|i| i.id.clone()),
                );
            } else if self.item(&row.reference).is_some() {
                ids.push(row.reference.clone());
                self.descendants(&row.reference, &mut ids);
            }
            for id in ids {
                if seen.insert(id.clone()) {
                    order.push(id);
                }
            }
        }
        let mut cut = Vec::new();
        let items = order
            .into_iter()
            .filter(|id| {
                let item = &self.items[self.index[id]];
                if item.kind == "task" {
                    return false;
                }
                match self.state_for(&by_ref, item).as_deref() {
                    Some("cut") => {
                        cut.push(id.clone());
                        false
                    }
                    _ => true,
                }
            })
            .collect::<Vec<_>>();
        let cuts = rows.iter().filter(|r| r.state == "cut").cloned().collect();
        Scoped { items, cut, cuts }
    }
}

pub(crate) struct Scoped {
    pub(crate) items: Vec<String>,
    pub(crate) cut: Vec<String>,
    pub(crate) cuts: Vec<ScopeRow>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Record {
    pub(crate) asked: usize,
    pub(crate) passes: usize,
    pub(crate) helped: usize,
    pub(crate) last_ms: Option<i64>,
}

#[derive(Default)]
pub(crate) struct Evidence {
    pub(crate) by_label: BTreeMap<String, Record>,
    pub(crate) due: BTreeSet<String>,
    pub(crate) read: bool,
}

impl Evidence {
    pub(crate) fn load(store: &Path, root: &Path, today: NaiveDate) -> Self {
        let mut evidence = Evidence::default();
        if !store.is_file() {
            return evidence;
        }
        let Ok(conn) = Connection::open_with_flags(store, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
            return evidence;
        };
        let _ = conn.busy_timeout(std::time::Duration::from_secs(2));
        if let Ok(mut statement) = conn.prepare(
            "SELECT label_key, coalesce(problem_kind, ''), mark, help, cold, answered_at_ms FROM attempts",
        ) && let Ok(rows) = statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, bool>(4)?,
                r.get::<_, i64>(5)?,
            ))
        }) {
            evidence.read = true;
            for (label, kind, mark, help, cold, at) in rows.flatten() {
                let mut keys = vec![label];
                let kind = crate::learner::label_key(&kind);
                if !kind.is_empty() && !keys.contains(&kind) {
                    keys.push(kind);
                }
                for key in keys {
                    let record = evidence.by_label.entry(key).or_default();
                    record.asked += 1;
                    if mark == "right" && help == "none" && cold {
                        record.passes += 1;
                    }
                    if help != "none" {
                        record.helped += 1;
                    }
                    record.last_ms = Some(record.last_ms.map_or(at, |last| last.max(at)));
                }
            }
        }
        if let Ok(items) = crate::practice::snapshot(&conn, root) {
            for item in items {
                if !item.state.closed() && item.state.due_on(today) {
                    evidence
                        .due
                        .insert(crate::learner::label_key(&item.facts.label));
                }
            }
        }
        evidence
    }

    pub(crate) fn of(&self, id: &str) -> Record {
        self.by_label.get(id).cloned().unwrap_or_default()
    }

    pub(crate) fn due(&self, id: &str) -> bool {
        self.due.contains(id)
    }

    pub(crate) fn phrase(&self, id: &str) -> String {
        let record = self.of(id);
        let mut text = match (record.asked, record.passes) {
            (0, _) => "never asked".to_owned(),
            (asked, 0) => format!("asked {asked}, no blank-page pass"),
            (asked, passes) => format!("asked {asked}, {passes} with no help and cold"),
        };
        if self.due(id) {
            text.push_str(", due");
        }
        text
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Counts {
    pub(crate) items: usize,
    pub(crate) never_asked: usize,
    pub(crate) no_pass: usize,
    pub(crate) due: usize,
}

impl Counts {
    pub(crate) fn add(&mut self, evidence: &Evidence, id: &str) {
        let record = evidence.of(id);
        self.items += 1;
        if record.asked == 0 {
            self.never_asked += 1;
        }
        if record.passes == 0 {
            self.no_pass += 1;
        }
        if evidence.due(id) {
            self.due += 1;
        }
    }

    pub(crate) fn to_json(self) -> Value {
        json!({
            "items": self.items,
            "never_asked": self.never_asked,
            "no_blank_page_pass": self.no_pass,
            "due": self.due,
        })
    }
}

pub(crate) struct Planned {
    pub(crate) asked_by: BTreeMap<String, Vec<String>>,
    pub(crate) carried_by: BTreeMap<String, (String, Option<String>)>,
    pub(crate) in_block: BTreeMap<String, i64>,
}

pub(crate) fn planned(map: &Map, slug: &str) -> Planned {
    let mut asked_by: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut in_block = BTreeMap::new();
    for block in map.blocks.get(slug).into_iter().flatten() {
        for section in &block.sections {
            in_block.entry(section.clone()).or_insert(block.n);
            let mut under = Vec::new();
            map.descendants(section, &mut under);
            for id in under {
                in_block.entry(id).or_insert(block.n);
            }
        }
        for task in &block.tasks {
            let tag = format!("{} (block {})", task, block.n);
            asked_by.entry(task.clone()).or_default().push(tag.clone());
            for used in map.uses.get(task).into_iter().flatten() {
                asked_by.entry(used.clone()).or_default().push(tag.clone());
            }
        }
    }
    let carried_by = map
        .carried
        .get(slug)
        .into_iter()
        .flatten()
        .map(|c| (c.idea.clone(), (c.by.clone(), c.reserve.clone())))
        .collect();
    Planned {
        asked_by,
        carried_by,
        in_block,
    }
}

impl Planned {
    pub(crate) fn asked(&self, id: &str) -> bool {
        self.asked_by.contains_key(id) || self.carried_by.contains_key(id)
    }
}

pub(crate) fn reach_of(map: &Map, evidence: &Evidence, id: &str) -> Value {
    let mut used: Vec<String> = map.uses.get(id).cloned().unwrap_or_default();
    for child in map.children.get(id).into_iter().flatten() {
        if !used.contains(child) {
            used.push(child.clone());
        }
    }
    let mut counts = Counts::default();
    for used in &used {
        counts.add(evidence, used);
    }
    let item = map.item(id);
    json!({
        "id": id,
        "title": item.map(|i| i.title.clone()),
        "kind": item.map(|i| i.kind.clone()),
        "uses": counts.items,
        "never_asked": counts.never_asked,
        "no_blank_page_pass": counts.no_pass,
        "due": counts.due,
        "itself": evidence.phrase(id),
    })
}

pub(crate) fn days_left(at: Option<&str>, today: NaiveDate) -> Option<i64> {
    let date = NaiveDate::parse_from_str(at?.get(..10)?, "%Y-%m-%d").ok()?;
    Some((date - today).num_days())
}
