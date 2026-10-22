//! Typed snapshot reads, candidate selection, and bounded packet construction.
mod evidence;
mod pages;
mod prepare;
mod readability;
mod records;
pub(crate) use readability::{validate_memory_envelopes, validate_observation_envelope};
mod requests;
use crate::learner::{model::SchemaVersion, search};
use pages::{Builder, CursorScope, Dependencies, Packet};
use records::{HostBinding, Identity, Status, db};
use requests::ObservationKind;
pub use requests::{ContextReadRequest, MemoryRequest, NextCall, Request};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug)]
pub enum Tool {
    Context,
    Memory,
}
/// Native tool selection constrains actions independently of model arguments.
pub fn prepare_snapshot_for_tool(
    conn: &Connection,
    host_kind: &str,
    host_thread_id: &str,
    raw: Value,
    tool: Tool,
) -> Result<PreparedRead, String> {
    let request: Request = match tool {
        Tool::Context => decode_protocol::<ContextReadRequest>(&raw)?.into(),
        Tool::Memory => decode_protocol::<MemoryRequest>(&raw)?.into(),
    };
    request.validate()?;
    prepare_decoded_snapshot(conn, host_kind, host_thread_id, raw, request)
}
pub struct PreparedRead {
    packet: Packet,
    max: usize,
    request: Value,
    action: String,
    host_kind: String,
    host_thread_id: String,
    trace: String,
    trace_capable: bool,
}
fn dependencies(
    request: &Request,
    memory: i64,
    observation: i64,
    review: i64,
    event: i64,
) -> Dependencies {
    let snapshot = |m, o, r| Dependencies::Snapshot {
        memory_generation: m,
        observation_generation: o,
        review_generation: r,
        event_generation: None,
    };
    match request {
        Request::SessionChoices(_) | Request::PendingEvents(_) | Request::PendingWork(_) => {
            Dependencies::Live
        }
        Request::Observations(r) => match r.kind {
            ObservationKind::MemoryRefs | ObservationKind::EventMemoryRefs => {
                snapshot(Some(memory), Some(observation), None)
            }
            ObservationKind::Event => Dependencies::Snapshot {
                memory_generation: r.memory_id.as_ref().map(|_| memory),
                observation_generation: r.memory_id.as_ref().map(|_| observation),
                review_generation: None,
                event_generation: Some(event),
            },
            ObservationKind::Observation if r.memory_id.is_some() => {
                snapshot(Some(memory), Some(observation), None)
            }
            _ => snapshot(None, Some(observation), None),
        },
        Request::Reviews(_) => snapshot(Some(memory), None, Some(review)),
        _ => snapshot(Some(memory), None, None),
    }
}
fn shape_error(path: &str, detail: &str) -> String {
    let message = format!("invalid_variant: {path}: {detail}");
    if message.len() > 4096 {
        let mut end = 4096;
        while !message.is_char_boundary(end) {
            end -= 1
        }
        format!("{} [error detail truncated]", &message[..end])
    } else {
        message
    }
}
fn action_fields(raw: &Value) -> Result<(&str, Value), String> {
    let object = raw
        .as_object()
        .ok_or_else(|| shape_error("request", "Expected a read object"))?;
    let action = object
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| shape_error("action", "Required string action"))?;
    let mut fields = object.clone();
    fields.remove("action");
    Ok((action, Value::Object(fields)))
}
fn decode_fields<T: serde::de::DeserializeOwned>(fields: &Value) -> Result<T, String> {
    serde_path_to_error::deserialize::<_, T>(fields.clone())
        .map_err(|e| shape_error(&e.path().to_string(), &e.inner().to_string()))
}
fn decode_protocol<T: requests::ReadProtocol>(request: &Value) -> Result<T, String> {
    if pages::size(request) > 1024 * 1024 {
        return Err("field_too_large: request".into());
    }
    T::decode(request)
}
/// Unit-test entry that names only the action and reads through the same
/// per-tool decoders production uses. Returns only after the read snapshot has
/// closed. No control-admission guard belongs around this call; the adapter
/// separately guards optional tracing.
#[cfg(test)]
pub(crate) fn prepare_snapshot(
    conn: &Connection,
    host_kind: &str,
    host_thread_id: &str,
    raw: Value,
) -> Result<PreparedRead, String> {
    let tool = raw
        .get("action")
        .and_then(Value::as_str)
        .map_or(Tool::Memory, Tool::for_action);
    prepare_snapshot_for_tool(conn, host_kind, host_thread_id, raw, tool)
}
fn check_read_cancellation() -> Result<(), String> {
    if crate::request::OperationCancellation::none().is_cancelled() {
        Err("request_cancelled: learner read cancelled".into())
    } else {
        Ok(())
    }
}

fn prepare_decoded_snapshot(
    conn: &Connection,
    host_kind: &str,
    host_thread_id: &str,
    raw: Value,
    request: Request,
) -> Result<PreparedRead, String> {
    check_read_cancellation()?;
    let progress = search::ReadProgress::install(conn)?;
    let max = request.max_bytes();
    let trace = format!(
        "trace-{}",
        pages::hex(&Sha256::digest(
            format!(
                "{}:{}:{:?}",
                host_kind,
                host_thread_id,
                std::time::SystemTime::now()
            )
            .as_bytes()
        ))
    );
    conn.execute_batch("BEGIN DEFERRED").map_err(db)?;
    let result = (|| {
        let state = records::workspace_state(conn)?;
        #[cfg(test)]
        crate::request::test_checkpoint("learner_snapshot_open");
        let observation_id = match &request {
            Request::Observations(r) => r.observation_id.as_deref(),
            _ => None,
        };
        let event_id = match &request {
            Request::Observations(r) => r.event_id.as_deref(),
            _ => None,
        };
        for (table, column, id) in [
            ("learning_sessions", "session_id", request.session_id()),
            ("learner_memories", "memory_id", request.memory_id()),
            ("observations", "observation_id", observation_id),
            ("learning_events", "event_id", event_id),
        ] {
            if let Some(id) = id {
                let exists: bool = conn
                    .query_row(
                        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {column}=?1)"),
                        [id],
                        |r| r.get(0),
                    )
                    .map_err(db)?;
                if !exists {
                    return Err(format!("unknown_reference: {column}"));
                }
            }
        }
        let active = conn
            .query_row(
                "SELECT count(*) FROM learner_memories WHERE lifecycle='active'",
                [],
                |r| r.get(0),
            )
            .map_err(db)?;
        let trace_capable:bool=conn.query_row("SELECT learning_active=1 AND recording_enabled=1 FROM host_bindings WHERE host_kind=?1 AND host_thread_id=?2",params![host_kind,host_thread_id],|r|r.get(0)).optional().map_err(db)?.unwrap_or(false);
        let deps = dependencies(
            &request,
            state.memory_generation,
            state.observation_generation,
            state.review_generation,
            state.event_generation,
        );
        let live = matches!(deps, Dependencies::Live);
        let scope = CursorScope::new(&request, &state.workspace_id, deps);
        let mut packet = Packet::new(
            &state,
            request.action(),
            active,
            search::index_current(conn, false).map_err(db)?,
            trace.clone(),
            live,
        );
        // Successful receipt with its full ID is larger than either disabled or
        // unavailable status with a null ID. Reserve that exact maximum envelope.
        packet.trace_status = if trace_capable {
            "recorded"
        } else {
            "disabled"
        };
        if !trace_capable {
            packet.trace_id = None;
        }
        let mut builder = Builder::new(packet, max);
        if matches!(
            &request,
            Request::Prepare(_)
                | Request::Search(_)
                | Request::Status(_)
                | Request::SessionChoices(_)
        ) {
            evidence::pending_notice(conn, &mut builder, (host_kind, host_thread_id))?;
        }
        match &request {
            Request::Prepare(r) | Request::Search(r) => prepare::prepare(
                conn,
                &request,
                r,
                &mut builder,
                &scope,
                (host_kind, host_thread_id),
            )?,
            Request::Browse(r) => {
                prepare::directory(conn, &request, r.topic.as_deref(), &mut builder, &scope)?
            }
            Request::Topics(_) => evidence::topics(conn, &request, &mut builder, &scope)?,
            Request::Read(r) => evidence::read(conn, r, &mut builder)?,
            Request::History(r) => evidence::history(conn, &request, r, &mut builder, &scope)?,
            Request::Reviews(r) => evidence::reviews(conn, &request, r, &mut builder, &scope)?,
            Request::Observations(r) => {
                let scan = evidence::observations(conn, &request, r, &mut builder, &scope);
                check_read_cancellation()?;
                match scan {
                    Err(e) if progress.exhausted() && e.contains("interrupted") => {
                        builder.required(|p| {
                            p.warnings.push("query_cancelled: incomplete coverage; narrow the query or page without query".into());
                            p.coverage.query_execution_complete = Some(false);
                        })?;
                    }
                    other => other?,
                }
            }

            Request::SessionChoices(_) => {
                evidence::session_choices(conn, &request, &mut builder, &scope)?
            }
            Request::PendingEvents(r) => evidence::pending_events(
                conn,
                &request,
                r,
                &mut builder,
                &scope,
                (host_kind, host_thread_id),
            )?,
            Request::PendingWork(r) => {
                evidence::pending_work(conn, &request, r, &mut builder, &scope)?
            }
            Request::Status(_) => {
                let status = Status {
                    identity: Identity {
                        schema: SchemaVersion::V2,
                        package_version: env!("CARGO_PKG_VERSION"),
                        teaching_policy_version: crate::adapter::TEACHING_POLICY_VERSION,
                        host_kind: host_kind.into(),
                        capture: "native_when_available_else_model_reported",
                    },
                    storage_family: SchemaVersion::V2,
                    schema_version: crate::learner::store::SCHEMA_VERSION,
                    instruction_identity: crate::learner::SCHEMA,
                    host_binding: HostBinding {
                        host_kind: host_kind.into(),
                        host_thread_id: host_thread_id.into(),
                        recording_active: trace_capable,
                    },
                    observation_index_current: search::index_current(conn, true).map_err(db)?,
                };
                builder.required(|p| p.status = Some(status))?;
                evidence::session_context(
                    conn,
                    request.session_id(),
                    &mut builder,
                    (host_kind, host_thread_id),
                )?;
            }
        }
        check_read_cancellation()?;
        Ok((builder.finish()?, trace_capable))
    })();
    let result = check_read_cancellation().and(result);
    drop(progress);
    match result {
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
        Ok((packet, trace_capable)) => {
            if let Err(e) = conn.execute_batch("COMMIT") {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(db(e));
            }
            Ok(PreparedRead {
                packet,
                max,
                request: raw,
                action: request.action().into(),
                host_kind: host_kind.into(),
                host_thread_id: host_thread_id.into(),
                trace,
                trace_capable,
            })
        }
    }
}
/// The adapter holds only the relevant host admission gate during this optional
/// side effect. A failed/disallowed trace always returns the useful packet.
pub fn finish_trace(
    conn: &Connection,
    mut prepared: PreparedRead,
    allow_trace: bool,
) -> Result<Value, String> {
    #[cfg(test)]
    crate::request::test_checkpoint("before_trace");
    let effective = allow_trace
        && prepared.trace_capable
        && !crate::request::OperationCancellation::none().is_cancelled();
    if !effective {
        prepared.packet.trace_id = None;
        prepared.packet.trace_status = "disabled";
    }
    if let Some(status) = &mut prepared.packet.status {
        status.host_binding.recording_active = effective;
    }
    if effective {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let packet_json = serde_json::to_string(&prepared.packet).map_err(|e| e.to_string())?;
        #[cfg(test)]
        crate::request::test_checkpoint("before_trace_insert");
        let inserted = if crate::request::OperationCancellation::none().is_cancelled() {
            Ok(0)
        } else {
            conn.execute("INSERT INTO context_receipts(trace_id,host_kind,host_thread_id,session_id,tool_action,request_json,response_json,created_at_ms) SELECT ?1,?2,?3,COALESCE(?8,session_id),?4,?5,?6,?7 FROM host_bindings WHERE host_kind=?2 AND host_thread_id=?3 AND learning_active=1 AND recording_enabled=1",params![prepared.trace,prepared.host_kind,prepared.host_thread_id,prepared.action,prepared.request.to_string(),packet_json,now,prepared.packet.session.as_ref().map(|s|s.session_id.as_str())])
        };
        match inserted {
            Ok(1) => {}
            Ok(_) => {
                prepared.packet.trace_id = None;
                prepared.packet.trace_status = "disabled";
            }
            Err(_) => {
                prepared.packet.trace_id = None;
                prepared.packet.trace_status = "unavailable";
            }
        }
    }
    if pages::size(&prepared.packet) > prepared.max {
        return Err("budget_too_small: final packet exceeds reserved envelope".into());
    }
    serde_json::to_value(prepared.packet).map_err(|e| e.to_string())
}
#[cfg(test)]
pub(crate) use evidence::SESSION_CHOICES_SQL;
#[cfg(test)]
pub(crate) fn session_choice_rows(conn: &Connection, last: &str) -> Result<Vec<Value>, String> {
    evidence::session_choice_rows(conn, last)?
        .into_iter()
        .map(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        .collect()
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod oversized_event_tests;

#[cfg(test)]
mod packet_tests;
#[cfg(test)]
pub(crate) mod test_support;
