//! Each answer the tutor marks is stored as its own row, in chat and on a
//! paper, and `magnesium attempts` lists them.
use crate::support::{TestEnvironment, run_hook_with_host_flag, v2};
use rusqlite::Connection;
use serde_json::{Value, json};

const THREAD: &str = "conv";

fn prompt(e: &TestEnvironment, prompt_id: &str, text: &str) -> String {
    let input = json!({"session_id":THREAD,"cwd":e.workspace,"hook_event_name":"UserPromptSubmit","prompt_id":prompt_id,"prompt":text});
    let notice = v2::context(&run_hook_with_host_flag(
        e,
        &e.workspace,
        "user-prompt-submit",
        input,
        "claude",
    ));
    notice
        .split_once("current event ")
        .unwrap()
        .1
        .split(['.', ' '])
        .next()
        .unwrap()
        .to_owned()
}

fn step(e: &TestEnvironment, request: Value) -> Value {
    v2::tool(e, "claude", THREAD, "learning_step", request)
}

fn refused(e: &TestEnvironment, request: Value) -> Value {
    let output = v2::tool_output(e, "claude", THREAD, "learning_step", request);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value.get("error").is_some(), "{value}");
    value
}

fn observation(event: &str, description: &str) -> Value {
    json!({"client_ref":"work","kind":"learner_work","description":description,
        "events":[{"event_id":event,"role":"learner_response"}]})
}

fn update(id: &str, observation: Value) -> Value {
    json!({"schema":"learner-model/v2","action":"update","request_id":id,
        "observations":[observation]})
}

fn store(e: &TestEnvironment) -> Connection {
    Connection::open(e.workspace.join(".magnesium/state.sqlite")).unwrap()
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn attempts(e: &TestEnvironment) -> Vec<Value> {
    let output = e.command().args(["attempts", "--json"]).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    value["attempts"].as_array().unwrap().clone()
}

#[test]
fn after_a_marked_paper_and_a_chat_lesson_every_answer_is_listed() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    v2::start(&e, "claude", THREAD);

    // A marked paper: one observation, one attempt per answered part.
    let answers = prompt(
        &e,
        "p-1",
        "MockA answers. 1: x = 3 or 1/2 (S). 2a: mean 2.5 (U). 2b: ? (G)",
    );
    let mut paper = observation(&answers, "MockA under timed conditions.");
    paper["conditions_text"] = json!("45 minutes, no notes.");
    paper["attempts"] = json!([
        {"label":"quadratic formula","question":"Solve 2x² − 7x + 3 = 0","first_try":"right","help":"none","cold":true,"confidence":"sure",
         "paper":{"name":"MockA","question":"1","marks":4,"out_of":4}},
        {"label":"expected value","question":"Find E(X) for the table","first_try":"partly","help":"none","cold":true,"confidence":"unsure",
         "paper":{"name":"MockA","question":"2a","marks":1.5,"out_of":3},
         "gap":{"type":"A","what_went_wrong":"Summed x·P(x) but dropped one term; the key needs all four."}},
        {"label":"variance","question":"Find Var(X)","first_try":"wrong","help":"none","cold":true,"confidence":"guess",
         "paper":{"name":"MockA","question":"2b","marks":0,"out_of":3},
         "gap":{"type":"K","what_went_wrong":"Wrote ?; the key needs E(X²) − E(X)²."}}
    ]);
    let marked = step(&e, update("paper", paper));
    assert_eq!(marked["update_status"], "committed", "{marked}");
    // A paper's lost marks have their gap-log redos, so they make no quick check.
    assert!(marked.get("quick_checks").is_none(), "{marked}");

    // A chat lesson: a wrong first try, then a right one after a hint.
    let wrong = prompt(&e, "p-2", "b is 7");
    let mut chat = observation(&wrong, "Took b as 7 in x² − 7x + 10, dropping its sign.");
    chat["attempt"] = json!({"label":"sign of b in the quadratic formula","question":"In x² − 7x + 10 = 0, what is b?",
        "first_try":"wrong","help":"none","cold":false,"seconds":40});
    let missed = step(&e, update("chat-1", chat));
    assert_eq!(
        missed["quick_checks"]["open_labels"],
        json!(["sign of b in the quadratic formula"]),
        "{missed}"
    );
    let right = prompt(&e, "p-3", "b is -7");
    let mut chat = observation(&right, "Read b as −7 after a hint about the sign.");
    chat["attempt"] = json!({"label":"sign of b in the quadratic formula","question":"In 3x² − 5x − 2 = 0, what is b?",
        "first_try":"right","help":"hint","cold":false,"confidence":"unsure"});
    step(&e, update("chat-2", chat));

    let listed = attempts(&e);
    let facts: Vec<_> = listed
        .iter()
        .map(|a| {
            (
                a["question"].as_str().unwrap(),
                a["paper_question"].as_str(),
                a["mark"].as_str().unwrap(),
                a["marks"].as_f64(),
                a["help"].as_str().unwrap(),
                a["confidence"].as_str(),
                a["cold"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        facts,
        [
            (
                "Solve 2x² − 7x + 3 = 0",
                Some("1"),
                "right",
                Some(4.0),
                "none",
                Some("sure"),
                true
            ),
            (
                "Find E(X) for the table",
                Some("2a"),
                "partly",
                Some(1.5),
                "none",
                Some("unsure"),
                true
            ),
            (
                "Find Var(X)",
                Some("2b"),
                "wrong",
                Some(0.0),
                "none",
                Some("guess"),
                true
            ),
            (
                "In x² − 7x + 10 = 0, what is b?",
                None,
                "wrong",
                None,
                "none",
                None,
                false
            ),
            (
                "In 3x² − 5x − 2 = 0, what is b?",
                None,
                "right",
                None,
                "hint",
                Some("unsure"),
                false
            ),
        ]
    );
    // Each row links the learner's own words and has a date.
    let conn = store(&e);
    for attempt in &listed {
        let text: String = conn
            .query_row(
                "SELECT text FROM learning_events WHERE event_id=?1",
                [attempt["response_event_id"].as_str().unwrap()],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!text.is_empty());
        assert_eq!(
            attempt["answered_at"].as_str().unwrap().len(),
            16,
            "{attempt}"
        );
    }
    assert_eq!(listed[3]["seconds"], 40);

    // The plain listing gives one line per answer.
    let output = e.command().arg("attempts").output().unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("MockA Q2a: Find E(X) for the table\n    partly 1.5/3, help none, confidence unsure, cold. Tests: expected value"),
        "{text}"
    );
    assert!(
        text.contains(
            "In 3x² − 5x − 2 = 0, what is b?\n    right, help hint, confidence unsure, not cold."
        ),
        "{text}"
    );

    // Export carries them, because the record is the learner's.
    let destination = e.root.join("export");
    let output = e
        .command()
        .arg("export")
        .arg(&destination)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let exported: Value =
        serde_json::from_slice(&std::fs::read(destination.join("learner.json")).unwrap()).unwrap();
    assert_eq!(exported["attempts"].as_array().unwrap().len(), 5);
}

#[test]
fn an_attempt_whose_facts_contradict_is_refused_and_nothing_is_stored() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    v2::start(&e, "claude", THREAD);
    let event = prompt(&e, "p-1", "b is -7");
    let chat = |attempt: Value| {
        let mut o = observation(&event, "Read b.");
        o["attempt"] = attempt;
        o
    };
    let cases = [
        (
            // Helped work stays helped.
            chat(
                json!({"label":"sign of b","question":"What is b?","first_try":"right","help":"hint","cold":true}),
            ),
            "observations[0].attempt.cold",
        ),
        (
            chat(
                json!({"label":"sign of b","question":"  ","first_try":"right","help":"none","cold":false}),
            ),
            "observations[0].attempt.question",
        ),
        (
            chat(
                json!({"label":"variance","question":"Find Var(X)","first_try":"right","help":"none","cold":true,
                    "paper":{"name":"MockA","question":"2b","marks":2,"out_of":3}}),
            ),
            "observations[0].attempt.first_try",
        ),
        (
            chat(
                json!({"label":"variance","question":"Find Var(X)","first_try":"right","help":"none","cold":true,
                    "paper":{"name":"MockA","question":"2b","marks":4,"out_of":3}}),
            ),
            "observations[0].attempt.paper.marks",
        ),
        (
            {
                let mut o = observation(&event, "Read b.");
                o["attempts"] = json!([{"label":"sign of b","question":"What is b?","first_try":"right","help":"none","cold":false}]);
                o
            },
            "observations[0].attempts[0].paper",
        ),
    ];
    for (index, (observation, path)) in cases.into_iter().enumerate() {
        let value = refused(&e, update(&format!("bad-{index}"), observation));
        assert_eq!(value["error"]["recovery"]["field_path"], path, "{value}");
    }
    // The fields are required: an attempt without question or cold is refused.
    let value = refused(
        &e,
        update(
            "old-shape",
            chat(json!({"label":"sign of b","first_try":"right","help":"none"})),
        ),
    );
    assert!(value["error"].to_string().contains("question"), "{value}");
    let conn = store(&e);
    assert_eq!(count(&conn, "SELECT count(*) FROM attempts"), 0);
    assert_eq!(count(&conn, "SELECT count(*) FROM observations"), 0);
}

#[test]
fn a_later_answer_on_a_label_the_same_day_is_never_cold() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    v2::start(&e, "claude", THREAD);
    let chat = |event: &str, question: &str, cold: bool| {
        let mut o = observation(event, "Read b with its sign.");
        o["attempt"] = json!({"label":"sign of b in the quadratic formula","question":question,
            "first_try":"right","help":"none","cold":cold});
        o
    };
    // Yesterday's answer does not stop today's first one being cold.
    let first = prompt(&e, "p-1", "b is -7");
    step(
        &e,
        update("day-0", chat(&first, "b in x² − 7x + 10?", false)),
    );
    store(&e)
        .execute(
            "UPDATE attempts SET answered_at_ms=answered_at_ms-2*86400000",
            [],
        )
        .unwrap();
    let check = prompt(&e, "p-2", "b is -5, so x = 4 or 1");
    step(
        &e,
        update("check", chat(&check, "Solve x² − 5x + 4 = 0", true)),
    );

    // A later answer on that label the same day is refused as cold, and
    // accepted as not cold.
    let later = prompt(&e, "p-3", "b is -3");
    let value = refused(&e, update("later", chat(&later, "b in x² − 3x?", true)));
    assert_eq!(
        value["error"]["recovery"]["field_path"], "observations[0].attempt.cold",
        "{value}"
    );
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Solve x² − 5x + 4 = 0"),
        "{value}"
    );
    step(&e, update("later-2", chat(&later, "b in x² − 3x?", false)));
    // Another label the same day is unaffected.
    let other = prompt(&e, "p-4", "the discriminant is 1");
    let mut o = observation(&other, "Found the discriminant.");
    o["attempt"] = json!({"label":"discriminant","question":"b² − 4ac for x² − 3x + 2?",
        "first_try":"right","help":"none","cold":true});
    step(&e, update("other", o));

    // A paper's parts on one tag, sent together, are all cold.
    let answers = prompt(&e, "p-5", "MockB: 1a x=2, 1b x=-1");
    let mut paper = observation(&answers, "MockB, timed.");
    paper["attempts"] = json!([
        {"label":"factorising","question":"Factorise x² − x − 2","first_try":"right","help":"none","cold":true,
         "paper":{"name":"MockB","question":"1a","marks":2,"out_of":2}},
        {"label":"factorising","question":"Hence solve x² − x − 2 = 0","first_try":"right","help":"none","cold":true,
         "paper":{"name":"MockB","question":"1b","marks":2,"out_of":2}}
    ]);
    step(&e, update("paper", paper));

    let colds: Vec<(String, bool)> = attempts(&e)
        .iter()
        .map(|a| {
            (
                a["question"].as_str().unwrap().to_owned(),
                a["cold"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        colds,
        [
            ("b in x² − 7x + 10?".to_owned(), false),
            ("Solve x² − 5x + 4 = 0".to_owned(), true),
            ("b in x² − 3x?".to_owned(), false),
            ("b² − 4ac for x² − 3x + 2?".to_owned(), true),
            ("Factorise x² − x − 2".to_owned(), true),
            ("Hence solve x² − x − 2 = 0".to_owned(), true),
        ]
    );
}
