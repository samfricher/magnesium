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
//! lesson in the window and one in a terminal pose the same checks, are
//! refused the same stages, and end at the same answer.

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

/// Stages that bring new material, a plan or a paper. While the checks alone
/// are under way they wait; marking an answer and closing do not.
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
/// active, and only a recorded answer settles a check.
pub(crate) fn from_tool(
    cwd: &Path,
    by: Option<&str>,
    asker: impl FnOnce(&Workspace) -> Asker,
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
    // The sitting's opener goes out here when it has not yet, so the usual
    // one, with its paper redos, is not given later in the sitting.
    let mut report = crate::practice::GapLogReport::default();
    let opener = sittings
        .take_opener(&workspace, now, &mut report)
        .ok()
        .flatten()
        .or_else(|| sittings.checks_only_opener(&workspace).ok().flatten());
    let Some(opener) = opener else {
        return json!({"error": {"code": "sittings_unavailable", "message": "the sitting's checks could not be read"}});
    };
    // In the live runs of 2026-11-16 a tutor told only to follow the opener
    // posed the checks with no lesson active, so no answer was recorded.
    let note = if asker.lesson_active {
        "Pose the checks and end as sitting_opener says.".to_owned()
    } else {
        "No lesson is active in this conversation, and an answer is recorded only in one: before your reply, start or resume the Magnesium session as your instructions say (a new one with goal \"Quick checks\", with the learner's message as native_capture). Then pose the checks and end as sitting_opener says.".to_owned()
    };
    let mut out = answer(checks, note);
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

/// While the checks alone are under way, a stage that brings new material, a
/// plan or a paper is refused, with the checks left to pose.
pub(crate) fn refusal(cwd: &Path, operation: &str, request: &Value) -> Option<Value> {
    let stage = request.get("stage")?.as_str()?;
    if operation != "context"
        || request
            .get("action")
            .is_some_and(|action| action != "instructions")
        || !NEW_WORK.contains(&stage)
    {
        return None;
    }
    let workspace = Workspace::discover(cwd).ok()??;
    crate::adapter::check_workspace_binding(&workspace).ok()?;
    let sittings = Sittings::open_existing(&workspace).ok()??;
    let left = sittings.checks_only_left().ok()??;
    let mut refusal = json!({
        "status": "unavailable",
        "recommended_stage": "review_and_transfer",
        "error": {
            "code": "quick_checks_alone",
            "message": format!(
                "The learner asked for the quick checks alone, and {left} {} left. This stage brings new material, a plan or a paper, so it waits until the sitting ends. Pose the next check, or record the learner's decline. If they asked for something else instead, record a decline for each check not yet answered; the sitting then ends, and this stage opens.",
                if left == 1 { "is" } else { "are" }
            ),
        },
    });
    if let Ok(Some(opener)) = sittings.checks_only_opener(&workspace) {
        refusal["sitting_opener"] = opener.text.into();
        if crate::sittings::checks_wanted() {
            refusal["sitting_checks"] = opener.checks;
            refusal["sitting_redos"] = opener.redos;
        }
    }
    Some(refusal)
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
