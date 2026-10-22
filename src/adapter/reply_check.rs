use rusqlite::{Connection, OptionalExtension, params};

use crate::learner::HostContext;

const WORD_LIMIT: usize = 200;

pub(crate) fn task_reminder() -> String {
    format!(" Teaching reply: one idea, under {WORD_LIMIT} words, end with one task.")
}

pub(crate) const CONFUSION_REMINDER: &str =
    " If the learner is confused, ask one question to find out why before explaining.";

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

#[cfg(test)]
mod tests {
    use super::*;

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
