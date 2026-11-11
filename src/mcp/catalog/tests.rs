use super::*;
#[test]
fn read_tool_schemas_advertise_their_actual_scoped_actions() {
    fn actions(node: &Value, result: &mut std::collections::BTreeSet<String>) {
        let action = &node["properties"]["action"];
        if let Some(value) = action["const"].as_str() {
            result.insert(value.into());
        }
        if let Some(values) = action["enum"].as_array() {
            for value in values {
                result.insert(value.as_str().unwrap().into());
            }
        }
        for key in ["anyOf", "oneOf"] {
            if let Some(branches) = node[key].as_array() {
                for branch in branches {
                    actions(branch, result);
                }
            }
        }
    }
    for (name, expected) in [
        (
            "context",
            vec![
                "instructions",
                "status",
                "prepare",
                "session_choices",
                "pending_events",
                "pending_work",
            ],
        ),
        (
            "memory",
            vec![
                "search",
                "browse",
                "topics",
                "read",
                "history",
                "observations",
                "reviews",
                "pending_work",
            ],
        ),
    ] {
        let tool = get_tool(name).unwrap();
        let mut actual = std::collections::BTreeSet::new();
        actions(&Value::Object((*tool.input_schema).clone()), &mut actual);
        assert_eq!(
            actual,
            expected.into_iter().map(String::from).collect(),
            "{name}"
        );
    }
}

#[test]
fn generated_schemas_keep_structured_memory_edits_and_protocol_identity() {
    let tool = get_tool("learning_step").unwrap();
    let serialized = serde_json::to_string(&tool.input_schema).unwrap();
    for expected in [
        "learner-model/v2",
        "current_basis",
        "change_basis",
        "recording_off",
        "reported_events",
    ] {
        assert!(serialized.contains(expected), "missing {expected}");
    }
    for removed in [
        "attestation_id",
        "criterion_id",
        "verified_correct",
        "schema_version",
    ] {
        assert!(!serialized.contains(removed), "retired {removed}");
    }
}

#[test]
fn generated_write_schema_advertises_only_valid_basis_and_closed_choices() {
    let tool = get_tool("learning_step").unwrap();
    let root = Value::Object((*tool.input_schema).clone());
    let update = root["anyOf"]
        .as_array()
        .unwrap()
        .iter()
        .find(|branch| branch["properties"]["action"]["enum"] == serde_json::json!(["update"]))
        .unwrap();
    let properties = &update["properties"];
    let observation = &properties["observations"]["items"]["properties"];
    assert_eq!(
        observation["kind"]["enum"],
        serde_json::json!([
            "learner_work",
            "learner_question",
            "self_report",
            "correction",
            "teaching_response",
            "external_check"
        ])
    );
    assert_eq!(
        observation["events"]["items"]["properties"]["role"]["enum"],
        serde_json::json!([
            "learner_response",
            "task_context",
            "assistance",
            "teaching_context",
            "check_result",
            "correction_context"
        ])
    );
    assert_eq!(
        properties["reported_events"]["items"]["properties"]["speaker"]["enum"],
        serde_json::json!(["learner", "tutor", "tool", "source"])
    );
    assert_eq!(
        properties["event_dispositions"]["items"]["properties"]["outcome"]["enum"],
        serde_json::json!(["processed", "no_change"])
    );
    for branch in properties["memory_edits"]["items"]["oneOf"]
        .as_array()
        .unwrap()
    {
        let edit = &branch["properties"];
        let operation = edit["operation"]["const"].as_str().unwrap();
        if operation == "revise" {
            assert_eq!(
                edit["change_kind"]["enum"],
                serde_json::json!(["new_observation", "reinterpretation", "editorial"])
            );
        }
        if matches!(operation, "create" | "revise") {
            let current = &edit["record"]["properties"]["current_basis"];
            let choices = current["items"]["oneOf"].as_array().unwrap();
            assert_eq!(choices.len(), 2);
            assert_eq!(
                choices[0]["required"],
                serde_json::json!(["observation_id"])
            );
            assert_eq!(
                choices[1]["required"],
                serde_json::json!(["new_observation"])
            );
            assert_eq!(current["minItems"], 1);
            assert_eq!(current["maxItems"], 8);
            assert_eq!(
                edit["change_basis"]["items"]["oneOf"][2]["required"],
                serde_json::json!(["event_id"])
            );
        }
    }
    let review_update = properties["review_updates"]["items"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .find(|branch| branch["properties"]["operation"]["const"] == "update")
        .unwrap();
    assert_eq!(
        review_update["properties"]["status"]["enum"],
        serde_json::json!(["suggested", "done", "dismissed"])
    );
}
