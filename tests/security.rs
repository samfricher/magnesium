use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde_json::{Value, json};
use support::TestEnvironment;

fn output_text(output: &std::process::Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

#[test]
fn malformed_config_does_not_echo_private_source_text_across_cli_boundary() {
    let environment = TestEnvironment::from_fixture("basic").expect("create security fixture");
    let init = environment
        .command()
        .arg("init")
        .output()
        .expect("run init");
    assert!(init.status.success(), "init failed: {}", output_text(&init));

    fs::write(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("config.toml"),
        include_bytes!("fixtures/security/malformed-config.toml"),
    )
    .expect("write malformed config");
    let info = environment
        .command()
        .args(["info", "--json"])
        .output()
        .expect("run info");
    let text = output_text(&info);
    assert!(!text.contains("SECURITY_CONFIG_SENTINEL_7f3c"));
    assert!(text.contains("invalid TOML"), "missing parse cause: {text}");
}

#[test]
fn corrupt_canonical_store_is_not_replaced_and_reports_recovery_guidance() {
    let environment = TestEnvironment::from_fixture("basic").expect("create security fixture");
    let init = environment
        .command()
        .arg("init")
        .output()
        .expect("run init");
    assert!(init.status.success(), "init failed: {}", output_text(&init));

    let store = environment
        .workspace
        .join(support::CONTROL_DIR)
        .join("state.sqlite");
    let sentinel = b"canonical-corruption-sentinel";
    fs::write(&store, sentinel).expect("corrupt canonical store");
    let info = environment
        .command()
        .args(["info", "--json"])
        .output()
        .expect("run info");
    let text = output_text(&info);
    assert!(
        !info.status.success(),
        "corrupt store must be rejected: {text}"
    );
    assert_eq!(fs::read(&store).expect("read canonical store"), sentinel);
    assert!(!text.contains("canonical-corruption-sentinel"), "{text}");
}

#[test]
fn source_secret_fixture_is_excluded_before_the_mcp_search_boundary() {
    let environment = TestEnvironment::from_fixture("basic").expect("create security fixture");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/security/secret-content.md"),
        environment.workspace.join("lesson.md"),
    )
    .expect("copy secret fixture");
    fs::write(
        environment.workspace.join("normal.md"),
        "ordinary searchable phrase",
    )
    .expect("write normal source control");
    let init = environment
        .command()
        .arg("init")
        .output()
        .expect("run init");
    assert!(init.status.success(), "init failed: {}", output_text(&init));

    support::sources::prepare_text_fixture_separately(
        &environment,
        "normal.md",
        "ordinary searchable phrase",
    );
    let mut mcp = SecurityMcp::spawn(&environment);
    let oversized_frame = json!({
        "jsonrpc": "2.0",
        "id": 0,
        "method": "notifications/oversized-fixture",
        "padding": "x".repeat(1024 * 1024),
    });
    send_mcp(&mut mcp.stdin, &oversized_frame);
    let initialized = mcp.request(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "magnesium-security-test", "version": env!("CARGO_PKG_VERSION")}
        }
    }));
    assert_eq!(initialized["id"], 1);
    mcp.notify(&json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    }));
    let request = |id| {
        json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": "search_sources", "arguments": {
                "schema_version": 1, "query": "ordinary searchable phrase", "limit": 4
            }}
        })
    };
    let (_, response) = search_marker_once(
        &mut mcp,
        &request,
        2,
        "normal.md",
        "ordinary searchable phrase",
        true,
    );
    let payload: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("search result text"),
    )
    .expect("parse search payload");
    let hits = payload["hits"].as_array().expect("search hits");
    assert!(
        hits.iter().any(|hit| hit["path"] == "normal.md"),
        "normal source positive control missing: {payload}"
    );
    assert!(hits.iter().all(|hit| hit["path"] != "lesson.md"));
    let response_text = response.to_string();
    assert!(!response_text.contains("fixture-only-not-a-real-secret"));
    let stderr = mcp.close();
    assert!(!String::from_utf8_lossy(&stderr).contains("fixture-only-not-a-real-secret"));

    let index = environment
        .workspace
        .join(support::CONTROL_DIR)
        .join("index/source-search.sqlite");
    let connection = Connection::open(&index).expect("open source index");
    let row: (Option<String>, i64) = connection
        .query_row(
            "SELECT content_sha256,
                    (SELECT count(*) FROM source_candidates candidate
                     WHERE candidate.manifest_id = source_manifest.manifest_id)
             FROM source_manifest WHERE display_path = 'lesson.md'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read excluded source row");
    assert_eq!(row, (None, 0));
    for entry in fs::read_dir(index.parent().expect("index parent")).expect("read index files") {
        let path = entry.expect("read index entry").path();
        if path.is_file() {
            assert!(
                !fs::read(path)
                    .expect("read index artifact")
                    .windows(b"fixture-only-not-a-real-secret".len())
                    .any(|window| window == b"fixture-only-not-a-real-secret")
            );
        }
    }
    let mut private_artifacts = Vec::new();
    let control = environment.workspace.join(support::CONTROL_DIR);
    if control.is_dir() {
        collect_files(&control, &mut private_artifacts);
    }
    for artifact in private_artifacts {
        let bytes = fs::read(&artifact).expect("read private source-search artifact");
        assert!(
            !bytes
                .windows(b"fixture-only-not-a-real-secret".len())
                .any(|window| window == b"fixture-only-not-a-real-secret"),
            "secret leaked into {artifact:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn managed_index_symlink_is_rejected_without_touching_external_sentinel() {
    use std::os::unix::fs::symlink;

    let environment = TestEnvironment::from_fixture("basic").expect("create security fixture");
    let init = environment
        .command()
        .arg("init")
        .output()
        .expect("run init");
    assert!(init.status.success(), "init failed: {}", output_text(&init));
    let index = environment
        .workspace
        .join(support::CONTROL_DIR)
        .join("index/source-search.sqlite");
    let external = environment.root.join("external-index.sqlite");
    fs::write(&external, b"external-index-sentinel").expect("write external sentinel");
    if index.exists() {
        fs::remove_file(&index).unwrap();
    }
    fs::create_dir_all(index.parent().unwrap()).unwrap();
    symlink(&external, &index).expect("install managed index symlink");

    // Search is what opens the managed index; it must refuse the symlink.
    let search = support::v2::tool_output(
        &environment,
        "codex",
        "index-symlink",
        "search_sources",
        json!({"query":"lesson"}),
    );
    let text = output_text(&search);
    assert!(!search.status.success(), "{text}");
    assert!(text.contains("contains a symbolic link"), "{text}");
    assert_eq!(
        fs::read(&external).expect("read external sentinel"),
        b"external-index-sentinel"
    );
    assert!(!text.contains("external-index-sentinel"));
}

struct SecurityMcp {
    child: Child,
    stdin: ChildStdin,
    responses: Receiver<io::Result<String>>,
    stderr: Receiver<io::Result<Vec<u8>>>,
}

impl SecurityMcp {
    fn spawn(environment: &TestEnvironment) -> Self {
        let mut child = environment
            .command()
            .args([
                "_internal",
                "mcp-server",
                "--plugin-version",
                env!("CARGO_PKG_VERSION"),
                "--adapter-protocol-version",
                "2",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn MCP process");
        let stdin = child.stdin.take().expect("MCP stdin");
        let stdout = BufReader::new(child.stdout.take().expect("MCP stdout"));
        let stderr = child.stderr.take().expect("MCP stderr");
        let (sender, responses) = mpsc::channel();
        thread::spawn(move || read_mcp_lines(stdout, sender));
        let (stderr_sender, stderr_receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = BufReader::new(stderr)
                .take(16 * 1024 + 1)
                .bytes()
                .collect::<io::Result<Vec<_>>>();
            let _ = stderr_sender.send(result);
        });
        Self {
            child,
            stdin,
            responses,
            stderr: stderr_receiver,
        }
    }

    fn request(&mut self, message: &Value) -> Value {
        send_mcp(&mut self.stdin, message);
        let line = self
            .responses
            .recv_timeout(Duration::from_secs(5))
            .expect("MCP response deadline")
            .expect("read MCP response");
        serde_json::from_str(&line).expect("parse MCP response")
    }

    fn notify(&mut self, message: &Value) {
        send_mcp(&mut self.stdin, message);
    }

    fn close(mut self) -> Vec<u8> {
        drop(self.stdin);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().expect("poll MCP process") {
                assert!(status.success(), "MCP process exited with {status}");
                let stderr = self
                    .stderr
                    .recv_timeout(Duration::from_secs(5))
                    .expect("MCP stderr deadline")
                    .expect("read MCP stderr");
                assert!(stderr.len() <= 16 * 1024, "MCP stderr exceeded bound");
                return stderr;
            }
            if Instant::now() >= deadline {
                self.child.kill().expect("stop stalled MCP process");
                panic!("MCP process did not stop after stdin closed");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

fn search_marker_once(
    mcp: &mut SecurityMcp,
    request: &impl Fn(u64) -> Value,
    first_id: u64,
    expected_path: &str,
    expected_text: &str,
    require_complete: bool,
) -> (u64, Value) {
    let id = first_id;
    let response = mcp.request(&request(id));
    assert_eq!(response["id"], id);
    assert_ne!(
        response["result"]["isError"],
        Value::Bool(true),
        "structured search error: {response}"
    );
    let content = response["result"]["content"]
        .as_array()
        .expect("search content array");
    assert_eq!(content.len(), 1, "search content shape: {response}");
    assert_eq!(content[0]["type"], "text", "search content: {response}");
    let text = content[0]["text"].as_str().expect("search result text");
    assert!(
        text.len() < 8 * 1024,
        "search response exceeded bound: {text}"
    );
    let payload: Value = serde_json::from_str(text).expect("search result payload");
    assert_eq!(
        payload["schema"], "learner-model/v2",
        "search payload: {payload}"
    );
    assert!(
        payload["generation"].is_number(),
        "search payload: {payload}"
    );
    let coverage = payload["coverage"].as_str().expect("search coverage");
    assert!(
        matches!(
            coverage,
            "partial" | "indexing_on_demand" | "stale" | "complete"
        ),
        "unexpected search coverage {coverage}: {payload}"
    );
    let hits = payload["hits"].as_array().expect("search hits array");
    assert!(
        hits.iter()
            .all(|hit| hit["path"].is_string() && hit["snippet"].is_string()),
        "search hit shape: {payload}"
    );
    let found = hits.iter().any(|hit| {
        hit["path"] == expected_path
            && hit["snippet"]
                .as_str()
                .is_some_and(|snippet| snippet.contains(expected_text))
    });
    assert!(
        found,
        "prepared semantic search omitted expected source marker: {response}"
    );
    assert!(
        !require_complete || coverage == "complete",
        "prepared search is not complete: {response}"
    );
    (id + 1, response)
}

fn send_mcp(stdin: &mut impl Write, message: &Value) {
    serde_json::to_writer(&mut *stdin, message).expect("serialize MCP message");
    send_raw_mcp(stdin, b"\n");
}

fn send_raw_mcp(stdin: &mut impl Write, frame: &[u8]) {
    stdin.write_all(frame).expect("write MCP frame");
    if !frame.ends_with(b"\n") {
        stdin.write_all(b"\n").expect("terminate MCP frame");
    }
    stdin.flush().expect("flush MCP frame");
}

fn read_mcp_lines(mut stdout: impl BufRead, sender: mpsc::Sender<io::Result<String>>) {
    loop {
        let mut line = String::new();
        match stdout.read_line(&mut line) {
            Ok(0) => return,
            Ok(_) => {
                if sender.send(Ok(line)).is_err() {
                    return;
                }
            }
            Err(error) => {
                let _ = sender.send(Err(error));
                return;
            }
        }
    }
}
fn collect_files(directory: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(directory).expect("read artifact directory") {
        let path = entry.expect("read artifact entry").path();
        if path.is_dir() {
            collect_files(&path, files);
        } else {
            files.push(path);
        }
    }
}

#[test]
fn untrusted_diagnostic_payload_is_never_echoed() {
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    assert!(
        environment
            .command()
            .arg("init")
            .output()
            .unwrap()
            .status
            .success()
    );
    let path = environment
        .workspace
        .join(support::CONTROL_DIR)
        .join("diagnostics/latest-hook-error.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    for payload in [
        br#"{"event":"SECURITY_DIAGNOSTIC_SECRET","failure":"password=hunter2","at_ms":1}"#.to_vec(),
        br#"{"event":"hook","failure":"InputValidation","at_ms":1,"text":"SECURITY_DIAGNOSTIC_SECRET"}"#.to_vec(),
        vec![b'x';4097],
    ] {
        fs::write(&path,payload).unwrap();
        let output=environment.command().args(["info","--json"]).output().unwrap();
        let text=output_text(&output);
        assert!(output.status.success(),"{text}");
        assert!(!text.contains("SECURITY_DIAGNOSTIC_SECRET") && !text.contains("hunter2"),"{text}");
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["last_hook_error"], Value::Null, "{text}");
    }
}

#[cfg(unix)]
#[test]
fn local_store_and_sidecar_symlinks_preserve_external_bytes() {
    use std::os::unix::fs::symlink;
    for suffix in ["", "-wal", "-shm"] {
        let environment = TestEnvironment::from_fixture("basic").unwrap();
        assert!(
            environment
                .command()
                .arg("init")
                .output()
                .unwrap()
                .status
                .success()
        );
        let external = environment.root.join("external-private-data");
        fs::write(&external, b"PRIVATE_EXTERNAL_SENTINEL").unwrap();
        let path = environment
            .workspace
            .join(support::CONTROL_DIR)
            .join(format!("state.sqlite{suffix}"));
        if path.exists() {
            fs::remove_file(&path).unwrap();
        }
        symlink(&external, &path).unwrap();
        let output = environment
            .command()
            .args(["info", "--json"])
            .output()
            .unwrap();
        assert!(!output.status.success(), "{}", output_text(&output));
        assert!(!output_text(&output).contains("PRIVATE_EXTERNAL_SENTINEL"));
        assert_eq!(fs::read(&external).unwrap(), b"PRIVATE_EXTERNAL_SENTINEL");
        assert!(
            fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
