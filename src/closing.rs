//! The closing lines: what a sitting leaves the learner with when it ends.
//!
//! At most two lines, worked out here from the record and nowhere else: the
//! passes earned today, and when each skill answered today can count next.
//! Nothing at all when no answer was marked today. A declined question is not
//! an answer (as in `magnesium progress`), so a day of declines ends with
//! nothing. A wrong or partly right answer is an answer: it earns no pass, so
//! it is never named as earned, but its skill's next day is named.
//!
//! A sitting ends when the learner says they are stopping and the tutor tells
//! the runtime with `learning_step` `{"action":"end_sitting"}`, or asks for a
//! pause or a stop. The runtime keeps when it ended in the sittings store.
//! Claude Code in a terminal shows the lines once, from the Stop hook, under
//! the reply that ended it; the study window reads `magnesium closing --json`
//! after each turn and draws the same lines under the same reply. Neither
//! place words them itself, and the tutor is never given them to repeat.
use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;

use chrono::{Local, NaiveDate};
use serde_json::{Value, json};

use crate::learner::label_key;
use crate::sittings::{SittingNow, Sittings, local_day, now_ms};
use crate::workspace::Workspace;

/// The learning_step action the tutor sends when the learner stops.
pub(crate) const END_ACTION: &str = "end_sitting";

/// How many skills a line names before it counts the rest.
const NAMES_IN_LINE: usize = 3;
/// How many days the second line names before it counts the rest.
const DAYS_IN_LINE: usize = 3;

/// `learning_step` `{"schema":"learner-model/v2","action":"end_sitting"}`.
#[allow(dead_code)]
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct EndSittingRequest {
    pub schema: crate::learner::SchemaVersion,
    pub action: EndSittingTag,
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub(crate) enum EndSittingTag {
    #[serde(rename = "end_sitting")]
    EndSitting,
}

/// What the record says of today, for the closing lines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Closing {
    pub(crate) today: NaiveDate,
    /// Skills with a pass earned today that are not proven yet.
    pub(crate) earned: Vec<String>,
    /// Skills whose pass today proved them.
    pub(crate) proved: Vec<String>,
    /// Skills answered today and not proven, by the first day their next pass
    /// can count, earliest first. A skill waiting for a repair has no day and
    /// is left out.
    pub(crate) next: Vec<(NaiveDate, Vec<String>)>,
}

impl Closing {
    /// The lines, in the order they are shown.
    pub(crate) fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let first = match (self.earned.is_empty(), self.proved.is_empty()) {
            (true, true) => None,
            (false, true) => Some(format!("Earned today: a pass on {}.", names(&self.earned))),
            (true, false) => Some(format!(
                "Earned today: the {} that {} {}.",
                plural(self.proved.len(), "pass", "passes"),
                plural(self.proved.len(), "proves", "prove"),
                names(&self.proved)
            )),
            (false, false) => Some(format!(
                "Earned today: a pass on {}, and the {} that {} {}.",
                names(&self.earned),
                plural(self.proved.len(), "pass", "passes"),
                plural(self.proved.len(), "proves", "prove"),
                names(&self.proved)
            )),
        };
        lines.extend(first);
        if !self.next.is_empty() {
            let mut parts: Vec<String> = self
                .next
                .iter()
                .take(DAYS_IN_LINE)
                .map(|(day, skills)| format!("{} from {}", names(skills), self.day_words(*day)))
                .collect();
            let more: usize = self
                .next
                .iter()
                .skip(DAYS_IN_LINE)
                .map(|(_, skills)| skills.len())
                .sum();
            if more > 0 {
                parts.push(format!("{more} more later"));
            }
            lines.push(format!("Can count next: {}.", parts.join(", ")));
        }
        lines
    }

    fn day_words(&self, day: NaiveDate) -> String {
        if self.today.succ_opt() == Some(day) {
            "tomorrow".to_owned()
        } else {
            day.format("%a %-d %b").to_string()
        }
    }

    pub(crate) fn to_json(&self) -> Value {
        json!({
            "lines": self.lines(),
            "earned": self.earned,
            "proved": self.proved,
            "next": self
                .next
                .iter()
                .map(|(day, skills)| json!({"day": day.to_string(), "skills": skills}))
                .collect::<Vec<_>>(),
        })
    }
}

fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 { one } else { many }
}

/// "a", "a and b", "a, b and c", "a, b, c and 2 more".
fn names(skills: &[String]) -> String {
    let shown = &skills[..skills.len().min(NAMES_IN_LINE)];
    let rest = skills.len() - shown.len();
    match (shown, rest) {
        ([], _) => String::new(),
        ([one], 0) => one.clone(),
        (shown, 0) => format!(
            "{} and {}",
            shown[..shown.len() - 1].join(", "),
            shown[shown.len() - 1]
        ),
        (shown, rest) => format!("{} and {rest} more", shown.join(", ")),
    }
}

/// One marked answer, as the closing lines need it.
#[derive(Clone, Debug)]
pub(crate) struct Answered {
    pub(crate) attempt_id: String,
    pub(crate) key: String,
    pub(crate) day: NaiveDate,
    pub(crate) declined: bool,
}

/// The closing lines' facts from what `magnesium progress --json` prints and
/// the record's answers, oldest first. None when no answer was marked today, or when there
/// is nothing to say: no pass today and no day for any skill answered today.
pub(crate) fn from_record(
    progress: &Value,
    answers: &[Answered],
    today: NaiveDate,
) -> Option<Closing> {
    let today_ids: BTreeSet<&str> = answers
        .iter()
        .filter(|answer| answer.day == today)
        .map(|answer| answer.attempt_id.as_str())
        .collect();
    // Each skill answered today, by when it was first answered today: the
    // lines name skills in the order the learner met them.
    let mut answered_today: BTreeMap<&str, usize> = BTreeMap::new();
    for answer in answers.iter().filter(|a| a.day == today && !a.declined) {
        let at = answered_today.len();
        answered_today.entry(answer.key.as_str()).or_insert(at);
    }
    if answered_today.is_empty() {
        return None;
    }
    let order = |name: &String| {
        answered_today
            .get(label_key(name).as_str())
            .copied()
            .unwrap_or(usize::MAX)
    };
    let plan_skills = progress["plans"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|plan| plan["from"] == "record")
        .flat_map(|plan| plan["skills"].as_array().into_iter().flatten());
    let other_skills = progress["other"]["skills"].as_array().into_iter().flatten();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut closing = Closing {
        today,
        ..Closing::default()
    };
    let mut next: BTreeMap<NaiveDate, Vec<String>> = BTreeMap::new();
    for skill in plan_skills.chain(other_skills) {
        let Some(name) = skill["skill"].as_str() else {
            continue;
        };
        let key = label_key(name);
        if !seen.insert(key.clone()) {
            continue;
        }
        // A pass it holds now that an answer of today's earned. A pass made
        // today and lost to a miss later today is not held, so not named.
        let earned_today = skill["passed_by"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .any(|id| today_ids.contains(id));
        let proven = skill["proven"] == true;
        if earned_today {
            if proven {
                closing.proved.push(name.to_owned());
            } else {
                closing.earned.push(name.to_owned());
            }
        }
        if !proven
            && answered_today.contains_key(key.as_str())
            && let Some(day) = skill["next_on"]
                .as_str()
                .and_then(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
            // The lines never point at more to do today.
            && day > today
        {
            next.entry(day).or_default().push(name.to_owned());
        }
    }
    closing.earned.sort_by_key(order);
    closing.proved.sort_by_key(order);
    closing.next = next
        .into_iter()
        .map(|(day, mut skills)| {
            skills.sort_by_key(order);
            (day, skills)
        })
        .collect();
    (!closing.lines().is_empty()).then_some(closing)
}

fn load_answered(workspace: &Workspace) -> Result<Vec<Answered>, String> {
    let store = crate::practice::open_store(workspace)?;
    let mut statement = store
        .conn
        .prepare(
            "SELECT attempt_id, label_key, mark, answered_at_ms FROM attempts ORDER BY answered_at_ms, attempt_no",
        )
        .map_err(|e| e.to_string())?;
    statement
        .query_map([], |r| {
            Ok(Answered {
                attempt_id: r.get(0)?,
                key: r.get(1)?,
                declined: r.get::<_, String>(2)? == "declined",
                day: local_day(r.get(3)?),
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}

/// The closing lines for a study folder now. The one function that works
/// them out: the terminal, the window and a sitting of checks alone all show
/// what it returns. Ok(None) when there is nothing to show.
pub(crate) fn closing(workspace: &Workspace, now: i64) -> Result<Option<Closing>, String> {
    let progress = crate::progress::record(workspace, now)?;
    if progress["upgrade_needed"] == true {
        return Ok(None);
    }
    let answers = load_answered(workspace)?;
    Ok(from_record(&progress, &answers, local_day(now)))
}

/// End the current sitting for the conversation `by`, and say whether the
/// closing lines will show. Both the sitting end and the read change nothing
/// in the learner record.
pub(crate) fn end_sitting(
    workspace: &Workspace,
    by: Option<&str>,
    now: i64,
) -> Result<(Option<String>, Option<Closing>), String> {
    let mut sittings = Sittings::open(workspace)?;
    let ended = sittings.end(by, now)?;
    let closing = closing(workspace, now).ok().flatten();
    Ok((ended, closing))
}

/// Whether a learning_step request asks to end the sitting.
pub(crate) fn is_end_request(request: &Value) -> bool {
    request.get("action").and_then(Value::as_str) == Some(END_ACTION)
}

/// The tool's answer to `end_sitting`. The tutor is told that the learner
/// sees the lines, never what they say, so it has nothing to repeat.
pub(crate) fn end_from_tool(cwd: &std::path::Path, by: Option<&str>) -> Value {
    let workspace = match Workspace::discover(cwd) {
        Ok(Some(workspace)) => workspace,
        _ => {
            return json!({"error": {"code": "workspace_not_initialized", "message": "No Magnesium study folder here."}});
        }
    };
    if let Err(error) = crate::adapter::check_workspace_binding(&workspace) {
        return json!({"error": {"code": "workspace_binding", "message": error}});
    }
    match end_sitting(&workspace, by, now_ms()) {
        Ok((Some(_), shown)) => json!({
            "schema": crate::learner::SCHEMA,
            "action": END_ACTION,
            "sitting_ended": true,
            "closing_shown": shown.is_some(),
            "note": if shown.is_some() {
                "The sitting has ended. Under your reply, Magnesium shows the learner what the record says they earned today and when each skill they answered can count next. Do not say any of it yourself. Set no task and ask no question: close in a line."
            } else {
                "The sitting has ended. No answer was marked today, so Magnesium shows nothing. Set no task and ask no question: close in a line."
            },
        }),
        Ok((None, _)) => json!({
            "schema": crate::learner::SCHEMA,
            "action": END_ACTION,
            "sitting_ended": false,
            "closing_shown": false,
            "note": "No sitting has begun in this study folder, so there is nothing to end. Set no task and ask no question: close in a line.",
        }),
        Err(error) => json!({"error": {"code": "sittings_unavailable", "message": error}}),
    }
}

/// A pause or a stop the learner asked for ends the sitting too.
pub(crate) fn ends_on_control(request: &Value, result: &Value) -> bool {
    request.get("action").and_then(Value::as_str) == Some("control")
        && matches!(
            request.get("control").and_then(Value::as_str),
            Some("pause" | "stop")
        )
        && result.get("durable") == Some(&Value::Bool(true))
}

/// The lines for Claude Code's Stop hook, once for each end, and only in the
/// conversation that ended the sitting. The study window draws them itself
/// from `magnesium closing --json`, so a hook in a session it started says
/// nothing and leaves the end unclaimed.
pub(crate) fn stop_hook_lines(workspace: &Workspace, by: &str) -> Option<String> {
    if crate::mcp::catalog::window_surface() {
        return None;
    }
    let mut sittings = Sittings::open_existing(workspace).ok()??;
    let now_state = sittings.now_state().ok()??;
    let ended = now_state.ended_ms?;
    let other = now_state
        .ended_by
        .as_deref()
        .is_some_and(|ender| ender != by);
    if other || now_state.shown_ms.is_some_and(|shown| shown >= ended) {
        return None;
    }
    let now = now_ms();
    if !sittings.claim_end_shown(&now_state.id, now).ok()? {
        return None;
    }
    let lines = closing(workspace, now).ok()??.lines();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn sitting_json(state: Option<&SittingNow>) -> Value {
    match state {
        Some(state) => json!({
            "started_ms": state.started_ms,
            "ended_ms": state.ended_ms,
        }),
        None => Value::Null,
    }
}

fn closing_value(workspace: &Workspace, now: i64) -> Result<Value, String> {
    let progress = crate::progress::record(workspace, now)?;
    if progress["upgrade_needed"] == true {
        return Ok(progress);
    }
    let answers = load_answered(workspace)?;
    let today = local_day(now);
    let sitting = Sittings::open_existing(workspace)
        .ok()
        .flatten()
        .and_then(|sittings| sittings.now_state().ok().flatten());
    Ok(json!({
        "active": true,
        "today": today.to_string(),
        "sitting": sitting_json(sitting.as_ref()),
        "closing": from_record(&progress, &answers, today).map(|closing| closing.to_json()),
    }))
}

/// `magnesium closing`: the closing lines for today, as they would show if
/// the sitting ended now, and when the current sitting ended. Changes
/// nothing in the folder.
pub(crate) fn print_closing(as_json: bool) -> ExitCode {
    let result = (|| {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        match Workspace::discover(&cwd).map_err(|e| e.to_string())? {
            Some(workspace) => closing_value(&workspace, Local::now().timestamp_millis()),
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
        Ok(value) if value["upgrade_needed"] == true => {
            println!(
                "An earlier version of Magnesium set this folder up. Run `magnesium init --upgrade-instructions` to upgrade it, then ask again."
            );
            ExitCode::SUCCESS
        }
        Ok(value) if value["active"] == true => {
            for line in value["closing"]["lines"].as_array().into_iter().flatten() {
                println!("{}", line.as_str().unwrap_or_default());
            }
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
mod tests;
