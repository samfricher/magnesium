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
    (words >= WORD_LIMIT).then(|| format!(" Previous reply: {words} words."))
}

fn starts_with_phrase(text: &str, phrase: &str) -> bool {
    text.strip_prefix(phrase)
        .is_some_and(|rest| !rest.starts_with(char::is_alphanumeric))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn feedback_gives_the_length_of_a_reply_over_the_limit_and_nothing_otherwise() {
        let words = |n: usize| "word ".repeat(n);
        assert_eq!(judge(""), None);
        assert_eq!(judge(&words(10)), None);
        assert_eq!(judge(&words(199)), None);
        assert_eq!(
            judge(&words(200)).as_deref(),
            Some(" Previous reply: 200 words.")
        );
        assert_eq!(
            judge(&"word - | ".repeat(150)),
            None,
            "a token without a letter or digit is not a word"
        );
        assert_eq!(
            judge(&"This is the idea and it is what you need. ".repeat(85)).as_deref(),
            Some(" Previous reply: 850 words.")
        );
    }
}
