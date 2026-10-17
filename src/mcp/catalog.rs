use rmcp::model::{JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde_json::Value;
use std::sync::Arc;
pub(crate) const CANONICAL_OPERATIONS: [&str; 5] = [
    "context",
    "memory",
    "learning_step",
    "search_sources",
    "sources",
];
pub(crate) fn tool_definitions() -> Vec<Tool> {
    CANONICAL_OPERATIONS
        .iter()
        .filter_map(|name| get_tool(name))
        .collect()
}
pub(crate) fn get_tool(name: &str) -> Option<Tool> {
    let (description, schema) = match name {
        "context" => (
            "Read workspace learner-state status or prepare current descriptions for the goal.",
            generated_schema::<crate::adapter::ContextRequest>(),
        ),
        "memory" => (
            "Search, browse, read or inspect current learner memories and explicitly historical supporting work. Action reviews pages suggested reviews with current memory and needs_reconsideration notices; accepts memory_id, limit, cursor and max_bytes.",
            generated_schema::<crate::learner::read::MemoryRequest>(),
        ),
        "learning_step" => (
            "Record observations and descriptive memories atomically, or apply independent learner controls. Load context instructions operation tool_recipes for complete write examples. session.start requires client_ref and goal. Observations use description and events:[{event_id,role}]. Memory create/revise nests the complete description in record and supplies change_basis separately. Evidence references are objects, never strings. record.current_basis selects 1–8 observations, not the full history. Explicit preferences use self_report. Event dispositions accept only event_id/outcome; image provenance belongs in metadata.attachments/source_refs. A checkpoint requires session:{operation:use,session_id,expected_revision}, or an explicit session.start/resume. Read context.status for the current session revision; preserve the checkpoint when correcting a rejected request.",
            generated_schema::<crate::adapter::LearningStepRequest>(),
        ),
        "search_sources" => (
            "Search subject sources within this workspace's file/privacy boundaries.",
            generated_schema::<crate::adapter::SearchRequest>(),
        ),
        "sources" => (
            "Inspect original source anchors with extraction limitations and bounded original text.",
            generated_schema::<crate::adapter::SourcesRequest>(),
        ),
        _ => return None,
    };
    let description = if name == "learning_step" {
        format!(
            "{description} Native capture may accompany explicit session.{}. Existing turn_ref is context only; selected_unbound_event_ids explicitly associates prior pending work. Complete-captured can preserve an identical existing pending_task or clear it.",
            crate::learner::SessionEdit::CAPTURE_OPERATIONS.join(" or session.")
        )
    } else {
        description.to_owned()
    };
    let tool = Tool::new(name.to_owned(), description, schema).annotate(
        ToolAnnotations::new()
            .read_only(name != "learning_step")
            .destructive(false),
    );
    Some(tool)
}
pub(crate) fn generated_schema<T: JsonSchema>() -> Arc<JsonObject> {
    // MCP requires a root `inputSchema.type` of "object"; schemars omits it
    // for tagged-enum roots whose alternatives live under `oneOf`. Inlining
    // local definitions keeps every structured field's object shape at the
    // point the client reads it, because some clients stringify `$ref`
    // arguments and the runtime rejects the stringified payload.
    let settings = schemars::generate::SchemaSettings::default().with(|settings| {
        settings.inline_subschemas = true;
    });
    let value = serde_json::to_value(settings.into_generator().into_root_schema_for::<T>())
        .expect("serializing a generated Magnesium MCP schema cannot fail");
    let mut schema = value
        .as_object()
        .expect("a generated Magnesium MCP schema is an object")
        .clone();
    schema
        .entry("type".to_owned())
        .or_insert_with(|| Value::String("object".to_owned()));
    Arc::new(schema)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tool_surface_is_exactly_the_canonical_operations() {
        assert_eq!(
            tool_definitions()
                .iter()
                .map(|t| t.name.as_ref())
                .collect::<Vec<_>>(),
            CANONICAL_OPERATIONS
        );
    }
    #[test]
    fn context_advertises_installed_policy_delivery() {
        let context = get_tool("context").unwrap();
        let text = serde_json::to_string(&context.input_schema).unwrap();
        assert!(text.contains("instructions"));
        assert!(text.contains("orient_and_probe"));
    }

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
}
