use super::*;

fn day(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

fn answered(_id: &str, key: &str, on: &str, declined: bool) -> Answered {
    Answered {
        key: key.into(),
        day: day(on),
        declined,
    }
}

/// A skill as `magnesium progress --json` prints it: its passes now and
/// when today began.
fn skill(name: &str, (passes, before): (i64, i64), proven: bool, next_on: Option<&str>) -> Value {
    json!({
        "skill": name,
        "passes": passes,
        "passes_before_today": before,
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
        vec![skill("slope and Km", (1, 1), false, Some("2026-11-18"))],
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
            skill("slope and Km", (2, 1), false, Some("2026-11-19")),
            skill("rate equation", (0, 0), false, Some("2026-11-17")),
            skill("Hill coefficient", (0, 0), false, None),
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
        vec![skill("units of a rate", (0, 0), false, Some("2026-11-17"))],
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
            skill("rate equation", (3, 2), true, None),
            skill("slope and Km", (1, 0), false, Some("2026-11-17")),
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
    let progress = record(vec![skill("rate equation", (3, 2), true, None)], vec![]);
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
        vec![skill("units of a rate", (0, 0), false, Some("2026-11-17"))],
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
                    let passes = if at == 0 { (3, 2) } else { (1, 0) };
                    skill(name, passes, at == 0, (at > 0).then_some("2026-11-17"))
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
        vec![skill("Slope and Km", (1, 0), false, Some("2026-11-18"))],
        vec![skill("slope  and km", (1, 0), false, Some("2026-11-18"))],
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
    let progress = record(vec![skill("Hill coefficient", (0, 0), false, None)], vec![]);
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

fn fixture() -> (std::path::PathBuf, Workspace) {
    let root = std::env::temp_dir().join(crate::learner::store::new_id("closing-test").unwrap());
    std::fs::create_dir_all(root.join(crate::workspace::CONTROL_DIR)).unwrap();
    std::fs::write(
        root.join(crate::workspace::CONTROL_DIR).join("config.toml"),
        "schema_version = 1\nworkspace_id = \"closing-test\"\n",
    )
    .unwrap();
    let workspace = Workspace::discover(&root).unwrap().unwrap();
    crate::learner::LearnerStore::initialize(&workspace.store_path(), "closing-test").unwrap();
    (root, workspace)
}

/// A sitting begun by a learner message and ended by conversation "a".
fn ended_sitting(workspace: &Workspace) -> Sittings {
    let mut sittings = Sittings::open(workspace).unwrap();
    sittings.note_learner_message(workspace, now_ms()).unwrap();
    sittings.end(Some("a"), now_ms()).unwrap().unwrap();
    sittings
}

#[test]
fn an_end_whose_lines_could_not_be_worked_out_is_not_claimed() {
    let (root, workspace) = fixture();
    let sittings = ended_sitting(&workspace);
    // The read fails: nothing shows, and the end is still there to show.
    assert_eq!(stop_hook_lines_with(&workspace, "a", |_| None), None);
    assert_eq!(sittings.now_state().unwrap().unwrap().shown_ms, None);
    let lines = || Some(vec!["one".to_owned(), "two".to_owned()]);
    // Another conversation's Stop hook says nothing and claims nothing.
    assert_eq!(stop_hook_lines_with(&workspace, "b", |_| lines()), None);
    assert_eq!(
        stop_hook_lines_with(&workspace, "a", |_| lines()).as_deref(),
        Some("one\ntwo")
    );
    // Once.
    assert_eq!(stop_hook_lines_with(&workspace, "a", |_| lines()), None);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_learners_next_message_sets_an_unclaimed_end_aside() {
    let (root, workspace) = fixture();
    let mut sittings = ended_sitting(&workspace);
    // The goodbye was stopped, so no Stop hook claimed the end. The next
    // message reopens the sitting, and its Stop hook has nothing to show.
    sittings.reopen().unwrap();
    assert_eq!(sittings.now_state().unwrap().unwrap().ended_ms, None);
    let lines = || Some(vec!["one".to_owned()]);
    assert_eq!(stop_hook_lines_with(&workspace, "a", |_| lines()), None);
    // Ending again in the same sitting shows the lines again.
    sittings.end(Some("a"), now_ms() + 1).unwrap();
    assert_eq!(
        stop_hook_lines_with(&workspace, "a", |_| lines()).as_deref(),
        Some("one")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_store_from_before_the_end_table_is_read_without_changing_it() {
    let (root, workspace) = fixture();
    drop(Sittings::open(&workspace).unwrap());
    let path = workspace.control().join(crate::sittings::STORE_FILE);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("DROP TABLE sitting_ends;").unwrap();
    drop(conn);
    let mut sittings = Sittings::open(&workspace).unwrap();
    sittings.note_learner_message(&workspace, now_ms()).unwrap();
    drop(sittings);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("DROP TABLE sitting_ends;").unwrap();
    drop(conn);
    let state = crate::sittings::now_state_read_only(&workspace)
        .unwrap()
        .unwrap();
    assert_eq!(state.ended_ms, None);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let has_table: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='sitting_ends')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!has_table, "a read made the table");
    std::fs::remove_dir_all(root).unwrap();
}
