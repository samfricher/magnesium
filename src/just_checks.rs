//! "Just the checks": a sitting that poses the quick checks that are due and
//! nothing else, then ends with the closing lines and no task.
//!
//! A learner asks for it in their own words, with the plugin's
//! `/magnesium:checks`, or with the study window's control, which sends
//! [`WINDOW_WORDS`] as a new lesson's first words. The prompt hook takes the
//! ask from words it can tell for certain ([`asked_in_words`]); the tutor
//! tells the runtime of any other with `learning_step`
//! `{"schema":"learner-model/v2","action":"just_checks"}`. Both make the same
//! sitting through [`crate::sittings::Sittings::ask_checks_only`], so a
//! lesson in the window and one in a terminal pose the same checks and end at
//! the same answer. A request for anything else is the learner's to make: the
//! tutor loading a stage for it makes the sitting an ordinary one.

use std::path::Path;

use serde_json::{Value, json};

use crate::sittings::{Sittings, local_day, now_ms};
use crate::workspace::Workspace;

/// The learning_step action the tutor sends when the learner asks.
pub(crate) const ACTION: &str = "just_checks";

/// The words the study window's control sends. The direct engine knows them
/// too (`app/crates/core/src/direct.rs`); here they are one of [`ASKS`].
#[cfg(test)]
const WINDOW_WORDS: &str = "Just the quick checks.";

/// The plugin's command for it.
const COMMAND: &str = "/magnesium:checks";

/// Whole messages that ask for the checks alone, in any case and with any
/// stop. Other wordings reach the runtime through the tutor.
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

/// Stages that bring new material, a plan or a paper: the tutor loads one
/// when the learner asks for more than the checks.
const NEW_WORK: [&str; 5] = [
    "orient_and_probe",
    "plan_and_route",
    "teach_and_practice",
    "papers",
    "papers_loop",
];

/// What the tutor is told in the result that settled the last check.
pub(crate) const ENDED_NOTE: &str = "That was the last quick check of this sitting of checks alone, so Magnesium has ended the sitting: under your reply the learner sees what their record says of today. Give this answer's feedback, then close in a line. Do not say what they earned, set no task and ask no question. If they asked for something else instead, do it now: the sitting of checks is over.";

/// `learning_step` `{"schema":"learner-model/v2","action":"just_checks"}`.
#[allow(dead_code)]
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
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

/// Whether a learning_step request asks for the checks alone.
pub(crate) fn is_request(request: &Value) -> bool {
    request.get("action").and_then(Value::as_str) == Some(ACTION)
}

/// Whether a learner's whole message asks for the checks alone: the plugin's
/// command, or one of [`ASKS`], on its own or as the words of the plugin's
/// `/magnesium:learn` (as the study window sends a lesson's first words).
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

/// What the conversation that asks can do with the checks: whether it
/// records, and whether a lesson is active in it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Asker {
    pub(crate) records: bool,
    pub(crate) lesson_active: bool,
}

/// The tool's answer to `just_checks`, asked by the conversation `by`. An
/// answer is recorded only in a conversation that records and has a lesson
/// active, and only a recorded answer settles a check, so with checks to pose
/// and no lesson active here, `start` starts one, named "Quick checks". The
/// learner asked for it, which is the explicit start capture needs.
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
    if !asker.records {
        return answer(
            0,
            "Recording is off in this conversation, so no answer to a quick check can be marked now. Say so in a line, set no task and ask no question.".to_owned(),
        );
    }
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
    // The opener goes out here when it has not yet, and claims the usual
    // one, so its paper redos are not given later in the sitting.
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
    // In the live runs of 2026-11-16 a tutor left to start the lesson itself
    // once posed the checks with none active, so no answer was recorded, and
    // once took five tries to start it.
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

/// The first day after `today` on which a quick check is due.
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

/// What the tutor is told when a stage it loads makes a sitting of checks
/// alone an ordinary one.
pub(crate) const LIFTED_NOTE: &str = "The learner asked for more than the quick checks, so this is an ordinary sitting now. The checks not yet answered stay due.";

/// The conversation `by` loads a stage that brings new material, a plan or a
/// paper: the learner asked for more than the checks, so its sitting of
/// checks alone becomes an ordinary one. Returns whether it was one.
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

/// Recording off in a conversation: no answer there can be recorded, so its
/// checks could never settle, and its sitting is an ordinary one again.
pub(crate) fn lifts_on_control(request: &Value, result: &Value) -> bool {
    request.get("action").and_then(Value::as_str) == Some("control")
        && request.get("control").and_then(Value::as_str) == Some("recording_off")
        && result.get("durable") == Some(&Value::Bool(true))
}

/// Make the sitting of checks alone for the conversation `by` an ordinary
/// one. Returns whether it was one.
pub(crate) fn lift(cwd: &Path, by: Option<&str>) -> bool {
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
