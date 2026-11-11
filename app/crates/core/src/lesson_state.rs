//! What the direct engine tells the tutor about the lesson, in plain lines.
//!
//! The whole state goes with the first message of a conversation. After
//! that only what changed goes, so each turn's text stays small: the time,
//! newly due quick checks, the help a kind of problem now needs, and the
//! names already in use for skills with an open quick check.

use serde_json::Value;

use crate::record::{clip, DueCheck, Lesson};

/// The facts a turn's state is made from, beyond the lesson itself.
#[derive(Debug, Default)]
pub struct Facts {
    /// The local time, as "Thu 12 Nov 2026, 09:10".
    pub now: String,
    /// A line on a stop, pause or recording off the learner asked for.
    pub halt: Option<String>,
    /// Send the whole state, as at the start of a conversation.
    pub full: bool,
    /// Where the lesson stopped last time: the checkpoint's next action.
    pub stopped_at: Option<String>,
    /// The learner chose to go on without the quick checks for now.
    pub skip_checks: bool,
    /// The quick checks newly due, to list in this turn.
    pub new_checks: Vec<DueCheck>,
    /// Paper questions due for a redo.
    pub redos_due: u64,
    /// The runtime's `earlier_answers` on this goal.
    pub earlier_answers: Vec<Value>,
    /// What the record knows about the learner on this goal: one line each.
    pub known: Vec<String>,
    /// Lines on the help each kind of problem needs now.
    pub help: Vec<String>,
    /// The names of skills with an open quick check.
    pub open_labels: Vec<String>,
}

/// One quick check, as the tutor needs it. The key is the tutor's, to mark
/// the answer, and is never shown before it.
fn check_line(check: &DueCheck) -> String {
    let mut line = format!("{}. Skill \"{}\".", check.number, check.label);
    if !check.misses.is_empty() {
        line.push_str(&format!(
            " Earlier misses: {}.",
            check
                .misses
                .iter()
                .map(|miss| clip(miss, 240))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    match (&check.question, &check.key) {
        (Some(question), Some(key)) => line.push_str(&format!(
            " Stored question: {} Key, for you alone until they answer: {}",
            clip(question, 400),
            clip(key, 400)
        )),
        (Some(question), None) => {
            line.push_str(&format!(" Stored question: {}", clip(question, 400)))
        }
        _ => line.push_str(" No stored question: make a new one."),
    }
    line
}

/// The text of `<lesson-state>` for one turn, or None when nothing is worth
/// saying beyond the time.
pub fn text(lesson: &Lesson, facts: &Facts) -> String {
    let mut lines = vec![format!("Now: {}", facts.now)];
    if let Some(halt) = &facts.halt {
        lines.push(halt.clone());
    }
    if facts.full {
        lines.push(format!(
            "Goal: {}",
            lesson
                .goal
                .as_deref()
                .unwrap_or("not set yet: the learner's message names it")
        ));
        lines.push(if lesson.steps.is_empty() {
            "Steps: none set yet".to_owned()
        } else {
            format!(
                "Steps: {}",
                lesson
                    .steps
                    .iter()
                    .map(|step| format!("{} ({})", step.step, step.status))
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        });
        if let Some(stopped) = &facts.stopped_at {
            lines.push(format!("Where the lesson stopped: {stopped}"));
        }
        lines.push(match &lesson.task {
            Some(task) => format!("Open task: {}", task.text),
            None => "Open task: none".to_owned(),
        });
        if facts.earlier_answers.is_empty() {
            lines.push("Earlier answers on this goal: none".to_owned());
        } else {
            lines.push("Earlier answers on this goal, latest first:".to_owned());
            for answer in facts.earlier_answers.iter().take(8) {
                let text = |key: &str| answer[key].as_str().unwrap_or_default().to_owned();
                lines.push(format!(
                    "- {} {}, help {}: \"{}\" (skill \"{}\")",
                    text("day"),
                    text("mark"),
                    text("help"),
                    clip(&text("question"), 200),
                    text("label")
                ));
            }
        }
    }
    if !facts.known.is_empty() {
        lines.push(
            "The learner's memories here: to revise one, give its id in the record's memory field."
                .to_owned(),
        );
        lines.extend(facts.known.iter().map(|line| format!("- {line}")));
    }
    if facts.skip_checks {
        lines.push(
            "The learner chose to go on without the quick checks for now. They stay due: do not pose them."
                .to_owned(),
        );
    } else if !facts.new_checks.is_empty() {
        lines.push(
            "A new sitting has begun. Quick checks due, one at a time, before new material:"
                .to_owned(),
        );
        lines.extend(facts.new_checks.iter().map(check_line));
    } else if facts.full {
        lines.push("Quick checks due: none".to_owned());
    }
    if facts.redos_due > 0 {
        lines.push(format!(
            "Paper questions due for a redo: {}. They are done in a lesson with the Claude Code tutor, not here.",
            facts.redos_due
        ));
    }
    if !facts.help.is_empty() {
        lines.push("Help each kind of problem needs now:".to_owned());
        lines.extend(facts.help.iter().map(|line| format!("- {line}")));
    }
    if !facts.open_labels.is_empty() {
        lines.push(format!(
            "Skills with an open quick check (reuse a name for the same misunderstanding): {}",
            facts
                .open_labels
                .iter()
                .map(|label| format!("\"{label}\""))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    lines.join("\n")
}

/// The message the tutor receives: the state, then the learner's words.
pub fn message(state: &str, words: &str) -> String {
    format!("<lesson-state>\n{state}\n</lesson-state>\n\n<learner>\n{words}\n</learner>")
}

/// The help lines from a result's `help_levels`.
pub fn help_lines(levels: &Value) -> Vec<String> {
    levels["kinds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|kind| {
            let name = kind["kind"].as_str()?;
            let level = kind["level"].as_str().unwrap_or("none yet");
            Some(match (kind["give_now"].as_str(), kind["if_stuck_next"].as_str()) {
                (Some(now), _) => format!("\"{name}\" (level {level}): give now: {now}"),
                (None, Some(next)) => {
                    format!("\"{name}\" (level {level}): if the learner is stuck on a new one: {next}")
                }
                _ => format!("\"{name}\" (level {level})"),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::Step;
    use serde_json::json;

    #[test]
    fn the_first_state_is_whole_and_later_ones_say_only_what_changed() {
        let lesson = Lesson {
            goal: Some("Quadratics".into()),
            steps: vec![Step {
                step: "Read a, b, c".into(),
                status: "doing".into(),
            }],
            ..Lesson::default()
        };
        let facts = Facts {
            now: "Thu 12 Nov 2026, 09:10".into(),
            full: true,
            new_checks: vec![DueCheck {
                number: 1,
                label: "sign of b".into(),
                misses: vec!["took b as 5".into()],
                question: Some("What is b in x² − 3x?".into()),
                key: Some("−3".into()),
            }],
            earlier_answers: vec![
                json!({"day": "2026-11-11", "mark": "wrong", "help": "none", "question": "What is b?", "label": "sign of b"}),
            ],
            redos_due: 2,
            ..Facts::default()
        };
        let full = text(&lesson, &facts);
        for part in [
            "Goal: Quadratics",
            "Steps: Read a, b, c (doing)",
            "Open task: none",
            "- 2026-11-11 wrong, help none: \"What is b?\" (skill \"sign of b\")",
            "1. Skill \"sign of b\". Earlier misses: took b as 5. Stored question: What is b in x² − 3x? Key, for you alone until they answer: −3",
            "Paper questions due for a redo: 2.",
        ] {
            assert!(full.contains(part), "{part}\n{full}");
        }
        let later = text(
            &lesson,
            &Facts {
                now: "Thu 12 Nov 2026, 09:12".into(),
                help: help_lines(
                    &json!({"kinds": [{"kind": "discrete expected value", "level": "full", "if_stuck_next": "a partly worked example"}]}),
                ),
                ..Facts::default()
            },
        );
        assert!(!later.contains("Goal:"), "{later}");
        assert!(later.contains("\"discrete expected value\" (level full): if the learner is stuck on a new one: a partly worked example"), "{later}");
        assert_eq!(
            message("Now: x", "hello"),
            "<lesson-state>\nNow: x\n</lesson-state>\n\n<learner>\nhello\n</learner>"
        );
    }
}
