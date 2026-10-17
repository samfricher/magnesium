use std::io;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, ErrorData, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::{RoleServer, ServerHandler, ServiceExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

use super::catalog::{get_tool, tool_definitions};
use super::{check_routing, compatibility_error, routing_error, tool_error, tool_success};
use crate::adapter::{self, InvocationIdentity};
use crate::request::{OperationCancellation, enter_request_cancellation};

const MAX_FRAME_BYTES: usize = 1024 * 1024;
const READ_CHUNK_BYTES: usize = 8 * 1024;

/// Ordinary source/read work and validated controls have separate bounded
/// capacity. Eight ordinary jobs cannot starve a stop/off request; the two
/// control workers still obey the existing host-scoped admission and SQLite
/// locking. No filesystem or database work runs on the frame executor.
const BLOCKING_WORKER_PERMITS: usize = 8;
const CONTROL_WORKER_PERMITS: usize = 2;

fn blocking_worker_semaphore() -> &'static tokio::sync::Semaphore {
    static SEMAPHORE: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    SEMAPHORE.get_or_init(|| tokio::sync::Semaphore::new(BLOCKING_WORKER_PERMITS))
}
fn control_worker_semaphore() -> &'static tokio::sync::Semaphore {
    static SEMAPHORE: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    SEMAPHORE.get_or_init(|| tokio::sync::Semaphore::new(CONTROL_WORKER_PERMITS))
}

#[cfg(test)]
pub(crate) fn blocking_worker_permits() -> usize {
    BLOCKING_WORKER_PERMITS
}

#[cfg(test)]
pub(crate) fn blocking_worker_available_permits() -> usize {
    blocking_worker_semaphore().available_permits()
}

/// Bridge an MCP request cancellation signal into the blocking worker's
/// cancellation token. The returned flag is observed by
/// [`OperationCancellation`] checks on the worker thread; `cancelled` is the
/// request's `CancellationToken::cancelled()` future (or a test future). The
/// returned watcher task must be aborted when the request completes, because
/// normal completion never cancels the request token.
fn bridge_cancellation<F>(cancelled: F) -> (Arc<AtomicBool>, tokio::task::JoinHandle<()>)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let flag = Arc::new(AtomicBool::new(false));
    let watcher_flag = Arc::clone(&flag);
    let watcher = tokio::spawn(async move {
        cancelled.await;
        watcher_flag.store(true, Ordering::Relaxed);
    });
    (flag, watcher)
}

/// Signals running work and releases the watcher when the handler future
/// completes or is dropped. Normal MCP completion does not cancel the request
/// token, so the watcher must be aborted here instead of parking until the
/// process exits.
struct CancelOnDrop {
    flag: Arc<AtomicBool>,
    watcher: tokio::task::JoinHandle<()>,
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.flag.store(true, Ordering::Relaxed);
        self.watcher.abort();
    }
}

struct WorkerRequest {
    bound_root: Option<std::path::PathBuf>,
    name: String,
    arguments: Option<rmcp::model::JsonObject>,
    host: Option<crate::learner::HostContext>,
    finish_safety_control: bool,
    #[cfg(test)]
    hooks: Option<Arc<TestRequestHooks>>,
}

async fn dispatch_on_blocking_worker<F>(
    work: WorkerRequest,
    cancellation: OperationCancellation,
    cancelled: F,
    control: bool,
) -> CallToolResponse
where
    F: std::future::Future<Output = ()> + Send,
{
    let semaphore = if control {
        control_worker_semaphore()
    } else {
        blocking_worker_semaphore()
    };
    let permit = tokio::select! {
        biased;
        () = cancelled => return tool_error("invoke tool", "request_cancelled: queued work did not start"),
        permit = semaphore.acquire() => match permit {
            Ok(permit) => permit,
            Err(_) => return tool_error("schedule blocking tool call", "blocking worker shut down"),
        }
    };
    match tokio::task::spawn_blocking(move || {
        let _permit = permit;
        // Stop/off controls that reach a worker finish their safety transition even if
        // their caller disappears. Ordinary jobs keep the same token through
        // routing, source preparation, learner work and optional tracing.
        let effective = if work.finish_safety_control {
            OperationCancellation::none()
        } else {
            cancellation
        };
        let _scope = enter_request_cancellation(&effective);
        if effective.is_cancelled() {
            return tool_error("invoke tool", "request_cancelled: work did not start");
        }
        #[cfg(test)]
        crate::request::test_checkpoint("worker_start");
        #[cfg(test)]
        if let Some(hooks) = &work.hooks {
            (hooks.checkpoint)(&work.name, "routing");
        }
        let cwd = {
            #[cfg(test)]
            if let Some(hooks) = &work.hooks {
                Ok(hooks.cwd.clone())
            } else {
                std::env::current_dir()
            }
            #[cfg(not(test))]
            std::env::current_dir()
        };
        let cwd = match cwd {
            Ok(cwd) => cwd,
            Err(error) => return tool_error("determine current directory", &error.to_string()),
        };
        if let Err(error) = check_binding(work.bound_root.as_deref(), &cwd) {
            return routing_error(error);
        }
        let independent_control = work.name == "learning_step"
            && work
                .arguments
                .as_ref()
                .and_then(|args| args.get("action"))
                .and_then(serde_json::Value::as_str)
                == Some("control");
        if !is_read_only_instruction_call(&work.name, work.arguments.as_ref())
            && !independent_control
            && let Err(error) = check_routing(&cwd)
        {
            return routing_error(error);
        }
        if effective.is_cancelled() {
            return tool_error(
                "invoke tool",
                "request_cancelled: routing completed without starting domain work",
            );
        }
        #[cfg(test)]
        if let Some(hooks) = &work.hooks {
            (hooks.checkpoint)(&work.name, "dispatch");
        }
        let result = match dispatch_tool_call(&cwd, &work.name, work.arguments, work.host) {
            Ok(value) => tool_success(value),
            Err((action, error)) => tool_error(&action, &error),
        };
        #[cfg(test)]
        if let Some(hooks) = &work.hooks {
            (hooks.checkpoint)(&work.name, "worker_finished");
        }
        result
    })
    .await
    {
        Ok(result) => result,
        Err(_) => tool_error(
            "join blocking tool call",
            "blocking tool worker was cancelled",
        ),
    }
}

#[cfg(test)]
type RequestCheckpoint = Arc<dyn Fn(&str, &'static str) + Send + Sync>;

#[cfg(test)]
struct TestRequestHooks {
    cwd: std::path::PathBuf,
    checkpoint: RequestCheckpoint,
}

fn pin_binding(root: Option<std::path::PathBuf>) -> Option<std::path::PathBuf> {
    root.and_then(|root| std::fs::canonicalize(&root).ok().or(Some(root)))
}

fn check_binding(
    bound: Option<&std::path::Path>,
    cwd: &std::path::Path,
) -> Result<(), super::RoutingFailure> {
    let Some(bound) = bound else {
        return Ok(());
    };
    let workspace =
        crate::workspace::Workspace::discover(cwd).map_err(super::RoutingFailure::from)?;
    if workspace
        .is_some_and(|workspace| crate::adapter::host::binding_matches(bound, workspace.root()))
    {
        Ok(())
    } else {
        Err(super::RoutingFailure::from(std::io::Error::other(
            "workspace binding mismatch",
        )))
    }
}

fn is_read_only_instruction_call(name: &str, arguments: Option<&rmcp::model::JsonObject>) -> bool {
    name == "context"
        && arguments
            .and_then(|arguments| arguments.get("action"))
            .and_then(serde_json::Value::as_str)
            == Some("instructions")
}

pub(crate) struct MagnesiumServer {
    identity: InvocationIdentity,
    bound_root: Option<std::path::PathBuf>,
    #[cfg(test)]
    hooks: Option<Arc<TestRequestHooks>>,
}

impl MagnesiumServer {
    fn bound(identity: InvocationIdentity) -> Self {
        Self {
            identity,
            // Pin the initial canonical binding before the frame runtime starts.
            // Each worker revalidates this fixed root before domain dispatch.
            bound_root: pin_binding(crate::adapter::host::explicit_root()),
            #[cfg(test)]
            hooks: None,
        }
    }
}

impl ServerHandler for MagnesiumServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("magnesium", env!("CARGO_PKG_VERSION")))
            .with_instructions(format!(
                "Magnesium tools self-gate on the nearest initialized {} workspace.",
                crate::workspace::CONTROL_DIR
            ))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if let Err(error) = adapter::validate(&self.identity) {
            return Ok(compatibility_error(&self.identity, error));
        }
        let client_name = context
            .peer
            .peer_info()
            .map(|info| info.client_info.name.clone());
        let native_host = match native_host_from_meta(client_name.as_deref(), &context.meta.0) {
            Ok(host) => host,
            Err(error) => return Ok(tool_error("resolve native host", &error)),
        };
        // Only bounded in-memory transport validation belongs here. The same
        // typed control validator owns both reservation and domain admission.
        let control = request.name == "learning_step"
            && request.arguments.as_ref().is_some_and(|arguments| {
                adapter::control_admission::valid_control_input(&serde_json::Value::Object(
                    arguments.clone(),
                ))
            });
        let finish_safety_control = control
            && request
                .arguments
                .as_ref()
                .and_then(|args| args.get("control"))
                .and_then(serde_json::Value::as_str)
                != Some("recording_on");
        let request_ct = context.ct.clone();
        let (cancellation_flag, watcher) =
            bridge_cancellation(async move { request_ct.cancelled().await });
        let _cancel_on_drop = CancelOnDrop {
            flag: Arc::clone(&cancellation_flag),
            watcher,
        };
        let cancellation = OperationCancellation::with_flag(cancellation_flag);
        #[cfg(test)]
        let cancellation = if let Some(hooks) = &self.hooks {
            let hook = hooks.checkpoint.clone();
            let name = request.name.clone().into_owned();
            cancellation.with_test_hook(Arc::new(move |phase| hook(&name, phase)))
        } else {
            cancellation
        };
        Ok(dispatch_on_blocking_worker(
            WorkerRequest {
                bound_root: self.bound_root.clone(),
                name: request.name.into_owned(),
                arguments: request.arguments,
                host: native_host,
                finish_safety_control,
                #[cfg(test)]
                hooks: self.hooks.clone(),
            },
            cancellation,
            context.ct.cancelled(),
            control,
        )
        .await)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let result = ListToolsResult::with_all_items(tool_definitions());
        // MCP 2026-07-28 requires cache hints on list results; Claude Code
        // rejects the whole list without them. Older peers keep the legacy shape.
        if context
            .protocol_version()
            .is_some_and(|version| version >= rmcp::model::ProtocolVersion::V_2026_07_28)
        {
            return Ok(result
                .with_ttl_ms(0)
                .with_cache_scope(rmcp::model::CacheScope::Private));
        }
        Ok(result)
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        get_tool(name)
    }
}

/// Codex supplies this envelope itself for every call, independently of model
/// arguments. One app-server connection may serve many conversation threads.
fn native_host_from_meta(
    client_name: Option<&str>,
    meta: &rmcp::model::MetaObject,
) -> Result<Option<crate::learner::HostContext>, String> {
    if client_name != Some("codex-mcp-client") {
        return Ok(None);
    }
    let Some(thread) = meta.get("threadId") else {
        return Ok(None);
    };
    let thread = thread.as_str().ok_or("invalid native threadId metadata")?;
    let host = crate::adapter::host::HostIdentity::codex(thread)?;
    // Claude Code exports its session ID to every subprocess, including a
    // Codex started from a Claude shell, so it cannot contradict Codex metadata.
    for (kind, key) in crate::adapter::host::HOST_SESSION_ENV {
        if kind == crate::adapter::host::HostKind::Claude {
            continue;
        }
        if let Ok(value) = std::env::var(key) {
            let observed = crate::adapter::host::HostIdentity::new(kind, &value)?;
            if !kind.is_codex() || observed.stored_session_id() != host.stored_session_id() {
                return Err("native host metadata conflicts with process identity".into());
            }
        }
    }
    Ok(Some(crate::learner::HostContext {
        host_kind: "codex".into(),
        host_thread_id: host.stored_session_id(),
    }))
}

fn dispatch_tool_call(
    cwd: &std::path::Path,
    name: &str,
    arguments: Option<rmcp::model::JsonObject>,
    host: Option<crate::learner::HostContext>,
) -> Result<serde_json::Value, (String, String)> {
    crate::adapter::dispatch_with_host(
        cwd,
        name,
        serde_json::Value::Object(arguments.unwrap_or_default()),
        host,
    )
    .map_err(|error| (format!("invoke {name}"), error))
}

pub(crate) fn run(
    plugin_version: Option<String>,
    protocol_version: Option<String>,
    host_kind: Option<String>,
) -> ExitCode {
    if let Some(kind) = host_kind {
        match crate::adapter::host::HostKind::parse(&kind) {
            Ok(kind) => crate::adapter::host::set_launch_host(kind),
            Err(error) => {
                eprintln!("Magnesium MCP server: --host-kind {kind}: {error}");
                return ExitCode::from(2);
            }
        }
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Magnesium MCP runtime initialization failed: {error}");
            return ExitCode::from(1);
        }
    };
    let server = MagnesiumServer::bound(InvocationIdentity {
        plugin_version,
        protocol_version,
    });
    let result = runtime.block_on(async {
        let service = server
            .serve(BoundedStdio::new())
            .await
            .map_err(|error| error.to_string())?;
        service.waiting().await.map_err(|error| error.to_string())
    });
    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Magnesium MCP server failed: {error}");
            ExitCode::from(1)
        }
    }
}

struct BoundedStdio {
    input: tokio::io::Stdin,
    output: Arc<Mutex<tokio::io::Stdout>>,
    buffered: Vec<u8>,
    discarding_oversized_frame: bool,
}

impl BoundedStdio {
    fn new() -> Self {
        Self {
            input: tokio::io::stdin(),
            output: Arc::new(Mutex::new(tokio::io::stdout())),
            buffered: Vec::with_capacity(READ_CHUNK_BYTES),
            discarding_oversized_frame: false,
        }
    }

    async fn receive_frame(&mut self) -> Option<Vec<u8>> {
        loop {
            if let Some(newline) = self.buffered.iter().position(|byte| *byte == b'\n') {
                let mut frame: Vec<u8> = self.buffered.drain(..=newline).collect();
                let oversized = self.discarding_oversized_frame || frame.len() > MAX_FRAME_BYTES;
                self.discarding_oversized_frame = false;
                if oversized {
                    continue;
                }
                frame.pop();
                if frame.last() == Some(&b'\r') {
                    frame.pop();
                }
                if !frame.is_empty() {
                    return Some(frame);
                }
                continue;
            }

            if self.buffered.len() > MAX_FRAME_BYTES {
                self.buffered.clear();
                self.discarding_oversized_frame = true;
            }

            let mut chunk = [0_u8; READ_CHUNK_BYTES];
            match self.input.read(&mut chunk).await {
                Ok(0) => return None,
                Ok(length) => self.buffered.extend_from_slice(&chunk[..length]),
                Err(_) => return None,
            }
        }
    }
}

impl rmcp::transport::Transport<RoleServer> for BoundedStdio {
    type Error = io::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send + 'static {
        let output = Arc::clone(&self.output);
        async move {
            let bytes = serde_json::to_vec(&item).map_err(io::Error::other)?;
            let mut output = output.lock().await;
            output.write_all(&bytes).await?;
            output.write_all(b"\n").await?;
            output.flush().await
        }
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
        loop {
            let frame = self.receive_frame().await?;
            if let Ok(message) = serde_json::from_slice(&frame) {
                return Some(message);
            }
        }
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        self.output.lock().await.flush().await
    }
}

#[cfg(test)]
mod runtime_bounds_tests {
    #[test]
    fn only_instruction_context_bypasses_runtime_access_lease() {
        let instructions = serde_json::from_value::<rmcp::model::JsonObject>(serde_json::json!({
            "action": "instructions"
        }))
        .expect("instruction arguments");
        let status = serde_json::from_value::<rmcp::model::JsonObject>(serde_json::json!({
            "action": "status"
        }))
        .expect("status arguments");
        assert!(super::is_read_only_instruction_call(
            "context",
            Some(&instructions)
        ));
        assert!(!super::is_read_only_instruction_call(
            "context",
            Some(&status)
        ));
        assert!(!super::is_read_only_instruction_call(
            "practice",
            Some(&instructions)
        ));
        assert!(!super::is_read_only_instruction_call("context", None));
    }

    use super::*;
    use std::sync::Mutex;

    pub(super) static SERIAL: Mutex<()> = Mutex::new(());

    #[test]
    fn blocking_worker_bound_is_fixed_and_initially_idle() {
        let _serial = SERIAL.lock().expect("serial worker tests");
        assert_eq!(blocking_worker_permits(), 8);
        assert_eq!(blocking_worker_available_permits(), 8);
    }

    #[test]
    fn native_thread_identity_only_uses_valid_codex_envelope_metadata() {
        let empty = rmcp::model::MetaObject::default();
        assert!(
            native_host_from_meta(Some("codex-mcp-client"), &empty)
                .unwrap()
                .is_none()
        );
        let malicious: rmcp::model::MetaObject =
            serde_json::from_value(serde_json::json!({"threadId": "another-conversation"}))
                .unwrap();
        assert!(
            native_host_from_meta(Some("other-client"), &malicious)
                .unwrap()
                .is_none()
        );
        for value in [
            serde_json::json!(12),
            serde_json::json!(""),
            serde_json::json!("bad\nthread"),
        ] {
            let meta: rmcp::model::MetaObject =
                serde_json::from_value(serde_json::json!({"threadId":value})).unwrap();
            assert!(native_host_from_meta(Some("codex-mcp-client"), &meta).is_err());
        }
    }

    #[test]
    fn request_cancellation_bridge_signals_running_work() {
        let _serial = SERIAL.lock().expect("serial worker tests");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            // A cancellation signal sets the flag.
            let (sender, receiver) = tokio::sync::oneshot::channel::<()>();
            let (flag, watcher) = bridge_cancellation(async move {
                let _ = receiver.await;
            });
            tokio::task::yield_now().await;
            assert!(
                !flag.load(Ordering::Relaxed),
                "an uncancelled request must not signal work"
            );
            sender.send(()).expect("signal request cancellation");
            let signalled = tokio::time::timeout(std::time::Duration::from_secs(1), async {
                while !flag.load(Ordering::Relaxed) {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
            .await
            .is_ok();
            assert!(signalled, "request cancellation must signal running work");
            drop(CancelOnDrop {
                flag: Arc::clone(&flag),
                watcher,
            });

            // A completed request releases a still-parked watcher instead of
            // leaving it alive until process exit. `CancelOnDrop` performs the
            // abort; normal MCP completion never cancels the request token.
            let baseline = tokio::runtime::Handle::current()
                .metrics()
                .num_alive_tasks();
            let (flag, watcher) = bridge_cancellation(std::future::pending());
            let guard = CancelOnDrop {
                flag: Arc::clone(&flag),
                watcher,
            };
            tokio::task::yield_now().await;
            assert!(
                tokio::runtime::Handle::current()
                    .metrics()
                    .num_alive_tasks()
                    > baseline,
                "the watcher must be alive before the guard drops"
            );
            drop(guard);
            let released = tokio::time::timeout(std::time::Duration::from_secs(1), async {
                while tokio::runtime::Handle::current()
                    .metrics()
                    .num_alive_tasks()
                    > baseline
                {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
            .await
            .is_ok();
            assert!(
                released,
                "completed requests must release the cancellation watcher"
            );
        });
    }
}

#[cfg(test)]
mod request_lifetime_tests;
