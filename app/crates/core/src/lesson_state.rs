//! What the direct engine tells the tutor about the lesson, in plain lines.
//!
//! The whole state goes with the first message of a conversation. After
//! that only what changed goes, so each turn's text stays small: the time,
//! newly due quick checks and paper redos, the help a kind of problem now
//! needs, and the names already in use for skills with an open quick check.

use serde_json::Value;

use crate::record::{clip, DueCheck, DueRedo, Lesson, WaitingRepair};

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
    /// The learner chose to go on without what is due for now.
    pub skip_checks: bool,
    /// The quick checks newly due, to list in this turn.
    pub new_checks: Vec<DueCheck>,
    /// The paper redos newly due, to list in this turn.
    pub new_redos: Vec<DueRedo>,
    /// How long the learner took to answer the open task, when it is a
    /// timed paper redo and the window saw it posed.
    pub took: Option<std::time::Duration>,
    /// The open task is a timed paper redo the window did not see posed,
    /// as after a resume: no clock ran on it.
    pub no_clock: bool,
    /// What the record made of the last answer to a paper redo or a due
    /// quick check.
    pub outcome: Option<String>,
    /// Why no task is open, after a reply the learner stopped or one whose
    /// record could not be read.
    pub no_task: Option<String>,
    /// The repairs newly waiting, to list in this turn.
    pub new_repairs: Vec<WaitingRepair>,
    /// What the record made of the last repair.
    pub repair_outcome: Option<String>,
    /// The runtime's `earlier_answers` on this goal.
    pub earlier_answers: Vec<Value>,
    /// What the record knows about the learner on this goal: one line each.
    pub known: Vec<String>,
    /// Lines on the help each kind of problem needs now.
    pub help: Vec<String>,
    /// The names of skills with an open quick check.
    pub open_labels: Vec<String>,
    /// The course graphs in the study folder, as paths from it. They do not
    /// change during a conversation, so only the whole state names them.
    pub graphs: Vec<String>,
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

/// One paper redo, as the tutor needs it: the rows are the program's to
/// record, the rest is for making and marking the question.
fn redo_line(redo: &DueRedo) -> String {
    let mut line = format!("{}. Tag \"{}\"", redo.number, redo.tag);
    if !redo.from.is_empty() {
        line.push_str(&format!(", from {}", redo.from.join(" and ")));
    }
    line.push('.');
    if !redo.misses.is_empty() {
        line.push_str(&format!(
            " What went wrong: {}.",
            redo.misses
                .iter()
                .map(|miss| clip(miss, 240))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    if redo.timed {
        line.push_str(" Timed: it was lost for slowness, so it passes only in time.");
    }
    match (&redo.question, &redo.key) {
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

/// One repair, as the tutor needs it: the rows are the program's to record.
fn repair_line(repair: &WaitingRepair) -> String {
    let mut line = format!("Repair {}. Tag \"{}\"", repair.number, repair.tag);
    if !repair.from.is_empty() {
        line.push_str(&format!(", from {}", repair.from.join(" and ")));
    }
    line.push('.');
    if !repair.misses.is_empty() {
        line.push_str(&format!(
            " What went wrong: {}.",
            repair
                .misses
                .iter()
                .map(|miss| clip(miss, 240))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    line
}

/// A time as the tutor reads it: "4 min 10 s".
fn minutes(took: std::time::Duration) -> String {
    let seconds = took.as_secs();
    match seconds / 60 {
        0 => format!("{seconds} s"),
        minutes => format!("{minutes} min {} s", seconds % 60),
    }
}

/// The text of `<lesson-state>` for one turn.
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
        lines.extend(
            facts
                .graphs
                .iter()
                .map(|path| format!("Course graph: {path}")),
        );
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
            "The learner chose to go on without the quick checks and paper redos for now. They stay due: do not pose them."
                .to_owned(),
        );
    } else {
        if !facts.new_checks.is_empty() {
            lines.push(
                "A new sitting has begun. Quick checks due, one at a time, before new material:"
                    .to_owned(),
            );
            lines.extend(facts.new_checks.iter().map(check_line));
        } else if facts.full {
            lines.push("Quick checks due: none".to_owned());
        }
        if !facts.new_redos.is_empty() {
            lines.push(
                if facts.new_checks.is_empty() {
                    "A new sitting has begun. Paper redos due, one at a time, before new material:"
                } else {
                    "Paper redos due, one at a time, after the quick checks:"
                }
                .to_owned(),
            );
            lines.extend(facts.new_redos.iter().map(redo_line));
        }
    }
    let (here, before): (Vec<&WaitingRepair>, Vec<&WaitingRepair>) = facts
        .new_repairs
        .iter()
        .partition(|repair| repair.failed_here);
    if !here.is_empty() {
        lines.push(
            "A paper redo failed in this lesson, and its question waits for a repair. Repair it now, unless the learner declines:"
                .to_owned(),
        );
        lines.extend(here.into_iter().map(repair_line));
    }
    if !before.is_empty() && !facts.skip_checks {
        lines.push(
            "Paper questions that already waited for a repair when this sitting began. After the quick checks and paper redos, name them in one line and take the first, unless the learner wants their goal first:"
                .to_owned(),
        );
        lines.extend(before.into_iter().map(repair_line));
    }
    if let Some(outcome) = &facts.repair_outcome {
        lines.push(outcome.clone());
    }
    if let Some(no_task) = &facts.no_task {
        lines.push(no_task.clone());
    }
    if let Some(outcome) = &facts.outcome {
        lines.push(outcome.clone());
    }
    if let Some(took) = facts.took {
        lines.push(format!(
            "The open task is a timed paper redo: this message came {} after the window showed it.",
            minutes(took)
        ));
    } else if facts.no_clock {
        lines.push(
            "The open task is a timed paper redo, and the window did not see it posed, so it has no time for this answer: a right answer cannot count this time, and the redo stays due. Say so."
                .to_owned(),
        );
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
            new_redos: vec![
                DueRedo {
                    number: 1,
                    rows: vec!["midterm-1#1".into(), "midterm-1#4".into()],
                    tag: "km".into(),
                    timed: true,
                    from: vec!["Diagnostic Q3".into(), "Mock A Q2".into()],
                    misses: vec!["slope read as intercept".into()],
                    question: None,
                    key: None,
                },
                DueRedo {
                    number: 2,
                    rows: vec!["midterm-1#2".into()],
                    tag: "rate law".into(),
                    timed: false,
                    from: vec![],
                    misses: vec![],
                    question: Some("Write the rate law.".into()),
                    key: Some("v = k[A]".into()),
                },
            ],
            ..Facts::default()
        };
        let full = text(&lesson, &facts);
        for part in [
            "Goal: Quadratics",
            "Steps: Read a, b, c (doing)",
            "Open task: none",
            "- 2026-11-11 wrong, help none: \"What is b?\" (skill \"sign of b\")",
            "1. Skill \"sign of b\". Earlier misses: took b as 5. Stored question: What is b in x² − 3x? Key, for you alone until they answer: −3",
            "Paper redos due, one at a time, after the quick checks:",
            "1. Tag \"km\", from Diagnostic Q3 and Mock A Q2. What went wrong: slope read as intercept. Timed: it was lost for slowness, so it passes only in time. No stored question: make a new one.",
            "2. Tag \"rate law\". Stored question: Write the rate law. Key, for you alone until they answer: v = k[A]",
        ] {
            assert!(full.contains(part), "{part}\n{full}");
        }
        // The rows are the program's to record: the tutor is not shown them.
        assert!(!full.contains("midterm-1#"), "{full}");
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
        assert!(!later.contains("redo"), "{later}");
        // A sitting with redos alone, the learner's skip, and a timed answer.
        let redos_only = text(
            &lesson,
            &Facts {
                new_redos: facts.new_redos.clone(),
                took: Some(std::time::Duration::from_secs(250)),
                ..Facts::default()
            },
        );
        assert!(
            redos_only.contains(
                "A new sitting has begun. Paper redos due, one at a time, before new material:"
            ),
            "{redos_only}"
        );
        assert!(
            redos_only.contains("this message came 4 min 10 s after the window showed it"),
            "{redos_only}"
        );
        // A timed redo the window did not see posed, and the record's
        // verdict on the last redo.
        let unclocked = text(
            &lesson,
            &Facts {
                no_clock: true,
                outcome: Some("The record holds the last paper redo as a pass.".into()),
                ..Facts::default()
            },
        );
        assert!(
            unclocked
                .contains("it has no time for this answer: a right answer cannot count this time"),
            "{unclocked}"
        );
        assert!(
            unclocked.contains("The record holds the last paper redo as a pass."),
            "{unclocked}"
        );
        let skipped = text(
            &lesson,
            &Facts {
                skip_checks: true,
                new_redos: facts.new_redos.clone(),
                ..Facts::default()
            },
        );
        assert!(
            skipped.contains("They stay due: do not pose them."),
            "{skipped}"
        );
        assert!(!skipped.contains("Tag \"km\""), "{skipped}");
        assert!(later.contains("\"discrete expected value\" (level full): if the learner is stuck on a new one: a partly worked example"), "{later}");
        assert_eq!(
            message("Now: x", "hello"),
            "<lesson-state>\nNow: x\n</lesson-state>\n\n<learner>\nhello\n</learner>"
        );
    }

    #[test]
    fn the_whole_state_names_a_course_graph_only_when_there_is_one() {
        let lesson = Lesson::default();
        let none = text(
            &lesson,
            &Facts {
                full: true,
                ..Facts::default()
            },
        );
        assert!(!none.contains("Course graph"), "{none}");
        let graphs = vec!["Study/midterm-1/graph.yaml".to_owned()];
        let one = text(
            &lesson,
            &Facts {
                full: true,
                graphs: graphs.clone(),
                ..Facts::default()
            },
        );
        assert!(
            one.lines()
                .any(|line| line == "Course graph: Study/midterm-1/graph.yaml"),
            "{one}"
        );
        // A later state says only what changed, and a graph does not change.
        let later = text(
            &lesson,
            &Facts {
                graphs,
                ..Facts::default()
            },
        );
        assert!(!later.contains("Course graph"), "{later}");
    }
}
