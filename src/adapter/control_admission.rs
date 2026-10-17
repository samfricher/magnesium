//! One owner for control admission, canonical recovery, and local fail-closed fallback.
//! Read snapshots run outside this owner; only their optional trace side effect is gated.
use super::sha256_hex;
use super::{ControlRequest, control_safety::Guard, native_capture};
use crate::{
    learner::{HostContext, LearnerStore},
    workspace::Workspace,
};
use serde_json::{Value, json};
use std::path::Path;

/// Admission errors reach the caller as text, so they carry the whole shape
/// instead of one missing field per retry.
const CONTROL_SHAPE: &str = r#"Send {"schema":"learner-model/v2","action":"control","request_id":"<new>","control":"pause"|"stop"|"recording_off"|"recording_on","turn_ref":"<current event from the Magnesium prompt notice>"}; without a current event, replace turn_ref with "reported_event":{"external_event_key":"<new>","speaker":"learner","kind":"message","text":"<the learner's exact words>"}"#;
const NATIVE_SESSION_SHAPE: &str = r#""session":{"operation":"start","client_ref":"<name>","goal":"<goal>"} or "session":{"operation":"resume","session_id":"<id>","expected_revision":<revision>}"#;

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ControlAction {
    Pause,
    Stop,
    RecordingOff,
    RecordingOn,
}
impl ControlAction {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pause => "pause",
            Self::Stop => "stop",
            Self::RecordingOff => "recording_off",
            Self::RecordingOn => "recording_on",
        }
    }
    fn stopping(self) -> bool {
        self != Self::RecordingOn
    }
    fn recording(self) -> bool {
        matches!(self, Self::RecordingOff | Self::RecordingOn)
    }
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ControlTag {
    Control,
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum CrossProcessProtection {
    CanonicalControl,
    Available,
    PersistedDeny,
    Unavailable,
}
#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum RestartDurability {
    SafetyLatch,
    Unavailable,
}
#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum CaptureAdmission {
    Unchanged,
    RequiresSafetyMetadataRecovery,
    SubsequentCallsInThisConnectionOnly,
}
/// Typed outcomes keep historical success separate from newly authorized recovery.
enum ControlResult {
    Applied {
        receipt: Value,
        cleanup: Result<(), String>,
        protection: CrossProcessProtection,
    },
    Replayed {
        receipt: Value,
        cleanup: Result<(), String>,
    },
    Failed {
        error: crate::learner::LearnerError,
        stopped: bool,
        latched: Result<(), String>,
    },
    LocalOnly {
        request_id: String,
        action: ControlAction,
        error: String,
    },
    StorageUnavailable {
        request_id: String,
        action: ControlAction,
        protection: CrossProcessProtection,
        safety_error: Option<String>,
    },
}
impl ControlResult {
    fn into_value(self) -> Value {
        match self {
            Self::Applied {
                mut receipt,
                cleanup,
                protection,
            } => {
                annotate_cleanup(&mut receipt, cleanup);
                receipt["cross_process_protection"] = json!(protection);
                receipt
            }
            Self::Replayed {
                mut receipt,
                cleanup,
            } => {
                receipt["replayed"] = json!(true);
                receipt["capture_admission"] = json!(CaptureAdmission::Unchanged);
                annotate_cleanup(&mut receipt, cleanup);
                receipt
            }
            Self::Failed {
                error,
                stopped,
                latched,
            } => {
                let shared = stopped && latched.is_ok();
                json!({"schema": crate::learner::SCHEMA,"error":error,"local_stop":stopped,
                    "restart_durability":if shared {RestartDurability::SafetyLatch} else {RestartDurability::Unavailable},
                    "cross_process_protection":if shared {CrossProcessProtection::PersistedDeny} else {CrossProcessProtection::Unavailable},
                    "safety_error":latched.err()})
            }
            Self::LocalOnly {
                request_id,
                action,
                error,
            } => json!({
                "schema":crate::learner::SCHEMA,"request_id":request_id,"action":action,
                "durable":false,"local_stop":true,"restart_durability":RestartDurability::Unavailable,
                "cross_process_protection":CrossProcessProtection::Unavailable,
                "error":{"code":"control_safety_unavailable","message":error},
                "capture_admission":CaptureAdmission::SubsequentCallsInThisConnectionOnly,
                "recovery":"After storage recovers, explicitly use recording_on or session.start/session.resume, as appropriate, in this same live tool connection."
            }),
            Self::StorageUnavailable {
                request_id,
                action,
                protection,
                safety_error,
            } => {
                let shared = matches!(protection, CrossProcessProtection::PersistedDeny);
                let message = if shared {
                    "Canonical control storage failed. Shared deny protection remains active until explicit control recovery."
                } else {
                    "Local teaching/capture stopped in this running connection; durable storage failed. Restarted hook processes cannot inherit this in-memory stop. After recovery, explicitly use recording_on or session.start/session.resume in this same live tool connection; recovery in another process cannot clear its local stop."
                };
                json!({"schema":crate::learner::SCHEMA,"request_id":request_id,"action":action,
                    "local_stop":true,"durable":false,"capture_origin":"model_reported",
                    "restart_durability":if shared {RestartDurability::SafetyLatch} else {RestartDurability::Unavailable},
                    "cross_process_protection":protection,"safety_error":safety_error,
                    "error":{"code":"storage_unavailable","message":message}})
            }
        }
    }
}
/// A control stops only the calling conversation, matching its fail-closed
/// protection. Tutors often stop there, so after a committed pause or stop say
/// where the lesson is still recording; this read is guidance, not a guarantee.
fn note_lesson_active_elsewhere(
    store: &LearnerStore,
    host: &crate::learner::HostContext,
    receipt: &mut Value,
) {
    let others: Result<i64, _> = store.connection().query_row(
        "SELECT count(*) FROM host_bindings other
         WHERE other.learning_active = 1
           AND other.session_id = (SELECT session_id FROM host_bindings
                                   WHERE host_kind = ?1 AND host_thread_id = ?2)
           AND NOT (other.host_kind = ?1 AND other.host_thread_id = ?2)",
        rusqlite::params![host.host_kind, host.host_thread_id],
        |row| row.get(0),
    );
    if let Some(others) = others.ok().filter(|count| *count > 0) {
        receipt["lesson_active_in_other_conversations"] = json!(others);
        receipt["next_step"] = json!(format!(
            "This stopped capture in this conversation only; the lesson is still active in {others} other conversation(s). If the learner meant the lesson, also send session.pause for it, which stops every conversation."
        ));
    }
}
fn annotate_cleanup(receipt: &mut Value, cleanup: Result<(), String>) {
    if let Err(error) = cleanup {
        receipt["safety_error"] = json!(error);
        receipt["capture_admission"] = json!(CaptureAdmission::RequiresSafetyMetadataRecovery);
    }
}

type SuppressionToken = std::sync::Arc<()>;
#[derive(Default)]
pub(super) struct LocalSuppression {
    entries: std::collections::HashMap<String, SuppressionToken>,
}
impl LocalSuppression {
    pub(super) fn contains(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }
    pub(super) fn token(&self, key: &str) -> Option<SuppressionToken> {
        self.entries.get(key).cloned()
    }
    pub(super) fn insert(&mut self, key: String) -> SuppressionToken {
        let token = std::sync::Arc::new(());
        self.entries.insert(key, token.clone());
        token
    }
    // Never clear a newer overlapping failure with an older recovery result.
    pub(super) fn clear_if(&mut self, key: &str, observed: Option<&SuppressionToken>) {
        if let (Some(current), Some(observed)) = (self.entries.get(key), observed)
            && std::sync::Arc::ptr_eq(current, observed)
        {
            self.entries.remove(key);
        }
    }
    #[cfg(test)]
    pub(super) fn remove(&mut self, key: &str) -> bool {
        self.entries.remove(key).is_some()
    }
}
pub(super) fn suppressed() -> &'static std::sync::Mutex<LocalSuppression> {
    static SET: std::sync::OnceLock<std::sync::Mutex<LocalSuppression>> =
        std::sync::OnceLock::new();
    SET.get_or_init(Default::default)
}

pub(super) fn recording_suppressed() -> &'static std::sync::Mutex<LocalSuppression> {
    static SET: std::sync::OnceLock<std::sync::Mutex<LocalSuppression>> =
        std::sync::OnceLock::new();
    SET.get_or_init(Default::default)
}
pub(super) fn suppression_for(
    control: ControlAction,
) -> &'static std::sync::Mutex<LocalSuppression> {
    if control.recording() {
        recording_suppressed()
    } else {
        suppressed()
    }
}

pub(super) fn local_capture_suppressed(
    workspace: &Workspace,
    host: &crate::learner::HostContext,
) -> Result<bool, String> {
    let key = format!(
        "{}:{}:{}",
        sha256_hex(&crate::workspace::native_path_key(workspace.root())),
        host.host_kind,
        host.host_thread_id
    );
    Ok(suppressed()
        .lock()
        .map_err(|error| error.to_string())?
        .contains(&key)
        || recording_suppressed()
            .lock()
            .map_err(|error| error.to_string())?
            .contains(&key))
}

#[derive(Clone, Copy)]
enum UpdateOperation {
    Resume,
    CompleteCaptured,
    CaptureAndUpdate,
}
impl UpdateOperation {
    /// Decode only bounded admission identity here. The store's typed decoder
    /// validates the entire semantic request after independent permitted capture.
    fn from_envelope(request: &Value) -> Self {
        if matches!(
            serde_json::from_value::<crate::learner::UpdateMode>(request["mode"].clone()),
            Ok(crate::learner::UpdateMode::CompleteCaptured)
        ) {
            Self::CompleteCaptured
        } else if crate::learner::SessionEdit::accepts_capture_operation(
            request
                .pointer("/session/operation")
                .and_then(Value::as_str),
        ) {
            Self::Resume
        } else {
            Self::CaptureAndUpdate
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum CaptureBlock {
    SharedDeny,
    LocalDeny,
}
pub(crate) struct CaptureGuard {
    _guard: Guard,
    pub(crate) blocked: Option<CaptureBlock>,
}

struct ControlAttempt {
    guard: Guard,
    action: ControlAction,
    latched: Result<(), String>,
    local_before: Option<SuppressionToken>,
}
/// The control gate is keyed by workspace plus trusted native host/thread identity.
pub(crate) struct Admission<'a> {
    workspace: &'a Workspace,
    host: &'a HostContext,
    key: String,
}
impl<'a> Admission<'a> {
    pub(crate) fn new(workspace: &'a Workspace, host: &'a HostContext) -> Self {
        Self {
            workspace,
            host,
            key: suppression_key(workspace.root(), host),
        }
    }
    pub(crate) fn control(&self, store: &LearnerStore, request: Value) -> Result<Value, String> {
        let req: ControlRequest = crate::learner::update::decode_at(&request, "", self.host)
            .map_err(|e| format!("{e}. {CONTROL_SHAPE}"))?;
        let event = match control_event(store, self.host, &req) {
            Ok(event) => event,
            Err(ControlEventError::Input(error)) => {
                return Err(format!("{error}. {CONTROL_SHAPE}"));
            }
            Err(ControlEventError::Storage(error)) => {
                return storage_failure(
                    self.workspace.root(),
                    self.host,
                    "learning_step",
                    &request,
                    error.to_string(),
                );
            }
        };
        let attempt = match self.begin_control(req.control) {
            Ok(attempt) => attempt,
            Err(error) if req.control.stopping() => {
                return Ok(ControlResult::LocalOnly {
                    request_id: req.request_id,
                    action: req.control,
                    error,
                }
                .into_value());
            }
            Err(error) => return Err(error),
        };
        let canonical =
            store.control_with_outcome(self.host, &req.request_id, req.control.as_str(), &event);
        let applied_stop = matches!(req.control, ControlAction::Pause | ControlAction::Stop)
            && matches!(&canonical, Ok(outcome) if outcome.applied);
        let mut value = self.finish_control(attempt, canonical)?.into_value();
        if applied_stop {
            note_lesson_active_elsewhere(store, self.host, &mut value);
        }
        Ok(value)
    }
    fn begin_control(&self, action: ControlAction) -> Result<ControlAttempt, String> {
        let local_before = suppression_for(action)
            .lock()
            .map_err(|e| e.to_string())?
            .token(&self.key);
        let mut guard = match Guard::acquire(self.workspace, self.host) {
            Ok(guard) => guard,
            Err(error) => {
                // Without the cross-process gate only subsequent local calls are covered.
                if action.stopping() {
                    suppression_for(action)
                        .lock()
                        .map_err(|e| e.to_string())?
                        .insert(self.key.clone());
                }
                return Err(error);
            }
        };
        let latched = guard.before_control(action.as_str());
        Ok(ControlAttempt {
            guard,
            action,
            latched,
            local_before,
        })
    }
    fn finish_control(
        &self,
        mut attempt: ControlAttempt,
        canonical: crate::learner::Result<crate::learner::capture::ControlOutcome>,
    ) -> Result<ControlResult, String> {
        Ok(match canonical {
            Ok(outcome) if !outcome.applied => ControlResult::Replayed {
                receipt: outcome.receipt,
                cleanup: attempt.guard.after_replay(),
            },
            Ok(outcome) => {
                let cleanup = attempt.guard.after_control(attempt.action.as_str());
                if attempt.action.stopping() || cleanup.is_ok() {
                    suppression_for(attempt.action)
                        .lock()
                        .map_err(|e| e.to_string())?
                        .clear_if(&self.key, attempt.local_before.as_ref());
                }
                let protection = if attempt.action.stopping() {
                    CrossProcessProtection::CanonicalControl
                } else if attempt.latched.is_ok() {
                    CrossProcessProtection::Available
                } else {
                    CrossProcessProtection::Unavailable
                };
                ControlResult::Applied {
                    receipt: outcome.receipt,
                    cleanup,
                    protection,
                }
            }
            Err(error) => {
                if attempt.action.stopping() {
                    let mut local = suppression_for(attempt.action)
                        .lock()
                        .map_err(|e| e.to_string())?;
                    if attempt.latched.is_ok() {
                        local.clear_if(&self.key, attempt.local_before.as_ref());
                    } else {
                        local.insert(self.key.clone());
                    }
                }
                ControlResult::Failed {
                    error,
                    stopped: attempt.action.stopping(),
                    latched: attempt.latched,
                }
            }
        })
    }
    pub(crate) fn update(&self, store: &LearnerStore, request: Value) -> Result<Value, String> {
        let host = self.host;
        let suppression_key = &self.key;
        let learning_before = suppressed()
            .lock()
            .map_err(|e| e.to_string())?
            .token(suppression_key);
        let mut safety = Guard::acquire(self.workspace, host)?;
        match store.replay(host, &request) {
            Ok(Some(receipt)) => {
                return serde_json::to_value(receipt).map_err(|e| e.to_string());
            }
            Err(error) => {
                return Ok(json!({"schema":crate::learner::SCHEMA,"error":error}));
            }
            Ok(None) => {}
        }
        if safety.recording_blocked()
            || recording_suppressed()
                .lock()
                .map_err(|e| e.to_string())?
                .contains(suppression_key)
        {
            return Err(
                        "recording_disabled: recording_on in this same live tool connection is required after a local privacy stop"
                            .into(),
                    );
        }
        let native = if let Some(proof) = request.get("native_capture").filter(|v| !v.is_null()) {
            if request["schema"] != crate::learner::SCHEMA
                || request["action"] != "update"
                || !crate::learner::SessionEdit::accepts_capture_operation(
                    request
                        .pointer("/session/operation")
                        .and_then(serde_json::Value::as_str),
                )
            {
                return Err(format!(
                    "invalid_variant: native_capture requires explicit session.{}: {NATIVE_SESSION_SHAPE}",
                    crate::learner::SessionEdit::CAPTURE_OPERATIONS.join(" or session.")
                ));
            }
            let proof: crate::learner::NativeCaptureRequest =
                serde_json::from_value(proof.clone()).map_err(|e| e.to_string())?;
            let (_, _, recording) = store.binding(host).map_err(|e| e.to_string())?;
            if !recording {
                return Err("recording_disabled: native capture remains disabled".into());
            }
            Some(native_capture::verify(
                self.workspace,
                host,
                &proof.ticket,
                &proof.text,
                crate::learner::store::now_ms(),
            )?)
        } else {
            None
        };
        let operation = UpdateOperation::from_envelope(&request);
        let resuming = matches!(operation, UpdateOperation::Resume);
        if matches!(operation, UpdateOperation::CaptureAndUpdate)
            && (safety.learning_blocked()
                || suppressed()
                    .lock()
                    .map_err(|e| e.to_string())?
                    .contains(suppression_key))
        {
            return Err("recording_disabled: current host is locally stopped; explicitly start/resume in this same live tool connection after storage recovers".into());
        }
        match store.update_with_native_capture(host, request, native.as_ref()) {
            Ok(value) => {
                if native.is_some() {
                    // The transactional consumed-event guard also prevents replay if
                    // best-effort transient marker cleanup fails.
                    let _ = native_capture::invalidate(self.workspace, host);
                }
                if resuming {
                    suppressed()
                        .lock()
                        .map_err(|e| e.to_string())?
                        .clear_if(suppression_key, learning_before.as_ref());
                }
                let mut value = serde_json::to_value(value).map_err(|e| e.to_string())?;
                if resuming && let Err(error) = safety.after_resume() {
                    value["safety_error"] = json!(error);
                    value["capture_admission"] = json!("requires_safety_metadata_recovery");
                }
                Ok(value)
            }
            Err(error) => Ok(json!({"schema":crate::learner::SCHEMA,"error":error})),
        }
    }
    pub(crate) fn capture_guard(&self) -> Result<CaptureGuard, String> {
        let guard = Guard::acquire(self.workspace, self.host)?;
        let blocked = if guard.capture_blocked() {
            Some(CaptureBlock::SharedDeny)
        } else if local_capture_suppressed(self.workspace, self.host)? {
            Some(CaptureBlock::LocalDeny)
        } else {
            None
        };
        Ok(CaptureGuard {
            _guard: guard,
            blocked,
        })
    }
    /// The callback runs under the host gate only after the caller closes its snapshot.
    pub(crate) fn trace<T>(&self, store: &LearnerStore, finish: impl FnOnce(bool) -> T) -> T {
        let guard = self.capture_guard();
        let allowed = guard.as_ref().is_ok_and(|g| g.blocked.is_none())
            && store
                .binding(self.host)
                .is_ok_and(|(_, active, recording)| active && recording);
        finish(allowed)
    }
}
fn suppression_key(scope: &Path, host: &HostContext) -> String {
    format!(
        "{}:{}:{}",
        sha256_hex(&crate::workspace::native_path_key(scope)),
        host.host_kind,
        host.host_thread_id
    )
}
pub(crate) fn storage_failure(
    cwd: &Path,
    host: &crate::learner::HostContext,
    operation: &str,
    request: &serde_json::Value,
    error: String,
) -> Result<serde_json::Value, String> {
    let scope = Workspace::discover(cwd)
        .ok()
        .flatten()
        .map(|w| w.root().to_path_buf())
        .unwrap_or_else(|| cwd.to_path_buf());
    let key = suppression_key(&scope, host);
    let (result, token) =
        control_storage_failure_with_token(operation, request, &key, host, error)?;
    let ControlResult::StorageUnavailable {
        request_id, action, ..
    } = result
    else {
        unreachable!("fallback outcome")
    };
    let shared = (|| {
        let workspace = Workspace::discover(cwd)
            .map_err(|error| error.to_string())?
            .ok_or("No workspace for shared deny")?;
        let mut safety = Guard::acquire(&workspace, host)?;
        safety.before_control(action.as_str())
    })();
    let (protection, safety_error) = match shared {
        Ok(()) => {
            suppression_for(action)
                .lock()
                .map_err(|error| error.to_string())?
                .clear_if(&key, Some(&token));
            (CrossProcessProtection::PersistedDeny, None)
        }
        Err(error) => (CrossProcessProtection::Unavailable, Some(error)),
    };
    Ok(ControlResult::StorageUnavailable {
        request_id,
        action,
        protection,
        safety_error,
    }
    .into_value())
}

/// A tool-only current learner report can stop this running host even if the
/// database cannot open. A native event reference alone is not re-certified
/// without reading its current native capture; source/memory text cannot help.
#[cfg(test)]
pub(super) fn control_storage_failure(
    operation: &str,
    request: &serde_json::Value,
    key: &str,
    error: String,
) -> Result<serde_json::Value, String> {
    control_storage_failure_with_token(operation, request, key, &super::tool_host(), error)
        .map(|(result, _)| result.into_value())
}
fn control_storage_failure_with_token(
    operation: &str,
    request: &serde_json::Value,
    key: &str,
    host: &HostContext,
    error: String,
) -> Result<(ControlResult, SuppressionToken), String> {
    if operation != "learning_step" || request["action"] != "control" {
        return Err(error);
    }
    let control: ControlRequest =
        crate::learner::update::decode_at(request, "", host).map_err(|e| e.to_string())?;
    let input = validated_input(&control).map_err(|error| error.into_learner(host).to_string())?;
    if !control.control.stopping() {
        return Err(error);
    }
    if matches!(input, ControlInput::Native(_)) {
        return Err("control_unavailable: turn_ref cannot be validated while storage is unavailable; provide an explicitly model-reported current learner control to stop locally".into());
    }
    let token = suppression_for(control.control)
        .lock()
        .map_err(|e| e.to_string())?
        .insert(key.to_owned());
    Ok((
        ControlResult::StorageUnavailable {
            request_id: control.request_id,
            action: control.control,
            protection: CrossProcessProtection::Unavailable,
            safety_error: None,
        },
        token,
    ))
}

/// Validating a wire request is bounded and needs neither routing nor storage.
/// The scheduler and the admission owner use the same actual input contract.
pub(crate) fn valid_control_input(request: &Value) -> bool {
    serde_json::from_value::<ControlRequest>(request.clone())
        .is_ok_and(|request| validated_input(&request).is_ok())
}

enum ControlInput<'a> {
    Native(&'a str),
    Reported(&'a crate::learner::RawEvent),
}
struct ControlInputError {
    field: &'static str,
    detail: &'static str,
}
impl ControlInputError {
    fn into_learner(self, host: &HostContext) -> crate::learner::LearnerError {
        crate::learner::update::field_error(self.field, self.detail, host)
    }
}
fn validated_input(req: &ControlRequest) -> Result<ControlInput<'_>, ControlInputError> {
    let invalid = |field, detail| ControlInputError { field, detail };
    if req.request_id.is_empty() || req.request_id.len() > 256 {
        return Err(invalid("request_id", "requires 1–256 bytes"));
    }
    match (&req.turn_ref, &req.reported_event) {
        (None, None) => Err(invalid(
            "reported_event",
            "provide exactly one of turn_ref (current native learner event) or reported_event (current learner control)",
        )),
        (Some(_), Some(_)) => Err(invalid(
            "turn_ref",
            "provide exactly one of turn_ref or reported_event, not both",
        )),
        (Some(reference), None) => {
            if reference.is_empty() || reference.len() > crate::learner::limits::REFERENCE_BYTES {
                return Err(invalid(
                    "turn_ref",
                    "requires a nonempty current native learner event ID of at most 256 bytes",
                ));
            }
            Ok(ControlInput::Native(reference))
        }
        (None, Some(event)) => {
            if event.speaker != crate::learner::Speaker::Learner {
                return Err(invalid(
                    "reported_event.speaker",
                    "requires learner; source/tutor text cannot authorize control",
                ));
            }
            if event.external_event_key.is_empty() || event.external_event_key.len() > 512 {
                return Err(invalid(
                    "reported_event.external_event_key",
                    "requires 1–512 bytes identifying the current learner control",
                ));
            }
            Ok(ControlInput::Reported(event))
        }
    }
}

/// Only a genuine database failure can authorize deny-only storage recovery.
/// Invalid input never reaches the admission gate or any safety-state mutation.
#[derive(Debug)]
pub(super) enum ControlEventError {
    Input(crate::learner::LearnerError),
    Storage(rusqlite::Error),
}

pub(super) fn control_event(
    store: &crate::learner::LearnerStore,
    host: &crate::learner::HostContext,
    req: &ControlRequest,
) -> Result<crate::learner::RawEvent, ControlEventError> {
    use rusqlite::OptionalExtension;
    let input =
        validated_input(req).map_err(|error| ControlEventError::Input(error.into_learner(host)))?;
    let reference = match input {
        ControlInput::Reported(event) => {
            let mut event = event.clone();
            event.metadata = Some(crate::learner::EventMetadata {
                origin: Some("model_reported".into()),
                ..Default::default()
            });
            return Ok(event);
        }
        ControlInput::Native(reference) => reference,
    };
    let latest = store
        .conn
        .query_row(
            "SELECT event_id, external_event_key, kind, text, metadata_json, occurred_at_ms
         FROM learning_events
         WHERE host_kind = ?1 AND host_thread_id = ?2
           AND speaker = 'learner' AND origin = 'host_observed'
         ORDER BY event_no DESC LIMIT 1",
            rusqlite::params![host.host_kind, host.host_thread_id],
            |row| {
                let metadata =
                    serde_json::from_str(&row.get::<_, String>(4)?).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                Ok((
                    row.get::<_, String>(0)?,
                    crate::learner::RawEvent {
                        external_event_key: row.get(1)?,
                        speaker: crate::learner::Speaker::Learner,
                        kind: row.get(2)?,
                        text: row.get(3)?,
                        metadata,
                        occurred_at_ms: row.get(5)?,
                    },
                ))
            },
        )
        .optional()
        .map_err(ControlEventError::Storage)?;
    if let Some((id, event)) = latest
        && reference == id
    {
        return Ok(event);
    }
    let mut error = crate::learner::update::field_error(
        "turn_ref",
        "control requires the current native learner event for this host/thread",
        host,
    );
    error.code = "unknown_reference".into();
    Err(ControlEventError::Input(error))
}
