use std::path::Path;

use serde_json::{Value, json};

use crate::sittings::{Sittings, local_day, now_ms};
use crate::workspace::Workspace;

pub(crate) const ACTION: &str = "just_checks";

#[cfg(test)]
const WINDOW_WORDS: &str = "Just the quick checks.";

const COMMAND: &str = "/magnesium:checks";

const ASKS: [&str; 8] = [
    "just the checks",
    "just the quick checks",
    "only the checks",
    "only the quick checks",
    "just checks",
    "just quick checks",
    "checks only",
    "quick checks only",
];

const NEW_WORK: [&str; 5] = [
    "orient_and_probe",
    "plan_and_route",
    "teach_and_practice",
    "papers",
    "papers_loop",
];

pub(crate) const ENDED_NOTE: &str = "That was the last quick check of this sitting of checks alone, so Magnesium has ended the sitting: under your reply the learner sees what their record says of today. Give this answer's feedback, then close in a line. Do not say what they earned, set no task and ask no question. If they asked for something else instead, do it now: the sitting of checks is over.";

#[allow(dead_code)]
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[schemars(
    description = "`learning_step` `{\"schema\":\"learner-model/v2\",\"action\":\"just_checks\"}`."
)]
#[serde(deny_unknown_fields)]
pub(crate) struct JustChecksRequest {
    pub schema: crate::learner::SchemaVersion,
    pub action: JustChecksTag,
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub(crate) enum JustChecksTag {
    #[serde(rename = "just_checks")]
    JustChecks,
}

pub(crate) fn is_request(request: &Value) -> bool {
    request.get("action").and_then(Value::as_str) == Some(ACTION)
}

pub(crate) fn asked_in_words(text: &str) -> bool {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix(COMMAND) {
        return rest.is_empty() || rest.starts_with(char::is_whitespace);
    }
    let words = text
        .strip_prefix("/magnesium:learn")
        .filter(|rest| rest.starts_with(char::is_whitespace))
        .unwrap_or(text);
    let words = words
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let words = words.trim_end_matches(['.', '!', ' ']);
    let words = words
        .strip_suffix(" please")
        .or_else(|| words.strip_suffix(", please"))
        .unwrap_or(words)
        .trim_end_matches([',', ' ']);
    ASKS.contains(&words)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Asker {
    pub(crate) lesson_active: bool,
}

pub(crate) fn from_tool(
    cwd: &Path,
    by: Option<&str>,
    asker: impl FnOnce(&Workspace) -> Asker,
    start: impl FnOnce() -> Result<(), String>,
) -> Value {
    let workspace = match Workspace::discover(cwd) {
        Ok(Some(workspace)) => workspace,
        _ => {
            return json!({"error": {"code": "workspace_not_initialized", "message": "No Magnesium study folder here."}});
        }
    };
    if let Err(error) = crate::adapter::check_workspace_binding(&workspace) {
        return json!({"error": {"code": "workspace_binding", "message": error}});
    }
    let answer = |checks: usize, note: String| {
        json!({
            "schema": crate::learner::SCHEMA,
            "action": ACTION,
            "checks": checks,
            "note": note,
        })
    };
    let asker = asker(&workspace);
    let now = now_ms();
    let mut sittings = match Sittings::open(&workspace) {
        Ok(sittings) => sittings,
        Err(error) => {
            return json!({"error": {"code": "sittings_unavailable", "message": error}});
        }
    };
    let checks = match sittings.ask_checks_only(&workspace, by, now) {
        Ok(checks) => checks,
        Err(error) => {
            return json!({"error": {"code": "sittings_unavailable", "message": error}});
        }
    };
    if checks == 0 {
        let next = next_check_day(&workspace, local_day(now));
        let mut out = answer(
            0,
            match next {
                Some(day) => format!(
                    "No quick check is due now. The next is due {}. Say so in a line, set no task and ask no question.",
                    day.format("%a %-d %b")
                ),
                None => {
                    "No quick check is waiting. Say so in a line, set no task and ask no question."
                        .to_owned()
                }
            },
        );
        out["next_due"] = next.map(|day| day.to_string()).into();
        return out;
    }
    let opener = sittings
        .take_checks_only_opener(&workspace, by, now)
        .ok()
        .flatten()
        .or_else(|| {
            sittings
                .checks_only_opener(&workspace, by, now)
                .ok()
                .flatten()
        });
    let Some(opener) = opener else {
        return json!({"error": {"code": "sittings_unavailable", "message": "the sitting's checks could not be read"}});
    };
    let started = !asker.lesson_active && start().is_ok();
    let note = if asker.lesson_active || started {
        "Pose the checks as sitting_opener says.".to_owned()
    } else {
        "No lesson is active in this conversation, and an answer is recorded only in one: before your reply, start one with learning_step {\"schema\":\"learner-model/v2\",\"action\":\"update\",\"request_id\":\"<new>\",\"session\":{\"operation\":\"start\",\"client_ref\":\"lesson\",\"goal\":\"Quick checks\"},\"native_capture\":{\"ticket\":\"<the prompt notice's ticket>\",\"text\":\"<the learner's message, exactly>\"}}, or resume theirs as your instructions say. Then pose the checks and end as sitting_opener says.".to_owned()
    };
    let mut out = answer(checks, note);
    if started {
        out["lesson_started"] = "Quick checks".into();
    }
    out["sitting_opener"] = opener.text.into();
    if crate::sittings::checks_wanted() {
        out["sitting_checks"] = opener.checks;
        out["sitting_redos"] = opener.redos;
    }
    out
}

fn next_check_day(workspace: &Workspace, today: chrono::NaiveDate) -> Option<chrono::NaiveDate> {
    let store = crate::practice::open_store(workspace).ok()?;
    let items = crate::practice::snapshot(&store.conn, workspace.root()).ok()?;
    items
        .iter()
        .filter(|item| item.facts.origin == crate::learner::practice::Origin::Chat)
        .filter(|item| !item.state.closed())
        .filter_map(|item| item.state.next_on)
        .filter(|day| *day > today)
        .min()
}

pub(crate) const LIFTED_NOTE: &str = "The learner asked for more than the quick checks, so this is an ordinary sitting now. The checks not yet answered stay due.";

pub(crate) fn lift_on_stage(
    cwd: &Path,
    operation: &str,
    request: &Value,
    by: Option<&str>,
) -> bool {
    let stage = request.get("stage").and_then(Value::as_str);
    if operation != "context"
        || request
            .get("action")
            .is_some_and(|action| action != "instructions")
        || !stage.is_some_and(|stage| NEW_WORK.contains(&stage))
    {
        return false;
    }
    lift(cwd, by)
}

fn lift(cwd: &Path, by: Option<&str>) -> bool {
    let Some(workspace) = Workspace::discover(cwd).ok().flatten() else {
        return false;
    };
    if crate::adapter::check_workspace_binding(&workspace).is_err() {
        return false;
    }
    Sittings::open_existing(&workspace)
        .ok()
        .flatten()
        .is_some_and(|mut sittings| sittings.lift_checks_only(by).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_that_ask_for_the_checks_alone_are_told_for_certain() {
        for text in [
            "/magnesium:checks",
            "/magnesium:checks please",
            "just the checks",
            "Just the checks.",
            "  JUST THE QUICK CHECKS!  ",
            "Just the checks, please.",
            "only the quick checks",
            "/magnesium:learn Just the quick checks.",
            WINDOW_WORDS,
        ] {
            assert!(asked_in_words(text), "{text}");
        }
        for text in [
            "/magnesium:checksum",
            "/magnesium:learn",
            "not just the checks, teach me Km",
            "just the checks and then Km",
            "quick check 1: x = 2",
            "teach me the checks",
        ] {
            assert!(!asked_in_words(text), "{text}");
        }
    }
}
