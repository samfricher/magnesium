//! Fixed-metadata diagnostics; never persist learner text in failure logs.
use crate::learner::store::LearnerStore;
use crate::workspace::{Workspace, reject_symlink_components, write_private_atomic};
use serde_json::{Value, json};
use std::process::ExitCode;
#[derive(Clone, Copy, Debug)]
pub(crate) enum HookFailure {
    AdapterCompatibility,
    InputReadDecode,
    WorkspaceDiscovery,
    InputValidation,
    HookProcessing,
    OutputSerialization,
}
pub(crate) fn record_hook_error(workspace: &Workspace, event: &str, failure: HookFailure) {
    let path = workspace
        .control()
        .join("diagnostics/latest-hook-error.json");
    if reject_symlink_components(&path, workspace.control()).is_err() {
        return;
    }
    let event = match event {
        "SessionStart" | "UserPromptSubmit" | "Stop" | "SessionEnd" => event,
        _ => "hook",
    };
    let value = json!({"event":event,"failure":format!("{failure:?}"),"at_ms":chrono::Utc::now().timestamp_millis()});
    if let Ok(bytes) = serde_json::to_vec(&value) {
        let _ = write_private_atomic(&path, &bytes);
    }
}
pub(crate) fn latest_hook_failure(workspace: &Workspace) -> Value {
    let path = workspace
        .control()
        .join("diagnostics/latest-hook-error.json");
    if reject_symlink_components(&path, workspace.control()).is_err() {
        return Value::Null;
    }
    let Ok(metadata) = std::fs::symlink_metadata(&path) else {
        return Value::Null;
    };
    if !metadata.is_file() || metadata.len() > 4096 {
        return Value::Null;
    }
    let Ok(bytes) = std::fs::read(&path) else {
        return Value::Null;
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return Value::Null;
    };
    let Some(object) = value.as_object() else {
        return Value::Null;
    };
    if object.len() != 3
        || ![
            "SessionStart",
            "UserPromptSubmit",
            "Stop",
            "SessionEnd",
            "hook",
        ]
        .contains(&value["event"].as_str().unwrap_or(""))
        || ![
            "AdapterCompatibility",
            "InputReadDecode",
            "WorkspaceDiscovery",
            "InputValidation",
            "HookProcessing",
            "OutputSerialization",
        ]
        .contains(&value["failure"].as_str().unwrap_or(""))
        || value["at_ms"].as_i64().is_none()
    {
        return Value::Null;
    }
    value
}
pub(crate) fn inspect(workspace: &Workspace) -> Result<Value, String> {
    let id = workspace.id().map_err(|e| e.to_string())?;
    reject_symlink_components(&workspace.store_path(), workspace.control())
        .map_err(|e| e.to_string())?;
    let store = LearnerStore::open(&workspace.store_path(), &id).map_err(|e| e.to_string())?;
    let snapshot = store
        .conn
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let (memories,pending):(i64,i64)=snapshot.query_row("SELECT (SELECT count(*) FROM learner_memories WHERE lifecycle='active'),(SELECT count(*) FROM learning_events WHERE processing='pending')",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
    let generations:Value=snapshot.query_row("SELECT memory_generation,observation_generation,memory_fts_generation,observation_fts_generation,event_generation,review_generation FROM workspace_meta",[],|r|Ok(json!({"memory":r.get::<_,i64>(0)?,"observation":r.get::<_,i64>(1)?,"memory_fts":r.get::<_,i64>(2)?,"observation_fts":r.get::<_,i64>(3)?,"event":r.get::<_,i64>(4)?,"review":r.get::<_,i64>(5)?}))).map_err(|e|e.to_string())?;
    let instructions = match crate::workspace::instruction_bundle::installed_identity(
        workspace.control(),
    ) {
        Ok(identity) => {
            json!({"status":"current", "bundle_version":identity.version, "bundle_hash":identity.hash, "policy_status":identity.status})
        }
        Err(detail) => json!({"status":"unavailable", "detail":detail}),
    };
    Ok(
        json!({"active":true,"instructions":instructions,"schema":"learner-model/v2","schema_version":crate::learner::store::SCHEMA_VERSION,"workspace_id":id,"workspace_path":workspace.root().to_string_lossy(),"runtime_version":env!("CARGO_PKG_VERSION"),"current_memory_count":memories,"pending_event_count":pending,"generations":generations,"index_current":crate::learner::search::index_current(&snapshot,false).map_err(|e|e.to_string())? && crate::learner::search::index_current(&snapshot,true).map_err(|e|e.to_string())?,"memory_index_current":crate::learner::search::index_current(&snapshot,false).map_err(|e|e.to_string())?,"observation_index_current":crate::learner::search::index_current(&snapshot,true).map_err(|e|e.to_string())?,"last_hook_error":latest_hook_failure(workspace)}),
    )
}
pub(crate) fn print_info(as_json: bool) -> ExitCode {
    let result = (|| {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        match Workspace::discover(&cwd).map_err(|e| e.to_string())? {
            Some(w) => inspect(&w),
            None => Ok(
                json!({"active":false,"reason":"workspace_not_initialized","schema":"learner-model/v2"}),
            ),
        }
    })();
    match result {
        Ok(value) => {
            if as_json {
                println!("{value}")
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).unwrap_or_default()
                )
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("magnesium: {e}");
            ExitCode::FAILURE
        }
    }
}
