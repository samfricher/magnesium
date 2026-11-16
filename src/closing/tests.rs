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
const LONG: &str = "when to use the t distribution rather than the normal, for a small sample whose standard deviation is not known";

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
fn a_pass_today_and_the_first_day_another_can_count_make_two_lines() {
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
    // The next line names the first day only; slope and Km's later day is
    // on the workspace's page.
    assert_eq!(
        closing.lines(),
        [
            "Earned today: a pass on slope and Km.",
            "Can count from tomorrow: rate equation.",
        ]
    );
    assert_eq!(closing.next.len(), 2, "the data keeps every day");
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
        ["Can count from tomorrow: units of a rate."]
    );
}

#[test]
fn a_skill_proved_today_leads_and_the_other_passes_are_counted() {
    let progress = record(
        vec![
            skill("rate equation", &["p1", "p2", "p3"], true, None),
            skill("slope and Km", &["s1"], false, Some("2026-11-17")),
        ],
        vec![],
    );
    let answers = [
        answered("s1", "slope and km", TODAY, false),
        answered("p3", "rate equation", TODAY, false),
    ];
    let lines = from_record(&progress, &answers, day(TODAY))
        .unwrap()
        .lines();
    assert_eq!(
        lines,
        [
            "Proven today: rate equation, and a pass on 1 more.",
            "Can count from tomorrow: slope and Km.",
        ]
    );
    // Proven alone: one line.
    let progress = record(
        vec![skill("rate equation", &["p1", "p2", "p3"], true, None)],
        vec![],
    );
    let lines = from_record(&progress, &answers[1..], day(TODAY))
        .unwrap()
        .lines();
    assert_eq!(lines, ["Proven today: rate equation."]);
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
    assert_eq!(lines, ["Can count from tomorrow: units of a rate."]);
}

#[test]
fn every_line_fits_whatever_the_names_and_their_number() {
    let names = [
        "pooled variance",
        "degrees of freedom of a t statistic",
        LONG,
        "margin of error",
        "sign of b in the quadratic formula",
        "reading a confidence interval",
    ];
    for count in 1..=names.len() {
        for start in 0..names.len() {
            let chosen: Vec<&str> = (0..count)
                .map(|at| names[(start + at) % names.len()])
                .collect();
            let other: Vec<Value> = chosen
                .iter()
                .enumerate()
                .map(|(at, name)| {
                    skill(
                        name,
                        &[&format!("x{at}")],
                        at == 0,
                        (at > 0).then_some("2026-11-17"),
                    )
                })
                .collect();
            let answers: Vec<Answered> = chosen
                .iter()
                .enumerate()
                .map(|(at, name)| answered(&format!("x{at}"), &label_key(name), TODAY, false))
                .collect();
            let closing = from_record(&record(vec![], other), &answers, day(TODAY)).unwrap();
            for line in closing.lines() {
                assert!(
                    line.chars().count() <= LINE_CHARS,
                    "{} chars: {line}",
                    line.chars().count()
                );
            }
        }
    }
}

#[test]
fn names_that_fit_are_named_and_the_rest_counted() {
    let short = |names: &[&str]| -> Vec<String> { names.iter().map(|n| (*n).to_owned()).collect() };
    assert_eq!(
        fit(
            "Earned today: a pass on ",
            &short(&["pooled variance", "margin of error"]),
            "."
        ),
        "Earned today: a pass on pooled variance and margin of error."
    );
    assert_eq!(
        fit(
            "Earned today: a pass on ",
            &short(&[
                "pooled variance",
                "margin of error",
                "reading a confidence interval"
            ]),
            "."
        ),
        "Earned today: a pass on pooled variance and 2 more."
    );
    // A name too long for the line is cut at a word, and says so.
    assert_eq!(
        fit(
            "Earned today: a pass on ",
            &short(&[LONG, "margin of error"]),
            "."
        ),
        "Earned today: a pass on when to use the t… and 1 more."
    );
    assert_eq!(
        fit("Can count from Thu 19 Nov: ", &short(&[LONG]), "."),
        "Can count from Thu 19 Nov: when to use the t distribution…"
    );
    assert_eq!(clip_at_word("supercalifragilistic", 5), "super");
    assert_eq!(clip_at_word("normal, for a small", 8), "normal");
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
            "Can count from Wed 18 Nov: Slope and Km.",
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
