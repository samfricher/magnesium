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
    Ok(answers_previous_prompt
        .then(|| ReplyCheck::of(&reply).feedback())
        .flatten())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Ending {
    Task,
    NoTask,
    NotJudged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReplyCheck {
    words: usize,
    ending: Ending,
}

impl ReplyCheck {
    fn of(reply: &str) -> Self {
        let lines = classify(reply);
        let ending = if sets_work(&last_block(&lines)) {
            Ending::Task
        } else if reads_as_english(&lines) {
            Ending::NoTask
        } else {
            Ending::NotJudged
        };
        Self {
            words: count_words(reply.split_whitespace()),
            ending,
        }
    }

    fn feedback(self) -> Option<String> {
        let faults: Vec<String> = [
            (self.words >= WORD_LIMIT).then(|| format!("{} words", self.words)),
            (self.ending == Ending::NoTask).then(|| "no task".to_owned()),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!faults.is_empty()).then(|| format!(" Previous reply: {}.", faults.join(", ")))
    }
}

fn count_words<'a>(tokens: impl Iterator<Item = &'a str>) -> usize {
    tokens
        .filter(|token| token.chars().any(char::is_alphanumeric))
        .count()
}

const ENGLISH_WORDS: &[&str] = &[
    "and", "are", "be", "but", "by", "can", "do", "does", "for", "from", "has", "have", "how",
    "if", "is", "it", "its", "not", "of", "or", "so", "that", "the", "then", "there", "they",
    "this", "to", "was", "we", "what", "when", "which", "why", "will", "with", "you", "your",
];

const OTHER_LANGUAGE_WORDS: &[&str] = &[
    "con", "das", "de", "del", "der", "des", "di", "die", "du", "ein", "eine", "el", "em", "es",
    "est", "et", "ist", "la", "las", "le", "les", "los", "mit", "não", "nicht", "os", "para",
    "por", "que", "uma", "un", "una", "und", "une", "y",
];

fn reads_as_english(lines: &[(Line, &str)]) -> bool {
    let prose: Vec<String> = lines
        .iter()
        .filter(|(kind, _)| matches!(kind, Line::Text | Line::ListItem))
        .flat_map(|(_, text)| text.split_whitespace())
        .map(|token| {
            token
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .collect();
    let letters = prose
        .iter()
        .flat_map(|word| word.chars())
        .filter(|c| c.is_alphabetic());
    let (latin, other_script) = letters.fold((0, 0), |(latin, other), c| {
        if (c as u32) < 0x250 {
            (latin + 1, other)
        } else {
            (latin, other + 1)
        }
    });
    let count = |list: &[&str]| {
        prose
            .iter()
            .filter(|word| list.contains(&word.as_str()))
            .count()
    };
    other_script <= latin && count(OTHER_LANGUAGE_WORDS) <= count(ENGLISH_WORDS)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Line {
    Blank,
    Text,
    ListItem,
    Material,
}

fn last_block<'a>(lines: &[(Line, &'a str)]) -> Vec<&'a str> {
    let end = before_material(lines, lines.len());
    let mut start = with_the_question_of_trailing_options(lines, paragraph_start(lines, end), end);
    let last: Vec<&str> = lines[start..end].iter().map(|(_, text)| *text).collect();
    if is_sign_off(&last) {
        let before = before_material(lines, start);
        start =
            with_the_question_of_trailing_options(lines, paragraph_start(lines, before), before);
        return lines[start..before]
            .iter()
            .map(|(_, text)| *text)
            .chain(last)
            .collect();
    }
    last
}

fn before_material(lines: &[(Line, &str)], mut end: usize) -> usize {
    while end > 0 && matches!(lines[end - 1].0, Line::Blank | Line::Material) {
        end -= 1;
    }
    end
}

fn is_sign_off(paragraph: &[&str]) -> bool {
    !paragraph.is_empty()
        && count_words(paragraph.iter().flat_map(|line| line.split_whitespace())) <= 4
        && !paragraph
            .iter()
            .any(|line| line.chars().any(|c| c.is_ascii_digit()))
        && !sets_work(paragraph)
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

const QUESTION_MARKS: [char; 3] = ['?', '？', '؟'];
const SENTENCE_ENDS: [char; 7] = ['.', '!', '?', '。', '！', '？', '؟'];
const CLOSERS: &[char] = &[')', ']', '"', '\'', '’', '”', '»'];

fn sets_work(block: &[&str]) -> bool {
    let sentences: Vec<String> = block
        .iter()
        .flat_map(|line| sentences(&plain(line)))
        .collect();
    sentences.iter().enumerate().any(|(index, sentence)| {
        let answered = sentences
            .get(index + 1)
            .is_some_and(|next| starts_with_phrase(next, "because"));
        (asks(sentence) && !answered) || instructs(sentence)
    })
}

fn plain(line: &str) -> String {
    let mut in_code = false;
    line.chars()
        .filter_map(|c| match c {
            '`' => {
                in_code = !in_code;
                Some(' ')
            }
            _ if in_code => None,
            '*' | '_' | '#' | '>' => None,
            _ => Some(c),
        })
        .collect::<String>()
        .to_lowercase()
        .replace('’', "'")
}

fn sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut sentences = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < chars.len() {
        if SENTENCE_ENDS.contains(&chars[index]) {
            let mut end = index + 1;
            while end < chars.len() && CLOSERS.contains(&chars[end]) {
                end += 1;
            }
            let wide = !chars[index].is_ascii();
            if end == chars.len() || wide || chars[end].is_whitespace() {
                sentences.push(
                    chars[start..end]
                        .iter()
                        .collect::<String>()
                        .trim()
                        .to_owned(),
                );
                start = end;
                index = end;
                continue;
            }
        }
        index += 1;
    }
    sentences.push(chars[start..].iter().collect::<String>().trim().to_owned());
    sentences.retain(|sentence| !sentence.is_empty());
    sentences
}

const CHECK_INS: &[&str] = &[
    "any other questions",
    "any questions",
    "anything else",
    "clear so far",
    "clear?",
    "do you want",
    "does it help",
    "does it make sense",
    "does that help",
    "does that make sense",
    "does this help",
    "does this make sense",
    "got it",
    "is that clear",
    "is there anything else",
    "is this clear",
    "make sense",
    "makes sense",
    "need another",
    "ok?",
    "okay?",
    "questions?",
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
    "want another",
    "want me",
    "would you like",
];

fn asks(sentence: &str) -> bool {
    let start = sentence.trim_start_matches(|c: char| !c.is_alphanumeric());
    sentence.trim_end_matches(CLOSERS).ends_with(QUESTION_MARKS)
        && (!CHECK_INS
            .iter()
            .any(|check_in| starts_with_phrase(start, check_in))
            || mentions_a_task(start))
}

const TASK_LABELS: &[&str] = &[
    "challenge",
    "exercise",
    "now you",
    "over to you",
    "practice",
    "practise",
    "problem",
    "question",
    "quick check",
    "task",
    "try it",
    "try this",
    "your problem",
    "your task",
    "your turn",
];

const OPENERS: &[&str] = &[
    "and",
    "first",
    "if you like",
    "if you want",
    "next",
    "now",
    "ok",
    "okay",
    "please",
    "so",
    "then",
    "when you are ready",
    "when you're ready",
];

const TASK_PHRASES: &[&str] = &[
    "figure out",
    "find out",
    "give it a go",
    "have a go",
    "it's your turn",
    "let's practice",
    "let's practise",
    "let's try",
    "now you",
    "over to you",
    "you try",
    "your task",
    "your turn",
];

const TASK_WORDS: &[&str] = &[
    "apply",
    "attempt",
    "calculate",
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
    "divide",
    "draw",
    "estimate",
    "evaluate",
    "expand",
    "explain",
    "factor",
    "factorise",
    "factorize",
    "fill",
    "find",
    "fix",
    "guess",
    "identify",
    "implement",
    "integrate",
    "justify",
    "label",
    "match",
    "multiply",
    "pick",
    "plot",
    "predict",
    "prove",
    "rank",
    "rearrange",
    "rewrite",
    "simplify",
    "sketch",
    "solve",
    "sort",
    "spot",
    "substitute",
    "subtract",
    "summarise",
    "summarize",
    "trace",
    "translate",
    "try",
    "write",
];

const MATH: &str = "<math>";
const WORDS_WITH_FOLLOWERS: &[(&str, &[&str])] = &[
    (
        "add",
        &[MATH, "a", "an", "both", "it", "the", "them", "these", "up"],
    ),
    (
        "answer",
        &[
            "all",
            "both",
            "each",
            "question",
            "questions",
            "the",
            "these",
            "this",
        ],
    ),
    (
        "check",
        &[
            "by", "each", "if", "it", "that", "the", "these", "this", "whether", "your",
        ],
    ),
    (
        "consider",
        &[
            MATH, "how", "these", "this", "what", "whether", "which", "why",
        ],
    ),
    (
        "give",
        &[
            "a", "an", "both", "each", "it", "me", "one", "the", "three", "two", "your",
        ],
    ),
    (
        "list",
        &[
            MATH, "all", "each", "every", "five", "four", "some", "the", "three", "two", "your",
        ],
    ),
    (
        "name",
        &[
            "a", "all", "an", "each", "every", "one", "the", "this", "three", "two", "your",
        ],
    ),
    (
        "practice",
        &["by", "it", "on", "the", "these", "this", "using", "with"],
    ),
    (
        "practise",
        &["by", "it", "on", "the", "these", "this", "using", "with"],
    ),
    ("read", &["it", "the", "this", "through"]),
    (
        "recall",
        &["a", "an", "how", "the", "what", "which", "why", "your"],
    ),
    (
        "say",
        &["how", "if", "it", "what", "whether", "which", "why"],
    ),
    ("see", &["if", "whether"]),
    (
        "show",
        &[
            "all", "each", "how", "it", "me", "that", "the", "what", "which", "why", "your",
        ],
    ),
    (
        "state",
        &[
            "a", "all", "an", "each", "the", "what", "whether", "which", "why", "your",
        ],
    ),
    ("tell", &["me", "us"]),
    (
        "test",
        &[
            "all", "each", "if", "it", "the", "these", "this", "whether", "with", "your",
        ],
    ),
    ("think", &["about", "through"]),
    (
        "use",
        &[
            MATH, "a", "an", "both", "each", "it", "the", "them", "these", "this", "those", "your",
        ],
    ),
    ("work", &["out", "through", "on"]),
];

fn instructs(sentence: &str) -> bool {
    let sentence = sentence.trim_start_matches(|c: char| !c.is_alphanumeric());
    if starts_a_task(sentence) {
        return true;
    }
    match sentence.split_once(':') {
        Some((label, _)) if label.split_whitespace().count() <= 3 => TASK_LABELS
            .iter()
            .any(|task_label| starts_with_phrase(label.trim(), task_label)),
        _ => false,
    }
}

fn starts_a_task(text: &str) -> bool {
    let mut rest = text;
    while let Some(opener) = OPENERS
        .iter()
        .find(|opener| starts_with_phrase(rest, opener))
    {
        rest = rest[opener.len()..].trim_start_matches(|c: char| !c.is_alphanumeric());
    }
    let mut words = rest
        .split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric() && c != '$' && c != '\\'));
    let (Some(first), next) = (words.next(), words.next()) else {
        return false;
    };
    TASK_PHRASES
        .iter()
        .any(|phrase| starts_with_phrase(rest, phrase))
        || TASK_WORDS.contains(&first)
        || WORDS_WITH_FOLLOWERS.iter().any(|(word, followers)| {
            *word == first && next.is_some_and(|next| follows(next, followers))
        })
}

fn follows(word: &str, followers: &[&str]) -> bool {
    let math = word.chars().count() == 1 && word.chars().all(char::is_alphabetic)
        || word.chars().any(|c| c.is_ascii_digit())
        || word.starts_with(['$', '\\']);
    followers.contains(&word) || (math && followers.contains(&MATH))
}

fn mentions_a_task(text: &str) -> bool {
    TASK_PHRASES.iter().any(|phrase| text.contains(phrase))
        || text
            .split_whitespace()
            .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()))
            .any(|word| TASK_WORDS.contains(&word))
}

fn starts_with_phrase(text: &str, phrase: &str) -> bool {
    text.strip_prefix(phrase)
        .is_some_and(|rest| phrase.ends_with('?') || !rest.starts_with(char::is_alphanumeric))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ending(reply: &str) -> Ending {
        ReplyCheck::of(reply).ending
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
            "It's your turn. What is the derivative of x^2 sin x?\n\nTake your time.",
            "Think about what happens to the margin when n quadruples.",
            "Consider n = 400 and work out the new margin.",
            "See if you can rewrite 12/18 in lowest terms.",
            "Multiply both sides by 3 and tell me what x is.",
            "Substitute x = 2 into the expression.",
            "Figure out which coefficient is b in 3x^2 - 5x + 2.",
            "Add 3 to both sides, then rearrange for x.",
            "Say which coefficient is b.",
            "When you're ready, compute the variance.",
            "Now compute the margin for n = 400.\n\nGood luck!",
            "Which statement is true?\n\nA) b varies\nB) b is fixed\nC) b is x",
            "Which statement is true?\n\n- b varies\n\n- b is fixed",
            "Predict the output:\n\n```python\nprint(len('abc'))\n```",
            "Solve for x:\n\n$$\nx^2 - 5x + 6 = 0\n$$",
            "Try these:\n\n1. 3 + 4\n2. 5 + 6",
            "> **Task.** Differentiate $x^3$.",
            "Can you find the root of $x - 4$?",
            "Ok, what is 3 + 4?",
            "Good.\n\nQuestion 2: factor x^2 - 5x + 6.",
            "I am stuck too.\n\nYour problem: a die shows 6 half the time. Find E.",
            "Would you like to try one: 3x - 7 = 11?",
            "Ready to try n = 400?",
            "Would you like to have a go at n = 400?",
            "那么，x 的系数是多少？",
            "ما هو معامل x؟",
        ] {
            assert_eq!(ending(reply), Ending::Task, "{reply}");
        }
    }

    #[test]
    fn a_summary_a_check_in_or_bare_material_at_the_end_is_not_a_task() {
        for reply in [
            "Why does this work? Because the identity cancels the root.\n\nThat is the key idea.",
            "Why does the margin halve? Because it scales with 1/sqrt(n).",
            "The coefficient b scales x. Does that make sense?",
            "That is the key idea. Does this help?",
            "That is the whole idea. Clear so far?",
            "That covers it. Want another example?",
            "That covers it. Questions?",
            "That covers it. Is there anything else you'd like to know?",
            "Any questions?",
            "Would you like another example?",
            "That covers it. Ready?",
            "Recall that the derivative of x^2 is 2x.",
            "So b is the coefficient of x.\n\nLet's summarise: a scales x^2, b scales x, c is constant.",
            "x = 3 gives 9 - 15 + 6 = 0.\n\nLet's check: 3^2 - 5*3 + 6 = 0, so it works.",
            "First we factor. Finally, apply the zero-product rule to get x = 2 or x = 3.",
            "List comprehensions build a new list from an old one.",
            "Practice makes this automatic.",
            "Use cases include caching and memoisation.",
            "Test statistics follow a t distribution here.",
            "Name resolution follows the LEGB rule.",
            "That is the whole idea. The docs are at https://docs.python.org/3/search.html?q=recursion for more detail.",
            "The base case stops the recursion.\n\nTip: use small inputs when you trace.",
            "Tip: use the smallest input that still shows the bug.",
            "Note: use the outer derivative first.",
            "Remember: check your units at the end.",
            "In C that is `a ? b : c`.",
            "The method is `empty?` in Ruby.",
            "That covers the base case.\n\nLet me know if you have any questions.",
            "",
            "```python\nprint(1)\n```",
            "Key points:\n\n- b is fixed\n- x varies",
            "| x | y |\n|---|---|\n| 1 | 2 |",
        ] {
            assert_eq!(ending(reply), Ending::NoTask, "{reply}");
        }
    }

    #[test]
    fn a_reply_not_in_english_is_not_judged_for_a_task() {
        for reply in [
            "Bien. El caso base es la lista vacía.\n\nCalcula la suma de [1, 2, 3] paso a paso.",
            "Gut. Der Basisfall ist die leere Liste. Berechne die Summe von [1, 2, 3].",
        ] {
            assert_eq!(ending(reply), Ending::NotJudged, "{reply}");
        }
        let long = format!("{}Calcula la suma.", "Es la lista vacía. ".repeat(60));
        assert_eq!(
            ReplyCheck::of(&long).feedback().as_deref(),
            Some(" Previous reply: 243 words.")
        );
    }

    #[test]
    fn known_misses_stay_as_documented() {
        for (reply, judged) in [
            ("Here's one to try:\n\n$$3x - 7 = 11$$", Ending::NoTask),
            (
                "Fix the bug in this function:\n\n```python\nreturn x+\n```\n\nHint: look at line 2.",
                Ending::NoTask,
            ),
            ("Once you're ready, compute the variance.", Ending::NoTask),
            (
                "Why does this matter? It matters since n grows.",
                Ending::Task,
            ),
            ("Work out is a phrasal verb.", Ending::Task),
            ("Show me is a phrase; the answer was 4.", Ending::Task),
            ("Start by factoring the top.", Ending::NoTask),
            ("Go ahead and factor the top.", Ending::NoTask),
            ("Take a moment to factor the top.", Ending::NoTask),
            (
                "Using the product rule, differentiate x sin x.",
                Ending::NoTask,
            ),
            ("Differentiate x sin x. Good luck on step 2!", Ending::Task),
            (
                "Differentiate x sin x.\n\nGood luck on step 2!",
                Ending::NoTask,
            ),
            (
                "In Spanish, la suma de los números es el total de la lista.",
                Ending::NotJudged,
            ),
        ] {
            assert_eq!(ending(reply), judged, "{reply}");
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
            feedback("That is the whole idea.".into()).as_deref(),
            Some(" Previous reply: no task.")
        );
        assert_eq!(
            feedback("This is the idea and it is what you need. ".repeat(85)).as_deref(),
            Some(" Previous reply: 850 words, no task.")
        );
    }
}
