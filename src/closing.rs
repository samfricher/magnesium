use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;

use chrono::{Local, NaiveDate};
use serde_json::{Value, json};

use crate::learner::label_key;
use crate::sittings::{SittingNow, Sittings, local_day, now_ms};
use crate::workspace::Workspace;

pub(crate) const END_ACTION: &str = "end_sitting";

pub(crate) const END_GUIDANCE: &str = " When the learner says they are stopping for now, send learning_step {\"schema\":\"learner-model/v2\",\"action\":\"end_sitting\"} before your reply, and set no task.";

const LINE_CHARS: usize = 64;
const NAMES_IN_LINE: usize = 3;

#[allow(dead_code)]
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[schemars(
    description = "`learning_step` `{\"schema\":\"learner-model/v2\",\"action\":\"end_sitting\"}`."
)]
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Closing {
    pub(crate) today: NaiveDate,
    pub(crate) earned: Vec<String>,
    pub(crate) proved: Vec<String>,
    pub(crate) next: Vec<(NaiveDate, Vec<String>)>,
}

impl Closing {
    pub(crate) fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let first = match (self.earned.is_empty(), self.proved.is_empty()) {
            (true, true) => None,
            (false, true) => Some(fit("Earned today: a pass on ", &self.earned, ".")),
            (true, false) => Some(fit("Proven today: ", &self.proved, ".")),
            (false, false) => Some(fit(
                "Proven today: ",
                &self.proved,
                &format!(", and a pass on {} more.", self.earned.len()),
            )),
        };
        lines.extend(first);
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
    let suffix = if rest.is_empty() && suffix == "." {
        ""
    } else {
        suffix
    };
    let room = LINE_CHARS.saturating_sub(length(prefix) + length(&rest) + length(suffix) + 1);
    format!("{prefix}{}…{rest}{suffix}", clip_at_word(first, room))
}

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

#[derive(Clone, Debug)]
pub(crate) struct Answered {
    pub(crate) key: String,
    pub(crate) day: NaiveDate,
    pub(crate) declined: bool,
}

pub(crate) fn from_record(
    progress: &Value,
    answers: &[Answered],
    today: NaiveDate,
) -> Option<Closing> {
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
        let passes = skill["passes"].as_i64().unwrap_or(0);
        let before = skill["passes_before_today"].as_i64().unwrap_or(passes);
        let earned_today = passes > before;
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
            "SELECT label_key, mark, answered_at_ms FROM attempts ORDER BY answered_at_ms, attempt_no",
        )
        .map_err(|e| e.to_string())?;
    statement
        .query_map([], |r| {
            Ok(Answered {
                key: r.get(0)?,
                declined: r.get::<_, String>(1)? == "declined",
                day: local_day(r.get(2)?),
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}

pub(crate) fn closing(workspace: &Workspace, now: i64) -> Result<Option<Closing>, String> {
    let progress = crate::progress::record(workspace, now)?;
    if progress["upgrade_needed"] == true {
        return Ok(None);
    }
    let answers = load_answered(workspace)?;
    Ok(from_record(&progress, &answers, local_day(now)))
}

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

pub(crate) fn is_end_request(request: &Value) -> bool {
    request.get("action").and_then(Value::as_str) == Some(END_ACTION)
}

pub(crate) fn end_from_tool(
    cwd: &std::path::Path,
    caller: Option<&crate::learner::HostContext>,
) -> Result<Value, String> {
    let workspace = match Workspace::discover(cwd) {
        Ok(Some(workspace)) => workspace,
        _ => {
            return Ok(
                json!({"error": {"code": "workspace_not_initialized", "message": "No Magnesium study folder here."}}),
            );
        }
    };
    if let Err(error) = crate::adapter::check_workspace_binding(&workspace) {
        return Ok(json!({"error": {"code": "workspace_binding", "message": error}}));
    }
    if let Some(host) = caller {
        crate::adapter::refuse_changed_conversation(&workspace, host)?;
    }
    let by = caller.map(|host| host.host_thread_id.as_str());
    Ok(match end_sitting(&workspace, by, now_ms()) {
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
    })
}

pub(crate) fn stop_hook_lines(workspace: &Workspace, by: &str) -> Option<String> {
    if crate::mcp::catalog::window_surface() {
        return None;
    }
    stop_hook_lines_with(workspace, by, |now| {
        closing(workspace, now).ok().flatten().map(|c| c.lines())
    })
}

fn stop_hook_lines_with(
    workspace: &Workspace,
    by: &str,
    lines_at: impl FnOnce(i64) -> Option<Vec<String>>,
) -> Option<String> {
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
    let lines = lines_at(now)?;
    if !sittings.claim_end_shown(&now_state.id, now).ok()? {
        return None;
    }
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
    let sitting = crate::sittings::now_state_read_only(workspace)
        .ok()
        .flatten();
    Ok(json!({
        "active": true,
        "today": today.to_string(),
        "sitting": sitting_json(sitting.as_ref()),
        "closing": from_record(&progress, &answers, today).map(|closing| closing.to_json()),
    }))
}

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
