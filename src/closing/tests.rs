use super::*;

fn day(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

fn answered(id: &str, key: &str, on: &str, declined: bool) -> Answered {
    Answered {
        attempt_id: id.into(),
        key: key.into(),
        day: day(on),
        declined,
    }
}

fn skill(name: &str, passed_by: &[&str], proven: bool, next_on: Option<&str>) -> Value {
    json!({
        "skill": name,
        "passed_by": passed_by,
        "proven": proven,
        "next_on": next_on,
    })
}

fn record(plan: Vec<Value>, other: Vec<Value>) -> Value {
    json!({
        "active": true,
        "plans": [{"plan": "midterm-1", "from": "record", "skills": plan}],
        "other": {"skills": other},
    })
}

const TODAY: &str = "2026-11-16";

#[test]
fn nothing_shows_when_no_answer_was_marked_today() {
    let progress = record(
        vec![skill("slope and Km", &["a1"], false, Some("2026-11-18"))],
        vec![],
    );
    // Answered on an earlier day only.
    let answers = [answered("a1", "slope and km", "2026-11-14", false)];
    assert_eq!(from_record(&progress, &answers, day(TODAY)), None);
    // A decline is the learner's choice, not an answer: a day of declines
    // ends with nothing.
    let declined = [answered("d1", "slope and km", TODAY, true)];
    assert_eq!(from_record(&progress, &declined, day(TODAY)), None);
}

#[test]
fn a_pass_today_and_its_next_day_make_two_lines() {
    let progress = record(
        vec![
            skill("slope and Km", &["a0", "a1"], false, Some("2026-11-19")),
            skill("rate equation", &[], false, Some("2026-11-17")),
            skill("Hill coefficient", &[], false, None),
        ],
        vec![],
    );
    let answers = [
        answered("a0", "slope and km", "2026-11-12", false),
        answered("a1", "slope and km", TODAY, false),
        answered("w1", "rate equation", TODAY, false),
    ];
    let closing = from_record(&progress, &answers, day(TODAY)).unwrap();
    assert_eq!(
        closing.lines(),
        [
            "Earned today: a pass on slope and Km.",
            "Can count next: rate equation from tomorrow, slope and Km from Thu 19 Nov.",
        ]
    );
}

#[test]
fn a_wrong_answer_earns_nothing_but_names_its_next_day() {
    let progress = record(
        vec![],
        vec![skill("units of a rate", &[], false, Some("2026-11-17"))],
    );
    let answers = [answered("w1", "units of a rate", TODAY, false)];
    let closing = from_record(&progress, &answers, day(TODAY)).unwrap();
    assert_eq!(
        closing.lines(),
        ["Can count next: units of a rate from tomorrow."]
    );
}

#[test]
fn a_pass_that_proves_a_skill_says_so_and_names_no_next_day() {
    let progress = record(
        vec![
            skill("rate equation", &["p1", "p2", "p3"], true, None),
            skill("slope and Km", &["s1"], false, Some("2026-11-17")),
        ],
        vec![],
    );
    let answers = [
        answered("p3", "rate equation", TODAY, false),
        answered("s1", "slope and km", TODAY, false),
    ];
    let lines = from_record(&progress, &answers, day(TODAY))
        .unwrap()
        .lines();
    assert_eq!(
        lines,
        [
            "Earned today: a pass on slope and Km, and the pass that proves rate equation.",
            "Can count next: slope and Km from tomorrow.",
        ]
    );
    // Proven alone: one line.
    let progress = record(
        vec![skill("rate equation", &["p1", "p2", "p3"], true, None)],
        vec![],
    );
    let lines = from_record(&progress, &answers[..1], day(TODAY))
        .unwrap()
        .lines();
    assert_eq!(lines, ["Earned today: the pass that proves rate equation."]);
}

#[test]
fn a_pass_lost_to_a_later_miss_today_is_not_named() {
    // Right this morning, wrong this afternoon: the skill holds no pass now.
    let progress = record(
        vec![],
        vec![skill("units of a rate", &[], false, Some("2026-11-17"))],
    );
    let answers = [
        answered("r1", "units of a rate", TODAY, false),
        answered("w1", "units of a rate", TODAY, false),
    ];
    let lines = from_record(&progress, &answers, day(TODAY))
        .unwrap()
        .lines();
    assert_eq!(lines, ["Can count next: units of a rate from tomorrow."]);
}

#[test]
fn many_skills_are_named_three_at_a_time_and_the_rest_counted() {
    let names = ["a", "b", "c", "d", "e"];
    let other: Vec<Value> = names
        .iter()
        .enumerate()
        .map(|(at, name)| {
            skill(
                name,
                &[&format!("x{at}")],
                false,
                Some(&format!("2026-11-{}", 17 + at)),
            )
        })
        .collect();
    let answers: Vec<Answered> = names
        .iter()
        .enumerate()
        .map(|(at, name)| answered(&format!("x{at}"), name, TODAY, false))
        .collect();
    let lines = from_record(&record(vec![], other), &answers, day(TODAY))
        .unwrap()
        .lines();
    assert_eq!(
        lines,
        [
            "Earned today: a pass on a, b, c and 2 more.",
            "Can count next: a from tomorrow, b from Wed 18 Nov, c from Thu 19 Nov, 2 more later.",
        ]
    );
}

#[test]
fn a_skill_in_a_plan_and_out_of_one_is_named_once_and_a_file_plan_is_skipped() {
    let mut progress = record(
        vec![skill("Slope and Km", &["a1"], false, Some("2026-11-18"))],
        vec![skill("slope  and km", &["a1"], false, Some("2026-11-18"))],
    );
    progress["plans"]
        .as_array_mut()
        .unwrap()
        .push(json!({"plan": "final", "from": "file"}));
    let answers = [answered("a1", "slope and km", TODAY, false)];
    let lines = from_record(&progress, &answers, day(TODAY))
        .unwrap()
        .lines();
    assert_eq!(
        lines,
        [
            "Earned today: a pass on Slope and Km.",
            "Can count next: Slope and Km from Wed 18 Nov.",
        ]
    );
}

#[test]
fn nothing_shows_when_there_is_no_pass_and_no_day() {
    // A paper row waiting for its repair has no next day.
    let progress = record(vec![skill("Hill coefficient", &[], false, None)], vec![]);
    let answers = [answered("w1", "hill coefficient", TODAY, false)];
    assert_eq!(from_record(&progress, &answers, day(TODAY)), None);
}

#[test]
fn only_a_committed_pause_or_stop_ends_the_sitting() {
    let control = |word: &str| json!({"action": "control", "control": word});
    let durable = json!({"durable": true});
    assert!(ends_on_control(&control("pause"), &durable));
    assert!(ends_on_control(&control("stop"), &durable));
    assert!(!ends_on_control(&control("recording_off"), &durable));
    assert!(!ends_on_control(&control("stop"), &json!({"error": {}})));
    assert!(!ends_on_control(&json!({"action": "update"}), &durable));
    assert!(is_end_request(
        &json!({"schema": "learner-model/v2", "action": "end_sitting"})
    ));
    assert!(!is_end_request(&json!({"action": "update"})));
}
