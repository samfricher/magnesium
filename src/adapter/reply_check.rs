use rusqlite::{Connection, OptionalExtension, params};

use crate::learner::HostContext;

const WORD_LIMIT: usize = 200;

pub(crate) fn reminder() -> String {
    format!(
        " Teaching reply: one idea, under {WORD_LIMIT} words, end with one task if the learner wants practice."
    )
}

pub(crate) const ASK_WHY_CONFUSED: &str = " Ask a confused learner why before explaining, unless they asked for an explanation or solution.";

const CONFUSION_PHRASES: &[&str] = &[
    "does not make sense",
    "doesn't make sense",
    "i do not understand",
    "i don't get it",
    "i don't get why",
    "i don't understand",
    "i dont understand",
    "it's confusing",
    "makes no sense",
    "that's confusing",
    "this is confusing",
    "you lost me",
];

const EXPLANATION_REQUESTS: &[&str] = &[
    "can you explain",
    "could you explain",
    "explain again",
    "explain it again",
    "give me the answer",
    "give me the solution",
    "just tell me",
    "please explain",
    "show me how",
    "show me the answer",
    "show me the solution",
    "solve it for me",
    "tell me the answer",
    "walk me through",
    "what is the answer",
    "what's the answer",
    "would you explain",
];

const REQUEST_OPENERS: &[&str] = &["explain", "just explain", "please explain"];

pub(crate) fn wants_the_confusion_question(prompt: &str) -> bool {
    let words = format!(
        " {} ",
        prompt
            .to_lowercase()
            .replace('’', "'")
            .split(|c: char| !c.is_alphanumeric() && c != '\'')
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    );
    let says = |phrases: &[&str]| {
        phrases
            .iter()
            .any(|phrase| words.contains(&format!(" {phrase} ")))
    };
    let opens_with_a_request = prompt
        .to_lowercase()
        .split(['.', ',', ';', ':', '!', '?', '\n'])
        .any(|clause| {
            REQUEST_OPENERS
                .iter()
                .any(|opener| starts_with_phrase(clause.trim(), opener))
        });
    (says(CONFUSION_PHRASES) || calls_themselves_confused(&words))
        && !says(EXPLANATION_REQUESTS)
        && !opens_with_a_request
}

fn calls_themselves_confused(words: &str) -> bool {
    let words: Vec<&str> = words.split_whitespace().collect();
    (0..words.len()).any(|start| {
        let subject = match words[start..] {
            ["i'm" | "im", ..] => 1,
            ["i", "am", ..] => 2,
            _ => return false,
        };
        let rest = &words[start + subject..words.len().min(start + subject + 3)];
        rest.iter()
            .take_while(|word| !matches!(**word, "not" | "never" | "no"))
            .any(|word| matches!(*word, "confused" | "lost"))
    })
}

/// Judges the latest recorded reply only when it answers the latest recorded
/// prompt before this one, so a turn the hooks missed does not pass off an
/// older reply as the previous one.
pub(crate) fn feedback(
    conn: &Connection,
    host: &HostContext,
    current_event: &str,
) -> rusqlite::Result<Option<String>> {
    let latest = |speaker: &str,
                  kind: &str,
                  column: &str|
     -> rusqlite::Result<Option<(String, String)>> {
        conn.query_row(
            &format!("SELECT external_event_key,{column} FROM learning_events WHERE host_kind=?1 AND host_thread_id=?2 AND origin='host_observed' AND speaker=?3 AND kind=?4 AND event_id<>?5 ORDER BY event_no DESC LIMIT 1"),
            params![host.host_kind, host.host_thread_id, speaker, kind, current_event],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
    };
    let Some((reply_turn, reply)) = latest("tutor", "delivery", "text")? else {
        return Ok(None);
    };
    let prompt = latest("learner", "message", "''")?.map(|(key, _)| key);
    let answers_previous_prompt = prompt == Some(super::native_capture::event_key(&reply_turn));
    Ok(answers_previous_prompt.then(|| judge(&reply)).flatten())
}

fn judge(reply: &str) -> Option<String> {
    let words = reply
        .split_whitespace()
        .filter(|token| token.chars().any(char::is_alphanumeric))
        .count();
    let faults: Vec<String> = [
        (words >= WORD_LIMIT).then(|| format!("{words} words")),
        (!sets_a_task(reply)).then(|| "no task".to_owned()),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!faults.is_empty()).then(|| format!(" Previous reply: {}.", faults.join(", ")))
}

const QUESTION_MARKS: [char; 3] = ['?', '？', '؟'];

const FENCES: [(&str, &str); 4] = [("```", "```"), ("~~~", "~~~"), ("$$", "$$"), ("\\[", "\\]")];

/// Questions about the tutor's own next move, or whether the reply landed.
const CHECK_INS: &[&str] = &[
    "any questions",
    "do you want me to",
    "does that make sense",
    "does this make sense",
    "shall i",
    "should i",
    "want me to",
    "would you like me to",
];

const TASK_LABELS: &[&str] = &[
    "challenge",
    "exercise",
    "hint",
    "practice",
    "problem",
    "question",
    "task",
    "try this",
    "your task",
    "your turn",
];

fn sets_a_task(reply: &str) -> bool {
    let ending = ending(reply).join("\n");
    ending.split(['.', '!', '?', '\n']).any(is_task_label) || asks_the_learner(&ending)
}

/// The last paragraph of prose and any code, maths or list after it, since a
/// question often comes just before the code, equation or options it is about.
fn ending(reply: &str) -> Vec<&str> {
    let mut fence = None;
    let lines: Vec<(&str, bool)> = reply
        .lines()
        .map(|line| {
            let line = line.trim();
            let material = if let Some(close) = fence {
                if line.starts_with(close) || line.ends_with(close) {
                    fence = None;
                }
                true
            } else if let Some((open, close)) =
                FENCES.iter().find(|(open, _)| line.starts_with(open))
            {
                if !line[open.len()..].ends_with(close) {
                    fence = Some(*close);
                }
                true
            } else {
                is_list_item(line)
            };
            (line, material)
        })
        .collect();
    let is_prose = |(line, material): &(&str, bool)| !line.is_empty() && !material;
    let end = lines.iter().rposition(is_prose).map_or(0, |last| last + 1);
    let start = lines[..end]
        .iter()
        .rposition(|line| !is_prose(line))
        .map_or(0, |gap| gap + 1);
    lines[start..].iter().map(|(line, _)| *line).collect()
}

fn is_list_item(line: &str) -> bool {
    let marker = line.split(' ').next().unwrap_or_default();
    let label = marker.strip_suffix(['.', ')']).unwrap_or_default();
    ["-", "*", "+"].contains(&marker)
        || (label.len() == 1 && label.chars().all(char::is_alphanumeric))
        || ((2..=3).contains(&label.len()) && label.bytes().all(|byte| byte.is_ascii_digit()))
}

fn is_task_label(sentence: &str) -> bool {
    let sentence = sentence
        .trim_start_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    TASK_LABELS.iter().any(|label| {
        sentence.strip_prefix(label).is_some_and(|rest| {
            let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit() || " *_".contains(c));
            rest.is_empty() || rest.starts_with([':', '–', '—'])
        })
    })
}

fn asks_the_learner(text: &str) -> bool {
    text.split_inclusive(QUESTION_MARKS)
        .filter(|part| part.ends_with(QUESTION_MARKS))
        .any(|question| {
            let clause = question
                .rsplit(['.', '!', ',', ';', ':', '\n'])
                .next()
                .unwrap_or_default()
                .trim_start_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();
            !CHECK_INS
                .iter()
                .any(|check_in| starts_with_phrase(&clause, check_in))
        })
}

fn starts_with_phrase(text: &str, phrase: &str) -> bool {
    text.strip_prefix(phrase)
        .is_some_and(|rest| !rest.starts_with(char::is_alphanumeric))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_or_a_task_label_at_the_end_is_a_task() {
        for reply in [
            "The root is 3.\n\nWhat is the other root?",
            "So b is fixed.\n\n**Your turn:** solve 2x + 3 = 11.",
            "b is fixed. Your turn: name the coefficient of x.",
            "Your turn!",
            "> **Task.** Differentiate $x^3$.",
            "Good.\n\nQuestion 2: factor x^2 - 5x + 6.",
            "Exercise 3.2: rewrite 12/18 in lowest terms.",
            "Your turn — solve 2x = 6.",
            "What is the derivative of x^2 sin x?\n\nHint: use the product rule.",
            "Which statement is true?\n\nA) b varies\nB) b is fixed\nC) b is x",
            "Which statement is true?\n\n- b varies\n\n- b is fixed",
            "What does this print?\n\n```python\nprint(len('abc'))\n```",
            "Which root is larger?\n\n$$\nx^2 - 5x + 6 = 0\n$$",
            "Try these:\n\n1. What is 3 + 4?\n2. What is 5 + 6?",
            "Would you like to try one: 3x - 7 = 11?",
            "Should I solve it, or do you want to try first?",
            "那么，x 的系数是多少？",
            "ما هو معامل x؟",
        ] {
            assert!(sets_a_task(reply), "{reply}");
        }
    }

    #[test]
    fn an_offer_a_check_in_or_a_statement_at_the_end_is_not_a_task() {
        for reply in [
            "b scales x, so it stays fixed. Would you like me to explain it differently?",
            "b scales x, so it stays fixed. Shall I write the next step?",
            "b scales x, so it stays fixed. Should I solve it?",
            "Spot on! b is the coefficient of x.",
            "Sort of… b is fixed, but x varies.",
            "**Want me to show another example?**",
            "Okay, do you want me to go through it again?",
            "The coefficient b scales x. Does that make sense?",
            "That covers it. Any questions?",
            "That is the key idea.",
            "Problem solved: the base case stops the recursion.",
            "Tip: use small inputs when you trace.",
            "Why does this work? Because the identity cancels the root.\n\nThat is the key idea.",
            "Key points:\n\n- b is fixed\n- x varies",
            "```python\nprint(1)\n```",
            "",
        ] {
            assert!(!sets_a_task(reply), "{reply}");
        }
    }

    #[test]
    fn known_misses_stay_as_documented() {
        for (reply, task) in [
            ("Differentiate x sin x.", false),
            ("Solve for x:\n\n$$x^2 - 5x + 6 = 0$$", false),
            ("It's your turn to factor x^2 - 1.", false),
            ("What is b?\n\nGood luck!", false),
            ("In C that is `a ? b : c`.", true),
            ("Would you like another example?", true),
        ] {
            assert_eq!(sets_a_task(reply), task, "{reply}");
        }
    }

    #[test]
    fn confusion_question_follows_what_the_learner_says() {
        for prompt in [
            "I'm confused about b",
            "I don’t understand why it halves",
            "this makes no sense",
            "you lost me",
            "Honestly, I am still confused.",
            "I don't understand the solution you showed",
            "I don't understand your explanation",
        ] {
            assert!(wants_the_confusion_question(prompt), "{prompt}");
        }
        for prompt in [
            "is it 950?",
            "That wasn't confusing at all",
            "How do I read a confusion matrix?",
            "I'm not confused, just slow",
            "I'm stuck on this one",
            "I'm confused, can you explain it again?",
            "I don't understand. Just tell me the answer.",
            "I'm lost, walk me through the solution",
            "I'm confused. Explain the chain rule again.",
            "I don't get it, please explain",
        ] {
            assert!(!wants_the_confusion_question(prompt), "{prompt}");
        }
    }

    #[test]
    fn feedback_names_each_fault_and_nothing_when_the_rule_held() {
        let words = |n: usize| "word ".repeat(n);
        assert_eq!(judge(&format!("{}What next?", words(10))), None);
        assert_eq!(judge(&format!("{}Why?", words(198))), None);
        assert_eq!(
            judge(&format!("{}Why?", words(199))).as_deref(),
            Some(" Previous reply: 200 words.")
        );
        assert_eq!(
            judge(&format!("{}Why?", "word - | ".repeat(150))),
            None,
            "a token without a letter or digit is not a word"
        );
        assert_eq!(
            judge("That is the whole idea.").as_deref(),
            Some(" Previous reply: no task.")
        );
        assert_eq!(
            judge(&"This is the idea and it is what you need. ".repeat(85)).as_deref(),
            Some(" Previous reply: 850 words, no task.")
        );
    }
}
