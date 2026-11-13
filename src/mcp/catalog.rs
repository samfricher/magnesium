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
            "Show your reply to the learner. In this session the learner sees only what this tool sends, as you write it. Record the answer's observation and attempt first; then put the whole reply, task included, in text, as you would write it in chat. When a memory edit or a checkpoint is due, set save_after and send them in one learning_step after this call; otherwise the turn ends here. Write no other text. One call per turn.",
            generated_schema::<crate::adapter::ReplyRequest>(),
        ),
        "context" => (
            "Load Magnesium's teaching instructions, or read the learner's current state. It takes one of two shapes. An instruction load is {\"action\":\"instructions\",\"host\":\"claude\",\"stage\":<stage>}, where stage is orient_and_probe, plan_and_route, teach_and_practice, interpret_and_repair, review_and_transfer, consolidate_and_resume, papers or papers_loop; add \"operation\": state_and_evidence, source_use or tool_recipes to load that contract. A learner-state read is {\"schema\":\"learner-model/v2\",\"action\": status, prepare, session_choices or pending_events, ...}, with no host or stage. prepare returns the current memory descriptions for a goal; status gives the current session revision a checkpoint needs. A session start or resume through learning_step already returns prepare for its goal, so call prepare yourself only for another goal or after a compaction. Reading changes nothing: it does not resume, bind, assess or process a session.",
            generated_schema::<crate::adapter::ContextRequest>(),
        ),
        "memory" => (
            "Search, browse, read or inspect current learner memories and explicitly historical supporting work. Search is {\"schema\":\"learner-model/v2\",\"action\":\"search\",\"goal\":\"<topic>\",\"original_query\":\"<learner's words>\"}; read is {\"schema\":\"learner-model/v2\",\"action\":\"read\",\"refs\":[{\"kind\":\"current_memory\"|\"observation\"|\"event\",\"id\":\"<id>\"}]}; browse is {\"schema\":\"learner-model/v2\",\"action\":\"browse\"} with an optional topic. The other actions are topics, history, reviews, observations and pending_work. Action reviews pages suggested reviews with current memory and needs_reconsideration notices; accepts memory_id, limit, cursor and max_bytes. A search result can leave state_text out, so read a memory before relying on its description. An empty search does not mean there is no history: rephrase, use the learner's literal words, or browse. Reading never assesses or processes work.",
            generated_schema::<crate::learner::read::MemoryRequest>(),
        ),
        "learning_step" => (
            "Record observations and descriptive memories atomically, or apply independent learner controls. Load context instructions operation tool_recipes for complete write examples. session.start requires client_ref and goal. Observations use description and events:[{event_id,role}]. Memory create/revise nests the complete description in record and supplies change_basis separately. Evidence references are objects, never strings. record.current_basis selects 1–8 observations, not the full history. Explicit preferences use self_report. Event dispositions accept only event_id/outcome; image provenance belongs in metadata.attachments/source_refs. A checkpoint requires session:{operation:use,session_id,expected_revision}, or an explicit session.start/resume. Read context.status for the current session revision; preserve the checkpoint when correcting a rejected request.",
            generated_schema::<crate::adapter::LearningStepRequest>(),
        ),
        "search_sources" => (
            "Search the course material in this study folder for a passage, by plain words. Each result names a file and an anchor (a page or a line range) to read in full with the sources tool. Use it before teaching from or citing course material. It keeps to the folder's privacy rules: a file left out for a word in its name, and a PDF with no text layer, are not searched, so an empty result does not show that the material lacks the topic; rephrase in the material's own words, or narrow path_prefix to one folder.",
            generated_schema::<crate::adapter::SearchRequest>(),
        ),
        "sources" => (
            "Read the original text at a source anchor that search_sources returned. action inspect returns the text for path, anchor_kind (page or line), anchor_start and optional anchor_end, cut at 64 KB with truncated set when it was cut; action status returns the anchor's coverage and extraction limits without the text. Pass the content_sha256 the search result gave to make sure the file has not changed since. Parsed text can miss what a page image shows, such as layout, notation, plots or code indentation: when that matters to the point, Read the page itself.",
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
