//! The Claude lesson prompt notice: its teaching reminder, the feedback on
//! the previous reply, and how its clauses fit the hook's byte limit.
use super::*;
use serde_json::json;

fn claude_lesson() -> (
    std::path::PathBuf,
    crate::learner::LearnerStore,
    HostIdentity,
) {
    let (root, store, _) = super::tests::fixture();
    let host = HostIdentity::claude("teaching-reminder").unwrap();
    store.update(&host_context(&host),json!({"schema":"learner-model/v2","action":"update","request_id":"start","session":{"operation":"start","client_ref":"s","goal":"Quadratic coefficients"}})).unwrap();
    (root, store, host)
}
fn has_task_reminder(note: &str) -> bool {
    note.contains(&reply_check::task_reminder())
}
fn has_confusion_reminder(note: &str) -> bool {
    note.contains(reply_check::CONFUSION_REMINDER)
}
fn leave_unbound_work(store: &crate::learner::LearnerStore) {
    let earlier = host_context(&HostIdentity::claude("earlier-conversation").unwrap());
    let unbound = crate::learner::RawEvent {
        external_event_key: "unbound".into(),
        speaker: crate::learner::Speaker::Learner,
        kind: "message".into(),
        text: "An answer before any lesson".into(),
        metadata: None,
        occurred_at_ms: None,
    };
    store
        .capture(&earlier, &[unbound], "host_observed", true)
        .unwrap();
}
fn claude_prompt(root: &Path, host: &HostIdentity, turn: &str) -> String {
    host_user_prompt_submit(root, host, Some(turn), None, "Is b another unknown?")
        .unwrap()
        .unwrap()
}
fn claude_stop(root: &Path, host: &HostIdentity, turn: &str, reply: &str) -> Result<(), String> {
    host_response_stop_with_delivery(root, host, &host.stored_turn_id(turn).unwrap(), Some(reply))
}
#[test]
fn claude_prompt_notice_reports_only_the_reply_to_the_previous_prompt() {
    let (root, store, host) = claude_lesson();
    let first = claude_prompt(&root, &host, "one");
    assert!(has_task_reminder(&first), "{first}");
    assert!(has_confusion_reminder(&first), "{first}");
    assert!(!first.contains("Previous reply"), "{first}");

    claude_stop(&root, &host, "one", &"word ".repeat(850)).unwrap();
    assert!(
        claude_stop(&root, &host, "one", "Short now. What is b?").is_err(),
        "a repeated Stop with new text keeps the first reply"
    );
    let second = claude_prompt(&root, &host, "two");
    assert!(second.contains(" Previous reply: 850 words."), "{second}");
    assert!(second.contains("current event e-"), "{second}");
    assert!(second.contains(CLAUDE_LESSON_GUIDANCE), "{second}");
    assert!(second.len() <= 512, "{} bytes: {second}", second.len());
    assert_eq!(
        claude_prompt(&root, &host, "two"),
        second,
        "a retried prompt hook"
    );

    let after_missed_stop = claude_prompt(&root, &host, "three");
    assert!(
        !after_missed_stop.contains("Previous reply"),
        "{after_missed_stop}"
    );
    claude_stop(
        &root,
        &host,
        "three",
        "b is fixed. Your turn: name the coefficient of x.",
    )
    .unwrap();
    let after_good_reply = claude_prompt(&root, &host, "four");
    assert!(
        !after_good_reply.contains("Previous reply"),
        "{after_good_reply}"
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn claude_feedback_never_judges_a_reply_across_an_unrecorded_turn() {
    let (root, store, host) = claude_lesson();
    let context = host_context(&host);
    let control = |request: &str, action: &str| {
        let event = crate::learner::RawEvent {
            external_event_key: request.into(),
            speaker: crate::learner::Speaker::Learner,
            kind: "message".into(),
            text: action.into(),
            metadata: Some(crate::learner::EventMetadata {
                origin: Some("model_reported".into()),
                ..Default::default()
            }),
            occurred_at_ms: None,
        };
        store.control(&context, request, action, &event).unwrap();
    };
    claude_prompt(&root, &host, "one");
    control("off", "recording_off");
    claude_stop(&root, &host, "one", &"unrecorded ".repeat(300)).unwrap();
    claude_prompt(&root, &host, "two");
    control("on", "recording_on");
    claude_stop(&root, &host, "two", &"recorded ".repeat(300)).unwrap();
    let after_gap = claude_prompt(&root, &host, "three");
    assert!(!after_gap.contains("Previous reply"), "{after_gap}");
    claude_stop(&root, &host, "three", &"recorded ".repeat(300)).unwrap();
    let resumed = claude_prompt(&root, &host, "four");
    assert!(resumed.contains(" Previous reply: 300 words."), "{resumed}");
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
fn long_reply() -> String {
    "This is the idea and it is what you need. ".repeat(30)
}
#[test]
fn claude_feedback_skips_a_turn_after_a_prompt_hook_that_exited_early() {
    let (root, store, host) = claude_lesson();
    claude_prompt(&root, &host, "one");
    claude_stop(&root, &host, "one", &long_reply()).unwrap();
    claude_stop(&root, &host, "two", "Good. Now solve 2x = 6.").unwrap();
    let after = claude_prompt(&root, &host, "three");
    assert!(!after.contains("Previous reply"), "{after}");
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn claude_conversations_keep_their_own_feedback() {
    let (root, store, first) = claude_lesson();
    let second = HostIdentity::claude("teaching-reminder-second").unwrap();
    store.update(&host_context(&second),json!({"schema":"learner-model/v2","action":"update","request_id":"start-second","session":{"operation":"start","client_ref":"s2","goal":"Quadratic roots"}})).unwrap();
    claude_prompt(&root, &first, "a-1");
    claude_prompt(&root, &second, "b-1");
    claude_stop(&root, &first, "a-1", &long_reply()).unwrap();
    claude_stop(&root, &second, "b-1", "Good. Your turn: solve 2x = 6.").unwrap();
    let first_note = claude_prompt(&root, &first, "a-2");
    let second_note = claude_prompt(&root, &second, "b-2");
    assert!(
        first_note.contains(" Previous reply: 300 words."),
        "{first_note}"
    );
    assert!(!second_note.contains("Previous reply"), "{second_note}");
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
fn assert_note(note: &str, task: bool, confusion: bool, feedback: bool, unbound: bool) {
    assert_eq!(has_task_reminder(note), task, "task: {note}");
    assert_eq!(has_confusion_reminder(note), confusion, "confusion: {note}");
    assert_eq!(
        note.contains(" Previous reply: 300 words."),
        feedback,
        "feedback: {note}"
    );
    assert_eq!(
        note.contains("context.pending_work unbound_only:true"),
        unbound,
        "unbound: {note}"
    );
    assert!(note.contains(CLAUDE_LESSON_GUIDANCE), "{note}");
    assert!(note.contains("current event e-"), "{note}");
    assert!(note.len() <= 512, "{} bytes: {note}", note.len());
}
#[test]
fn a_long_reply_shortens_the_state_line_and_keeps_every_reminder() {
    let (root, store, host) = claude_lesson();
    store
        .conn
        .execute("UPDATE workspace_meta SET memory_generation=?1", [9999])
        .unwrap();
    claude_prompt(&root, &host, "one");

    claude_stop(&root, &host, "one", "Good. Your turn: solve 2x = 6.").unwrap();
    let short_reply = claude_prompt(&root, &host, "two");
    assert_note(&short_reply, true, true, false, false);
    assert!(short_reply.contains(STATE_ADVICE), "{short_reply}");

    claude_stop(&root, &host, "two", &long_reply()).unwrap();
    let after_long_reply = claude_prompt(&root, &host, "three");
    assert_note(&after_long_reply, true, true, true, false);
    assert!(
        !after_long_reply.contains(STATE_ADVICE),
        "{after_long_reply}"
    );

    leave_unbound_work(&store);
    claude_stop(&root, &host, "three", "Good. Your turn: solve 2x = 6.").unwrap();
    assert_note(
        &claude_prompt(&root, &host, "four"),
        true,
        true,
        false,
        true,
    );

    claude_stop(&root, &host, "four", &long_reply()).unwrap();
    assert_note(&claude_prompt(&root, &host, "five"), true, true, true, true);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn claude_prompt_outside_a_lesson_gets_no_reminder() {
    let (root, store, _) = claude_lesson();
    let other = HostIdentity::claude("no-lesson-here").unwrap();
    let note = claude_prompt(&root, &other, "one");
    assert!(!has_task_reminder(&note), "{note}");
    assert!(!has_confusion_reminder(&note), "{note}");
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn claude_prompt_notice_keeps_required_clauses_within_512_bytes_at_extreme_counts() {
    let (root, store, host) = claude_lesson();
    store
        .conn
        .execute("UPDATE workspace_meta SET memory_generation=?1", [i64::MAX])
        .unwrap();
    leave_unbound_work(&store);
    claude_prompt(&root, &host, "one");
    claude_stop(&root, &host, "one", &"w ".repeat(123_456)).unwrap();
    let notice = claude_prompt(&root, &host, "two");
    assert!(notice.len() <= 512, "{} bytes: {notice}", notice.len());
    for required in [
        "current event e-",
        CLAUDE_LESSON_GUIDANCE,
        "context.pending_work unbound_only:true",
        " Previous reply: 123456 words.",
        reply_check::CONFUSION_REMINDER,
    ] {
        assert!(notice.contains(required), "{required}: {notice}");
    }
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn claude_prompt_notice_without_a_native_ticket_stays_within_512_bytes() {
    let (root, store, _) = super::tests::fixture();
    let host = HostIdentity::claude("no-ticket").unwrap();
    leave_unbound_work(&store);
    // A prompt over the event size limit gets no native ticket.
    let text = "x".repeat(crate::learner::limits::EVENT_BYTES + 1);
    let notice = host_user_prompt_submit(&root, &host, Some("big"), None, &text)
        .unwrap()
        .unwrap();
    let limit = NoticeHook::UserPromptSubmit.additional_context_limit();
    assert!(notice.len() <= limit, "{} bytes: {notice}", notice.len());
    assert!(notice.ends_with(NATIVE_TICKET_FALLBACK), "{notice}");
    assert!(notice.contains(CLAUDE_START_GUIDANCE), "{notice}");
    assert!(notice.contains("unbound 1;"), "{notice}");
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn notice_clauses_give_way_in_order_and_required_ones_stay() {
    let clauses = vec![
        (Clause::Required, " lesson".into()),
        (Clause::TaskReminder, " task".into()),
        (Clause::ConfusionReminder, " confused".into()),
        (Clause::ReplyFeedback, " feedback".into()),
    ];
    let fit_to = |limit| fit("state".into(), clauses.clone(), limit);
    let full = 35 + STATE_ADVICE.len();
    assert_eq!(
        fit_to(full),
        format!("state{STATE_ADVICE} lesson task confused feedback")
    );
    // The state line loses its advice before any clause gives way.
    assert_eq!(fit_to(full - 1), "state lesson task confused feedback");
    assert_eq!(fit_to(35), "state lesson task confused feedback");
    assert_eq!(fit_to(34), "state lesson confused feedback");
    assert_eq!(fit_to(30), "state lesson confused feedback");
    assert_eq!(fit_to(29), "state lesson confused");
    assert_eq!(fit_to(20), "state lesson");
    assert_eq!(fit_to(1), "state lesson");
}
#[test]
fn notice_limits_match_the_claude_plugin_hooks() {
    let hooks: serde_json::Value =
        serde_json::from_str(include_str!("../../claude-plugin/hooks/hooks.json")).unwrap();
    for (event, hook) in [
        (
            "SessionStart",
            NoticeHook::SessionStart { core_rules: true },
        ),
        ("UserPromptSubmit", NoticeHook::UserPromptSubmit),
    ] {
        assert_eq!(
            hooks["hooks"][event][0]["hooks"][0]["additionalContextLimit"],
            hook.additional_context_limit(),
            "{event}"
        );
    }
}
fn deliveries(store: &crate::learner::LearnerStore, host: &HostIdentity) -> Vec<(String, String)> {
    let context = host_context(host);
    let mut statement = store
        .conn
        .prepare("SELECT external_event_key,text FROM learning_events WHERE host_thread_id=?1 AND speaker='tutor' AND kind='delivery' ORDER BY event_no")
        .unwrap();
    statement
        .query_map([&context.host_thread_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}
#[test]
fn a_reply_from_the_window_tool_is_kept_once_and_answers_its_prompt() {
    let (root, store, host) = claude_lesson();
    let context = host_context(&host);
    claude_prompt(&root, &host, "one");
    let long = "word ".repeat(300);
    host_reply(&root, &context, &long).unwrap();
    // A repeated call with the same words keeps one reply.
    host_reply(&root, &context, &long).unwrap();
    // The end-of-turn hook sees the same words, or none, and keeps nothing more.
    claude_stop(&root, &host, "one", long.trim()).unwrap();
    assert_eq!(deliveries(&store, &host).len(), 1);
    let learner_key = native_capture::event_key(&host.stored_turn_id("one").unwrap());
    assert_eq!(deliveries(&store, &host)[0].0, reply_key(&learner_key, 1));
    assert_eq!(answered_key(&reply_key(&learner_key, 2)), learner_key);
    // The reply check pairs it with the prompt it answers.
    let second = claude_prompt(&root, &host, "two");
    assert!(second.contains(" Previous reply: 300 words."), "{second}");

    // Text written besides the reply is kept as well, so nothing is lost.
    host_reply(&root, &context, "Right. Now: what is c?").unwrap();
    claude_stop(&root, &host, "two", "One more line the tutor wrote.").unwrap();
    let kept = deliveries(&store, &host);
    assert_eq!(kept.len(), 3, "{kept:?}");
    assert_eq!(kept[2].1, "One more line the tutor wrote.");

    assert!(host_reply(&root, &context, "  ").is_err(), "an empty reply");
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn a_reply_outside_a_recording_lesson_is_shown_and_not_kept() {
    let (root, store, _) = super::tests::fixture();
    let host = HostIdentity::claude("no-lesson").unwrap();
    host_reply(
        &root,
        &host_context(&host),
        "Hello. What would you like to learn?",
    )
    .unwrap();
    assert!(deliveries(&store, &host).is_empty());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn a_learning_step_after_the_reply_is_known_as_the_turns_last_write() {
    let (root, store, host) = claude_lesson();
    let context = host_context(&host);
    claude_prompt(&root, &host, "one");
    assert!(!replied_this_turn(&root, &host).unwrap(), "no reply yet");
    host_reply(&root, &context, "Right. Now: what is c?").unwrap();
    assert!(replied_this_turn(&root, &host).unwrap());
    // The next learner message starts a turn with no reply.
    claude_prompt(&root, &host, "two");
    assert!(!replied_this_turn(&root, &host).unwrap());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn a_refused_save_after_the_reply_gets_one_retry() {
    let (root, store, host) = claude_lesson();
    claude_prompt(&root, &host, "one");
    host_reply(&root, &host_context(&host), "Right. Now: what is c?").unwrap();
    assert!(
        !refused_save_after_reply(&root, &host).unwrap(),
        "the first refusal: retry"
    );
    assert!(
        refused_save_after_reply(&root, &host).unwrap(),
        "the retry refused too: the turn ends"
    );
    // A new turn starts with its own retry.
    claude_prompt(&root, &host, "two");
    assert!(!refused_save_after_reply(&root, &host).unwrap());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
