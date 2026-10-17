//! Actual MCP handler regressions. Worker and SQLite boundaries use explicit
//! synchronization; timeouts only bound failures, never establish ordering.
use super::*;
use serde_json::{Value, json};
use std::sync::Condvar;
use std::time::Duration;

struct TestTransport {
    incoming: tokio::sync::mpsc::Receiver<RxJsonRpcMessage<RoleServer>>,
    outgoing: tokio::sync::mpsc::Sender<TxJsonRpcMessage<RoleServer>>,
}
impl rmcp::transport::Transport<RoleServer> for TestTransport {
    type Error = io::Error;
    fn send(
        &mut self,
        message: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let outgoing = self.outgoing.clone();
        async move {
            outgoing
                .send(message)
                .await
                .map_err(|_| io::Error::other("test transport closed"))
        }
    }
    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
        self.incoming.recv().await
    }
    async fn close(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
struct Fixture {
    root: std::path::PathBuf,
    store: crate::learner::LearnerStore,
    host: crate::learner::HostContext,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir()
            .join(crate::learner::store::new_id("mcp-request-lifetime").unwrap());
        std::fs::create_dir_all(root.join(crate::workspace::CONTROL_DIR)).unwrap();
        std::fs::write(
            root.join(crate::workspace::CONTROL_DIR).join("config.toml"),
            "schema_version = 1\nworkspace_id = \"mcp-lifetime\"\n",
        )
        .unwrap();
        let workspace = crate::workspace::Workspace::discover(&root)
            .unwrap()
            .unwrap();
        let store =
            crate::learner::LearnerStore::initialize(&workspace.store_path(), "mcp-lifetime")
                .unwrap();
        let host = crate::learner::HostContext {
            host_kind: "codex".into(),
            host_thread_id: thread_id(),
        };
        store.update(&host, json!({"schema":crate::learner::SCHEMA,"action":"update","request_id":"start","session":{"operation":"start","client_ref":"s","goal":"request lifetime"}})).unwrap();
        Self { root, store, host }
    }
    fn server(&self, hook: RequestCheckpoint) -> MagnesiumServer {
        MagnesiumServer {
            identity: InvocationIdentity {
                plugin_version: Some(env!("CARGO_PKG_VERSION").into()),
                protocol_version: Some("2".into()),
            },
            bound_root: Some(self.root.clone()),
            hooks: Some(Arc::new(TestRequestHooks {
                cwd: self.root.clone(),
                checkpoint: hook,
            })),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
}
fn request(name: &str, arguments: Value) -> CallToolRequestParams {
    serde_json::from_value(json!({"name":name,"arguments":arguments})).unwrap()
}
fn thread_id() -> String {
    std::env::var("CODEX_THREAD_ID").unwrap_or_else(|_| "mcp-lifetime-thread".into())
}
fn context(peer: &rmcp::service::Peer<RoleServer>, id: i64) -> RequestContext<RoleServer> {
    let mut context = RequestContext::new(rmcp::model::RequestId::Number(id), peer.clone());
    context.meta.0.insert("threadId".into(), json!(thread_id()));
    context
}
type ServedFixture = (
    rmcp::service::RunningService<RoleServer, MagnesiumServer>,
    tokio::sync::mpsc::Sender<RxJsonRpcMessage<RoleServer>>,
    tokio::sync::mpsc::Receiver<TxJsonRpcMessage<RoleServer>>,
);

fn serve(server: MagnesiumServer) -> ServedFixture {
    let (incoming, rx) = tokio::sync::mpsc::channel(32);
    let (tx, outgoing) = tokio::sync::mpsc::channel(32);
    let info = serde_json::from_value(json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"codex-mcp-client","version":"test"}})).unwrap();
    (
        rmcp::service::serve_directly(
            server,
            TestTransport {
                incoming: rx,
                outgoing: tx,
            },
            Some(info),
        ),
        incoming,
        outgoing,
    )
}
fn control() -> CallToolRequestParams {
    request(
        "learning_step",
        json!({"schema":crate::learner::SCHEMA,"action":"control","request_id":"off","control":"recording_off","reported_event":{"external_event_key":"off-input","speaker":"learner","kind":"message","text":"Stop recording"}}),
    )
}
fn response_json(response: CallToolResponse) -> Value {
    serde_json::to_value(rmcp::model::ServerResult::from(response)).unwrap()
}

// RAII ensures assertions cannot leave blocking test jobs parked forever.
#[derive(Default)]
struct Gate {
    open: std::sync::Mutex<bool>,
    changed: Condvar,
}
impl Gate {
    fn wait(&self) {
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.changed.wait(open).unwrap();
        }
    }
    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }
}
struct Release(Arc<Gate>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[test]
fn saturated_ordinary_handlers_do_not_delay_control_or_async_frames() {
    let _serial = super::runtime_bounds_tests::SERIAL
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new();
    runtime().block_on(async {
        let gate = Arc::new(Gate::default());
        let _release = Release(gate.clone());
        let (entered, mut entered_rx) = tokio::sync::mpsc::unbounded_channel();
        let hook = Arc::new(move |name: &str, phase| {
            if name == "sources" && phase == "routing" {
                entered.send(()).unwrap();
                gate.wait();
            }
        });
        let (service, incoming, mut outgoing) = serve(fixture.server(hook));
        let server =
            Arc::new(fixture.server(service.service().hooks.as_ref().unwrap().checkpoint.clone()));
        let mut jobs = vec![];
        for id in 1..=8 {
            let server = server.clone();
            let context = context(service.peer(), id);
            jobs.push(tokio::spawn(async move {
                server
                    .call_tool(request("sources", json!({"action":"status"})), context)
                    .await
            }));
        }
        for _ in 0..8 {
            tokio::time::timeout(Duration::from_secs(2), entered_rx.recv())
                .await
                .unwrap()
                .unwrap();
        }
        assert_eq!(blocking_worker_available_permits(), 0);
        // The actual service loop handles another JSON-RPC frame while all
        // ordinary workers are held at the filesystem-routing boundary.
        incoming
            .send(
                serde_json::from_value(json!({"jsonrpc":"2.0","id":100,"method":"tools/list"}))
                    .unwrap(),
            )
            .await
            .unwrap();
        let frame = tokio::time::timeout(Duration::from_secs(2), outgoing.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(serde_json::to_value(frame).unwrap()["id"], 100);
        let reply = tokio::time::timeout(
            Duration::from_secs(2),
            server.call_tool(control(), context(service.peer(), 101)),
        )
        .await
        .unwrap()
        .unwrap();
        let value = response_json(reply);
        assert_ne!(value["isError"], true, "{value}");
        assert!(
            !fixture.store.binding(&fixture.host).unwrap().2,
            "control must commit before ordinary jobs are released"
        );
        _release.0.release();
        for job in jobs {
            job.await.unwrap().unwrap();
        }
        assert_eq!(blocking_worker_available_permits(), 8);
    });
}

#[test]
fn cancelled_queued_handler_never_starts_ordinary_work() {
    let _serial = super::runtime_bounds_tests::SERIAL
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new();
    runtime().block_on(async {
        let starts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = starts.clone();
        let (service, _incoming, _outgoing) = serve(fixture.server(Arc::new(move |_, phase| {
            if phase == "routing" {
                count.fetch_add(1, Ordering::Relaxed);
            }
        })));
        let mut permits = vec![];
        for _ in 0..8 {
            permits.push(blocking_worker_semaphore().acquire().await.unwrap());
        }
        let context = context(service.peer(), 1);
        let cancelled = context.ct.clone();
        let mut call = Box::pin(service.service().call_tool(
            request(
                "memory",
                json!({"schema":crate::learner::SCHEMA,"action":"browse"}),
            ),
            context,
        ));
        // Poll once to enter permit acquisition, then cancel without releasing
        // any permit. This establishes queued order without timing sleeps.
        std::future::poll_fn(|cx| {
            assert!(call.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        cancelled.cancel();
        let result = tokio::time::timeout(Duration::from_secs(2), call)
            .await
            .unwrap()
            .unwrap();
        assert!(
            response_json(result)
                .to_string()
                .contains("request_cancelled")
        );
        assert_eq!(starts.load(Ordering::Relaxed), 0);
        drop(permits);
        assert_eq!(blocking_worker_available_permits(), 8);
    });
}

fn add_memories(fixture: &Fixture) {
    fixture.store.conn.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<2048) INSERT INTO learner_memories(memory_id,title,scope,summary,state_text,current_basis_json,lifecycle,revision,created_at_ms,updated_at_ms) SELECT printf('m-%04d',x),'algebra','scope','Search target summary','whole state','[]','active',1,0,0 FROM n;").unwrap();
    crate::learner::search::reindex(&fixture.store.conn).unwrap();
}

#[test]
fn cancelled_running_handler_interrupts_ranked_sqlite_scan() {
    let _serial = super::runtime_bounds_tests::SERIAL
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new();
    add_memories(&fixture);
    runtime().block_on(async {
        let gate = Arc::new(Gate::default()); let _release = Release(gate.clone());
        let (entered, mut entered_rx) = tokio::sync::mpsc::unbounded_channel();
        let ranking = AtomicBool::new(false);
        let paused = AtomicBool::new(false);
        let hook = Arc::new(move |_: &str, phase| {
            if phase == "learner_ranking" { ranking.store(true, Ordering::Relaxed); }
            if phase == "learner_read_progress" && ranking.load(Ordering::Relaxed) && !paused.swap(true, Ordering::Relaxed) { entered.send(()).unwrap(); gate.wait(); }
        });
        let (service, _incoming, _outgoing) = serve(fixture.server(hook.clone()));
        let server = Arc::new(fixture.server(hook));
        let context = context(service.peer(), 1); let cancelled = context.ct.clone();
        let task = tokio::spawn(async move { server.call_tool(request("memory", json!({"schema":crate::learner::SCHEMA,"action":"search","intents":[{"id":"algebra","purpose":"matching","queries":[{"any_of":["algebra"]}]}]})), context).await });
        tokio::time::timeout(Duration::from_secs(2), entered_rx.recv()).await.unwrap().unwrap();
        cancelled.cancel(); tokio::task::yield_now().await;
        _release.0.release();
        let reply = tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap().unwrap();
        assert!(response_json(reply).to_string().contains("request_cancelled"));
        assert_eq!(fixture.store.conn.query_row("SELECT count(*) FROM context_receipts", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    });
}

#[test]
fn handler_cancellation_at_semantic_and_commit_boundaries_keeps_receipts_truthful() {
    let _serial = super::runtime_bounds_tests::SERIAL
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    for phase in ["before_semantic", "before_commit", "after_commit"] {
        let fixture = Fixture::new();
        runtime().block_on(async {
            let gate = Arc::new(Gate::default()); let _release = Release(gate.clone());
            let (entered, mut entered_rx) = tokio::sync::mpsc::unbounded_channel();
            let hook = Arc::new(move |_: &str, observed| { if observed == phase { entered.send(()).unwrap(); gate.wait(); } });
            let (service, _incoming, _outgoing) = serve(fixture.server(hook.clone()));
            let server = Arc::new(fixture.server(hook));
            let context = context(service.peer(), 1); let cancelled = context.ct.clone();
            let payload = json!({"schema":crate::learner::SCHEMA,"action":"update","request_id":"cancellable-update","reported_events":[{"external_event_key":"cancellable-event","speaker":"learner","kind":"message","text":"independent raw input"}]});
            let sent = payload.clone();
            let task = tokio::spawn(async move { server.call_tool(request("learning_step", sent), context).await });
            tokio::time::timeout(Duration::from_secs(2), entered_rx.recv()).await.unwrap().unwrap();
            cancelled.cancel(); tokio::task::yield_now().await;
            _release.0.release();
            let reply = tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap().unwrap();
            let reply = response_json(reply);
            let receipts: i64 = fixture.store.conn.query_row("SELECT count(*) FROM operation_receipts WHERE request_id='cancellable-update'", [], |r| r.get(0)).unwrap();
            assert_eq!(fixture.store.conn.query_row("SELECT count(*) FROM learning_events WHERE external_event_key='cancellable-event' AND processing='pending'", [], |r| r.get::<_,i64>(0)).unwrap(), 1);
            if phase == "after_commit" {
                assert!(!reply.to_string().contains("request_cancelled"), "{reply}");
                assert_eq!(receipts, 1);
                let replay = fixture.store.update(&fixture.host, payload).unwrap();
                assert_eq!(replay.update_status, "committed");
                assert_eq!(replay.captured_event_ids.len(), 1);
            } else {
                assert!(reply.to_string().contains("request_cancelled"), "{reply}");
                assert_eq!(receipts, 0);
            }
            assert!(fixture.store.binding(&fixture.host).unwrap().2);
        });
    }
}

#[test]
fn dropped_admitted_control_finishes_safety_and_releases_reserved_permit() {
    let _serial = super::runtime_bounds_tests::SERIAL
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new();
    runtime().block_on(async {
        let gate = Arc::new(Gate::default());
        let _release = Release(gate.clone());
        let (entered, mut entered_rx) = tokio::sync::mpsc::unbounded_channel();
        let (finished, mut finished_rx) = tokio::sync::mpsc::unbounded_channel();
        let hook = Arc::new(move |_: &str, phase| {
            if phase == "dispatch" {
                entered.send(()).unwrap();
                gate.wait();
            }
            if phase == "worker_finished" {
                finished.send(()).unwrap();
            }
        });
        let (service, _incoming, _outgoing) = serve(fixture.server(hook.clone()));
        let server = Arc::new(fixture.server(hook));
        let context = context(service.peer(), 1);
        let task = tokio::spawn(async move { server.call_tool(control(), context).await });
        tokio::time::timeout(Duration::from_secs(2), entered_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(control_worker_semaphore().available_permits(), 1);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(
            control_worker_semaphore().available_permits(),
            1,
            "dropped caller cannot free a running worker's permit"
        );
        _release.0.release();
        tokio::time::timeout(Duration::from_secs(2), finished_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(!fixture.store.binding(&fixture.host).unwrap().2);
        // Acquiring both reserved permits proves the detached worker released
        // its permit only after finishing, without polling an elapsed timer.
        let one = control_worker_semaphore().acquire().await.unwrap();
        let two = control_worker_semaphore().acquire().await.unwrap();
        drop((one, two));
        assert_eq!(control_worker_semaphore().available_permits(), 2);
    });
}

#[test]
fn handler_cancellation_before_trace_skips_optional_persistence() {
    let _serial = super::runtime_bounds_tests::SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    runtime().block_on(async {
        let gate = Arc::new(Gate::default());
        let _release = Release(gate.clone());
        let (entered, mut entered_rx) = tokio::sync::mpsc::unbounded_channel();
        let hook = Arc::new(move |_: &str, phase| {
            if phase == "before_trace" {
                entered.send(()).unwrap();
                gate.wait();
            }
        });
        let (service, _incoming, _outgoing) = serve(fixture.server(hook.clone()));
        let server = Arc::new(fixture.server(hook));
        let context = context(service.peer(), 1);
        let cancelled = context.ct.clone();
        let task = tokio::spawn(async move {
            server
                .call_tool(
                    request(
                        "context",
                        json!({"schema":crate::learner::SCHEMA,"action":"status"}),
                    ),
                    context,
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(2), entered_rx.recv())
            .await
            .unwrap()
            .unwrap();
        cancelled.cancel();
        tokio::task::yield_now().await;
        _release.0.release();
        let reply = task.await.unwrap().unwrap();
        let packet: Value =
            serde_json::from_str(response_json(reply)["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(packet["trace_status"], "disabled");
        assert_eq!(
            fixture
                .store
                .conn
                .query_row("SELECT count(*) FROM context_receipts", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    });
}

#[test]
fn cancelled_recording_on_does_not_expand_capture_permission() {
    let _serial = super::runtime_bounds_tests::SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for phase in ["before_control_grant", "before_control_commit"] {
        let fixture = Fixture::new();
        fixture
            .store
            .conn
            .execute("UPDATE host_bindings SET recording_enabled=0", [])
            .unwrap();
        runtime().block_on(async {
            let gate = Arc::new(Gate::default());
            let _release = Release(gate.clone());
            let (entered, mut entered_rx) = tokio::sync::mpsc::unbounded_channel();
            let hook = Arc::new(move |_: &str, observed| {
                if observed == phase {
                    entered.send(()).unwrap();
                    gate.wait();
                }
            });
            let (service, _incoming, _outgoing) = serve(fixture.server(hook.clone()));
            let server = Arc::new(fixture.server(hook));
            let context = context(service.peer(), 1);
            let cancelled = context.ct.clone();
            let mut on = control();
            on.arguments
                .as_mut()
                .unwrap()
                .insert("control".into(), json!("recording_on"));
            let task = tokio::spawn(async move { server.call_tool(on, context).await });
            tokio::time::timeout(Duration::from_secs(2), entered_rx.recv())
                .await
                .unwrap()
                .unwrap();
            cancelled.cancel();
            tokio::task::yield_now().await;
            _release.0.release();
            let reply = task.await.unwrap().unwrap();
            assert!(
                response_json(reply)
                    .to_string()
                    .contains("request_cancelled")
            );
            assert!(!fixture.store.binding(&fixture.host).unwrap().2);
            assert_eq!(
                fixture
                    .store
                    .conn
                    .query_row(
                        "SELECT count(*) FROM operation_receipts WHERE request_id='off'",
                        [],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                0
            );
        });
    }
}

#[cfg(unix)]
#[test]
fn canonical_connection_binding_rejects_retargeted_root_symlink() {
    let _serial = super::runtime_bounds_tests::SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let first = Fixture::new();
    let second = Fixture::new();
    let link = first.root.join("bound-root");
    std::os::unix::fs::symlink(&first.root, &link).unwrap();
    let pinned = pin_binding(Some(link.clone()));
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&second.root, &link).unwrap();
    runtime().block_on(async {
        let mut server = second.server(Arc::new(|_, _| {}));
        server.bound_root = pinned;
        let (service, _incoming, _outgoing) = serve(server);
        let reply = service
            .service()
            .call_tool(control(), context(service.peer(), 1))
            .await
            .unwrap();
        assert!(
            response_json(reply)
                .to_string()
                .contains("workspace_route_invalid")
        );
        assert!(first.store.binding(&first.host).unwrap().2);
        assert!(second.store.binding(&second.host).unwrap().2);
    });
}

#[test]
fn malformed_control_does_not_use_reserved_capacity() {
    let _serial = super::runtime_bounds_tests::SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    runtime().block_on(async {
        let (service, _incoming, _outgoing) = serve(fixture.server(Arc::new(|_, _| {
            panic!("cancelled malformed control must never reach a worker")
        })));
        let mut permits = vec![];
        for _ in 0..8 {
            permits.push(blocking_worker_semaphore().acquire().await.unwrap());
        }
        let context = context(service.peer(), 1);
        let cancelled = context.ct.clone();
        let mut malformed = control();
        malformed
            .arguments
            .as_mut()
            .unwrap()
            .remove("reported_event");
        let mut call = Box::pin(service.service().call_tool(malformed, context));
        std::future::poll_fn(|cx| {
            assert!(call.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert_eq!(control_worker_semaphore().available_permits(), 2);
        cancelled.cancel();
        let reply = call.await.unwrap();
        assert!(
            response_json(reply)
                .to_string()
                .contains("request_cancelled")
        );
        drop(permits);
    });
}
