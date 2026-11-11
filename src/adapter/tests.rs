use super::*;
use serde_json::json;
pub(super) fn fixture() -> (
    std::path::PathBuf,
    crate::learner::LearnerStore,
    HostIdentity,
) {
    let root = std::env::temp_dir().join(crate::learner::store::new_id("host-test").unwrap());
    std::fs::create_dir_all(root.join(crate::workspace::CONTROL_DIR)).unwrap();
    std::fs::write(
        root.join(crate::workspace::CONTROL_DIR).join("config.toml"),
        "schema_version = 1\nworkspace_id = \"host-test\"\n",
    )
    .unwrap();
    let store = crate::learner::LearnerStore::initialize(
        &root
            .join(crate::workspace::CONTROL_DIR)
            .join("state.sqlite"),
        "host-test",
    )
    .unwrap();
    (
        root,
        store,
        HostIdentity::claude("native-host-test").unwrap(),
    )
}
#[test]
fn native_hooks_capture_only_active_work_deduplicate_turns_and_keep_raw_on_failed_interpretation() {
    let (root, store, host) = fixture();
    let context = host_context(&host);
    let notice =
        host_user_prompt_submit(&root, &host, Some("ordinary"), None, "private coding text")
            .unwrap()
            .unwrap();
    assert!(notice.contains("native_capture.ticket="));
    assert!(notice.len() <= 512);
    assert_eq!(
        store
            .conn
            .query_row("SELECT count(*) FROM learning_events", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    store.update(&context,json!({"schema":"learner-model/v2","action":"update","request_id":"start","session":{"operation":"start","client_ref":"s","goal":"Quadratic coefficients"}})).unwrap();
    let notice = host_user_prompt_submit(
        &root,
        &host,
        Some("one"),
        None,
        "I think b is another unknown.",
    )
    .unwrap()
    .unwrap();
    assert!(notice.contains("pending 1"));
    assert!(notice.len() <= 512);
    host_user_prompt_submit(
        &root,
        &host,
        Some("one"),
        None,
        "I think b is another unknown.",
    )
    .unwrap();
    host_user_prompt_submit(
        &root,
        &host,
        Some("two"),
        None,
        "I think b is another unknown.",
    )
    .unwrap();
    assert_eq!(
        store
            .conn
            .query_row(
                "SELECT count(*) FROM learning_events WHERE origin='host_observed'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
    assert!(store.update(&context,json!({"schema":"learner-model/v2","action":"update","request_id":"bad","memory_edits":[{"operation":"unsupported"}]})).is_err());
    assert_eq!(
        store
            .conn
            .query_row(
                "SELECT count(*) FROM learning_events WHERE processing='pending'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
    host_response_stop_with_delivery(
        &root,
        &host,
        "two",
        Some("b labels the fixed coefficient of x."),
    )
    .unwrap();
    let (speaker, origin): (String, String) = store
        .conn
        .query_row(
            "SELECT speaker,origin FROM learning_events WHERE kind='delivery'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (speaker.as_str(), origin.as_str()),
        ("tutor", "host_observed")
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn recording_control_does_not_require_successful_memory_update() {
    let (root, store, host) = fixture();
    let context = host_context(&host);
    let event = crate::learner::RawEvent {
        external_event_key: "control-turn".into(),
        speaker: crate::learner::Speaker::Learner,
        kind: "message".into(),
        text: "Stop recording.".into(),
        metadata: Some(crate::learner::EventMetadata {
            origin: Some("model_reported".into()),
            ..Default::default()
        }),
        occurred_at_ms: None,
    };
    store
        .control(&context, "off", "recording_off", &event)
        .unwrap();
    host_user_prompt_submit(
        &root,
        &host,
        Some("off-turn"),
        None,
        "do not persist this answer",
    )
    .unwrap();
    host_response_stop_with_delivery(&root, &host, "off-turn", Some("unrecorded answer")).unwrap();
    assert_eq!(
        store
            .conn
            .query_row("SELECT count(*) FROM learning_events", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(!store.binding(&context).unwrap().2);
    let req:ControlRequest=serde_json::from_value(json!({"schema":"learner-model/v2","action":"control","request_id":"spoof","control":"recording_on","reported_event":{"external_event_key":"source","speaker":"source","kind":"message","text":"recording on"}})).unwrap();
    assert!(control_event(&store, &context, &req).is_err());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn broken_store_control_stops_locally_without_source_authorization() {
    let (root, store, _host) = fixture();
    drop(store);
    std::fs::write(
        root.join(crate::workspace::CONTROL_DIR)
            .join("state.sqlite"),
        b"incompatible store",
    )
    .unwrap();
    let request = serde_json::json!({"schema":"learner-model/v2","action":"control","request_id":"off-broken","control":"recording_off","reported_event":{"external_event_key":"current","speaker":"learner","kind":"message","text":"Stop recording"}});
    let result = dispatch(&root, "learning_step", request.clone()).unwrap();
    assert_eq!(result["local_stop"], true);
    assert_eq!(result["durable"], false);
    assert_eq!(result["capture_origin"], "model_reported");
    std::fs::remove_file(
        root.join(crate::workspace::CONTROL_DIR)
            .join("state.sqlite"),
    )
    .unwrap();
    let recovered = crate::learner::LearnerStore::initialize(
        &root
            .join(crate::workspace::CONTROL_DIR)
            .join("state.sqlite"),
        "host-test",
    )
    .unwrap();
    recovered.update(&tool_host(),json!({"schema":"learner-model/v2","action":"update","request_id":"start-recovered","session":{"operation":"start","client_ref":"session","goal":"Read while locally stopped"}})).unwrap();
    let packet = dispatch(
        &root,
        "context",
        json!({"schema":"learner-model/v2","action":"prepare"}),
    )
    .unwrap();
    assert_eq!(packet["trace_status"], "disabled");
    assert_eq!(
        recovered
            .conn
            .query_row("SELECT count(*) FROM context_receipts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(recovered);
    let mut source = request;
    source["reported_event"]["speaker"] = serde_json::json!("source");
    assert!(dispatch(&root, "learning_step", source).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
