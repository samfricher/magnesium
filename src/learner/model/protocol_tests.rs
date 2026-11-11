use super::*;
use serde_json::json;

fn advertised_choices_decode<T: serde::de::DeserializeOwned + schemars::JsonSchema + Serialize>() {
    let schema = serde_json::to_value(schemars::schema_for!(T)).unwrap();
    let choices = schema
        .get("enum")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| vec![schema["const"].clone()]);
    assert!(!choices.is_empty());
    for choice in choices {
        let decoded: T = serde_json::from_value(choice.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), choice);
    }
    assert!(serde_json::from_value::<T>(json!("unsupported")).is_err());
}

#[test]
fn every_advertised_enum_value_uses_the_actual_decoder() {
    advertised_choices_decode::<SchemaVersion>();
    advertised_choices_decode::<UpdateAction>();
    advertised_choices_decode::<UpdateMode>();
    advertised_choices_decode::<Speaker>();
    advertised_choices_decode::<ObservationKind>();
    advertised_choices_decode::<ObservationRole>();
    advertised_choices_decode::<ChangeKind>();
    advertised_choices_decode::<Disposition>();
    advertised_choices_decode::<ReviewStatus>();
    advertised_choices_decode::<Delivery>();
    advertised_choices_decode::<FirstTry>();
    advertised_choices_decode::<AttemptHelp>();
}

#[test]
fn decoder_rejects_invalid_closed_semantic_choices() {
    assert!(
        serde_json::from_value::<ObservationEvent>(json!({"event_id":"e","role":"answer"}))
            .is_err()
    );
    assert!(
        serde_json::from_value::<EventDisposition>(json!({"event_id":"e","outcome":"ignored"}))
            .is_err()
    );
    assert!(
        serde_json::from_value::<ObservationInput>(
            json!({"client_ref":"o","kind":"answer","description":"work","events":[]})
        )
        .is_err()
    );
}

#[test]
fn current_basis_rejects_challenge_event_during_decode() {
    let record = json!({"title":"title","scope":"scope","summary":"summary","state_text":"state","current_basis":[{"event_id":"e"}]});
    assert!(serde_json::from_value::<MemoryRecord>(record.clone()).is_err());
    let edit = json!({"operation":"retire","memory_id":"m","expected_revision":1,"reason":"challenge","change_basis":[{"event_id":"e"}]});
    assert!(serde_json::from_value::<MemoryEdit>(edit).is_ok());
}

#[test]
fn checkpoint_revision_only_accepts_known_or_after_edit() {
    for revision in [json!(1), json!("after_edit")] {
        assert!(
            serde_json::from_value::<MemoryRevisionRef>(
                json!({"memory_id":"m","revision":revision})
            )
            .is_ok()
        );
    }
    for revision in [
        json!("latest"),
        json!(1.5),
        json!({"revision":1}),
        json!(null),
    ] {
        assert!(
            serde_json::from_value::<MemoryRevisionRef>(
                json!({"memory_id":"m","revision":revision})
            )
            .is_err()
        );
    }
}

#[test]
fn invalid_optional_mode_retains_independently_permitted_native_capture() {
    let path = std::env::temp_dir().join(format!(
        "magnesium-typed-{}.sqlite",
        super::super::store::new_id("test").unwrap()
    ));
    let store = super::super::LearnerStore::initialize(&path, "typed-workspace").unwrap();
    let host = HostContext {
        host_kind: "test".into(),
        host_thread_id: "typed".into(),
    };
    let native = RawEvent {
        external_event_key: "native".into(),
        speaker: Speaker::Learner,
        kind: "message".into(),
        text: "x = 2".into(),
        metadata: Some(EventMetadata::default()),
        occurred_at_ms: None,
    };
    let request = json!({"schema":SCHEMA,"action":"update","mode":"unsupported","request_id":"invalid-mode","session":{"operation":"start","client_ref":"lesson","goal":"algebra"},"native_capture":{"ticket":"adapter-verified","text":native.text}});
    let error = store
        .update_with_native_capture(&host, request, Some(&native))
        .unwrap_err();
    assert_eq!(error.recovery.as_ref().unwrap()["field_path"], "mode");
    assert_eq!(error.captured_event_ids.len(), 1);
    let text: String = store
        .conn
        .query_row("SELECT text FROM learning_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(text, native.text);
    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn invalid_role_retains_raw_capture_and_exact_nested_error_path() {
    let path = std::env::temp_dir().join(format!(
        "magnesium-typed-{}.sqlite",
        super::super::store::new_id("test").unwrap()
    ));
    let store = super::super::LearnerStore::initialize(&path, "typed-workspace").unwrap();
    let host = HostContext {
        host_kind: "test".into(),
        host_thread_id: "typed".into(),
    };
    let request = json!({"schema":SCHEMA,"action":"update","request_id":"invalid-role","session":{"operation":"start","client_ref":"lesson","goal":"algebra"},"reported_events":[{"external_event_key":"raw","speaker":"learner","kind":"message","text":"x = 2"}],"observations":[{"client_ref":"o","kind":"learner_work","description":"work","events":[{"event_id":"irrelevant","role":"answer"}]}]});
    let error = store.update(&host, request).unwrap_err();
    assert_eq!(
        error.recovery.as_ref().unwrap()["field_path"],
        "observations[0].events[0].role"
    );
    assert_eq!(error.captured_event_ids.len(), 1);
    let text: String = store
        .conn
        .query_row("SELECT text FROM learning_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(text, "x = 2");
    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn oversized_unknown_key_keeps_error_and_recovery_bounded() {
    let host = HostContext {
        host_kind: "test".into(),
        host_thread_id: "typed".into(),
    };
    let mut request = json!({"schema":SCHEMA,"action":"update","request_id":"bounded"});
    request["🧩".repeat(4000)] = json!(true);
    let error = super::super::update::decode_update_request(&request, &host).unwrap_err();
    let path = error.recovery.as_ref().unwrap()["field_path"]
        .as_str()
        .unwrap();
    assert!(path.len() <= 512);
    assert!(path.ends_with("[path truncated]"));
    assert!(serde_json::to_vec(&error).unwrap().len() < 4096);
}

#[test]
fn tagged_memory_fields_keep_exact_nested_paths() {
    let host = HostContext {
        host_kind: "test".into(),
        host_thread_id: "typed".into(),
    };
    let request = json!({"schema":SCHEMA,"action":"update","request_id":"bad-title","memory_edits":[{"operation":"create","client_ref":"m","record":{"title":17,"scope":"scope","summary":"summary","state_text":"state","current_basis":[{"new_observation":"o"}]},"reason":"reason","change_basis":[{"new_observation":"o"}]}]});
    let error = super::super::update::decode_update_request(&request, &host).unwrap_err();
    assert_eq!(
        error.recovery.unwrap()["field_path"],
        "memory_edits[0].record.title"
    );
    let mut request = request;
    request["memory_edits"][0]["record"]["title"] = json!("title");
    request["memory_edits"][0]["record"]["current_basis"] = json!([{"observation_id":17}]);
    let error = super::super::update::decode_update_request(&request, &host).unwrap_err();
    assert_eq!(
        error.recovery.unwrap()["field_path"],
        "memory_edits[0].record.current_basis[0].observation_id"
    );
}

#[test]
fn unsupported_disposition_reason_reports_complete_nested_path() {
    let path = std::env::temp_dir().join(format!(
        "magnesium-typed-{}.sqlite",
        super::super::store::new_id("test").unwrap()
    ));
    let store = super::super::LearnerStore::initialize(&path, "typed-workspace").unwrap();
    let host = HostContext {
        host_kind: "test".into(),
        host_thread_id: "typed".into(),
    };
    let request = json!({"schema":SCHEMA,"action":"update","request_id":"invalid-reason","session":{"operation":"start","client_ref":"lesson","goal":"algebra"},"event_dispositions":[{"event_id":"e","outcome":"no_change","reason":"extra"}]});
    let error = store.update(&host, request).unwrap_err();
    assert_eq!(
        error.recovery.as_ref().unwrap()["field_path"],
        "event_dispositions[0].reason"
    );
    drop(store);
    let _ = std::fs::remove_file(path);
}
