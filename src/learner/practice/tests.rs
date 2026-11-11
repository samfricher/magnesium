use super::*;

fn day(n: u64) -> NaiveDate {
    days_after(NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(), n)
}

fn item(origin: Origin) -> ItemFacts {
    ItemFacts {
        item_id: "i".into(),
        origin,
        label: "km".into(),
        plan: Some("midterm-1".into()),
        row_no: Some(1),
        paper: Some("Diagnostic".into()),
        paper_question: Some("3".into()),
        logged_on: day(0),
        gap_type: Some(GapType::K),
        what_went_wrong: String::new(),
        repair: String::new(),
        confidence: Some(Confidence::Sure),
        next_question: None,
        answer_key: None,
        dropped: None,
        base_passes: 0,
        base_next_on: None,
        base_last_pass_on: None,
        base_history: String::new(),
        base_misses: Vec::new(),
        created_at_ms: 0,
        events: Vec::new(),
    }
}

fn answer(mark: FirstTry, help: AttemptHelp, cold: bool) -> AttemptFacts {
    AttemptFacts {
        attempt_id: "a".into(),
        mark,
        help,
        cold,
        slow: false,
        confidence: None,
        paper: None,
        paper_question: None,
        description: "dropped the sign".into(),
    }
}

fn event(no: i64, kind: EventKind, on: u64, attempt: Option<AttemptFacts>) -> ItemEvent {
    ItemEvent {
        event_no: no,
        kind,
        day: day(on),
        attempt,
    }
}

fn right(on: u64, no: i64) -> ItemEvent {
    event(
        no,
        EventKind::Attempt,
        on,
        Some(answer(FirstTry::Right, AttemptHelp::None, true)),
    )
}

#[test]
fn a_row_passed_on_three_allowed_days_closes_with_no_date_from_the_tutor() {
    let mut row = item(Origin::Paper);
    row.events = vec![event(
        1,
        EventKind::Made,
        0,
        Some(answer(FirstTry::Wrong, AttemptHelp::None, true)),
    )];
    let state = derive(&row, None);
    assert_eq!(
        (state.status(), state.next_on),
        ("open", None),
        "waits for its repair"
    );
    // The repair on day 0 dates the first redo on day 1, never the same day.
    row.events.push(event(2, EventKind::Repair, 0, None));
    assert_eq!(derive(&row, None).next_on, Some(day(1)));
    // A right answer before the redo is due counts neither way.
    row.events.push(right(0, 3));
    let state = derive(&row, None);
    assert_eq!(
        (state.passes, state.effect_of(3)),
        (0, Some(Effect::NoCount))
    );
    row.events.push(right(1, 4));
    let state = derive(&row, None);
    assert_eq!((state.status(), state.next_on), ("1 of 3", Some(day(4))));
    // A second pass the same day is not a second day.
    row.events.push(right(1, 5));
    assert_eq!(derive(&row, None).effect_of(5), Some(Effect::NoCount));
    row.events.push(right(4, 6));
    let state = derive(&row, None);
    assert_eq!((state.status(), state.next_on), ("2 of 3", Some(day(11))));
    row.events.push(right(11, 7));
    let state = derive(&row, None);
    assert_eq!((state.status(), state.next_on), ("closed", None));
    assert_eq!(
        state.history,
        vec![
            format!("{} pass", day(1)),
            format!("{} pass", day(4)),
            format!("{} pass", day(11))
        ]
    );
}

#[test]
fn a_failed_redo_starts_the_row_again_and_waits_for_a_new_repair() {
    let mut row = item(Origin::Paper);
    row.events = vec![
        event(1, EventKind::Made, 0, None),
        event(2, EventKind::Repair, 0, None),
        right(1, 3),
    ];
    let mut miss = answer(FirstTry::Partly, AttemptHelp::None, true);
    miss.confidence = Some(Confidence::Guess);
    row.events.push(event(4, EventKind::Attempt, 4, Some(miss)));
    let state = derive(&row, None);
    assert_eq!(state.effect_of(4), Some(Effect::Failed));
    assert_eq!((state.status(), state.next_on), ("open", None));
    assert_eq!(state.history, vec![format!("{} fail", day(4))]);
    assert_eq!(state.confidence, Some(Confidence::Guess));
    // A hinted answer on a due row is not a pass, so it fails it too.
    row.events.push(event(5, EventKind::Repair, 5, None));
    row.events.push(event(
        6,
        EventKind::Attempt,
        6,
        Some(answer(FirstTry::Right, AttemptHelp::Hint, false)),
    ));
    assert_eq!(derive(&row, None).effect_of(6), Some(Effect::Failed));
}

#[test]
fn the_gaps_shorten_before_the_test_and_a_row_can_run_out_of_days() {
    let mut row = item(Origin::Paper);
    row.events = vec![
        event(1, EventKind::Made, 0, None),
        event(2, EventKind::Repair, 0, None),
        right(1, 3),
    ];
    // Test on day 6: two redos left need days 4 and 5 at the latest.
    let state = derive(&row, Some(day(6)));
    assert_eq!(state.next_on, Some(day(4)));
    // Test on day 3: the gap never shortens below one day.
    assert_eq!(derive(&row, Some(day(3))).next_on, Some(day(2)));
    // A test already past shortens nothing.
    assert_eq!(derive(&row, Some(day(1))).next_on, Some(day(4)));
    assert_eq!(derive(&row, Some(day(0))).next_on, Some(day(4)));
    // No test, or one far off: the plain gaps.
    assert_eq!(derive(&row, None).next_on, Some(day(4)));
    assert_eq!(derive(&row, Some(day(60))).next_on, Some(day(4)));
}

#[test]
fn a_slow_answer_never_passes_a_type_s_row() {
    let mut row = item(Origin::Paper);
    row.gap_type = Some(GapType::S);
    let mut slow = answer(FirstTry::Right, AttemptHelp::None, true);
    slow.slow = true;
    row.events = vec![
        event(1, EventKind::Made, 0, None),
        event(2, EventKind::Repair, 0, None),
        event(3, EventKind::Attempt, 1, Some(slow.clone())),
    ];
    assert_eq!(derive(&row, None).effect_of(3), Some(Effect::Failed));
    row.gap_type = Some(GapType::K);
    assert_eq!(derive(&row, None).effect_of(3), Some(Effect::Passed));
}

#[test]
fn a_row_brought_in_keeps_its_passes_and_date() {
    let mut row = item(Origin::Paper);
    row.base_passes = 1;
    row.base_next_on = Some(day(3));
    row.base_last_pass_on = Some(day(0));
    row.base_history = "2026-11-02 pass".into();
    let state = derive(&row, None);
    assert_eq!((state.status(), state.next_on), ("1 of 3", Some(day(3))));
    row.events = vec![right(3, 1)];
    let state = derive(&row, None);
    assert_eq!((state.status(), state.next_on), ("2 of 3", Some(day(10))));
    assert_eq!(
        state.history,
        vec!["2026-11-02 pass".to_owned(), format!("{} pass", day(3))]
    );
}

#[test]
fn a_quick_check_follows_the_rules_of_26() {
    let mut check = item(Origin::Chat);
    check.events = vec![event(
        1,
        EventKind::Made,
        0,
        Some(answer(FirstTry::Wrong, AttemptHelp::None, false)),
    )];
    let state = derive(&check, None);
    assert_eq!((state.next_on, state.misses.len()), (Some(day(1)), 1));
    // Helped right on a due check: no pass and no fail.
    check.events.push(event(
        2,
        EventKind::Attempt,
        1,
        Some(answer(FirstTry::Right, AttemptHelp::Hint, false)),
    ));
    let state = derive(&check, None);
    assert_eq!(
        (state.effect_of(2), state.passes),
        (Some(Effect::Helped), 0)
    );
    // A decline is free.
    check.events.push(event(
        3,
        EventKind::Attempt,
        1,
        Some(answer(FirstTry::Declined, AttemptHelp::None, false)),
    ));
    assert_eq!(derive(&check, None).next_on, Some(day(1)));
    check.events.push(right(1, 4));
    let state = derive(&check, None);
    assert_eq!((state.passes, state.next_on), (1, Some(day(4))));
    // A miss before the next is due starts it over for the next day.
    check.events.push(event(
        5,
        EventKind::Attempt,
        2,
        Some(answer(FirstTry::Wrong, AttemptHelp::None, false)),
    ));
    let state = derive(&check, None);
    assert_eq!(
        (state.effect_of(5), state.passes, state.next_on),
        (Some(Effect::Restarted), 0, Some(day(3)))
    );
}

#[test]
fn a_right_answer_reached_with_a_hint_is_asked_cold_the_next_day_and_keeps_its_passes() {
    for help in [AttemptHelp::Hint, AttemptHelp::Step] {
        // Made by the helped answer itself.
        let mut check = item(Origin::Chat);
        check.events = vec![event(
            1,
            EventKind::Made,
            0,
            Some(answer(FirstTry::Right, help, false)),
        )];
        let state = derive(&check, None);
        assert_eq!((state.passes, state.next_on), (0, Some(day(1))), "{help:?}");
        assert!(state.misses.is_empty(), "a helped answer is not a miss");
        // Asked cold the next day, it passes, and is next due 3 days on.
        check.events.push(right(1, 2));
        assert_eq!(derive(&check, None).passes, 1);
        // Helped again before that: back to the next day, its pass kept.
        check.events.push(event(
            3,
            EventKind::Attempt,
            2,
            Some(answer(FirstTry::Right, help, false)),
        ));
        let state = derive(&check, None);
        assert_eq!(
            (state.effect_of(3), state.passes, state.next_on),
            (Some(Effect::Helped), 1, Some(day(3)))
        );
        // On the due day, an answer after a hint earns no pass and stays due.
        check.events.push(event(
            4,
            EventKind::Attempt,
            3,
            Some(answer(FirstTry::Right, help, false)),
        ));
        let state = derive(&check, None);
        assert_eq!(
            (state.effect_of(4), state.passes, state.next_on),
            (Some(Effect::Helped), 1, Some(day(3)))
        );
    }
}

#[test]
fn i_dont_know_brings_a_check_the_next_day_and_a_declined_due_check_stays_free() {
    // "I don't know" on a new task makes a check for the next day.
    let mut check = item(Origin::Chat);
    check.events = vec![event(
        1,
        EventKind::Made,
        0,
        Some(answer(FirstTry::Declined, AttemptHelp::None, false)),
    )];
    let state = derive(&check, None);
    assert_eq!((state.passes, state.next_on), (0, Some(day(1))));
    assert!(state.misses.is_empty(), "a decline is not a miss");
    // Declined when due: free, still due.
    check.events.push(event(
        2,
        EventKind::Attempt,
        1,
        Some(answer(FirstTry::Declined, AttemptHelp::None, false)),
    ));
    let state = derive(&check, None);
    assert_eq!(
        (state.effect_of(2), state.next_on),
        (Some(Effect::Declined), Some(day(1)))
    );
    // Passed, then "I don't know" before the next is due: the next day again.
    check.events.push(right(1, 3));
    check.events.push(event(
        4,
        EventKind::Attempt,
        2,
        Some(answer(FirstTry::Declined, AttemptHelp::None, false)),
    ));
    let state = derive(&check, None);
    assert_eq!(
        (state.effect_of(4), state.passes, state.next_on),
        (Some(Effect::Declined), 0, Some(day(3)))
    );
}
