//! The MCP server adapter exposing Magnesium services over JSON-RPC stdio.

pub(crate) mod catalog;
pub(crate) mod server;

use std::path::Path;

use rmcp::model::{CallToolResponse, CallToolResult, ContentBlock};
use serde_json::{Value, json};

pub(crate) use server::run;

use crate::adapter::{CompatibilityError, InvocationIdentity, SUPPORTED_PROTOCOL_VERSION};
use crate::workspace::resolve_initialized;

pub(crate) fn tool_success(value: Value) -> CallToolResponse {
    let text = serde_json::to_string(&value)
        .expect("serializing a bounded Magnesium MCP result cannot fail");
    CallToolResult::success(vec![ContentBlock::text(text)]).into()
}

pub(crate) fn tool_error(action: &str, error: &str) -> CallToolResponse {
    CallToolResult::error(vec![ContentBlock::text(format!(
        "Magnesium could not {action}: {error}"
    ))])
    .into()
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RoutingFailure {
    code: &'static str,
    retryable: bool,
    message: &'static str,
    recovery: &'static str,
}

impl From<std::io::Error> for RoutingFailure {
    fn from(_error: std::io::Error) -> Self {
        Self {
            code: "workspace_route_invalid",
            retryable: false,
            message: "The workspace-local learner store could not be opened",
            recovery: "Inspect the initialized workspace and its learner-model/v2 store",
        }
    }
}
pub(crate) fn check_routing(cwd: &Path) -> Result<(), RoutingFailure> {
    resolve_initialized(cwd)
        .map(|_| ())
        .map_err(RoutingFailure::from)
}

pub(crate) fn routing_error(error: RoutingFailure) -> CallToolResponse {
    let value = json!({
        "active": false,
        "outcome": "inactive",
        "reason": error.code,
        "retryable": error.retryable,
        "message": error.message,
        "recovery": error.recovery,
    });
    let text = serde_json::to_string(&value)
        .expect("serializing a bounded Magnesium routing error cannot fail");
    CallToolResult::error(vec![ContentBlock::text(text)]).into()
}

pub(crate) fn compatibility_error(
    identity: &InvocationIdentity,
    error: CompatibilityError,
) -> CallToolResponse {
    let value = json!({
        "active": false,
        "outcome": "incompatible_adapter",
        "reason": error.reason,
        "retryable": false,
        "message": error.detail,
        "observed": {
            "plugin_version": identity.plugin_version,
            "adapter_protocol_version": identity.protocol_version,
        },
        "supported": {
            "runtime_version": env!("CARGO_PKG_VERSION"),
            "plugin_version": env!("CARGO_PKG_VERSION"),
            "adapter_protocol_version": SUPPORTED_PROTOCOL_VERSION,
        },
        "recovery": "run `magnesium init` in this study folder to refresh the installed host plugins, then restart the assistant",
    });
    let text = serde_json::to_string(&value)
        .expect("serializing a bounded Magnesium compatibility error cannot fail");
    CallToolResult::error(vec![ContentBlock::text(text)]).into()
}
