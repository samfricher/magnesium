//! The closing lines: what a sitting leaves the learner with when it ends.
//!
//! At most two lines, worked out here from the record and nowhere else: the
//! passes earned today, and the first day a skill answered today can count
//! again, with the skills that can count then.
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

/// What a lesson's session-start notice says of the end of a sitting.
pub(crate) const END_GUIDANCE: &str = " When the learner says they are stopping for now, send learning_step {\"schema\":\"learner-model/v2\",\"action\":\"end_sitting\"} before your reply, and set no task.";

/// The longest a line may be. At the window's usual measure and text size the
/// reading column shows about 78 characters of this text on one line, and
/// about 66 with the plan beside the lesson in a window 1100 points wide (the
/// photographs of 2026-11-16); a few fewer leave room for wide letters.
const LINE_CHARS: usize = 64;
/// How many skills a line names before it counts the rest.
const NAMES_IN_LINE: usize = 3;

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
    /// The lines, in the order they are shown. Each fits in
    /// [`LINE_CHARS`], whatever the names, so that it is one line in the
    /// window at its usual measure and text size, and the terminal says the
    /// same words.
    pub(crate) fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let first = match (self.earned.is_empty(), self.proved.is_empty()) {
            (true, true) => None,
            (false, true) => Some(fit("Earned today: a pass on ", &self.earned, ".")),
            (true, false) => Some(fit("Proven today: ", &self.proved, ".")),
            // A skill proved today leads, as the larger step; the other
            // passes are counted, so the line keeps to its length.
            (false, false) => Some(fit(
                "Proven today: ",
                &self.proved,
                &format!(", and a pass on {} more.", self.earned.len()),
            )),
        };
        lines.extend(first);
        // The first day any skill answered today can count again, and the
        // skills that can count then. Later days are on the workspace's page.
        if let Some((day, skills)) = self.next.first() {
            lines.push(fit(
                &format!("Can count from {}: ", self.day_words(*day)),
                skills,
                ".",
            ));
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

/// "a", "a and b", "a, b and c"; with names left out, "a, b and 2 more".
fn names(shown: &[String], rest: usize) -> String {
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

/// `prefix`, then as many of `skills` as fit in [`LINE_CHARS`] with the
/// rest counted, then `suffix`. When even the first name does not fit, it is
/// cut at a word and ends in "…": the record's own words, shortened.
fn fit(prefix: &str, skills: &[String], suffix: &str) -> String {
    let length = |text: &str| text.chars().count();
    let most = skills.len().min(NAMES_IN_LINE);
    for shown in (1..=most).rev() {
        let line = format!(
            "{prefix}{}{suffix}",
            names(&skills[..shown], skills.len() - shown)
        );
        if length(&line) <= LINE_CHARS {
            return line;
        }
    }
    let Some(first) = skills.first() else {
        return format!("{prefix}{suffix}");
    };
    let rest = match skills.len() - 1 {
        0 => String::new(),
        more => format!(" and {more} more"),
    };
    // A cut name ends the sentence with its "…", not "….".
    let suffix = if rest.is_empty() && suffix == "." {
        ""
    } else {
        suffix
    };
    let room = LINE_CHARS.saturating_sub(length(prefix) + length(&rest) + length(suffix) + 1);
    format!("{prefix}{}…{rest}{suffix}", clip_at_word(first, room))
}

/// The longest start of `name` of at most `room` characters that ends at
/// the end of a word, or its first `room` characters when one word is longer.
fn clip_at_word(name: &str, room: usize) -> String {
    let mut kept = String::new();
    for word in name.split_whitespace() {
        let next = if kept.is_empty() {
            word.to_owned()
        } else {
            format!("{kept} {word}")
        };
        if next.chars().count() > room {
            break;
        }
        kept = next;
    }
    if kept.is_empty() {
        kept = name.chars().take(room).collect();
    }
    kept.trim_end_matches([',', ';', ':']).to_owned()
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
