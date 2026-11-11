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
/// The tool that carries the tutor's reply to the learner. It is offered only
/// in a session the study window starts, which the window marks with
/// [`SURFACE_ENV`]. Its input streams to the window as the tutor writes it,
/// where text after a tool call would arrive in one piece.
pub(crate) const REPLY_TOOL: &str = "reply";
pub(crate) const SURFACE_ENV: &str = "MAGNESIUM_SURFACE";
pub(crate) fn window_surface() -> bool {
    std::env::var(SURFACE_ENV).is_ok_and(|surface| surface == "window")
}
pub(crate) fn tool_definitions() -> Vec<Tool> {
    tool_definitions_for(window_surface())
}
pub(crate) fn tool_definitions_for(window: bool) -> Vec<Tool> {
    CANONICAL_OPERATIONS
        .iter()
        .copied()
        .chain(window.then_some(REPLY_TOOL))
        .filter_map(|name| tool_for(name, window))
        .collect()
}
pub(crate) fn get_tool(name: &str) -> Option<Tool> {
    tool_for(name, window_surface())
}
fn tool_for(name: &str, window: bool) -> Option<Tool> {
    let (description, schema) = match name {
        REPLY_TOOL if window => (
            "Show your reply to the learner. In this session the learner sees only what this tool sends, as you write it: after any learning_step, put the whole reply, task included, in text, as you would write it in chat, and end the turn with no other text. One call per turn.",
            generated_schema::<crate::adapter::ReplyRequest>(),
        ),
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
            .read_only(!matches!(name, "learning_step" | REPLY_TOOL))
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
mod tests;
