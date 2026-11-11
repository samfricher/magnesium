//! The record block the direct engine's tutor ends each reply with, and what
//! code makes of it.
//!
//! The tutor writes its reply, then one block:
//!
//! ```text
//! <record>
//! {"answer": "wrong", "help": "none", "skill": "sign of b", ...}
//! </record>
//! ```
//!
//! The block holds only the tutor's judgment. [`Cutter`] keeps it off the
//! screen while the reply streams, [`parse`] checks it, and [`Lesson::apply`]
//! turns it into the `learning_step` the runtime takes, adding what code
//! knows better than the model: which task was open, the help given on it
//! across turns, the Hint button, "I don't know", the confidence line, the
//! label of a due quick check, and whether the answer was cold.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Where the block starts. Everything before it is the reply.
pub const MARK: &str = "<record>";
const END: &str = "</record>";

/// The longest skill, kind or step the runtime keeps, in bytes.
const MAX_NAME: usize = 120;
const MAX_QUESTION: usize = 1000;
const MAX_NOTE: usize = 500;
const MAX_STEPS: usize = 12;

/// The lines of the checkpoint's plan_text that carry the open task to a
/// later conversation, which reads them back.
pub const TASK_LINE: &str = "Open task, posed to the learner: ";
pub const HELP_LINE: &str = "Help on it so far: ";
pub const CHECK_LINE: &str = "It is the quick check ";
pub const GUESS_LINE: &str = "It asks for a guess before teaching: its answer gets no mark.";
pub const AGAIN_LINE: &str =
    "It was asked again after its mark: its next answer is not a first try.";

/// The words the window's Hint button sends.
pub const HINT_WORDS: &str = "Can I have a hint?";
/// The words the window's "I don't know" button sends.
pub const DONT_KNOW_WORDS: &str = "I don't know";
/// The words the window's "I'm lost" button sends. Only the prompt acts on
/// them; they are kept here so a test can hold them to the page's.
pub const LOST_WORDS: &str = "I'm lost";

/// Passes the reply through as it streams and stops at the record block.
///
/// A tail that could be the start of the marker is held back until the next
/// piece shows what it is, and so is trailing white space, so the window
/// never shows a stray `<rec` or blank lines before the block.
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

    /// Take more of the reply, and give back what may be shown now.
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
        // Hold back the longest tail that begins the marker. The marker is
        // ASCII, so the cut is on a character boundary.
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

    /// The reply as the window shows it once whole: everything before the
    /// block, trimmed.
    pub fn reply(&self) -> String {
        let end = self.cut.unwrap_or(self.text.len());
        let reply = &self.text[..end];
        // A reply cut off in the middle of the marker shows no part of it.
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

    /// The text of the block, without its tags, when the reply has one.
    pub fn block(&self) -> Option<&str> {
        let rest = &self.text[self.cut? + MARK.len()..];
        Some(match rest.find(END) {
            Some(end) => &rest[..end],
            None => rest,
        })
    }

    /// Whether anything has been written yet.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// The end of `text[..end]` without trailing white space, but not before
/// `floor`, which is already shown.
fn trim_end(text: &str, floor: usize, end: usize) -> usize {
    let trimmed = text[..end].trim_end_matches([' ', '\n', '\t', '\r']).len();
    trimmed.max(floor).min(end.max(floor))
}

/// A first try's mark, in the runtime's words.
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

/// Help on a task, least first, in the runtime's words.
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

/// One step of the goal's route.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub step: String,
    pub status: String,
}

/// A control the learner asked for in their own words, as the runtime's
/// `learning_step` `action:"control"` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Control {
    Pause,
    Stop,
    RecordingOff,
    RecordingOn,
}

impl Control {
    pub fn word(self) -> &'static str {
        match self {
            Self::Pause => "pause",
            Self::Stop => "stop",
            Self::RecordingOff => "recording_off",
            Self::RecordingOn => "recording_on",
        }
    }
}

/// A misunderstanding the reply found or saw cleared, for the learner's
/// memory of that skill.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryNote {
    pub skill: String,
    pub text: String,
    #[serde(default)]
    pub cleared: bool,
}

/// The tutor's judgment on one turn.
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
    /// The tutor's judgment that the answer came from a blank page: it has
    /// not taught, shown, hinted at or corrected this skill in this sitting.
    pub cold: bool,
    pub control: Option<Control>,
}

/// Read a record block. The error says, in a few words, what is wrong, for
/// the hidden request that asks again.
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
    let control = match text_of("control")?.as_deref() {
        None => None,
        Some("pause") => Some(Control::Pause),
        Some("stop") => Some(Control::Stop),
        Some("recording_off") => Some(Control::RecordingOff),
        Some("recording_on") => Some(Control::RecordingOn),
        Some(other) => {
            return Err(format!(
                "control {other:?} is not pause, stop, recording_off, recording_on or null"
            ))
        }
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
        control,
    })
}

/// At most `bytes` bytes of `text`, cut on a character boundary.
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

/// A quick check due in this sitting, from the runtime's `sitting_checks`.
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

/// The task the learner has been set and not yet answered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenTask {
    pub text: String,
    /// The most help given on it so far.
    pub help: Help,
    /// A prediction asked for before teaching: its answer gets no mark.
    pub guess: bool,
    /// The label of the due quick check this task poses, if it poses one.
    pub check: Option<String>,
    /// Asked again after a mark: its next answer is not a first try.
    pub again: bool,
    /// The learner already had no starting move on it. Having none again on
    /// the same problem does not move its kind's help on a second time.
    #[serde(default)]
    pub stuck: bool,
    /// The skill the reply that posed it named, for an answer whose own
    /// record names none, as "I don't know" can leave it.
    #[serde(default)]
    pub skill: String,
}

/// What the engine knows of the lesson between turns.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Lesson {
    pub session_id: Option<String>,
    pub revision: Option<i64>,
    pub goal: Option<String>,
    pub task: Option<OpenTask>,
    pub steps: Vec<Step>,
    /// The quick checks due in this sitting and not yet posed.
    pub checks: Vec<DueCheck>,
    /// Each turn's request gets a new id: the runtime refuses an id used
    /// before anywhere in the study folder.
    pub turns: u64,
    /// Today, as "2026-11-12", and the skills answered on it, by their
    /// label in lower case: the runtime refuses a cold answer on a label
    /// already answered earlier that day.
    pub today: String,
    pub answered_today: Vec<String>,
}

impl Lesson {
    /// Start a new day's list of answered skills when the day has changed.
    pub fn set_today(&mut self, day: &str) {
        if self.today != day {
            self.today = day.to_owned();
            self.answered_today.clear();
        }
    }

    /// Note a skill answered today.
    pub fn answered(&mut self, label: &str) {
        let key = label.trim().to_lowercase();
        if !self.answered_today.contains(&key) {
            self.answered_today.push(key);
        }
    }

    fn answered_earlier_today(&self, label: &str) -> bool {
        self.answered_today.contains(&label.trim().to_lowercase())
    }
}

/// One message from the learner, as the engine sees it.
#[derive(Clone, Debug, Default)]
pub struct Message {
    /// The learner's exact words.
    pub words: String,
    /// The event the runtime captured them as.
    pub event_id: Option<String>,
}

impl Message {
    fn is(&self, words: &str) -> bool {
        self.words.trim().eq_ignore_ascii_case(words)
    }

    /// The confidence the window adds as its own line, "Confidence: sure".
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

/// What one turn writes: the request for `learning_step`, and whether it
/// recorded an attempt.
#[derive(Clone, Debug)]
pub struct Write {
    pub request: Value,
    pub marked: bool,
    /// Why the tutor marked the message as an answer and no attempt could
    /// be written: no task known to be open, or the message not captured.
    pub lost: Option<&'static str>,
}

/// The open task in the checkpoint's plan_text.
fn plan_text(task: &OpenTask) -> String {
    let mut lines = vec![
        format!("{TASK_LINE}{}", clip(&task.text, MAX_QUESTION)),
        format!("{HELP_LINE}{}.", task.help.word()),
    ];
    if let Some(check) = &task.check {
        lines.push(format!("{CHECK_LINE}\"{check}\""));
    }
    if task.guess {
        lines.push(GUESS_LINE.to_owned());
    }
    if task.again {
        lines.push(AGAIN_LINE.to_owned());
    }
    lines.join("\n")
}

/// The longest description a memory keeps here; older lines go first.
const MAX_MEMORY_TEXT: usize = 1500;

/// The memory edit for a misunderstanding found or cleared, made the way a
/// lesson run by the plugin makes one: a description of the skill, revised
/// in place, resting on this turn's observation (`client_ref` "answer").
/// `found` is the memory of that skill the runtime already holds, from a
/// memory search, if any. `day` is today, as "2026-11-12".
pub fn memory_edit(note: &MemoryNote, found: Option<&Value>, goal: &str, day: &str) -> Value {
    let line = format!(
        "{day}: {}{}",
        if note.cleared { "cleared: " } else { "" },
        note.text
    );
    let summary = if note.cleared {
        format!("Cleared: {}", note.text)
    } else {
        note.text.clone()
    };
    let new = json!({"new_observation": "answer"});
    match found {
        Some(memory) => {
            // Keep the description whole while it fits, dropping its oldest
            // lines first; the runtime keeps every revision.
            let mut lines: Vec<String> = memory["state_text"]
                .as_str()
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect();
            lines.push(line);
            while lines.len() > 1
                && lines.iter().map(|l| l.len() + 1).sum::<usize>() > MAX_MEMORY_TEXT
            {
                lines.remove(0);
            }
            let mut basis: Vec<Value> = memory["current_basis"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            basis.push(new.clone());
            let keep = basis.len().saturating_sub(8);
            let basis: Vec<Value> = basis.into_iter().skip(keep).collect();
            json!({
                "operation": "revise",
                "memory_id": memory["memory_id"],
                "expected_revision": memory["revision"],
                "record": {
                    "title": memory["title"],
                    "scope": memory["scope"],
                    "summary": clip(&summary, MAX_NOTE),
                    "state_text": clip(&lines.join("\n"), MAX_MEMORY_TEXT),
                    "current_basis": basis,
                },
                "change_kind": "new_observation",
                "reason": if note.cleared { "The misunderstanding was cleared." } else { "A misunderstanding was found." },
                "change_basis": [new],
            })
        }
        None => json!({
            "operation": "create",
            "client_ref": "memory",
            "record": {
                "title": note.skill,
                "scope": clip(if goal.is_empty() { note.skill.as_str() } else { goal }, MAX_NAME),
                "summary": clip(&summary, MAX_NOTE),
                "state_text": line,
                "current_basis": [new],
            },
            "reason": if note.cleared { "The misunderstanding was cleared." } else { "A misunderstanding was found." },
            "change_basis": [new],
        }),
    }
}

/// The memory a search found for this skill: the one whose title is the
/// skill's name.
pub fn memory_for<'a>(search: &'a Value, skill: &str) -> Option<&'a Value> {
    search["memories"].as_array()?.iter().find(|memory| {
        memory["title"]
            .as_str()
            .is_some_and(|title| title.trim().eq_ignore_ascii_case(skill.trim()))
            && memory["lifecycle"]
                .as_str()
                .is_none_or(|life| life == "active")
    })
}

/// The quick check a reply poses, by the words "Quick check N", the way the
/// runtime spots it in a stored reply.
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

impl Lesson {
    /// Apply one turn's record: update what the engine knows and return the
    /// `learning_step` that keeps it. `reply` is the reply as shown, and
    /// `request_id` names the request uniquely in the study folder.
    pub fn apply(
        &mut self,
        record: &Record,
        message: &Message,
        reply: &str,
        request_id: &str,
    ) -> Write {
        let mut open = self.task.clone();
        // The Hint button asks for help on the task in hand.
        if message.is(HINT_WORDS) {
            if let Some(task) = open.as_mut() {
                task.help = task.help.max(Help::Hint);
            }
        }
        let mut answer = record.answer;
        // "I don't know" on a task that is not a guess is a decline, never
        // a wrong answer.
        if message.is(DONT_KNOW_WORDS)
            && open.as_ref().is_some_and(|task| !task.guess && !task.again)
        {
            answer = Some(Mark::Declined);
        }
        let mut observations = Vec::new();
        let mut marked = false;
        let mut lost = None;
        let markable = open.as_ref().filter(|task| !task.guess && !task.again);
        if let (Some(mark), Some(task), Some(event)) = (answer, markable, &message.event_id) {
            // The runtime refuses an empty label, so a decline the engine
            // marks itself always has one: the check's, the answer's, the
            // task's, or the goal's.
            let label = clip(
                &task
                    .check
                    .clone()
                    .or_else(|| Some(record.skill.clone()).filter(|s| !s.trim().is_empty()))
                    .or_else(|| Some(task.skill.clone()).filter(|s| !s.trim().is_empty()))
                    .or_else(|| self.goal.clone().filter(|s| !s.trim().is_empty()))
                    .unwrap_or_else(|| "the task in hand".to_owned()),
                MAX_NAME,
            );
            // Cold: from a blank page, with no help. A due check is asked
            // cold. Any other answer is cold when the tutor judges it so and
            // the runtime would take it: no help, not a decline, and no
            // earlier answer on its label today.
            let unaided = task.help == Help::None && mark != Mark::Declined;
            let cold = unaided
                && (task.check.is_some() || (record.cold && !self.answered_earlier_today(&label)));
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
            self.answered(&label);
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
                // A guess, or a task asked again after its mark, is not
                // marked by rule: nothing is lost.
                (Some(task), _) if task.guess || task.again => None,
                (None, _) => Some("no task was known to be open"),
                (Some(_), None) => Some("the learner's message was not captured"),
                _ => None,
            };
        }
        // A misunderstanding found or cleared needs an observation of this
        // message to rest on, even when it was not an answer.
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

        // The task now open.
        let answered = answer.is_some() && open.is_some();
        let answered_text = open.as_ref().map(|task| task.text.clone());
        let was_stuck = open
            .as_ref()
            .is_some_and(|task| task.stuck || (record.no_start && marked));
        if answered {
            open = None;
        }
        let posed = posed_check(reply, &self.checks);
        match &record.task {
            Some(text) => match open.as_mut() {
                Some(task) if same_task(&task.text, text) => {
                    task.help = task.help.max(record.help);
                }
                _ => {
                    // After a decline there was no try, so the same task
                    // asked again, often after a worked example, gets its
                    // first one.
                    let same = answered
                        && answered_text
                            .as_deref()
                            .is_some_and(|before| same_task(before, text));
                    let again = same && answer != Some(Mark::Declined);
                    open = Some(OpenTask {
                        text: text.clone(),
                        help: record.help,
                        guess: record.guess,
                        check: posed.as_ref().map(|check| check.label.clone()),
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
        }
        self.task = open;
        if let Some(steps) = &record.steps {
            self.steps = steps.clone();
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
        if let Some(event) = &message.event_id {
            request["event_dispositions"] = json!([{
                "event_id": event,
                "outcome": if observations.is_empty() { "no_change" } else { "processed" },
            }]);
        }
        if let (Some(session), Some(revision)) = (&self.session_id, self.revision) {
            request["session"] =
                json!({"operation": "use", "session_id": session, "expected_revision": revision});
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
        // Every split into three pieces, at every byte that is a boundary.
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
        // One character at a time.
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
        // A fenced block, and a null answer.
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
        }
    }

    fn record(json: &str) -> Record {
        parse(json).unwrap()
    }

    #[test]
    fn an_answer_to_the_open_task_is_marked_with_the_help_given_on_it() {
        let mut lesson = lesson();
        // The reply poses a task.
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
        // The learner presses Hint; the reply gives one and leaves the task open.
        lesson.apply(
            &record(r#"{"answer": null, "help": "hint", "task": null}"#),
            &message(HINT_WORDS),
            "Think of a difference of squares.",
            "r-2",
        );
        // The model forgets the hint when it marks the answer: code does not.
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
        // The task restated in the same words keeps its help.
        lesson.task.as_mut().unwrap().help = Help::Hint;
        lesson.apply(
            &record(r#"{"answer": null, "task": "Solve x² + 2x − 15 = 0"}"#),
            &message("what is b again?"),
            "b is 2. Solve x² + 2x − 15 = 0",
            "r-3",
        );
        assert_eq!(lesson.task.as_ref().unwrap().help, Help::Hint);
        // After a decline there was no try: the same task asked again, as
        // after a worked example, gets its first one.
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
        // No starting move on it, then none again on the same problem after
        // the example: the kind's help moves on once, not twice.
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
        // A second answer on the same label today is never sent cold.
        open(&mut lesson, "A die pays its face: the average per roll?");
        let second = lesson.apply(&record(cold), &message("3.5"), "Right.", "r-2");
        assert_eq!(second.request["observations"][0]["attempt"]["cold"], false);
        // A new day starts a new list.
        lesson.set_today("2026-11-13");
        open(&mut lesson, "A spinner pays 1 or 11, 80/20: the average?");
        let next_day = lesson.apply(&record(cold), &message("3"), "Right.", "r-3");
        assert_eq!(next_day.request["observations"][0]["attempt"]["cold"], true);
        // Not cold when the tutor does not say so, after a hint, or as a decline.
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
    fn a_control_is_read_from_the_block() {
        let record = parse(r#"{"answer": null, "control": "recording_off"}"#).unwrap();
        assert_eq!(record.control, Some(Control::RecordingOff));
        assert!(parse(r#"{"answer": null, "control": "forget me"}"#)
            .unwrap_err()
            .contains("control"));
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
        // The model calls "I don't know" no answer and names no skill; the
        // engine marks it declined, and the runtime needs a label.
        let declined = lesson.apply(
            &record(r#"{"answer": null}"#),
            &message(DONT_KNOW_WORDS),
            "Fine.",
            "r-2",
        );
        let attempt = &declined.request["observations"][0]["attempt"];
        assert_eq!(attempt["first_try"], "declined");
        assert_eq!(attempt["label"], "sign of b");
        // With no skill anywhere, the goal names it.
        lesson.task = Some(OpenTask {
            text: "Another?".into(),
            help: Help::None,
            guess: false,
            check: None,
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
    fn a_due_check_keeps_its_label_and_is_cold_without_help() {
        let mut lesson = lesson();
        lesson.checks = vec![DueCheck {
            number: 1,
            label: "sign of b in the quadratic formula".into(),
            misses: vec![],
            question: None,
            key: None,
        }];
        lesson.apply(
            &record(r#"{"answer": null, "skill": "reading b", "task": "What is b in 2x² − 7x − 4 = 0?"}"#),
            &message("I'm back."),
            "Quick check 1: What is b in 2x² − 7x − 4 = 0?",
            "r-1",
        );
        assert!(lesson.checks.is_empty());
        let answer = lesson.apply(
            &record(r#"{"answer": "right", "skill": "reading coefficients", "task": null}"#),
            &message("−7"),
            "Right.",
            "r-2",
        );
        let attempt = &answer.request["observations"][0]["attempt"];
        assert_eq!(attempt["label"], "sign of b in the quadratic formula");
        assert_eq!(attempt["cold"], true);
        // "Quick check 1" is not "Quick check 12".
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

    #[test]
    fn a_misunderstanding_makes_a_memory_or_revises_the_one_there() {
        let note = MemoryNote {
            skill: "sign of b".into(),
            text: "Drops the minus sign of b.".into(),
            cleared: false,
        };
        let create = memory_edit(&note, None, "Quadratics", "2026-11-12");
        assert_eq!(create["operation"], "create");
        assert_eq!(create["record"]["title"], "sign of b");
        assert_eq!(create["record"]["scope"], "Quadratics");
        assert_eq!(
            create["record"]["state_text"],
            "2026-11-12: Drops the minus sign of b."
        );
        let search = json!({"memories": [
            {"title": "Sign of B", "memory_id": "m-1", "revision": 2, "scope": "Quadratics",
             "state_text": "2026-11-11: Drops the minus sign of b.\nanchor: reads a from a bare x²",
             "current_basis": [{"observation_id": "o-1"}], "lifecycle": "active"}
        ]});
        let found = memory_for(&search, "sign of b").unwrap();
        let cleared = MemoryNote {
            cleared: true,
            text: "Keeps the sign now.".into(),
            ..note
        };
        let revise = memory_edit(&cleared, Some(found), "Quadratics", "2026-11-12");
        assert_eq!(revise["operation"], "revise");
        assert_eq!(revise["memory_id"], "m-1");
        assert_eq!(revise["expected_revision"], 2);
        let text = revise["record"]["state_text"].as_str().unwrap();
        assert!(text.starts_with("2026-11-11: Drops"), "{text}");
        assert!(
            text.contains("anchor: reads a from a bare x²"),
            "an anchor is kept: {text}"
        );
        assert!(
            text.ends_with("2026-11-12: cleared: Keeps the sign now."),
            "{text}"
        );
        assert_eq!(
            revise["record"]["current_basis"].as_array().unwrap().len(),
            2
        );
        assert!(memory_for(&search, "sign of c").is_none());
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
}
