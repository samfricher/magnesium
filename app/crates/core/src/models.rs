//! The models the agent offers, as it reports them, and the one the learner
//! picked to start lessons on.
//!
//! The list is the agent's own: Claude Code sends it in its reply to the
//! `initialize` request that opens every conversation, and the window keeps
//! the last one it was sent. Magnesium keeps no list of models of its own,
//! and adds nothing to what the agent says about each one.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What every lesson in the window ran on before the learner could pick:
/// Claude Code's default on 2026-11-11, named so that a later change of that
/// default could not change the tutor unseen. A window that kept state
/// before the pick existed starts on it, and so does a session it began.
pub const EARLIER: &str = "claude-fable-5-2";
/// Its name, for the start line before the agent has sent a list.
pub const EARLIER_NAME: &str = "Fable 5.2";

/// One model the agent offers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelOption {
    /// What the agent takes as the model: an alias such as `opus`, the word
    /// `default`, or a full id such as `claude-opus-4-6`.
    pub value: String,
    /// The full id `value` stands for, when the agent says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    /// Its name, as the agent shows it.
    pub name: String,
    /// The agent's one line about it.
    #[serde(default)]
    pub description: String,
    /// The effort levels it takes, as the agent names them, in its order.
    /// Empty when it takes none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub efforts: Vec<String>,
    /// Listed with the current models. An older one is a full id that no
    /// alias in the same list stands for.
    #[serde(default)]
    pub current: bool,
}

impl ModelOption {
    /// The id a pick of this row keeps: what it resolves to, so that a
    /// later change of what an alias stands for does not change the pick.
    pub fn id(&self) -> &str {
        self.resolved.as_deref().unwrap_or(&self.value)
    }

    fn is_alias(&self) -> bool {
        self.resolved.as_deref().is_some_and(|id| id != self.value)
    }
}

/// The model and effort the learner picked for new sessions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pick {
    /// The model's full id, as the agent takes it.
    pub model: String,
    /// Its name as the agent's list showed it, for the start line before the
    /// list has arrived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The effort level, or none for the agent's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

impl Pick {
    /// What every lesson ran on before the pick existed.
    pub fn earlier() -> Self {
        Self {
            model: EARLIER.into(),
            name: Some(EARLIER_NAME.into()),
            effort: None,
        }
    }
}

/// The models in the agent's reply to `initialize`, current ones marked.
/// None when the reply has no list of models.
pub fn parse(reply: &Value) -> Option<Vec<ModelOption>> {
    let rows = reply["models"].as_array()?;
    let mut models: Vec<ModelOption> = rows
        .iter()
        .filter(|row| row["disabled"].as_bool() != Some(true))
        .filter_map(|row| {
            let value = row["value"].as_str().filter(|v| usable_model(v))?;
            let resolved = row["resolvedModel"]
                .as_str()
                .filter(|id| usable_model(id))
                .map(str::to_owned);
            let name = row["displayName"]
                .as_str()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or(value);
            let efforts = if row["supportsEffort"].as_bool() == Some(true) {
                row["supportedEffortLevels"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter(|level| usable_effort(level))
                    .map(str::to_owned)
                    .collect()
            } else {
                Vec::new()
            };
            Some(ModelOption {
                value: value.to_owned(),
                resolved,
                name: clip(name, 80),
                description: clip(row["description"].as_str().unwrap_or_default().trim(), 300),
                efforts,
                current: false,
            })
        })
        .collect();
    // Current models are the ones an alias stands for, and the aliases come
    // first in the agent's own order. A list with no alias at all, such as
    // one from a provider that names every model in full, is not split.
    let split = models.iter().any(ModelOption::is_alias);
    let covered: Vec<String> = models
        .iter()
        .filter(|m| m.is_alias())
        .map(|m| m.id().to_owned())
        .collect();
    for model in &mut models {
        model.current = !split
            || model.is_alias()
            || model.resolved.is_none()
            || covered.iter().any(|id| id == model.id());
    }
    Some(models)
}

/// The row a pick shows as: a current model before the default row, the
/// default row before an older one. None when the list does not offer it.
pub fn find<'a>(models: &'a [ModelOption], model: &str) -> Option<&'a ModelOption> {
    let matches = |m: &&ModelOption| m.id() == model || m.value == model;
    models
        .iter()
        .filter(matches)
        .find(|m| m.current && m.value != "default")
        .or_else(|| models.iter().filter(matches).find(|m| m.value == "default"))
        .or_else(|| models.iter().find(matches))
}

/// The agent's default, as the list resolves it.
pub fn default_of(models: &[ModelOption]) -> Option<&ModelOption> {
    models.iter().find(|m| m.value == "default")
}

/// A model name the window may pass on a command line: no space, no leading
/// dash, so it can never be read as a flag.
pub fn usable_model(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && !value.starts_with('-')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._:-[]/@".contains(c))
}

/// An effort level the window may pass: one lowercase word.
pub fn usable_effort(level: &str) -> bool {
    (1..=16).contains(&level.len()) && level.chars().all(|c| c.is_ascii_lowercase())
}

fn clip(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The shape of the list one account was sent by Claude Code 2.1.322:
    /// fourteen rows, five of them aliases. Its names and lines are
    /// stand-ins written for the test.
    pub(crate) fn fourteen() -> Value {
        let row = |value: &str, resolved: &str, name: &str, efforts: Option<&[&str]>| {
            let mut row = json!({"value": value, "resolvedModel": resolved, "displayName": name, "description": format!("{name}, as the agent says it")});
            if let Some(levels) = efforts {
                row["supportsEffort"] = json!(true);
                row["supportedEffortLevels"] = json!(levels);
            }
            row
        };
        let all: &[&str] = &["low", "medium", "high", "xhigh", "max"];
        let no_xhigh: &[&str] = &["low", "medium", "high", "max"];
        json!({"commands": [], "models": [
            row("default", "claude-opus-5-5", "Default (recommended)", Some(all)),
            row("opus", "claude-opus-5-5", "Opus", Some(all)),
            row("fable", "claude-fable-5-2", "Fable", Some(all)),
            row("sonnet", "claude-sonnet-5-5", "Sonnet", Some(all)),
            row("haiku", "claude-haiku-5-5", "Haiku", Some(&["low", "medium", "high"][..])),
            row("claude-fable-5-1", "claude-fable-5-1", "Fable 5.1", Some(all)),
            row("claude-opus-5", "claude-opus-5", "Opus 5", Some(all)),
            row("claude-sonnet-5", "claude-sonnet-5", "Sonnet 5", Some(all)),
            row("claude-opus-4-8", "claude-opus-4-8", "Opus 4.8", Some(all)),
            row("claude-opus-4-7", "claude-opus-4-7", "Opus 4.7", Some(all)),
            row("claude-opus-4-6", "claude-opus-4-6", "Opus 4.6", Some(no_xhigh)),
            row("claude-sonnet-4-6", "claude-sonnet-4-6", "Sonnet 4.6", Some(no_xhigh)),
            row("claude-sonnet-4-5", "claude-sonnet-4-5", "Sonnet 4.5", Some(no_xhigh)),
            row("claude-haiku-4-5", "claude-haiku-4-5", "Haiku 4.5", None),
        ]})
    }

    #[test]
    fn the_aliases_are_current_and_the_full_ids_are_older_in_the_agents_order() {
        let models = parse(&fourteen()).unwrap();
        assert_eq!(models.len(), 14);
        let current: Vec<&str> = models
            .iter()
            .filter(|m| m.current)
            .map(|m| m.value.as_str())
            .collect();
        assert_eq!(current, ["default", "opus", "fable", "sonnet", "haiku"]);
        assert_eq!(models.iter().filter(|m| !m.current).count(), 9);
        assert_eq!(default_of(&models).unwrap().id(), "claude-opus-5-5");
    }

    #[test]
    fn each_model_keeps_the_effort_levels_it_reports_and_no_others() {
        let models = parse(&fourteen()).unwrap();
        let levels = |value: &str| {
            models
                .iter()
                .find(|m| m.value == value)
                .unwrap()
                .efforts
                .clone()
        };
        assert_eq!(levels("claude-opus-4-6"), ["low", "medium", "high", "max"]);
        assert_eq!(levels("haiku"), ["low", "medium", "high"]);
        assert!(levels("claude-haiku-4-5").is_empty());
    }

    #[test]
    fn a_pick_shows_as_the_current_row_that_stands_for_it() {
        let models = parse(&fourteen()).unwrap();
        // The default and `opus` are one model here: the pick shows as Opus.
        assert_eq!(find(&models, "claude-opus-5-5").unwrap().value, "opus");
        assert_eq!(find(&models, EARLIER).unwrap().value, "fable");
        assert_eq!(
            find(&models, "claude-opus-4-6").unwrap().value,
            "claude-opus-4-6"
        );
        assert!(find(&models, "claude-fable-4").is_none());
    }

    #[test]
    fn a_list_with_no_alias_is_not_split() {
        let models = parse(&json!({"models": [
            {"value": "qwen3:32b", "displayName": "Qwen3 32B", "description": ""},
            {"value": "llama4:scout", "resolvedModel": "llama4:scout", "displayName": "Llama 4 Scout"},
        ]}))
        .unwrap();
        assert!(models.iter().all(|m| m.current));
    }

    #[test]
    fn rows_that_could_be_read_as_flags_or_cannot_be_chosen_are_left_out() {
        let models = parse(&json!({"models": [
            {"value": "--dangerously-skip-permissions", "displayName": "x"},
            {"value": "two words", "displayName": "x"},
            {"value": "claude-opus-5-5", "displayName": "Opus", "disabled": true},
            {"value": "sonnet", "resolvedModel": "claude-sonnet-5-5", "displayName": "", "supportsEffort": true, "supportedEffortLevels": ["low", "-x", "HIGH"]},
        ]}))
        .unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "sonnet");
        assert_eq!(models[0].efforts, ["low"]);
        assert!(parse(&json!({"commands": []})).is_none());
    }
}
