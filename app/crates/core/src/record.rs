use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const MARK: &str = "<record>";
const END: &str = "</record>";

const MAX_NAME: usize = 120;
const MAX_QUESTION: usize = 1000;
const MAX_NOTE: usize = 500;
const MAX_STEPS: usize = 12;
const MAX_GOAL: usize = 500;

pub const TASK_LINE: &str = "Open task, posed to the learner: ";
pub const HELP_LINE: &str = "Help on it so far: ";
pub const CHECK_LINE: &str = "It is the quick check ";
pub const REDO_LINE: &str = "It is the paper redo ";
pub const GUESS_LINE: &str = "It asks for a guess before teaching: its answer gets no mark.";
pub const AGAIN_LINE: &str =
    "It was asked again after its mark: its next answer is not a first try.";

pub const HINT_WORDS: &str = "Can I have a hint?";
pub const DONT_KNOW_WORDS: &str = "I don't know";
pub const LOST_WORDS: &str = "I'm lost";

#[derive(Debug, Default)]
pub struct Cutter {
    text: String,
    shown: usize,
    cut: Option<usize>,
}

impl Cutter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, more: &str) -> String {
        self.text.push_str(more);
        if self.cut.is_some() {
            return String::new();
        }
        let mut from = self.shown.saturating_sub(MARK.len());
        while !self.text.is_char_boundary(from) {
            from -= 1;
        }
        if let Some(at) = self.text[from..].find(MARK).map(|at| from + at) {
            self.cut = Some(at);
            let end = trim_end(&self.text, self.shown, at);
            let fresh = self.text[self.shown..end].to_owned();
            self.shown = end.max(self.shown);
            return fresh;
        }
        let bytes = self.text.as_bytes();
        let hold = (1..MARK.len())
            .rev()
            .find(|k| *k <= bytes.len() && MARK.as_bytes().starts_with(&bytes[bytes.len() - k..]))
            .unwrap_or(0);
        let end = trim_end(&self.text, self.shown, self.text.len() - hold);
        let fresh = self.text[self.shown..end].to_owned();
        self.shown = end.max(self.shown);
        fresh
    }

    pub fn reply(&self) -> String {
        let end = self.cut.unwrap_or(self.text.len());
        let reply = &self.text[..end];
        let reply = match self.cut {
            Some(_) => reply,
            None => {
                let bytes = reply.as_bytes();
                let hold = (1..MARK.len())
                    .rev()
                    .find(|k| {
                        *k <= bytes.len() && MARK.as_bytes().starts_with(&bytes[bytes.len() - k..])
                    })
                    .unwrap_or(0);
                &reply[..reply.len() - hold]
            }
        };
        reply.trim().to_owned()
    }

    pub fn block(&self) -> Option<&str> {
        let rest = &self.text[self.cut? + MARK.len()..];
        Some(match rest.find(END) {
            Some(end) => &rest[..end],
            None => rest,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

fn trim_end(text: &str, floor: usize, end: usize) -> usize {
    let trimmed = text[..end].trim_end_matches([' ', '\n', '\t', '\r']).len();
    trimmed.max(floor).min(end.max(floor))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    Right,
    Partly,
    Wrong,
    Declined,
}

impl Mark {
    fn word(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Partly => "partly",
            Self::Wrong => "wrong",
            Self::Declined => "declined",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Help {
    #[default]
    None,
    Hint,
    Step,
    Solution,
}

impl Help {
    fn word(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Hint => "hint",
            Self::Step => "step",
            Self::Solution => "solution",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub step: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryNote {
    #[serde(default)]
    pub id: Option<String>,
    pub skill: String,
    pub text: String,
    #[serde(default)]
    pub cleared: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub answer: Option<Mark>,
    pub help: Help,
    pub skill: String,
    pub kind: Option<String>,
    pub no_start: bool,
    pub note: String,
    pub task: Option<String>,
    pub guess: bool,
    pub steps: Option<Vec<Step>>,
    pub memory: Option<MemoryNote>,
    pub cold: bool,
    pub slow: bool,
    pub next: Option<Next>,
    pub repair: Option<RepairNote>,
    pub repair_unreadable: bool,
    pub goal: Option<String>,
    #[serde(default)]
    pub sitting_ends: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairNote {
    pub number: i64,
    pub done: String,
    pub next: Option<Next>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Next {
    pub question: String,
    pub key: String,
}

pub fn parse(block: &str) -> Result<Record, String> {
    let text = block.trim();
    let (Some(open), Some(close)) = (text.find('{'), text.rfind('}')) else {
        return Err("it holds no JSON object".into());
    };
    let value: Value = serde_json::from_str(&text[open..=close])
        .map_err(|e| format!("its JSON does not parse ({e})"))?;
    let object = value.as_object().ok_or("it is not a JSON object")?;
    let text_of = |key: &str| -> Result<Option<String>, String> {
        match object.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.trim().to_owned())),
            Some(_) => Err(format!("{key} is not a string")),
        }
    };
    let flag = |key: &str| -> Result<bool, String> {
        match object.get(key) {
            None | Some(Value::Null) => Ok(false),
            Some(Value::Bool(b)) => Ok(*b),
            Some(_) => Err(format!("{key} is not true or false")),
        }
    };
    if !object.contains_key("answer") {
        return Err("answer is missing: every reply says whether the message was an answer".into());
    }
    let answer = match text_of("answer")?.as_deref() {
        None => None,
        Some("right") => Some(Mark::Right),
        Some("partly") => Some(Mark::Partly),
        Some("wrong") => Some(Mark::Wrong),
        Some("declined") => Some(Mark::Declined),
        Some(other) => {
            return Err(format!(
                "answer {other:?} is not right, partly, wrong, declined or null"
            ))
        }
    };
    let help = match text_of("help")?.as_deref() {
        None | Some("none") => Help::None,
        Some("hint") => Help::Hint,
        Some("step") => Help::Step,
        Some("solution") => Help::Solution,
        Some(other) => {
            return Err(format!(
                "help {other:?} is not none, hint, step or solution"
            ))
        }
    };
    let skill = text_of("skill")?.unwrap_or_default();
    if answer.is_some() && skill.is_empty() {
        return Err("an answer needs a skill".into());
    }
    let steps = match object.get("steps") {
        None | Some(Value::Null) => None,
        Some(Value::Array(items)) => {
            if items.len() > MAX_STEPS {
                return Err(format!("steps has more than {MAX_STEPS} entries"));
            }
            let mut steps = Vec::new();
            for item in items {
                let step = item["step"].as_str().map(str::trim).unwrap_or_default();
                let status = item["status"].as_str().unwrap_or_default();
                if step.is_empty() || !matches!(status, "todo" | "doing" | "done") {
                    return Err("each step needs a step and a status of todo, doing or done".into());
                }
                steps.push(Step {
                    step: clip(step, MAX_NAME),
                    status: status.to_owned(),
                });
            }
            Some(steps)
        }
        Some(_) => return Err("steps is not a list".into()),
    };
    let memory = match object.get("memory") {
        None | Some(Value::Null) => None,
        Some(Value::Object(note)) => {
            let skill = note
                .get("skill")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(skill.as_str())
                .to_owned();
            let text = note
                .get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default()
                .to_owned();
            if skill.is_empty() || text.is_empty() {
                return Err("memory needs a skill and a text".into());
            }
            Some(MemoryNote {
                id: note
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned),
                skill: clip(&skill, MAX_NAME),
                text: clip(&text, MAX_NOTE),
                cleared: note
                    .get("cleared")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        }
        Some(_) => return Err("memory is not an object".into()),
    };
    let next = object.get("next").and_then(next_of);
    let (repair, repair_unreadable) = match object.get("repair") {
        None | Some(Value::Null) => (None, false),
        Some(Value::Object(note)) => {
            let number = note.get("number").and_then(Value::as_i64);
            let done = note
                .get("done")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|done| !done.is_empty());
            match (number, done) {
                (Some(number), Some(done)) => (
                    Some(RepairNote {
                        number,
                        done: clip(done, MAX_NOTE),
                        next: note.get("next").and_then(next_of),
                    }),
                    false,
                ),
                _ => (None, true),
            }
        }
        Some(_) => (None, true),
    };
    Ok(Record {
        answer,
        help,
        skill: clip(&skill, MAX_NAME),
        kind: text_of("kind")?.map(|kind| clip(&kind, MAX_NAME)),
        no_start: flag("no_start")?,
        note: clip(&text_of("note")?.unwrap_or_default(), MAX_NOTE),
        task: text_of("task")?,
        guess: flag("guess")?,
        steps,
        memory,
        cold: flag("cold")?,
        slow: flag("slow")?,
        next,
        repair,
        repair_unreadable,
        goal: text_of("goal")?.map(|goal| clip(&goal, MAX_GOAL)),
        sitting_ends: flag("sitting_ends")?,
    })
}

fn next_of(value: &Value) -> Option<Next> {
    let next = value.as_object()?;
    let text = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| next.get(*key).and_then(Value::as_str))
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| clip(text, MAX_QUESTION))
    };
    Some(Next {
        question: text(&["question"])?,
        key: text(&["key", "answer_key"])?,
    })
}

pub fn clip(text: &str, bytes: usize) -> String {
    if text.len() <= bytes {
        return text.to_owned();
    }
    let mut end = bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].trim_end().to_owned()
}

fn same_task(a: &str, b: &str) -> bool {
    let words = |text: &str| {
        text.split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>()
            .join(" ")
    };
    words(a) == words(b)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DueCheck {
    pub number: i64,
    pub label: String,
    #[serde(default)]
    pub misses: Vec<String>,
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct DueRow {
    item: String,
    #[serde(default)]
    plan: Option<String>,
    tag: String,
    #[serde(default, rename = "type")]
    gap_type: Option<String>,
    #[serde(default)]
    what_went_wrong: String,
    #[serde(default)]
    paper: Option<String>,
    #[serde(default)]
    paper_question: Option<String>,
    #[serde(default)]
    question: Option<String>,
    #[serde(default)]
    key: Option<String>,
}

pub const MAX_REDOS: usize = 6;
const MAX_ROWS: usize = 12;
const MAX_ROW_BYTES: usize = 1200;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DueRedo {
    pub number: i64,
    pub rows: Vec<String>,
    pub tag: String,
    #[serde(default)]
    pub timed: bool,
    #[serde(default)]
    pub from: Vec<String>,
    #[serde(default)]
    pub misses: Vec<String>,
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
}

pub fn redos_from(rows: &Value, answered_today: &dyn Fn(&str) -> bool) -> Vec<DueRedo> {
    let rows: Vec<DueRow> = rows
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| serde_json::from_value(row.clone()).ok())
        .collect();
    let mut redos: Vec<(Option<String>, DueRedo)> = Vec::new();
    for row in rows {
        let (item, tag) = (row.item.trim(), row.tag.trim());
        if item.is_empty() || tag.is_empty() || answered_today(tag) {
            continue;
        }
        let key = tag.to_lowercase();
        let at = match redos
            .iter()
            .position(|(_, redo)| redo.tag.to_lowercase() == key)
        {
            Some(at) => {
                let (plan, redo) = &redos[at];
                let bytes: usize = redo.rows.iter().map(String::len).sum();
                if *plan != row.plan
                    || redo.rows.len() >= MAX_ROWS
                    || bytes + item.len() > MAX_ROW_BYTES
                {
                    continue;
                }
                at
            }
            None => {
                redos.push((
                    row.plan.clone(),
                    DueRedo {
                        number: 0,
                        rows: Vec::new(),
                        tag: tag.to_owned(),
                        timed: false,
                        from: Vec::new(),
                        misses: Vec::new(),
                        question: None,
                        key: None,
                    },
                ));
                redos.len() - 1
            }
        };
        let redo = &mut redos[at].1;
        redo.rows.push(item.to_owned());
        redo.timed |= row.gap_type.as_deref() == Some("S");
        let from = [row.paper, row.paper_question]
            .into_iter()
            .flatten()
            .filter(|part| !part.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" Q");
        if !from.is_empty() && !redo.from.contains(&from) {
            redo.from.push(from);
        }
        let miss = row.what_went_wrong.trim().to_owned();
        if !miss.is_empty() && !redo.misses.contains(&miss) {
            redo.misses.push(miss);
        }
        if redo.question.is_none() {
            if let Some(question) = row.question.filter(|q| !q.trim().is_empty()) {
                redo.question = Some(question);
                redo.key = row.key.filter(|k| !k.trim().is_empty());
            }
        }
    }
    redos
        .into_iter()
        .take(MAX_REDOS)
        .zip(1..)
        .map(|((_, redo), number)| DueRedo { number, ..redo })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Redo {
    #[serde(default)]
    pub number: i64,
    pub rows: Vec<String>,
    pub tag: String,
    #[serde(default)]
    pub timed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenTask {
    pub text: String,
    pub help: Help,
    pub guess: bool,
    pub check: Option<String>,
    #[serde(default)]
    pub redo: Option<Redo>,
    pub again: bool,
    #[serde(default)]
    pub stuck: bool,
    #[serde(default)]
    pub skill: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Lesson {
    pub session_id: Option<String>,
    pub revision: Option<i64>,
    pub goal: Option<String>,
    pub task: Option<OpenTask>,
    pub steps: Vec<Step>,
    pub checks: Vec<DueCheck>,
    #[serde(default)]
    pub redos: Vec<DueRedo>,
    pub turns: u64,
    pub today: String,
    pub answered_today: Vec<String>,
    #[serde(default)]
    pub sitting_labels: Vec<String>,
    #[serde(default)]
    pub posed_checks: Vec<DueCheck>,
    #[serde(default)]
    pub repairs: Vec<WaitingRepair>,
    #[serde(default)]
    pub repair_numbers: i64,
    #[serde(default)]
    pub sitting_unknown: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitingRepair {
    pub number: i64,
    pub rows: Vec<String>,
    pub tag: String,
    #[serde(default)]
    pub plan: Option<String>,
    #[serde(default)]
    pub from: Vec<String>,
    #[serde(default)]
    pub misses: Vec<String>,
    #[serde(default)]
    pub failed_here: bool,
}

const MAX_REPAIR_ROWS: usize = 12;

impl Lesson {
    pub fn begin_sitting(&mut self) {
        self.checks.clear();
        self.redos.clear();
        self.sitting_labels.clear();
        self.posed_checks.clear();
        self.repairs.clear();
    }

    pub fn take_waiting(&mut self, rows: &Value, failed_here: bool) -> Vec<WaitingRepair> {
        let mut made: Vec<i64> = Vec::new();
        for row in rows.as_array().into_iter().flatten() {
            let Ok(row) = serde_json::from_value::<DueRow>(row.clone()) else {
                continue;
            };
            let (item, tag) = (row.item.trim(), row.tag.trim());
            if item.is_empty()
                || tag.is_empty()
                || self
                    .repairs
                    .iter()
                    .any(|repair| repair.rows.iter().any(|r| r == item))
            {
                continue;
            }
            let at = match self.repairs.iter().position(|repair| {
                repair.tag.eq_ignore_ascii_case(tag)
                    && repair.plan == row.plan
                    && repair.rows.len() < MAX_REPAIR_ROWS
                    && made.contains(&repair.number)
            }) {
                Some(at) => at,
                None => {
                    self.repair_numbers += 1;
                    made.push(self.repair_numbers);
                    self.repairs.push(WaitingRepair {
                        number: self.repair_numbers,
                        rows: Vec::new(),
                        tag: tag.to_owned(),
                        plan: row.plan.clone(),
                        from: Vec::new(),
                        misses: Vec::new(),
                        failed_here,
                    });
                    self.repairs.len() - 1
                }
            };
            let repair = &mut self.repairs[at];
            repair.rows.push(item.to_owned());
            let from = [row.paper, row.paper_question]
                .into_iter()
                .flatten()
                .filter(|part| !part.trim().is_empty())
                .collect::<Vec<_>>()
                .join(" Q");
            if !from.is_empty() && !repair.from.contains(&from) {
                repair.from.push(from);
            }
            let miss = row.what_went_wrong.trim().to_owned();
            if !miss.is_empty() && !repair.misses.contains(&miss) {
                repair.misses.push(miss);
            }
        }
        self.repairs
            .iter()
            .filter(|repair| made.contains(&repair.number))
            .cloned()
            .collect()
    }

    pub fn touch(&mut self, label: &str) {
        let key = label.trim().to_lowercase();
        if !key.is_empty() && !self.sitting_labels.contains(&key) {
            self.sitting_labels.push(key);
        }
    }

    pub fn touched(&self, label: &str) -> bool {
        self.sitting_labels.contains(&label.trim().to_lowercase())
    }

    fn touch_task(&mut self, task: &OpenTask) {
        for label in task_labels(task) {
            self.touch(&label);
        }
    }

    pub fn forget_task(&mut self) -> (Option<OpenTask>, Option<DueCheck>) {
        let Some(task) = self.task.take() else {
            return (None, None);
        };
        self.touch_task(&task);
        let back = task.check.as_ref().and_then(|label| {
            let due = self
                .posed_checks
                .iter()
                .find(|due| &due.label == label)
                .cloned()?;
            if !self.checks.iter().any(|listed| listed.number == due.number) {
                self.checks.push(due.clone());
                self.checks.sort_by_key(|listed| listed.number);
            }
            Some(due)
        });
        (Some(task), back)
    }

    pub fn set_today(&mut self, day: &str) {
        if self.today != day {
            self.today = day.to_owned();
            self.answered_today.clear();
        }
    }

    pub fn spend_shown(&mut self, reply: &str) {
        let shown: Vec<i64> = redo_marks(reply)
            .into_iter()
            .map(|(_, number)| number)
            .collect();
        self.redos.retain(|due| !shown.contains(&due.number));
    }

    pub fn answered(&mut self, label: &str) {
        let key = label.trim().to_lowercase();
        if !self.answered_today.contains(&key) {
            self.answered_today.push(key);
        }
    }

    pub fn answered_earlier_today(&self, label: &str) -> bool {
        self.answered_today.contains(&label.trim().to_lowercase())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Message {
    pub words: String,
    pub event_id: Option<String>,
    pub no_clock: bool,
    pub talk: bool,
}

impl Message {
    fn is(&self, words: &str) -> bool {
        self.words.trim().eq_ignore_ascii_case(words)
    }

    pub fn confidence(&self) -> Option<&'static str> {
        self.words.lines().rev().find_map(|line| {
            match line.trim().strip_prefix("Confidence:")?.trim() {
                "sure" | "Sure" => Some("sure"),
                "unsure" | "Unsure" => Some("unsure"),
                "guess" | "Guess" => Some("guess"),
                _ => None,
            }
        })
    }
}

#[derive(Clone, Debug)]
pub struct Write {
    pub request: Value,
    pub marked: bool,
    pub lost: Option<&'static str>,
    pub redo: Option<RedoAnswer>,
    pub check: Option<bool>,
    pub not_a_redo: Option<NotARedo>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedoAnswer {
    pub tag: String,
    pub rows: Vec<String>,
    pub right: bool,
    pub other_label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotARedo {
    Unlisted,
    SetAside,
}

fn task_labels(task: &OpenTask) -> Vec<String> {
    task.check
        .iter()
        .chain(task.redo.as_ref().map(|redo| &redo.tag))
        .chain(std::iter::once(&task.skill))
        .filter(|name| !name.trim().is_empty())
        .cloned()
        .collect()
}

fn plan_text(task: &OpenTask) -> String {
    let mut lines = vec![
        format!("{TASK_LINE}{}", clip(&task.text, MAX_QUESTION)),
        format!("{HELP_LINE}{}.", task.help.word()),
    ];
    if let Some(check) = &task.check {
        lines.push(format!("{CHECK_LINE}\"{check}\""));
    }
    if let Some(redo) = &task.redo {
        lines.push(format!("{REDO_LINE}{}", json!(redo)));
    }
    if task.guess {
        lines.push(GUESS_LINE.to_owned());
    }
    if task.again {
        lines.push(AGAIN_LINE.to_owned());
    }
    lines.join("\n")
}

pub const STATE_LIMIT: usize = 16 * 1024;
const SUMMARY_LIMIT: usize = 1024;

const FOUND: &str = "Misunderstanding found in a lesson: ";
const CLEARED: &str = "Misunderstanding cleared in a lesson: ";
const LEAST_TEXT: usize = 24;

fn is_own(line: &str) -> bool {
    line.starts_with(FOUND) || line.starts_with(CLEARED)
}

pub fn memory_edit(note: &MemoryNote, found: Option<&Value>, goal: &str) -> Option<Value> {
    let prefix = if note.cleared { CLEARED } else { FOUND };
    let new = json!({"new_observation": "answer"});
    let reason = if note.cleared {
        "The misunderstanding was cleared."
    } else {
        "A misunderstanding was found."
    };
    let Some(memory) = found else {
        let line = clip(&format!("{prefix}{}", note.text), SUMMARY_LIMIT);
        return Some(json!({
            "operation": "create",
            "client_ref": "memory",
            "record": {
                "title": note.skill,
                "scope": clip(if goal.is_empty() { note.skill.as_str() } else { goal }, MAX_NAME),
                "summary": line,
                "state_text": line,
                "current_basis": [new],
            },
            "reason": reason,
            "change_basis": [new],
        }));
    };
    let state = memory["state_text"].as_str().unwrap_or_default();
    let mut lines: Vec<&str> = if state.is_empty() {
        Vec::new()
    } else {
        state.split('\n').collect()
    };
    let own = lines.iter().position(|line| is_own(line));
    let others: usize = lines
        .iter()
        .enumerate()
        .filter(|(at, _)| Some(*at) != own)
        .map(|(_, line)| line.len() + 1)
        .sum();
    let room = STATE_LIMIT.saturating_sub(others);
    if room < prefix.len() + LEAST_TEXT {
        return None;
    }
    let line = clip(&format!("{prefix}{}", note.text), room);
    match own {
        Some(at) => lines[at] = &line,
        None => lines.push(&line),
    }
    let state_text = lines.join("\n");
    debug_assert!(state_text.len() <= STATE_LIMIT);
    let summary = memory["summary"].as_str().unwrap_or_default();
    let summary = if summary.is_empty() || is_own(summary) {
        clip(&line, SUMMARY_LIMIT)
    } else {
        summary.to_owned()
    };
    let mut basis: Vec<Value> = memory["current_basis"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    basis.push(new.clone());
    let keep = basis.len().saturating_sub(8);
    let basis: Vec<Value> = basis.into_iter().skip(keep).collect();
    Some(json!({
        "operation": "revise",
        "memory_id": memory["memory_id"],
        "expected_revision": memory["revision"],
        "record": {
            "title": memory["title"],
            "scope": memory["scope"],
            "summary": summary,
            "state_text": state_text,
            "current_basis": basis,
        },
        "change_kind": "new_observation",
        "reason": reason,
        "change_basis": [new],
    }))
}

pub fn posed_check(reply: &str, checks: &[DueCheck]) -> Option<DueCheck> {
    let lower = reply.to_lowercase();
    checks
        .iter()
        .find(|check| {
            let needle = format!("quick check {}", check.number);
            lower.match_indices(&needle).any(|(start, _)| {
                !lower[start + needle.len()..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit())
            })
        })
        .cloned()
}

fn check_at(reply: &str, number: i64) -> Option<usize> {
    let lower = reply.to_lowercase();
    let needle = format!("quick check {number}");
    lower
        .rmatch_indices(&needle)
        .find(|(start, _)| {
            !lower[start + needle.len()..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        })
        .map(|(start, _)| start)
}

pub fn redo_marks(reply: &str) -> Vec<(usize, i64)> {
    const WORDS: &str = "paper redo ";
    let lower = reply.to_lowercase();
    lower
        .match_indices(WORDS)
        .filter_map(|(start, _)| {
            let rest = &lower[start + WORDS.len()..];
            let digits = rest.chars().take_while(char::is_ascii_digit).count();
            let number = rest[..digits].parse().ok()?;
            rest[digits..]
                .trim_start_matches(['*', '_', ' '])
                .starts_with(':')
                .then_some((start, number))
        })
        .collect()
}

impl Lesson {
    pub fn apply(
        &mut self,
        record: &Record,
        message: &Message,
        reply: &str,
        request_id: &str,
    ) -> Write {
        let mut open = self.task.clone();
        if message.is(HINT_WORDS) || message.is(LOST_WORDS) {
            if let Some(task) = open
                .as_mut()
                .filter(|task| task.check.is_none() && task.redo.is_none())
            {
                task.help = task.help.max(Help::Hint);
            }
        }
        if let Some(task) = open.as_ref().filter(|task| task.help > Help::None) {
            self.touch_task(task);
        }
        let mut answer = record.answer;
        if message.is(DONT_KNOW_WORDS)
            && open.as_ref().is_some_and(|task| !task.guess && !task.again)
        {
            answer = Some(Mark::Declined);
        }
        if message.talk {
            answer = None;
        }
        let mut observations = Vec::new();
        let mut practice: Vec<Value> = Vec::new();
        let mut marked = false;
        let mut lost = None;
        let mut redo_answer = None;
        let mut check_answer = None;
        let mut not_a_redo = None;
        let markable = open.as_ref().filter(|task| !task.guess && !task.again);
        if let (Some(mark), Some(task), Some(event)) = (answer, markable, &message.event_id) {
            let skill = clip(
                &Some(record.skill.clone())
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| Some(task.skill.clone()).filter(|s| !s.trim().is_empty()))
                    .or_else(|| self.goal.clone().filter(|s| !s.trim().is_empty()))
                    .unwrap_or_else(|| "the task in hand".to_owned()),
                MAX_NAME,
            );
            let label = clip(
                &task
                    .check
                    .clone()
                    .or_else(|| task.redo.as_ref().map(|redo| redo.tag.clone()))
                    .unwrap_or_else(|| skill.clone()),
                MAX_NAME,
            );
            let unaided = task.help == Help::None && mark != Mark::Declined;
            let clocked = !(message.no_clock && task.redo.as_ref().is_some_and(|redo| redo.timed));
            let seen =
                self.sitting_unknown || self.answered_earlier_today(&label) || self.touched(&label);
            let cold = unaided && clocked && record.cold && !seen;
            let mut attempt = json!({
                "label": label,
                "question": clip(&task.text, MAX_QUESTION),
                "first_try": mark.word(),
                "help": task.help.word(),
                "cold": cold,
            });
            if let Some(kind) = &record.kind {
                attempt["kind"] = json!(kind);
                if record.no_start && !task.stuck && matches!(mark, Mark::Declined | Mark::Wrong) {
                    attempt["no_start"] = json!(true);
                }
            }
            if let Some(confidence) = message.confidence() {
                attempt["confidence"] = json!(confidence);
            }
            if let Some(redo) = &task.redo {
                attempt["counts_for"] = json!(redo.rows);
                if record.slow && redo.timed && mark == Mark::Right {
                    attempt["slow"] = json!(true);
                }
            }
            if let Some(next) = record.next.as_ref().filter(|_| mark != Mark::Declined) {
                let stored = json!({"question": next.question, "answer_key": next.key});
                match &task.redo {
                    Some(redo) if redo.rows.len() > 1 => {
                        practice = redo
                            .rows
                            .iter()
                            .map(|row| json!({"item": row, "next": stored}))
                            .collect();
                    }
                    Some(_) => attempt["next"] = stored,
                    None if task.check.is_some() => attempt["next"] = stored,
                    None => {}
                }
            }
            if let Some(redo) = &task.redo {
                redo_answer = Some(RedoAnswer {
                    tag: redo.tag.clone(),
                    rows: redo.rows.clone(),
                    right: mark == Mark::Right,
                    other_label: skill.clone(),
                });
            } else if task.check.is_some() {
                check_answer = Some(mark == Mark::Right);
            }
            self.answered(&label);
            for name in [&label, &skill, &task.skill] {
                self.touch(name);
            }
            observations.push(json!({
                "client_ref": "answer",
                "kind": "learner_work",
                "description": if record.note.is_empty() { "An answer to the open task." } else { record.note.as_str() },
                "events": [{"event_id": event, "role": "learner_response"}],
                "attempt": attempt,
            }));
            marked = true;
        }
        if let (Some(_), false) = (answer, marked) {
            lost = match (&open, &message.event_id) {
                (Some(task), _) if task.guess || task.again => None,
                (None, _) => Some("no task was known to be open"),
                (Some(_), None) => Some("the learner's message was not captured"),
                _ => None,
            };
        }
        if let Some(note) = &record.memory {
            self.touch(&note.skill);
        }
        if record.memory.is_some() && observations.is_empty() {
            if let Some(event) = &message.event_id {
                observations.push(json!({
                    "client_ref": "answer",
                    "kind": "learner_work",
                    "description": if record.note.is_empty() { record.memory.as_ref().map(|m| m.text.as_str()).unwrap_or_default() } else { record.note.as_str() },
                    "events": [{"event_id": event, "role": "learner_response"}],
                }));
            }
        }

        let answered = answer.is_some() && open.is_some();
        let answered_text = open.as_ref().map(|task| task.text.clone());
        let was_stuck = open
            .as_ref()
            .is_some_and(|task| task.stuck || (record.no_start && marked));
        let marked_redo = open
            .as_ref()
            .filter(|_| answered && answer != Some(Mark::Declined))
            .and_then(|task| task.redo.as_ref())
            .map(|redo| redo.number);
        if answered {
            open = None;
        }
        if let (Some(task), Some(text), false) = (open.as_ref(), &record.task, message.talk) {
            if let Some(label) = task.check.as_ref().filter(|_| !same_task(&task.text, text)) {
                if let Some(due) = self.posed_checks.iter().find(|due| &due.label == label) {
                    if !self.checks.iter().any(|listed| listed.number == due.number) {
                        self.checks.push(due.clone());
                        self.checks.sort_by_key(|listed| listed.number);
                    }
                }
            }
        }
        let posed = posed_check(reply, &self.checks).filter(|_| !message.talk);
        let mark = redo_marks(reply).last().copied().filter(|_| !message.talk);
        let (check, redo_number) = match (
            posed
                .as_ref()
                .and_then(|check| check_at(reply, check.number)),
            mark,
        ) {
            (Some(check_at), Some((mark_at, number))) if mark_at > check_at => (None, Some(number)),
            (Some(_), _) => (posed.as_ref(), None),
            (None, mark) => (None, mark.map(|(_, number)| number)),
        };
        let waiting = open
            .as_ref()
            .and_then(|task| task.redo.clone().map(|redo| (redo, task.help)));
        let task = record.task.as_ref().filter(|_| !message.talk);
        match task {
            Some(text) => match open.as_mut() {
                Some(task) if same_task(&task.text, text) => {
                    task.help = task.help.max(record.help);
                }
                _ => {
                    let same = answered
                        && answered_text
                            .as_deref()
                            .is_some_and(|before| same_task(before, text));
                    let again = same && answer != Some(Mark::Declined);
                    let mut help = record.help;
                    let listed = redo_number
                        .and_then(|number| self.redos.iter().position(|due| due.number == number));
                    let redo = match (redo_number, listed, &waiting) {
                        (Some(number), Some(at), _) => {
                            let due = self.redos.remove(at);
                            if waiting.is_some() {
                                not_a_redo = Some(NotARedo::SetAside);
                            }
                            Some(Redo {
                                number,
                                rows: due.rows,
                                tag: due.tag,
                                timed: due.timed,
                            })
                        }
                        (Some(number), None, Some((redo, given))) if redo.number == number => {
                            help = help.max(*given);
                            Some(redo.clone())
                        }
                        (Some(number), None, _) if marked_redo == Some(number) => None,
                        (Some(_), None, _) => {
                            not_a_redo = Some(NotARedo::Unlisted);
                            None
                        }
                        (None, _, Some(_)) => {
                            not_a_redo = Some(NotARedo::SetAside);
                            None
                        }
                        (None, _, None) => None,
                    };
                    open = Some(OpenTask {
                        text: text.clone(),
                        help,
                        guess: record.guess,
                        check: check.map(|check| check.label.clone()),
                        redo,
                        again,
                        stuck: same && was_stuck,
                        skill: record.skill.clone(),
                    });
                }
            },
            None => {
                if let Some(task) = open.as_mut() {
                    task.help = task.help.max(record.help);
                }
            }
        }
        if let Some(check) = &posed {
            self.checks.retain(|due| due.number != check.number);
            if !self.posed_checks.iter().any(|due| due.label == check.label) {
                self.posed_checks.push(check.clone());
            }
        }
        if record.help > Help::None {
            if let Some(task) = &open {
                self.touch_task(task);
            }
            self.touch(&record.skill);
        }
        self.task = open;
        if let Some(steps) = &record.steps {
            self.steps = steps.clone();
        }
        let new_goal = record
            .goal
            .clone()
            .filter(|goal| self.goal.as_deref() != Some(goal.as_str()));
        if let Some(goal) = &new_goal {
            self.goal = Some(goal.clone());
        }

        let doing = self
            .steps
            .iter()
            .find(|step| step.status == "doing")
            .map(|step| step.step.clone());
        let goal = self.goal.clone().unwrap_or_default();
        let mut request = json!({
            "schema": "learner-model/v2",
            "action": "update",
            "request_id": request_id,
        });
        if !observations.is_empty() {
            request["observations"] = Value::Array(observations.clone());
        }
        if !practice.is_empty() {
            request["practice"] = Value::Array(practice);
        }
        if let Some(event) = &message.event_id {
            request["event_dispositions"] = json!([{
                "event_id": event,
                "outcome": if observations.is_empty() { "no_change" } else { "processed" },
            }]);
        }
        if let (Some(session), Some(revision)) = (&self.session_id, self.revision) {
            request["session"] =
                json!({"operation": "use", "session_id": session, "expected_revision": revision});
            if let Some(goal) = &new_goal {
                request["session"]["goal"] = json!(goal);
            }
            let mut checkpoint = json!({
                "current_problem": clip(doing.as_deref().unwrap_or(&goal), MAX_QUESTION),
                "working_diagnosis": record.note,
                "next_action": clip(
                    &self
                        .task
                        .as_ref()
                        .map(|task| task.text.clone())
                        .unwrap_or_else(|| format!("Continue toward the goal: {goal}")),
                    MAX_QUESTION,
                ),
                "plan_text": self.task.as_ref().map(plan_text),
                "pending_task": null,
            });
            if !self.steps.is_empty() {
                checkpoint["goal_steps"] = json!(self.steps);
            }
            request["checkpoint"] = checkpoint;
        }
        Write {
            request,
            marked,
            lost,
            redo: redo_answer,
            check: check_answer,
            not_a_redo,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cut(parts: &[&str]) -> (String, Cutter) {
        let mut cutter = Cutter::new();
        let shown: String = parts.iter().map(|part| cutter.push(part)).collect();
        (shown, cutter)
    }

    #[test]
    fn the_block_never_reaches_the_screen_however_the_reply_is_split() {
        let reply = "Right: b is −5, and a < b here.\n\nWhat is c in 2x² + x − 3 = 0?";
        let text = format!("{reply}\n\n<record>\n{{\"answer\": \"right\"}}\n</record>");
        let bounds: Vec<usize> = (1..text.len())
            .filter(|i| text.is_char_boundary(*i))
            .collect();
        for (n, a) in bounds.iter().enumerate() {
            for b in bounds[n..].iter().step_by(3) {
                let (shown, cutter) = cut(&[&text[..*a], &text[*a..*b], &text[*b..]]);
                assert_eq!(shown, reply, "split at {a} and {b}");
                assert_eq!(cutter.reply(), reply);
                assert_eq!(cutter.block().unwrap().trim(), "{\"answer\": \"right\"}");
            }
        }
        let pieces: Vec<String> = text.chars().map(String::from).collect();
        let refs: Vec<&str> = pieces.iter().map(String::as_str).collect();
        let (shown, _) = cut(&refs);
        assert_eq!(shown, reply);
    }

    #[test]
    fn a_reply_with_no_block_is_shown_whole_and_a_lone_angle_bracket_is_kept() {
        let (shown, cutter) = cut(&["If a <", " b, then", " b > a. <rec"]);
        assert_eq!(shown, "If a < b, then b > a.");
        assert_eq!(cutter.reply(), "If a < b, then b > a.");
        assert!(cutter.block().is_none());
    }

    #[test]
    fn a_block_is_read_and_checked() {
        let record = parse(
            r#"{"answer": "wrong", "help": "none", "skill": "sign of b", "kind": "read coefficients",
                "no_start": false, "note": "Dropped the sign.", "task": "What is b in x² − 4x?",
                "guess": false, "steps": [{"step": "Read a, b, c", "status": "doing"}]}"#,
        )
        .unwrap();
        assert_eq!(record.answer, Some(Mark::Wrong));
        assert_eq!(record.steps.unwrap()[0].status, "doing");
        let fenced =
            parse("```json\n{\"answer\": null, \"help\": \"hint\", \"task\": null}\n```").unwrap();
        assert_eq!(fenced.answer, None);
        assert_eq!(fenced.help, Help::Hint);
        for (bad, says) in [
            ("no json here", "no JSON"),
            ("{\"help\": \"none\"}", "answer is missing"),
            ("{\"answer\": \"maybe\"}", "not right"),
            ("{\"answer\": \"right\"}", "needs a skill"),
            ("{\"answer\": null, \"help\": \"lots\"}", "help"),
            (
                "{\"answer\": null, \"steps\": [{\"step\": \"x\", \"status\": \"started\"}]}",
                "status",
            ),
        ] {
            let error = parse(bad).unwrap_err();
            assert!(error.contains(says), "{bad}: {error}");
        }
    }

    fn lesson() -> Lesson {
        Lesson {
            session_id: Some("s-1".into()),
            revision: Some(3),
            goal: Some("Quadratics".into()),
            ..Lesson::default()
        }
    }

    fn message(words: &str) -> Message {
        Message {
            words: words.into(),
            event_id: Some("e-1".into()),
            ..Message::default()
        }
    }

    fn record(json: &str) -> Record {
        parse(json).unwrap()
    }

    #[test]
    fn an_answer_to_the_open_task_is_marked_with_the_help_given_on_it() {
        let mut lesson = lesson();
        let posed = lesson.apply(
            &record(r#"{"answer": null, "task": "Solve x² − 1 = 0", "steps": [{"step": "Solve", "status": "doing"}]}"#),
            &message("Teach me"),
            "Solve x² − 1 = 0",
            "r-1",
        );
        assert!(!posed.marked);
        assert_eq!(
            posed.request["event_dispositions"][0]["outcome"],
            "no_change"
        );
        assert_eq!(
            posed.request["checkpoint"]["next_action"],
            "Solve x² − 1 = 0"
        );
        assert_eq!(
            posed.request["checkpoint"]["goal_steps"][0]["step"],
            "Solve"
        );
        assert_eq!(posed.request["session"]["expected_revision"], 3);
        lesson.apply(
            &record(r#"{"answer": null, "help": "hint", "task": null}"#),
            &message(HINT_WORDS),
            "Think of a difference of squares.",
            "r-2",
        );
        let marked = lesson.apply(
            &record(r#"{"answer": "right", "help": "none", "skill": "difference of squares", "task": "Solve x² − 4 = 0"}"#),
            &message("x = 1 or x = −1\n\nConfidence: sure"),
            "Right.",
            "r-3",
        );
        assert!(marked.marked);
        let attempt = &marked.request["observations"][0]["attempt"];
        assert_eq!(attempt["first_try"], "right");
        assert_eq!(attempt["help"], "hint");
        assert_eq!(attempt["question"], "Solve x² − 1 = 0");
        assert_eq!(attempt["confidence"], "sure");
        assert_eq!(attempt["cold"], false);
        assert_eq!(
            marked.request["event_dispositions"][0]["outcome"],
            "processed"
        );
        assert_eq!(lesson.task.as_ref().unwrap().text, "Solve x² − 4 = 0");
        assert_eq!(lesson.task.as_ref().unwrap().help, Help::None);
    }

    #[test]
    fn a_guess_gets_no_mark_and_i_dont_know_is_a_decline() {
        let mut lesson = lesson();
        lesson.task = Some(OpenTask {
            text: "What changes for a weighted die?".into(),
            help: Help::None,
            guess: true,
            check: None,
            redo: None,
            again: false,
            stuck: false,
            skill: String::new(),
        });
        let guess = lesson.apply(
            &record(r#"{"answer": "wrong", "skill": "expected value", "task": "Find E for the weighted die"}"#),
            &message("the mean stays 3.5"),
            "Not quite.",
            "r-1",
        );
        assert!(!guess.marked);
        let idk = lesson.apply(
            &record(r#"{"answer": "wrong", "skill": "expected value", "kind": "discrete expected value", "no_start": true, "task": "Find E for the loaded coin"}"#),
            &message(DONT_KNOW_WORDS),
            "Here is a worked one.",
            "r-2",
        );
        let attempt = &idk.request["observations"][0]["attempt"];
        assert_eq!(attempt["first_try"], "declined");
        assert_eq!(attempt["no_start"], true);
        assert_eq!(attempt["kind"], "discrete expected value");
    }

    #[test]
    fn a_task_asked_again_after_its_mark_is_not_a_first_try() {
        let mut lesson = lesson();
        lesson.task = Some(OpenTask {
            text: "Solve 3x² − 2x − 8 = 0".into(),
            help: Help::None,
            guess: false,
            check: None,
            redo: None,
            again: false,
            stuck: false,
            skill: String::new(),
        });
        let wrong = lesson.apply(
            &record(
                r#"{"answer": "wrong", "skill": "formula", "task": "Solve 3x²  −  2x − 8 = 0"}"#,
            ),
            &message("x = 8"),
            "Not quite. Solve 3x² − 2x − 8 = 0",
            "r-1",
        );
        assert!(wrong.marked);
        assert!(lesson.task.as_ref().unwrap().again);
        let second = lesson.apply(
            &record(r#"{"answer": "right", "skill": "formula", "task": "Solve x² + 2x − 15 = 0"}"#),
            &message("x = 2 or −4/3"),
            "Right.",
            "r-2",
        );
        assert!(!second.marked);
        lesson.task.as_mut().unwrap().help = Help::Hint;
        lesson.apply(
            &record(r#"{"answer": null, "task": "Solve x² + 2x − 15 = 0"}"#),
            &message("what is b again?"),
            "b is 2. Solve x² + 2x − 15 = 0",
            "r-3",
        );
        assert_eq!(lesson.task.as_ref().unwrap().help, Help::Hint);
        let declined = lesson.apply(
            &record(
                r#"{"answer": "declined", "skill": "formula", "task": "Solve x² + 2x − 15 = 0"}"#,
            ),
            &message("I don't know"),
            "Here is a worked one. Now: Solve x² + 2x − 15 = 0",
            "r-4",
        );
        assert!(declined.marked);
        assert!(!lesson.task.as_ref().unwrap().again);
        let stuck = r#"{"answer": "declined", "skill": "formula", "kind": "quadratic formula", "no_start": true, "task": "Solve x² + 2x − 15 = 0"}"#;
        let first = lesson.apply(
            &record(stuck),
            &message("no idea how to start"),
            "Here is one worked.",
            "r-5",
        );
        assert_eq!(
            first.request["observations"][0]["attempt"]["no_start"],
            true
        );
        assert!(lesson.task.as_ref().unwrap().stuck);
        let second = lesson.apply(
            &record(stuck),
            &message("still no idea"),
            "Row by row, then.",
            "r-6",
        );
        assert!(second.request["observations"][0]["attempt"]
            .get("no_start")
            .is_none());
    }

    #[test]
    fn an_answer_is_cold_when_the_tutor_says_so_and_the_runtime_would_take_it() {
        let open = |lesson: &mut Lesson, text: &str| {
            lesson.task = Some(OpenTask {
                text: text.into(),
                help: Help::None,
                guess: false,
                check: None,
                redo: None,
                again: false,
                stuck: false,
                skill: String::new(),
            });
        };
        let mut lesson = lesson();
        lesson.set_today("2026-11-12");
        open(
            &mut lesson,
            "A fair coin pays 6 or 2: the average per play?",
        );
        let cold = r#"{"answer": "right", "skill": "expected value", "cold": true}"#;
        let first = lesson.apply(&record(cold), &message("4"), "Right.", "r-1");
        assert_eq!(first.request["observations"][0]["attempt"]["cold"], true);
        open(&mut lesson, "A die pays its face: the average per roll?");
        let second = lesson.apply(&record(cold), &message("3.5"), "Right.", "r-2");
        assert_eq!(second.request["observations"][0]["attempt"]["cold"], false);
        lesson.set_today("2026-11-13");
        open(&mut lesson, "A spinner pays 1 or 11, 80/20: the average?");
        let midnight = lesson.apply(&record(cold), &message("3"), "Right.", "r-3a");
        assert_eq!(
            midnight.request["observations"][0]["attempt"]["cold"],
            false
        );
        lesson.begin_sitting();
        lesson.set_today("2026-11-14");
        open(&mut lesson, "A spinner pays 1 or 11, 80/20: the average?");
        let next_day = lesson.apply(&record(cold), &message("3"), "Right.", "r-3");
        assert_eq!(next_day.request["observations"][0]["attempt"]["cold"], true);
        open(&mut lesson, "Another?");
        let not = lesson.apply(
            &record(r#"{"answer": "right", "skill": "variance"}"#),
            &message("2"),
            "Right.",
            "r-4",
        );
        assert_eq!(not.request["observations"][0]["attempt"]["cold"], false);
        open(&mut lesson, "One more?");
        lesson.apply(
            &record(r#"{"answer": null, "help": "hint"}"#),
            &message(HINT_WORDS),
            "Hint.",
            "r-5",
        );
        let hinted = lesson.apply(
            &record(r#"{"answer": "right", "skill": "median", "cold": true}"#),
            &message("5"),
            "Right.",
            "r-6",
        );
        assert_eq!(hinted.request["observations"][0]["attempt"]["cold"], false);
        open(&mut lesson, "Last?");
        let declined = lesson.apply(
            &record(r#"{"answer": "declined", "skill": "mode", "cold": true}"#),
            &message(DONT_KNOW_WORDS),
            "Fine.",
            "r-7",
        );
        assert_eq!(
            declined.request["observations"][0]["attempt"]["cold"],
            false
        );
    }

    #[test]
    fn a_control_left_by_older_instructions_is_ignored() {
        for control in ["pause", "stop", "forget me"] {
            let block =
                format!(r#"{{"answer": "right", "skill": "sign of b", "control": "{control}"}}"#);
            let record = parse(&block).unwrap();
            assert_eq!(record.answer, Some(Mark::Right));
            assert_eq!(record.skill, "sign of b");
        }
    }

    #[test]
    fn i_dont_know_with_no_skill_named_takes_the_tasks_skill() {
        let mut lesson = lesson();
        lesson.apply(
            &record(r#"{"answer": null, "skill": "sign of b", "task": "What is b in x² − 5x?"}"#),
            &message("Teach me"),
            "What is b in x² − 5x?",
            "r-1",
        );
        let declined = lesson.apply(
            &record(r#"{"answer": null}"#),
            &message(DONT_KNOW_WORDS),
            "Fine.",
            "r-2",
        );
        let attempt = &declined.request["observations"][0]["attempt"];
        assert_eq!(attempt["first_try"], "declined");
        assert_eq!(attempt["label"], "sign of b");
        lesson.task = Some(OpenTask {
            text: "Another?".into(),
            help: Help::None,
            guess: false,
            check: None,
            redo: None,
            again: false,
            stuck: false,
            skill: String::new(),
        });
        let bare = lesson.apply(
            &record(r#"{"answer": null}"#),
            &message(DONT_KNOW_WORDS),
            "Fine.",
            "r-3",
        );
        assert_eq!(
            bare.request["observations"][0]["attempt"]["label"],
            "Quadratics"
        );
    }

    #[test]
    fn a_due_check_keeps_its_label_and_is_cold_only_when_the_tutor_says_so() {
        let due = DueCheck {
            number: 1,
            label: "sign of b in the quadratic formula".into(),
            misses: vec![],
            question: None,
            key: None,
        };
        let posed = |block: &str| {
            let mut lesson = lesson();
            lesson.checks = vec![due.clone()];
            lesson.apply(
                &record(r#"{"answer": null, "skill": "reading b", "task": "What is b in 2x² − 7x − 4 = 0?"}"#),
                &message("I'm back."),
                "Quick check 1: What is b in 2x² − 7x − 4 = 0?",
                "r-1",
            );
            assert!(lesson.checks.is_empty());
            let answer = lesson.apply(&record(block), &message("−7"), "Right.", "r-2");
            assert_eq!(answer.check, Some(true));
            answer.request["observations"][0]["attempt"].clone()
        };
        let attempt =
            posed(r#"{"answer": "right", "skill": "reading coefficients", "task": null}"#);
        assert_eq!(attempt["label"], "sign of b in the quadratic formula");
        assert_eq!(attempt["cold"], false);
        let attempt = posed(
            r#"{"answer": "right", "skill": "reading coefficients", "cold": true, "task": null}"#,
        );
        assert_eq!(attempt["cold"], true);
        for earlier in [
            r#"{"answer": null, "help": "step", "skill": "sign of b in the quadratic formula", "task": "Try 3x² − 2x?"}"#,
            r#"{"answer": null, "memory": {"skill": "sign of b in the quadratic formula", "text": "Drops the sign."}}"#,
        ] {
            let mut lesson = lesson();
            lesson.apply(&record(earlier), &message("hm"), "Look at the sign.", "r-0");
            lesson.task = None;
            lesson.checks = vec![due.clone()];
            lesson.apply(
                &record(r#"{"answer": null, "task": "What is b in 2x² − 7x − 4 = 0?"}"#),
                &message("go on"),
                "Quick check 1: What is b in 2x² − 7x − 4 = 0?",
                "r-1",
            );
            let answer = lesson.apply(
                &record(r#"{"answer": "right", "skill": "b", "cold": true}"#),
                &message("−7"),
                "Right.",
                "r-2",
            );
            assert_eq!(
                answer.request["observations"][0]["attempt"]["cold"], false,
                "{earlier}"
            );
        }
        let twelve = DueCheck {
            number: 1,
            label: "x".into(),
            misses: vec![],
            question: None,
            key: None,
        };
        assert!(posed_check("Quick check 12: hi", std::slice::from_ref(&twelve)).is_none());
        assert!(posed_check("QUICK CHECK 1: hi", &[twelve]).is_some());
    }

    fn redo(number: i64, rows: &[&str], tag: &str, timed: bool) -> DueRedo {
        DueRedo {
            number,
            rows: rows.iter().map(|row| (*row).to_owned()).collect(),
            tag: tag.into(),
            timed,
            from: vec![],
            misses: vec![],
            question: None,
            key: None,
        }
    }

    fn none_today(_: &str) -> bool {
        false
    }

    fn with_redos() -> Lesson {
        let mut lesson = lesson();
        lesson.redos = vec![
            redo(1, &["midterm-1#1", "midterm-1#4"], "km", true),
            redo(2, &["midterm-1#2"], "rate law", false),
        ];
        lesson
    }

    fn pose(lesson: &mut Lesson, reply: &str, task: &str) -> Write {
        lesson.apply(
            &record(&json!({"answer": null, "skill": "reading a plot", "task": task}).to_string()),
            &message("I'm back."),
            reply,
            "r-pose",
        )
    }

    #[test]
    fn the_due_rows_of_one_tag_make_one_redo_and_a_tag_has_one_redo_in_a_sitting() {
        let row =
            |item: &str, plan: Option<&str>, tag: &str, kind: &str, question: Option<&str>| {
                json!({"item": item, "plan": plan, "tag": tag, "type": kind,
                "what_went_wrong": format!("miss on {item}"), "paper": "Diagnostic",
                "paper_question": item.rsplit('#').next(), "question": question,
                "key": question.map(|_| "the key")})
            };
        let redos = redos_from(
            &json!([
                row("midterm-1#2", Some("midterm-1"), "km", "K", None),
                row("final#1", Some("final"), "Km", "K", None),
                row(
                    "midterm-1#5",
                    Some("midterm-1"),
                    "Km",
                    "S",
                    Some("Find Km.")
                ),
                row("midterm-1#3", Some("midterm-1"), "rate law", "N", None),
            ]),
            &none_today,
        );
        assert_eq!(redos.len(), 2, "{redos:#?}");
        assert_eq!(
            redos.iter().map(|redo| redo.number).collect::<Vec<_>>(),
            [1, 2]
        );
        let km = &redos[0];
        assert_eq!(km.rows, ["midterm-1#2", "midterm-1#5"]);
        assert_eq!(km.tag, "km");
        assert!(km.timed, "one of its rows was lost for slowness");
        assert_eq!(km.from, ["Diagnostic Q2", "Diagnostic Q5"]);
        assert_eq!(km.misses, ["miss on midterm-1#2", "miss on midterm-1#5"]);
        assert_eq!(km.question.as_deref(), Some("Find Km."));
        assert_eq!(km.key.as_deref(), Some("the key"));
        assert!(!redos[1].timed);
        assert!(
            redos
                .iter()
                .all(|redo| !redo.rows.contains(&"final#1".to_owned())),
            "{redos:#?}"
        );
        let many: Vec<Value> = (1..=14)
            .map(|n| row(&format!("p#{n}"), Some("p"), "km", "K", None))
            .collect();
        let redos = redos_from(&json!(many), &none_today);
        assert_eq!(redos.len(), 1, "{redos:#?}");
        assert_eq!(redos[0].rows.len(), 12);
        let plan = "p".repeat(250);
        let long: Vec<Value> = (1..=12)
            .map(|n| row(&format!("{plan}#{n}"), Some(&plan), "km", "K", None))
            .collect();
        let redos = redos_from(&json!(long), &none_today);
        let bytes: usize = redos[0].rows.iter().map(String::len).sum();
        assert!(bytes <= MAX_ROW_BYTES, "{bytes}");
        assert!(redos[0].rows.len() >= 4, "{}", redos[0].rows.len());
        let mut lesson = lesson();
        lesson.redos = redos_from(&json!(long), &none_today);
        pose(&mut lesson, "Paper redo 1: What is Km?", &"q".repeat(1000));
        assert!(
            plan_text(lesson.task.as_ref().unwrap()).len() <= 4096,
            "the checkpoint's plan_text is bounded at 4 KiB"
        );
        let redos = redos_from(
            &json!([
                row("m#1", Some("m"), "Km", "K", None),
                row("m#2", Some("m"), "rate law", "K", None)
            ]),
            &|tag| tag.eq_ignore_ascii_case("km"),
        );
        assert_eq!(redos.len(), 1);
        assert_eq!((redos[0].number, redos[0].tag.as_str()), (1, "rate law"));
        let many: Vec<Value> = (1..=9)
            .map(|n| row(&format!("p#{n}"), Some("p"), &format!("tag {n}"), "K", None))
            .collect();
        assert_eq!(redos_from(&json!(many), &none_today).len(), MAX_REDOS);
        assert!(redos_from(&Value::Null, &none_today).is_empty());
    }

    #[test]
    fn a_paper_redo_counts_for_its_rows_under_its_tag() {
        let mut lesson = with_redos();
        pose(
            &mut lesson,
            "Paper redo 1: The plot crosses at −0.5: what is Km? You have 3 minutes.",
            "The plot crosses at −0.5: what is Km?",
        );
        assert_eq!(lesson.redos.len(), 1, "the redo posed is no longer waiting");
        assert_eq!(
            lesson.task.as_ref().unwrap().redo,
            Some(Redo {
                number: 1,
                rows: vec!["midterm-1#1".into(), "midterm-1#4".into()],
                tag: "km".into(),
                timed: true
            })
        );
        let answer = lesson.apply(
            &record(r#"{"answer": "right", "skill": "reading a plot", "cold": true, "slow": true,
                "next": {"question": "A plot crosses at −0.25: what is Km?", "key": "4"},
                "task": "Write the rate law for A → B, first order."}"#),
            &message("Km = 2"),
            "Right, but over the 3 minutes. Paper redo 2: Write the rate law for A → B, first order.",
            "r-2",
        );
        let attempt = &answer.request["observations"][0]["attempt"];
        assert_eq!(attempt["label"], "km");
        assert_eq!(attempt["cold"], true);
        assert_eq!(attempt["slow"], true);
        assert_eq!(attempt["counts_for"], json!(["midterm-1#1", "midterm-1#4"]));
        assert!(attempt.get("next").is_none(), "{attempt}");
        assert_eq!(
            answer.request["practice"],
            json!([
                {"item": "midterm-1#1", "next": {"question": "A plot crosses at −0.25: what is Km?", "answer_key": "4"}},
                {"item": "midterm-1#4", "next": {"question": "A plot crosses at −0.25: what is Km?", "answer_key": "4"}},
            ])
        );
        assert_eq!(
            answer.redo,
            Some(RedoAnswer {
                tag: "km".into(),
                rows: vec!["midterm-1#1".into(), "midterm-1#4".into()],
                right: true,
                other_label: "reading a plot".into()
            })
        );
        assert_eq!(answer.not_a_redo, None);
        assert_eq!(
            lesson.task.as_ref().unwrap().redo.as_ref().unwrap().tag,
            "rate law"
        );
        lesson.apply(
            &record(r#"{"answer": null, "help": "hint", "task": null}"#),
            &message(HINT_WORDS),
            "Think of what first order means.",
            "r-3",
        );
        let hinted = lesson.apply(
            &record(
                r#"{"answer": "right", "skill": "rate laws", "cold": true, "slow": true,
                "next": {"question": "And for 2A → B, second order?", "answer_key": "v = k[A]²"}}"#,
            ),
            &message("v = k[A]"),
            "Right.",
            "r-4",
        );
        let attempt = &hinted.request["observations"][0]["attempt"];
        assert_eq!(attempt["label"], "rate law");
        assert_eq!(attempt["help"], "hint");
        assert_eq!(attempt["cold"], false);
        assert_eq!(attempt["counts_for"], json!(["midterm-1#2"]));
        assert!(attempt.get("slow").is_none(), "{attempt}");
        assert_eq!(attempt["next"]["answer_key"], "v = k[A]²");
        assert!(hinted.request.get("practice").is_none());
        lesson.checks = vec![DueCheck {
            number: 1,
            label: "sign of b".into(),
            misses: vec![],
            question: None,
            key: None,
        }];
        lesson.apply(
            &record(r#"{"answer": null, "task": "What is b in x² − 5x?"}"#),
            &message("go on"),
            "Quick check 1: What is b in x² − 5x?",
            "r-5",
        );
        let check = lesson.apply(
            &record(r#"{"answer": "right", "skill": "b", "next": {"question": "What is b in 3x² + x?", "key": "1"}, "task": "Go on: what is c?"}"#),
            &message("−5"),
            "Right. Go on: what is c?",
            "r-6",
        );
        assert_eq!(
            check.request["observations"][0]["attempt"]["next"]["question"],
            "What is b in 3x² + x?"
        );
        assert_eq!(check.redo, None);
        let plain = lesson.apply(
            &record(r#"{"answer": "right", "skill": "c", "next": {"question": "x", "key": "y"}}"#),
            &message("6"),
            "Right.",
            "r-7",
        );
        assert!(plain.request["observations"][0]["attempt"]
            .get("next")
            .is_none());
    }

    #[test]
    fn a_redo_is_cold_only_when_the_tutor_says_so_and_its_clock_ran() {
        let answer = |lesson: &mut Lesson, block: &str, message: &Message| {
            lesson
                .apply(&record(block), message, "Right.", "r-answer")
                .request["observations"][0]["attempt"]["cold"]
                .clone()
        };
        let right = r#"{"answer": "right", "skill": "km", "cold": true}"#;
        let mut lesson = with_redos();
        pose(
            &mut lesson,
            "Paper redo 2: Write the rate law.",
            "Write the rate law.",
        );
        let taught = r#"{"answer": "right", "skill": "rate laws", "cold": false}"#;
        assert_eq!(answer(&mut lesson, taught, &message("v = k[A]")), false);
        let mut lesson = with_redos();
        lesson.answered("Rate Law");
        pose(
            &mut lesson,
            "Paper redo 2: Write the rate law.",
            "Write the rate law.",
        );
        assert_eq!(answer(&mut lesson, right, &message("v = k[A]")), false);
        let mut lesson = with_redos();
        pose(&mut lesson, "Paper redo 1: What is Km?", "What is Km?");
        let unclocked = Message {
            no_clock: true,
            ..message("2")
        };
        assert_eq!(answer(&mut lesson, right, &unclocked), false);
        let mut lesson = with_redos();
        pose(&mut lesson, "Paper redo 1: What is Km?", "What is Km?");
        assert_eq!(answer(&mut lesson, right, &message("2")), true);
        let mut lesson = with_redos();
        pose(
            &mut lesson,
            "Paper redo 2: Write the rate law.",
            "Write the rate law.",
        );
        let unclocked = Message {
            no_clock: true,
            ..message("v = k[A]")
        };
        assert_eq!(answer(&mut lesson, right, &unclocked), true);
    }

    #[test]
    fn a_redo_is_posed_only_by_its_words_with_the_colon_where_the_task_is_set() {
        let mut lesson = with_redos();
        let write = pose(
            &mut lesson,
            "Here is the fix. Practice: what is the slope here? Then we'll do paper redo 2.",
            "Practice: what is the slope here?",
        );
        assert_eq!(lesson.task.as_ref().unwrap().redo, None);
        assert_eq!(lesson.redos.len(), 2);
        assert_eq!(write.not_a_redo, None);
        lesson.task = None;
        lesson.apply(
            &record(r#"{"answer": null, "task": null}"#),
            &message("ok"),
            "Next comes Paper redo 1: get ready.",
            "r-1",
        );
        assert_eq!(lesson.redos.len(), 2, "nothing was posed");
        pose(
            &mut lesson,
            "We'll come back to paper redo 1. **Paper redo 2:** Write the rate law.",
            "Write the rate law.",
        );
        assert_eq!(
            lesson.task.as_ref().unwrap().redo.as_ref().unwrap().tag,
            "rate law"
        );
        assert_eq!(lesson.redos[0].number, 1, "redo 1 is still listed");
        assert_eq!(redo_marks("**Paper redo 1**: hi"), [(2, 1)]);
        assert_eq!(redo_marks("PAPER REDO 12: hi"), [(0, 12)]);
        assert!(redo_marks("paper redo 1 is next; paper redo x: no").is_empty());
        let check = DueCheck {
            number: 1,
            label: "sign of b".into(),
            misses: vec![],
            question: None,
            key: None,
        };
        let mut lesson = with_redos();
        lesson.checks = vec![check.clone()];
        pose(
            &mut lesson,
            "Quick check 1 can wait. Paper redo 1: What is Km?",
            "What is Km?",
        );
        let open = lesson.task.as_ref().unwrap();
        assert_eq!(open.check, None);
        assert_eq!(open.redo.as_ref().unwrap().number, 1);
        let mut lesson = with_redos();
        lesson.checks = vec![check];
        pose(
            &mut lesson,
            "Paper redo 1: comes after this. Quick check 1: What is b in x² − 5x?",
            "What is b in x² − 5x?",
        );
        let open = lesson.task.as_ref().unwrap();
        assert_eq!(open.check.as_deref(), Some("sign of b"));
        assert_eq!(open.redo, None);
        assert_eq!(lesson.redos.len(), 2);
    }

    #[test]
    fn a_task_that_only_looks_like_a_redo_is_flagged_for_the_learner() {
        let mut lesson = with_redos();
        pose(&mut lesson, "Paper redo 1: What is Km?", "What is Km?");
        lesson.apply(
            &record(r#"{"answer": null, "help": "hint", "task": null}"#),
            &message(HINT_WORDS),
            "Look at the x intercept.",
            "r-1",
        );
        let reworded = pose(
            &mut lesson,
            "Paper redo 1: Read Km off the x intercept of the plot.",
            "Read Km off the x intercept of the plot.",
        );
        let open = lesson.task.clone().unwrap();
        assert_eq!(open.redo.as_ref().unwrap().rows.len(), 2);
        assert_eq!(open.help, Help::Hint);
        assert_eq!(reworded.not_a_redo, None);
        let aside = pose(&mut lesson, "First: what is a slope?", "What is a slope?");
        assert_eq!(lesson.task.as_ref().unwrap().redo, None);
        assert_eq!(aside.not_a_redo, Some(NotARedo::SetAside));
        let mut lesson = with_redos();
        pose(
            &mut lesson,
            "Paper redo 2: Write the rate law.",
            "Write the rate law.",
        );
        let again = lesson.apply(
            &record(
                r#"{"answer": "declined", "skill": "rate laws", "task": "Write the rate law."}"#,
            ),
            &message(DONT_KNOW_WORDS),
            "Here is one worked. Paper redo 2: Write the rate law.",
            "r-2",
        );
        assert_eq!(lesson.task.as_ref().unwrap().redo, None);
        assert_eq!(again.not_a_redo, Some(NotARedo::Unlisted));
        let mut lesson = with_redos();
        let unknown = pose(&mut lesson, "Paper redo 7: What is Vmax?", "What is Vmax?");
        assert_eq!(unknown.not_a_redo, Some(NotARedo::Unlisted));
        let mut lesson = with_redos();
        pose(
            &mut lesson,
            "Paper redo 2: Write the rate law.",
            "Write the rate law.",
        );
        let marked = lesson.apply(
            &record(r#"{"answer": "right", "skill": "rate laws", "cold": true, "task": "Now solve x² = 4."}"#),
            &message("v = k[A]"),
            "Paper redo 2: right. Now solve x² = 4.",
            "r-3",
        );
        assert!(marked.marked);
        assert_eq!(marked.not_a_redo, None);
        assert_eq!(lesson.task.as_ref().unwrap().redo, None);
    }

    #[test]
    fn a_declined_redo_or_check_stores_no_next_question() {
        let mut lesson = with_redos();
        pose(&mut lesson, "Paper redo 1: What is Km?", "What is Km?");
        let declined = lesson.apply(
            &record(
                r#"{"answer": "declined", "skill": "km", "next": {"question": "q", "key": "k"}}"#,
            ),
            &message(DONT_KNOW_WORDS),
            "Fine.",
            "r-1",
        );
        let attempt = &declined.request["observations"][0]["attempt"];
        assert_eq!(attempt["first_try"], "declined");
        assert_eq!(attempt["counts_for"], json!(["midterm-1#1", "midterm-1#4"]));
        assert!(attempt.get("next").is_none(), "{attempt}");
        assert!(declined.request.get("practice").is_none());
    }

    #[test]
    fn a_reply_with_no_record_spends_the_redos_it_showed() {
        let mut lesson = with_redos();
        lesson.spend_shown("Right. We will get to paper redo 2 later. Paper redo 1: What is K");
        assert_eq!(
            lesson
                .redos
                .iter()
                .map(|due| due.number)
                .collect::<Vec<_>>(),
            [2],
            "the redo it began to pose is spent; the one it spoke of is not"
        );
    }

    #[test]
    fn slow_and_next_are_read_and_a_next_that_cannot_be_stored_costs_nothing() {
        let record = parse(
            r#"{"answer": "right", "skill": "km", "slow": true, "next": {"question": " Find Km. ", "key": "2"}}"#,
        )
        .unwrap();
        assert!(record.slow);
        assert_eq!(
            record.next,
            Some(Next {
                question: "Find Km.".into(),
                key: "2".into()
            })
        );
        for next in [
            "null",
            r#"{"question": "Find Km."}"#,
            r#"{"question": "Find Km.", "key": ""}"#,
            r#""later""#,
            "[]",
        ] {
            let record = parse(&format!(
                r#"{{"answer": "right", "skill": "km", "next": {next}}}"#
            ))
            .unwrap();
            assert_eq!(record.next, None, "{next}");
            assert_eq!(record.answer, Some(Mark::Right));
        }
    }

    #[test]
    fn a_memory_edit_rewrites_its_own_line_and_never_touches_another() {
        let note = |text: &str, cleared: bool| MemoryNote {
            id: Some("m1".into()),
            skill: "sign of b".into(),
            text: text.into(),
            cleared,
        };
        let create = memory_edit(
            &note("Drops the minus sign of b.", false),
            None,
            "Quadratics",
        )
        .unwrap();
        assert_eq!(create["operation"], "create");
        assert_eq!(create["record"]["scope"], "Quadratics");
        assert_eq!(
            create["record"]["state_text"],
            "Misunderstanding found in a lesson: Drops the minus sign of b."
        );
        let theirs = "Reads coefficients well.\nanchor: reads a from a bare x² (check question)\n";
        let memory = |state: &str| {
            json!({"memory_id": "m-1", "revision": 2, "title": "Sign of B", "scope": "Quadratics",
                   "summary": "Their summary.", "state_text": state,
                   "current_basis": [{"observation_id": "o-1"}]})
        };
        let first = memory_edit(
            &note("Drops the minus sign of b.", false),
            Some(&memory(theirs)),
            "",
        )
        .unwrap();
        let text = first["record"]["state_text"].as_str().unwrap().to_owned();
        assert!(
            text.starts_with(theirs),
            "theirs is kept byte for byte: {text:?}"
        );
        assert_eq!(first["record"]["summary"], "Their summary.");
        let second =
            memory_edit(&note("Keeps the sign now.", true), Some(&memory(&text)), "").unwrap();
        let text = second["record"]["state_text"].as_str().unwrap();
        assert!(text.starts_with(theirs));
        assert_eq!(text.matches("in a lesson:").count(), 1, "{text}");
        assert!(text.ends_with("Misunderstanding cleared in a lesson: Keeps the sign now."));
        let long = format!("anchor: kept\n{}", "x".repeat(STATE_LIMIT - 100));
        let cut = memory_edit(&note(&"y".repeat(500), false), Some(&memory(&long)), "").unwrap();
        let text = cut["record"]["state_text"].as_str().unwrap();
        assert!(
            text.starts_with(&long) && text.len() <= STATE_LIMIT,
            "{}",
            text.len()
        );
        let full = format!("anchor: kept\n{}", "x".repeat(STATE_LIMIT - 20));
        assert!(memory_edit(&note("Drops the sign.", false), Some(&memory(&full)), "").is_none());
    }

    #[test]
    fn a_lesson_with_no_session_writes_no_checkpoint_and_a_memory_note_rests_on_the_message() {
        let mut lesson = Lesson::default();
        let write = lesson.apply(
            &record(r#"{"answer": null, "note": "Thinks b is x.", "memory": {"skill": "b vs x", "text": "Reads b as the unknown x."}}"#),
            &message("b is x"),
            "Not quite.",
            "r-1",
        );
        assert!(write.request.get("checkpoint").is_none());
        assert_eq!(
            write.request["observations"][0]["description"],
            "Thinks b is x."
        );
        assert!(write.request["observations"][0].get("attempt").is_none());
    }
    #[test]
    fn a_repair_is_read_from_the_block_and_one_that_cannot_be_read_costs_nothing_else() {
        let done = parse(
            r#"{"answer": null, "repair": {"number": 2, "done": " taught the intercept ",
                "next": {"question": "Find Km.", "key": "4"}}}"#,
        )
        .unwrap();
        assert_eq!(
            done.repair,
            Some(RepairNote {
                number: 2,
                done: "taught the intercept".into(),
                next: Some(Next {
                    question: "Find Km.".into(),
                    key: "4".into()
                })
            })
        );
        assert!(!done.repair_unreadable);
        for bad in [
            r#"{"done": "taught it"}"#,
            r#"{"number": 1, "done": " "}"#,
            r#""repair 1""#,
        ] {
            let record = parse(&format!(
                r#"{{"answer": "right", "skill": "km", "repair": {bad}}}"#
            ))
            .unwrap();
            assert_eq!(record.repair, None, "{bad}");
            assert!(record.repair_unreadable, "{bad}");
            assert_eq!(record.answer, Some(Mark::Right), "the block is kept");
        }
        assert!(
            !parse(r#"{"answer": null, "repair": null}"#)
                .unwrap()
                .repair_unreadable
        );
    }

    #[test]
    fn rows_waiting_for_a_repair_make_one_repair_per_tag_and_plan_and_are_listed_once() {
        let mut lesson = lesson();
        let row = |item: &str, plan: &str, tag: &str| {
            json!({"item": item, "plan": plan, "tag": tag, "what_went_wrong": format!("miss on {item}"),
                "paper": "Diagnostic", "paper_question": item.rsplit('#').next()})
        };
        let made = lesson.take_waiting(
            &json!([
                row("m#1", "m", "km"),
                row("m#2", "m", "Km"),
                row("f#1", "f", "km"),
                row("m#3", "m", "rate law")
            ]),
            false,
        );
        assert_eq!(
            made.iter()
                .map(|r| (r.number, r.rows.clone()))
                .collect::<Vec<_>>(),
            [
                (1, vec!["m#1".to_owned(), "m#2".to_owned()]),
                (2, vec!["f#1".to_owned()]),
                (3, vec!["m#3".to_owned()])
            ]
        );
        assert_eq!(made[0].from, ["Diagnostic Q1", "Diagnostic Q2"]);
        let again =
            lesson.take_waiting(&json!([row("m#1", "m", "km"), row("m#4", "m", "km")]), true);
        assert_eq!(again.len(), 1);
        assert_eq!((again[0].number, again[0].failed_here), (4, true));
        lesson.begin_sitting();
        assert!(lesson.repairs.is_empty());
        let next = lesson.take_waiting(&json!([row("m#1", "m", "km")]), false);
        assert_eq!(next[0].number, 5);
    }
    #[test]
    fn a_button_press_on_due_work_is_no_help_until_a_reply_gives_some() {
        let press = |redo: bool, words: &str, block: &str| {
            let mut lesson = with_redos();
            lesson.checks = vec![DueCheck {
                number: 1,
                label: "sign of b".into(),
                misses: vec![],
                question: None,
                key: None,
            }];
            let reply = if redo {
                "Paper redo 2: Write the rate law."
            } else {
                "Quick check 1: What is b in x² − 5x?"
            };
            pose(&mut lesson, reply, "The task.");
            lesson.apply(&record(block), &message(words), "A line.", "r-1");
            lesson.task.unwrap().help
        };
        let warning = r#"{"answer": null, "help": "none"}"#;
        let hint = r#"{"answer": null, "help": "hint"}"#;
        for redo in [true, false] {
            for words in [HINT_WORDS, LOST_WORDS] {
                assert_eq!(press(redo, words, warning), Help::None, "{redo} {words}");
                assert_eq!(press(redo, words, hint), Help::Hint, "{redo} {words}");
            }
        }
        let mut lesson = lesson();
        pose(&mut lesson, "Solve x² = 4.", "Solve x² = 4.");
        lesson.apply(&record(warning), &message(HINT_WORDS), "A line.", "r-1");
        assert_eq!(lesson.task.unwrap().help, Help::Hint);
    }

    #[test]
    fn the_next_save_writes_a_checkpoint_without_a_forgotten_task() {
        let mut lesson = lesson();
        pose(&mut lesson, "Solve x² = 4.", "Solve x² = 4.");
        assert!(lesson.task.is_some());
        lesson.forget_task();
        let write = lesson.apply(
            &record(r#"{"answer": null}"#),
            &message("ok"),
            "Go on.",
            "r-2",
        );
        assert_eq!(write.request["checkpoint"]["plan_text"], Value::Null);
        assert!(write.request["session"]["operation"] == "use");
    }
}
