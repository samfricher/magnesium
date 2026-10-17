use super::*;

impl LearnerStore {
    #[cfg(test)]
    pub(crate) fn update(&self, host: &HostContext, payload: Value) -> Result<UpdateResult> {
        self.update_with_native_capture(host, payload, None)
    }
    /// The adapter supplies an independently validated transient native event.
    /// Never deserialize this trusted event from model arguments without proof.
    pub fn update_with_native_capture(
        &self,
        host: &HostContext,
        payload: Value,
        native: Option<&RawEvent>,
    ) -> Result<UpdateResult> {
        if let Some(result) = self.replay(host, &payload)? {
            return Ok(result);
        }
        // Complete-captured is semantic work only. Decode it before any raw
        // boundary so a malformed completion cannot smuggle in new recording.
        // Invalid optional modes stay in the ordinary path: admitted raw work
        // survives the subsequent typed semantic diagnostic, like other fields.
        if payload.get("mode").and_then(Value::as_str)
            == Some(UpdateMode::CompleteCaptured.as_str())
        {
            let request = decode_update_request(&payload, host)?;
            if native.is_some()
                || request.native_capture.is_some()
                || !request.reported_events.is_empty()
            {
                return Err(LearnerError::new(
                    "invalid_variant",
                    "mode.complete_captured forbids native_capture and reported_events",
                ));
            }
            // Error receipts name only actual captured records in this scope,
            // never an arbitrary or nonexistent model-supplied turn_ref.
            let captured = match &request.turn_ref {
                Some(turn) => {
                    let scoped: bool = self.conn.query_row(
                        "SELECT EXISTS(
                            SELECT 1 FROM learning_events e WHERE e.event_id = ?1
                              AND ((e.host_kind = ?2 AND e.host_thread_id = ?3)
                                   OR EXISTS(
                                       SELECT 1 FROM host_bindings b
                                       WHERE b.host_kind = ?2 AND b.host_thread_id = ?3
                                         AND b.session_id = e.session_id
                                   ))
                        )",
                        params![turn, host.host_kind, host.host_thread_id],
                        |row| row.get(0),
                    )?;
                    if scoped { vec![turn.clone()] } else { vec![] }
                }
                None => vec![],
            };
            return self
                .semantic_update(host, &request, &payload, &captured, &[])
                .map_err(|mut error| {
                    error.captured_event_ids = captured;
                    error
                });
        }
        let operation = payload
            .pointer("/session/operation")
            .and_then(Value::as_str);
        let start = SessionEdit::accepts_capture_operation(operation);
        let supplied = payload
            .get("native_capture")
            .filter(|value| !value.is_null());
        if supplied.is_some() != native.is_some() {
            return Err(LearnerError::new(
                "invalid_variant",
                "native_capture requires adapter-verified provenance",
            ));
        }
        let mut captured = vec![];
        if let Some(native) = native {
            if !start || payload["action"] != "update" || payload["schema"] != SCHEMA {
                return Err(LearnerError::new(
                    "invalid_variant",
                    "native_capture requires an explicit capture-bearing session operation",
                ));
            }
            let proof: NativeCaptureRequest = decode_at(supplied.unwrap(), "native_capture", host)?;
            limits::text("native_capture.ticket", &proof.ticket, 4096)?;
            // The adapter verified and stored the host's normalized text.
            let echoed = crate::adapter::native_capture::learner_text(host, &proof.text);
            if echoed != native.text.as_str() || native.speaker != Speaker::Learner {
                return Err(LearnerError::new(
                    "invalid_variant",
                    "verified native capture does not match exact learner text",
                ));
            }
            captured.extend(self.capture(
                host,
                std::slice::from_ref(native),
                "host_observed",
                true,
            )?);
        }
        let result = (|| {
            let reported: Vec<RawEvent> = decode_at(
                payload.get("reported_events").unwrap_or(&json!([])),
                "reported_events",
                host,
            )?;
            captured.extend(self.capture(host, &reported, "model_reported", start)?);
            // Only raw input produced by this capture boundary is eligible for
            // automatic first association. Existing turn_ref is context only.
            let attachment_ids = captured.clone();
            if let Some(turn) = payload.get("turn_ref").and_then(Value::as_str) {
                let belongs: bool = self.conn.query_row(
                    "SELECT EXISTS(
                    SELECT 1 FROM learning_events
                    WHERE event_id = ?1 AND host_kind = ?2 AND host_thread_id = ?3
                )",
                    params![turn, host.host_kind, host.host_thread_id],
                    |row| row.get(0),
                )?;
                if belongs && !captured.iter().any(|id| id == turn) {
                    captured.push(turn.into());
                }
            }
            let request = decode_update_request(&payload, host)?;
            self.semantic_update(host, &request, &payload, &captured, &attachment_ids)
        })();
        result.map_err(|mut error| {
            if start && !captured.is_empty() && error.recovery.is_none() {
                error.recovery = Some(start_retry_recovery(&self.conn, operation, &captured));
            }
            error.captured_event_ids = captured;
            error
        })
    }
}

/// A rejected start/resume has already committed its raw capture, but no
/// session association. captured_event_ids is the whole inventory: it can
/// include an already-bound turn_ref or tutor context, which the explicit
/// selector rightly refuses. Only stored learner input that is still pending
/// and unbound may be selected on the retry; everything else is cited context.
fn start_retry_recovery(
    conn: &rusqlite::Connection,
    operation: Option<&str>,
    captured: &[String],
) -> Value {
    let (operation, done) = match operation {
        Some("resume") => ("resume", "resumed"),
        _ => ("start", "started"),
    };
    let eligible = captured
        .iter()
        .filter_map(|id| {
            conn.query_row(
                "SELECT session_id IS NULL AND speaker = 'learner' AND processing = 'pending'
                 FROM learning_events WHERE event_id = ?1",
                [id],
                |row| row.get::<_, bool>(0),
            )
            .optional()
            .map(|eligible| eligible.unwrap_or(false).then(|| id.clone()))
            .transpose()
        })
        .collect::<rusqlite::Result<Vec<String>>>();
    let common = format!(
        "Nothing else committed and no session is {done} here, so session.use fails. Retry once with a new request_id: keep session.operation \"{operation}\" and the intended checkpoint, omit native_capture and reported_events, and cite events by their real IDs, never a placeholder."
    );
    match eligible {
        Ok(ids) if !ids.is_empty() => json!({
            "field_path": "session",
            "captured_event_ids": captured,
            "retry_attachment_ids": ids,
            "instruction": format!("{common} Add retry_attachment_ids to session.selected_unbound_event_ids, keeping any IDs you already selected. Other captured_event_ids are already recorded context: cite them, do not select them."),
        }),
        Ok(_) => json!({
            "field_path": "session",
            "captured_event_ids": captured,
            "retry_attachment_ids": [],
            "instruction": format!("{common} Do not add selected_unbound_event_ids for these events: they are already recorded context (cite them, for example as turn_ref), not new input to attach."),
        }),
        Err(_) => json!({
            "field_path": "session",
            "captured_event_ids": captured,
            "attachment_eligibility": "unknown",
            "instruction": format!("{common} Magnesium could not check which captured events are pending learner input; read them with memory action read before selecting any."),
        }),
    }
}
