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
fn claude_prompt(root: &Path, host: &HostIdentity, turn: &str) -> String {
    host_user_prompt_submit(root, host, Some(turn), None, "Is b another unknown?")
        .unwrap()
        .unwrap()
}
fn claude_stop(root: &Path, host: &HostIdentity, turn: &str, reply: &str) -> Result<(), String> {
    host_response_stop_with_delivery(root, host, &host.stored_turn_id(turn).unwrap(), Some(reply))
}
#[test]
fn claude_feedback_says_no_task_after_an_offer_a_check_in_or_a_remark() {
    let (root, store, host) = claude_lesson();
    claude_prompt(&root, &host, "0");
    for (turn, reply) in [
        "b scales x, so it stays fixed. Would you like me to explain it differently?",
        "b scales x, so it stays fixed. Shall I write the next step?",
        "b scales x, so it stays fixed. Should I solve it?",
        "Spot on! b is the coefficient of x.",
        "Sort of… b is fixed, but x varies.",
    ]
    .into_iter()
    .enumerate()
    {
        claude_stop(&root, &host, &turn.to_string(), reply).unwrap();
        let next = claude_prompt(&root, &host, &(turn + 1).to_string());
        assert!(
            next.contains(" Previous reply: no task."),
            "{reply}: {next}"
        );
    }
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn claude_prompt_notice_reports_only_the_reply_to_the_previous_prompt() {
    let (root, store, host) = claude_lesson();
    let first = claude_prompt(&root, &host, "one");
    assert!(first.contains(&reply_check::reminder()), "{first}");
    assert!(!first.contains("Previous reply"), "{first}");

    claude_stop(&root, &host, "one", &"word ".repeat(850)).unwrap();
    assert!(
        claude_stop(&root, &host, "one", "Short now. What is b?").is_err(),
        "a repeated Stop with new text keeps the first reply"
    );
    let second = claude_prompt(&root, &host, "two");
    assert!(
        second.contains(" Previous reply: 850 words, no task."),
        "{second}"
    );
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
    assert!(
        resumed.contains(" Previous reply: 300 words, no task."),
        "{resumed}"
    );
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
        first_note.contains(" Previous reply: 300 words, no task."),
        "{first_note}"
    );
    assert!(!second_note.contains("Previous reply"), "{second_note}");
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn claude_reminder_fits_with_unbound_work_pending() {
    let (root, store, host) = claude_lesson();
    store
        .conn
        .execute("UPDATE workspace_meta SET memory_generation=?1", [9999])
        .unwrap();
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
    let notice = claude_prompt(&root, &host, "one");
    assert!(notice.contains("unbound 1"), "{notice}");
    assert!(
        notice.contains("context.pending_work unbound_only:true"),
        "{notice}"
    );
    assert!(notice.contains(&reply_check::reminder()), "{notice}");
    assert!(notice.len() <= 512, "{} bytes: {notice}", notice.len());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn claude_confused_prompt_gets_the_confusion_clause_within_512_bytes() {
    let (root, store, host) = claude_lesson();
    let calm = claude_prompt(&root, &host, "one");
    assert!(!calm.contains(reply_check::ASK_WHY_CONFUSED), "{calm}");
    claude_stop(&root, &host, "one", &"word ".repeat(850)).unwrap();
    let confused = host_user_prompt_submit(
        &root,
        &host,
        Some("two"),
        None,
        "I'm confused, why is b fixed?",
    )
    .unwrap()
    .unwrap();
    for kept in [
        reply_check::ASK_WHY_CONFUSED,
        " Previous reply: 850 words, no task.",
        CLAUDE_LESSON_GUIDANCE,
        "current event e-",
    ] {
        assert!(confused.contains(kept), "{kept}: {confused}");
    }
    assert!(
        confused.len() <= 512,
        "{} bytes: {confused}",
        confused.len()
    );
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
    claude_prompt(&root, &host, "one");
    claude_stop(&root, &host, "one", &"w ".repeat(123_456)).unwrap();
    let notice = claude_prompt(&root, &host, "two");
    assert!(notice.len() <= 512, "{} bytes: {notice}", notice.len());
    for required in [
        "current event e-",
        CLAUDE_LESSON_GUIDANCE,
        "context.pending_work unbound_only:true",
        " Previous reply: 123456 words, no task.",
    ] {
        assert!(notice.contains(required), "{required}: {notice}");
    }
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn notice_clauses_give_way_in_order_and_required_ones_stay() {
    let clauses = vec![
        (Clause::Required, " lesson".into()),
        (Clause::TeachingReminder, " reminder".into()),
        (Clause::ReplyFeedback, " feedback".into()),
        (Clause::AskWhyConfused, " confused".into()),
    ];
    let fit_to = |limit| fit("state".into(), clauses.clone(), limit);
    assert_eq!(fit_to(512), "state lesson reminder feedback confused");
    assert_eq!(fit_to(35), "state lesson feedback confused");
    assert_eq!(fit_to(25), "state lesson confused");
    assert_eq!(fit_to(15), "state lesson");
    assert_eq!(fit_to(1), "state lesson");
}
#[test]
fn notice_limits_match_the_claude_plugin_hooks() {
    let hooks: serde_json::Value =
        serde_json::from_str(include_str!("../../claude-plugin/hooks/hooks.json")).unwrap();
    for (event, hook) in [
        ("SessionStart", NoticeHook::SessionStart),
        (
            "UserPromptSubmit",
            NoticeHook::UserPromptSubmit { prompt: "" },
        ),
    ] {
        assert_eq!(
            hooks["hooks"][event][0]["hooks"][0]["additionalContextLimit"],
            hook.additional_context_limit(),
            "{event}"
        );
    }
}
