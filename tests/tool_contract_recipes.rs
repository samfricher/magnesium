//! Run the shipped recovery recipes at the actual CLI and MCP boundaries.
use rusqlite::Connection;
use serde_json::{Value, json};
use support::{TestEnvironment, mcp::McpClient, v2};

struct Client<'a> {
    environment: &'a TestEnvironment,
    mcp: Option<McpClient>,
    id: u64,
}
impl<'a> Client<'a> {
    fn new(environment: &'a TestEnvironment, mcp: bool) -> Self {
        let mcp = mcp.then(|| {
            let mut client =
                McpClient::spawn_with_env(environment, true, &[("CODEX_THREAD_ID", "recipes")]);
            client.initialize();
            client
        });
        Self {
            environment,
            mcp,
            id: 1,
        }
    }
    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.id += 1;
        if let Some(client) = &mut self.mcp {
            let response = client.request(
                self.id,
                "tools/call",
                json!({"name":name,"arguments":arguments}),
            );
            parse_response(response["result"]["content"][0]["text"].as_str().unwrap())
        } else {
            let response = v2::tool_output(self.environment, "codex", "recipes", name, arguments);
            serde_json::from_slice(&response.stdout).unwrap()
        }
    }
    fn update(&mut self, request: Value) -> Value {
        let result = self.call("learning_step", request);
        assert_eq!(result["update_status"], "committed", "{result}");
        result
    }
    fn close(self) {
        if let Some(client) = self.mcp {
            client.close_cleanly();
        }
    }
}
fn parse_response(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| json!({"error":{"message":text}}))
}
fn recipe(text: &str, name: &str) -> Value {
    let marker = format!("<!-- executable-recipe: {name} -->\n```json\n");
    serde_json::from_str(
        text.split_once(&marker)
            .unwrap()
            .1
            .split_once("\n```")
            .unwrap()
            .0,
    )
    .unwrap()
}
fn replace(value: &mut Value, name: &str, replacement: &Value) {
    match value {
        Value::String(text) if text == name => *value = replacement.clone(),
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| replace(item, name, replacement)),
        Value::Object(items) => items
            .values_mut()
            .for_each(|item| replace(item, name, replacement)),
        _ => (),
    }
}
fn semantic_state(db: &Connection) -> Vec<String> {
    [
        "SELECT json_array(memory_id,revision,current_basis_json,state_text) FROM learner_memories ORDER BY memory_id",
        "SELECT json_array(session_id,revision,checkpoint_json) FROM learning_sessions ORDER BY session_id",
        "SELECT json_array(observation_id,description) FROM observations ORDER BY observation_id",
        "SELECT json_array(memory_id,revision,snapshot_json) FROM memory_changes ORDER BY memory_id,revision",
    ].iter().flat_map(|sql| {
        let mut statement = db.prepare(sql).unwrap();
        statement.query_map([], |row| row.get::<_, String>(0)).unwrap().map(Result::unwrap).collect::<Vec<_>>()
    }).collect()
}
fn run(mcp: bool) {
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&environment);
    let mut client = Client::new(&environment, mcp);
    let served = client.call("context", json!({"action":"instructions","host":"codex","stage":"interpret_and_repair","operation":"tool_recipes"}));
    let text = served["text"].as_str().unwrap();
    let events: Vec<_> = (0..9).map(|i| json!({"external_event_key":format!("input-{i}"),"speaker":"learner","kind":"message","text":format!("Reported coefficient attempt {i}")})).collect();
    let start = client.update(json!({"schema":"learner-model/v2","action":"update","request_id":"start","session":{"operation":"start","client_ref":"lesson","goal":"Coefficient roles"},"reported_events":events}));
    let session = &start["references"]["lesson"];
    let observations: Vec<_> = (0..9).map(|i| json!({"client_ref":format!("work-{i}"),"kind":"learner_work","description":format!("Reported attempt {i}; correctness not assessed"),"events":[{"event_id":start["captured_event_ids"][i],"role":"learner_response"}]})).collect();
    let basis: Vec<_> = (0..8)
        .map(|i| json!({"new_observation":format!("work-{i}")}))
        .collect();
    let checkpoint = json!({"current_problem":"Coefficient roles","working_diagnosis":"Reported work, unassessed","next_action":"Ask about the work","plan_text":"Keep this plan","memory_refs":[],"pending_task":null});
    let created = client.update(json!({"schema":"learner-model/v2","action":"update","request_id":"create","session":{"operation":"use","session_id":session,"expected_revision":1},"observations":observations,"memory_edits":[{"operation":"create","client_ref":"memory","record":{"title":"Coefficient roles","scope":"Quadratics","summary":"Reported attempts","state_text":"Reported attempts; correctness not assessed","current_basis":basis},"reason":"Keep reported work","change_basis":[{"new_observation":"work-0"}]}],"checkpoint":checkpoint}));
    let memory = &created["references"]["memory"];
    let newer_basis: Vec<_> = (1..=8)
        .map(|i| json!({"observation_id":created["references"][format!("work-{i}")]}))
        .collect();
    let latest = client.update(json!({"schema":"learner-model/v2","action":"update","request_id":"include-ninth-work","session":{"operation":"use","session_id":session,"expected_revision":created["revisions"][session.as_str().unwrap()]},"memory_edits":[{"operation":"revise","memory_id":memory,"expected_revision":1,"record":{"title":"Coefficient roles","scope":"Quadratics","summary":"Latest reported attempts","state_text":"Latest reported attempts; correctness not assessed","current_basis":newer_basis},"change_kind":"new_observation","reason":"Include the ninth reported attempt","change_basis":[{"observation_id":created["references"]["work-8"]}]}]}));
    let db = Connection::open(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("state.sqlite"),
    )
    .unwrap();
    let mut selection = recipe(text, "reselect_eight_supports");
    replace(&mut selection, "$SESSION_ID", session);
    replace(&mut selection, "$MEMORY_ID", memory);
    selection["session"]["expected_revision"] =
        latest["revisions"][session.as_str().unwrap()].clone();
    selection["memory_edits"][0]["expected_revision"] =
        latest["revisions"][memory.as_str().unwrap()].clone();
    for i in 1..=8 {
        replace(
            &mut selection,
            &format!("$O{i}"),
            &created["references"][format!("work-{}", i - 1)],
        );
    }
    replace(
        &mut selection,
        "$CURRENT_SUMMARY",
        &json!("Reported attempts"),
    );
    replace(
        &mut selection,
        "$CURRENT_DESCRIPTION",
        &json!("Reported work remains unassessed"),
    );
    let mut invalid = selection.clone();
    invalid["request_id"] = json!("too-many-supports");
    invalid["memory_edits"][0]["record"]["current_basis"]
        .as_array_mut()
        .unwrap()
        .push(json!({"observation_id":created["references"]["work-8"]}));
    invalid["reported_events"] = json!([{"external_event_key":"retained-input","speaker":"learner","kind":"message","text":"Please keep this response if the update fails."}]);
    invalid["observations"] = json!([{"client_ref":"rolled-back-work","kind":"learner_work","description":"This observation must roll back with the rejected edit","events":[{"event_id":start["captured_event_ids"][0],"role":"learner_response"}]}]);
    invalid["checkpoint"] = json!({"current_problem":"Rejected problem","working_diagnosis":"Do not save this diagnosis","next_action":"Do not replace the existing plan","memory_refs":[]});
    let before = semantic_state(&db);
    let rejected = client.call("learning_step", invalid);
    assert!(
        rejected["error"]["message"]
            .as_str()
            .unwrap()
            .contains("current_basis"),
        "{rejected}"
    );
    assert_eq!(semantic_state(&db), before);
    assert_eq!(v2::count(&environment, "learning_events"), 10);
    let captured = rejected["error"]["captured_event_ids"][0].clone();
    assert!(captured.is_string(), "{rejected}");
    selection["turn_ref"] = captured.clone();
    selection["event_dispositions"] = json!([{"event_id":captured,"outcome":"no_change"}]);
    let committed = client.update(selection.clone());
    let after = semantic_state(&db);
    let replay = client.update(selection);
    assert_eq!(replay["references"], committed["references"]);
    assert_eq!(semantic_state(&db), after);
    assert_eq!(v2::count(&environment, "learning_events"), 10);
    assert_eq!(v2::count(&environment, "observations"), 9);
    let old: String = db
        .query_row(
            "SELECT snapshot_json FROM memory_changes WHERE memory_id=?1 AND revision=2",
            [memory.as_str().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(old.contains(created["references"]["work-8"].as_str().unwrap()));
    let current: String = db
        .query_row(
            "SELECT current_basis_json FROM learner_memories WHERE memory_id=?1",
            [memory.as_str().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    let current: Value = serde_json::from_str(&current).unwrap();
    assert_eq!(current.as_array().unwrap().len(), 8);
    assert!(
        !current
            .as_array()
            .unwrap()
            .contains(&created["references"]["work-8"])
    );

    let preserved: String = db
        .query_row(
            "SELECT checkpoint_json FROM learning_sessions WHERE session_id=?1",
            [session.as_str().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&preserved).unwrap(),
        checkpoint
    );

    let preference_capture = client.update(json!({"schema":"learner-model/v2","action":"update","request_id":"preference-capture","reported_events":[{"external_event_key":"preference","speaker":"learner","kind":"message","text":"Please ask one question at a time."}]}));
    let mut preference = recipe(text, "observe_preference");
    replace(
        &mut preference,
        "$EVENT_ID",
        &preference_capture["captured_event_ids"][0],
    );
    let mut bad_kind = preference.clone();
    bad_kind["request_id"] = json!("bad-preference-kind");
    bad_kind["observations"][0]["kind"] = json!("learner_preference");
    let before = semantic_state(&db);
    let rejected = client.call("learning_step", bad_kind);
    assert_eq!(
        rejected["error"]["recovery"]["field_path"],
        "observations[0].kind"
    );
    assert_eq!(semantic_state(&db), before);
    client.update(preference);

    let mut image = recipe(text, "report_image_transcription");
    replace(&mut image, "$IMAGE_PATH", &json!("work/answer.png"));
    for field in ["source", "image_path"] {
        let mut bad_image = image.clone();
        bad_image["request_id"] = json!(format!("bad-image-{field}"));
        bad_image["reported_events"][0]["metadata"][field] = json!("answer.png");
        let count = v2::count(&environment, "learning_events");
        let rejected = client.call("learning_step", bad_image);
        assert!(rejected["error"].is_object(), "{rejected}");
        assert_eq!(v2::count(&environment, "learning_events"), count);
    }
    let image_result = client.update(image);
    let image_id = &image_result["captured_event_ids"][0];
    let (origin, metadata): (String, String) = db
        .query_row(
            "SELECT origin,metadata_json FROM learning_events WHERE event_id=?1",
            [image_id.as_str().unwrap()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(origin, "model_reported");
    let metadata: Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(metadata["attachments"][0]["path"], "work/answer.png");
    assert!(
        metadata["coverage"]
            .as_str()
            .unwrap()
            .contains("not verified")
    );
    let mut no_change = recipe(text, "no_change");
    replace(&mut no_change, "$EVENT_ID", image_id);
    let mut bad_disposition = no_change.clone();
    bad_disposition["request_id"] = json!("bad-reason");
    bad_disposition["event_dispositions"][0]["reason"] = json!("Unneeded field");
    assert_eq!(
        client.call("learning_step", bad_disposition)["error"]["recovery"]["field_path"],
        "event_dispositions[0].reason"
    );
    client.update(no_change);
    assert_eq!(v2::count(&environment, "observations"), 10);
    client.close();
}
#[test]
fn contract_recipes_and_recovery_through_cli() {
    run(false);
}
#[test]
fn contract_recipes_and_recovery_through_mcp() {
    run(true);
}

#[test]
fn default_prepare_budget_preserves_the_full_checkpoint_after_small_budget_rejection() {
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&environment);
    let mut client = Client::new(&environment, true);
    let start = client.update(json!({"schema":"learner-model/v2","action":"update","request_id":"budget-start","session":{"operation":"start","client_ref":"lesson","goal":"Keep the current task"}}));
    let session = &start["references"]["lesson"];
    let checkpoint = json!({"current_problem":"Problem details. ".repeat(80),"working_diagnosis":"Observed conditions. ".repeat(70),"next_action":"Preserve the pending question. ".repeat(50),"plan_text":"Plan details. ".repeat(200),"memory_refs":[],"pending_task":null});
    client.update(json!({"schema":"learner-model/v2","action":"update","request_id":"save-plan","session":{"operation":"use","session_id":session,"expected_revision":1},"checkpoint":checkpoint}));
    let db = Connection::open(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("state.sqlite"),
    )
    .unwrap();
    let before = semantic_state(&db);
    let small = client.call(
        "context",
        json!({"schema":"learner-model/v2","action":"prepare","session_id":session,"max_bytes":1800}),
    );
    assert!(
        small["error"]["message"]
            .as_str()
            .unwrap()
            .contains("budget"),
        "{small}"
    );
    assert_eq!(semantic_state(&db), before);
    let prepared = client.call(
        "context",
        json!({"schema":"learner-model/v2","action":"prepare","session_id":session}),
    );
    assert!(prepared.get("error").is_none(), "{prepared}");
    assert!(serde_json::to_vec(&prepared).unwrap().len() <= 24 * 1024);
    let encoded = prepared.to_string();
    for field in [
        "current_problem",
        "working_diagnosis",
        "next_action",
        "plan_text",
    ] {
        assert!(
            encoded.contains(checkpoint[field].as_str().unwrap()),
            "missing full {field}"
        );
    }
    assert_eq!(semantic_state(&db), before);
    client.close();
}

#[test]
fn cold_native_capture_rejects_changed_bytes_and_fallback_keeps_its_origin() {
    for (index, altered) in ["π = 2", "π  = 2\n", "pi = 2\n"].iter().enumerate() {
        let environment = TestEnvironment::from_fixture("basic").unwrap();
        v2::init(&environment);
        let original = "π = 2\n";
        let notice = v2::context(&v2::prompt(
            &environment,
            "codex",
            "recipes",
            Some("cold-turn"),
            original,
        ));
        let ticket = notice
            .split_once("native_capture.ticket=")
            .unwrap()
            .1
            .split(';')
            .next()
            .unwrap();
        let mut client = Client::new(&environment, true);
        let failed = client.call("learning_step", json!({"schema":"learner-model/v2","action":"update","request_id":"altered-native","session":{"operation":"start","client_ref":"lesson","goal":"Read learner work"},"native_capture":{"ticket":ticket,"text":altered}}));
        assert!(failed["error"].is_object(), "{failed}");
        assert_eq!(v2::count(&environment, "learning_events"), 0);
        let request = if index == 0 {
            json!({"schema":"learner-model/v2","action":"update","request_id":"exact-native","session":{"operation":"start","client_ref":"lesson","goal":"Read learner work"},"native_capture":{"ticket":ticket,"text":original}})
        } else {
            json!({"schema":"learner-model/v2","action":"update","request_id":"reported-fallback","session":{"operation":"start","client_ref":"lesson","goal":"Read learner work"},"reported_events":[{"external_event_key":"fallback","speaker":"learner","kind":"message","text":original,"metadata":{"origin":"model_reported","coverage":"Explicit fallback after native verification rejection"}}]})
        };
        let result = client.update(request.clone());
        client.update(request);
        assert_eq!(v2::count(&environment, "learning_events"), 1);
        let db = Connection::open(
            environment
                .workspace
                .join(support::CONTROL_DIR)
                .join("state.sqlite"),
        )
        .unwrap();
        let (text, origin): (String, String) = db
            .query_row(
                "SELECT text,origin FROM learning_events WHERE event_id=?1",
                [result["captured_event_ids"][0].as_str().unwrap()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(text, original);
        assert_eq!(
            origin,
            if index == 0 {
                "host_observed"
            } else {
                "model_reported"
            }
        );
        client.close();
    }
}
