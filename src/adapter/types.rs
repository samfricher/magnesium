use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SearchRequest {
    #[serde(default)]
    pub(crate) schema_version: Option<u64>,
    pub(crate) query: String,
    #[serde(default)]
    pub(crate) limit: Option<usize>,
    /// Optional relative path prefix below the workspace (for example `src`);
    /// omit to search the workspace root.
    #[serde(default)]
    pub(crate) path_prefix: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceAction {
    Inspect,
    Status,
}

impl SourceAction {
    pub(crate) fn includes_content(self) -> bool {
        !matches!(self, Self::Status)
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourcesRequest {
    pub(crate) action: SourceAction,
    #[serde(default)]
    pub(crate) schema_version: Option<u64>,
    #[serde(default)]
    #[schemars(length(min = 1, max = 256))]
    pub(crate) path: Option<String>,
    #[serde(default)]
    #[schemars(length(min = 64, max = 64))]
    pub(crate) content_sha256: Option<String>,
    #[serde(default)]
    #[schemars(length(min = 1, max = 16))]
    pub(crate) anchor_kind: Option<String>,
    #[serde(default)]
    pub(crate) anchor_start: Option<u32>,
    #[serde(default)]
    pub(crate) anchor_end: Option<u32>,
}
