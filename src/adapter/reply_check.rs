use rusqlite::{Connection, params};

use crate::learner::HostContext;

const WORD_LIMIT: usize = 200;

pub(crate) fn reminder() -> String {
    format!(" Teaching reply: one idea, under {WORD_LIMIT} words, end with one task.")
}

const TASK_STARTS: &[&str] = &[
    "answer",
    "apply",
    "attempt",
    "calculate",
    "challenge",
    "check",
    "choose",
    "classify",
    "compare",
    "complete",
    "compute",
    "convert",
    "decide",
    "define",
    "derive",
    "describe",
    "determine",
    "differentiate",
    "draw",
    "estimate",
    "evaluate",
    "exercise",
    "expand",
    "explain",
    "factor",
    "factorise",
    "factorize",
    "fill",
    "find",
    "give",
    "graph",
    "guess",
    "have a go",
    "identify",
    "implement",
    "integrate",
    "justify",
    "label",
    "list",
    "match",
    "name",
    "over to you",
    "pick",
    "plot",
    "practice",
    "practise",
    "predict",
    "prove",
    "question",
    "rank",
    "read",
    "recall",
    "rewrite",
    "show",
    "simplify",
    "sketch",
    "solve",
    "sort",
    "spot",
    "state",
    "summarise",
    "summarize",
    "task",
    "tell",
    "test",
    "trace",
    "translate",
    "try",
    "use",
    "work",
    "write",
    "your task",
    "your turn",
];

const OPENERS: &[&str] = &[
    "and", "finally", "first", "let's", "let’s", "next", "now", "ok", "okay", "please", "so",
    "then",
];

const CHECK_INS: &[&str] = &[
    "any questions",
    "clear?",
    "do you want",
    "does it make sense",
    "does that make sense",
    "does this make sense",
    "got it",
    "is that clear",
    "is this clear",
    "make sense",
    "makes sense",
    "ok?",
    "okay?",
    "ready for",
    "ready to",
    "ready?",
    "right?",
    "shall i",
    "shall we",
    "should i",
    "should we",
    "sound good",
    "sounds good",
    "want me",
    "would you like",
];

pub(crate) fn feedback(
    conn: &Connection,
    host: &HostContext,
    current_event: &str,
) -> rusqlite::Result<Option<String>> {
    let mut statement = conn.prepare(
        "SELECT speaker,kind,external_event_key,text FROM learning_events WHERE host_kind=?1 AND host_thread_id=?2 AND origin='host_observed' AND event_no<(SELECT event_no FROM learning_events WHERE event_id=?3) ORDER BY event_no DESC LIMIT 2",
    )?;
    let latest = statement
        .query_map(
            params![host.host_kind, host.host_thread_id, current_event],
            |row| {
                Ok(Recorded {
                    speaker: row.get(0)?,
                    kind: row.get(1)?,
                    key: row.get(2)?,
                    text: row.get(3)?,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(match latest.as_slice() {
        [reply, prompt] if reply.answers(prompt) => ReplyCheck::of(&reply.text).feedback(),
        _ => None,
    })
}

struct Recorded {
    speaker: String,
    kind: String,
    key: String,
    text: String,
}

impl Recorded {
    fn answers(&self, prompt: &Recorded) -> bool {
        self.speaker == "tutor"
            && self.kind == "delivery"
            && prompt.speaker == "learner"
            && prompt.kind == "message"
            && prompt.key == super::native_capture::event_key(&self.key)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReplyCheck {
    words: usize,
    ends_with_task: bool,
}

impl ReplyCheck {
    fn of(reply: &str) -> Self {
        Self {
            words: reply
                .split_whitespace()
                .filter(|token| token.chars().any(char::is_alphanumeric))
                .count(),
            ends_with_task: last_block(reply).into_iter().any(sets_work),
        }
    }

    fn feedback(self) -> Option<String> {
        let faults: Vec<String> = [
            (self.words >= WORD_LIMIT).then(|| format!("{} words", self.words)),
            (!self.ends_with_task).then(|| "no task".to_owned()),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!faults.is_empty()).then(|| format!(" Previous reply: {}.", faults.join(", ")))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Line {
    Blank,
    Text,
    ListItem,
    Material,
}

fn last_block(reply: &str) -> Vec<&str> {
    let lines = classify(reply);
    let mut end = lines.len();
    while end > 0 && matches!(lines[end - 1].0, Line::Blank | Line::Material) {
        end -= 1;
    }
    let start = with_the_question_of_trailing_options(&lines, paragraph_start(&lines, end), end);
    lines[start..end].iter().map(|(_, text)| *text).collect()
}

fn with_the_question_of_trailing_options(
    lines: &[(Line, &str)],
    mut start: usize,
    end: usize,
) -> usize {
    while start < end
        && lines[start..end]
            .iter()
            .all(|(kind, _)| matches!(kind, Line::ListItem | Line::Blank))
    {
        let mut before = start;
        while before > 0 && lines[before - 1].0 == Line::Blank {
            before -= 1;
        }
        let earlier = paragraph_start(lines, before);
        if earlier == before {
            break;
        }
        start = earlier;
    }
    start
}

fn paragraph_start(lines: &[(Line, &str)], end: usize) -> usize {
    let mut start = end;
    while start > 0 && matches!(lines[start - 1].0, Line::Text | Line::ListItem) {
        start -= 1;
    }
    start
}

fn classify(reply: &str) -> Vec<(Line, &str)> {
    let mut open_block: Option<&str> = None;
    reply
        .lines()
        .map(|line| {
            let line = line.trim();
            if let Some(close) = open_block {
                if line.starts_with(close) || line.ends_with(close) {
                    open_block = None;
                }
                return (Line::Material, line);
            }
            for (open, close) in [("```", "```"), ("~~~", "~~~"), ("$$", "$$"), ("\\[", "\\]")] {
                if let Some(rest) = line.strip_prefix(open) {
                    if !rest.ends_with(close) {
                        open_block = Some(close);
                    }
                    return (Line::Material, line);
                }
            }
            if line.is_empty() {
                (Line::Blank, line)
            } else if line.starts_with('|') || line.chars().all(|c| "-*_=".contains(c)) {
                (Line::Material, line)
            } else if let Some(item) = strip_list_marker(line) {
                (Line::ListItem, item)
            } else {
                (Line::Text, line)
            }
        })
        .collect()
}

fn strip_list_marker(line: &str) -> Option<&str> {
    if let Some(rest) = line.strip_prefix(['-', '*', '+']) {
        return rest.starts_with(' ').then(|| rest.trim_start());
    }
    let marker = line.strip_prefix('(').unwrap_or(line);
    let label_end = marker.find(|c: char| !c.is_ascii_alphanumeric())?;
    let (label, rest) = marker.split_at(label_end);
    let short = label.len() == 1 || label.bytes().all(|b| b.is_ascii_digit());
    let rest = rest.strip_prefix(['.', ')'])?;
    (!label.is_empty() && short && rest.starts_with(' ')).then(|| rest.trim_start())
}

fn sets_work(line: &str) -> bool {
    let plain: String = line
        .chars()
        .filter(|c| !matches!(c, '*' | '_' | '`' | '#' | '>'))
        .collect::<String>()
        .to_lowercase();
    plain
        .split_inclusive(['.', '!', '?', ':', ';'])
        .any(|sentence| asks(sentence) || instructs(sentence))
}

fn asks(sentence: &str) -> bool {
    let start = sentence.trim_start_matches(|c: char| !c.is_alphanumeric());
    sentence.trim_end().ends_with('?')
        && !CHECK_INS
            .iter()
            .any(|check_in| starts_with_phrase(start, check_in))
}

fn instructs(sentence: &str) -> bool {
    let mut rest = sentence.trim_start_matches(|c: char| !c.is_alphanumeric());
    while let Some(opener) = OPENERS
        .iter()
        .find(|opener| starts_with_phrase(rest, opener))
    {
        rest = rest[opener.len()..].trim_start_matches(|c: char| !c.is_alphanumeric());
    }
    TASK_STARTS
        .iter()
        .any(|start| starts_with_phrase(rest, start))
}

fn starts_with_phrase(text: &str, phrase: &str) -> bool {
    text.strip_prefix(phrase)
        .is_some_and(|rest| !rest.starts_with(char::is_alphanumeric))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(reply: &str) -> bool {
        ReplyCheck::of(reply).ends_with_task
    }

    #[test]
    fn a_question_or_an_instruction_at_the_end_is_a_task() {
        for reply in [
            "The root is 3.\n\nWhat is the other root?",
            "So b is fixed.\n\n**Your turn:** solve 2x + 3 = 11.",
            "Good.\n\nNow try the case where n is odd.",
            "First, find the mean. Then compare it with the median.",
            "Here is the idea. Give it a go and tell me what you get.",
            "Your turn!",
            "Let's try one: 3/4 of 12.",
            "Which statement is true?\n\nA) b varies\nB) b is fixed\nC) b is x",
            "Which statement is true?\n\n- b varies\n\n- b is fixed",
            "Predict the output:\n\n```python\nprint(len('abc'))\n```",
            "Solve for x:\n\n$$\nx^2 - 5x + 6 = 0\n$$",
            "Try these:\n\n1. 3 + 4\n2. 5 + 6",
            "> **Task.** Differentiate $x^3$.",
            "Can you find the root of $x - 4$?",
            "Ok, what is 3 + 4?",
        ] {
            assert!(task(reply), "{reply}");
        }
    }

    #[test]
    fn a_summary_a_check_in_or_bare_material_at_the_end_is_not_a_task() {
        for reply in [
            "Why does this work? Because the identity cancels the root.\n\nThat is the key idea.",
            "The coefficient b scales x. Does that make sense?",
            "Any questions?",
            "Would you like another example?",
            "That covers it. Ready?",
            "",
            "```python\nprint(1)\n```",
            "Key points:\n\n- b is fixed\n- x varies",
            "| x | y |\n|---|---|\n| 1 | 2 |",
        ] {
            assert!(!task(reply), "{reply}");
        }
    }

    #[test]
    fn known_misses_stay_as_documented() {
        for (reply, judged) in [
            ("Solve 2x = 4. Good luck!\n\nSee you soon.", false),
            ("When you're ready, compute the variance.", false),
            ("Here's one for you:\n\n$$\\int x e^x\\,dx$$", false),
            (
                "Why does this work? Because the identity cancels the root.",
                true,
            ),
            ("Work is force times distance.", true),
        ] {
            assert_eq!(task(reply), judged, "{reply}");
        }
    }

    #[test]
    fn words_are_tokens_with_a_letter_or_digit() {
        assert_eq!(ReplyCheck::of("Solve $x + 3 = 5$ - quickly!").words, 5);
        assert_eq!(ReplyCheck::of("| a | b |\n|---|---|").words, 2);
        assert_eq!(ReplyCheck::of("").words, 0);
    }

    #[test]
    fn feedback_names_each_fault_and_nothing_when_the_rule_held() {
        let words = |n: usize| "word ".repeat(n);
        let feedback = |reply: String| ReplyCheck::of(&reply).feedback();
        assert_eq!(feedback(format!("{}What next?", words(10))), None);
        assert_eq!(feedback(format!("{}Why?", words(198))), None);
        assert_eq!(
            feedback(format!("{}Why?", words(199))).as_deref(),
            Some(" Previous reply: 200 words.")
        );
        assert_eq!(
            feedback("Done.".into()).as_deref(),
            Some(" Previous reply: no task.")
        );
        assert_eq!(
            feedback(words(850)).as_deref(),
            Some(" Previous reply: 850 words, no task.")
        );
    }
}
