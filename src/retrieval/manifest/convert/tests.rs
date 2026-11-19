use super::*;

fn valid_response(source_sha256: &str) -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "method": "fake-worker",
        "method_version": "test-1",
        "source_sha256": source_sha256,
        "blocks": [
            {"anchor_kind": "page", "anchor_start": 1, "anchor_end": 1, "text": "Converted page one"},
            {"anchor_kind": "section", "anchor_start": 2, "anchor_end": 3, "text": "Converted section", "warning": "uncertain reading order"},
        ],
        "warnings": ["table relationship uncertain"],
        "coverage": "complete",
    })
}

#[test]
fn validation_accepts_a_well_formed_extraction() {
    let sha = "a".repeat(64);
    let extraction = validate_extraction(&valid_response(&sha), &sha).expect("valid extraction");
    assert_eq!(extraction.version, DOCUMENT_EXTRACTION_VERSION);
    assert_eq!(extraction.method, "fake-worker");
    assert_eq!(extraction.blocks.len(), 2);
    assert_eq!(
        extraction.blocks[1].warning.as_deref(),
        Some("uncertain reading order")
    );
    assert_eq!(extraction.warnings, vec!["table relationship uncertain"]);
    assert_eq!(extraction.coverage, ExtractionCoverage::Complete);
}

#[test]
fn validation_rejects_mismatches_and_bad_shapes() {
    let sha = "a".repeat(64);
    let other = "b".repeat(64);
    assert!(validate_extraction(&valid_response(&other), &sha).is_err());
    let mut versioned = valid_response(&sha);
    versioned["version"] = serde_json::json!(999);
    assert!(validate_extraction(&versioned, &sha).is_err());
    let mut kind = valid_response(&sha);
    kind["blocks"][0]["anchor_kind"] = serde_json::json!("chapter");
    assert!(validate_extraction(&kind, &sha).is_err());
    let mut range = valid_response(&sha);
    range["blocks"][0]["anchor_end"] = serde_json::json!(0);
    assert!(validate_extraction(&range, &sha).is_err());
    let mut method = valid_response(&sha);
    method["method"] = serde_json::json!("");
    assert!(validate_extraction(&method, &sha).is_err());
    let mut big = valid_response(&sha);
    big["blocks"] = serde_json::json!([
        {"anchor_kind": "page", "anchor_start": 1, "anchor_end": 1, "text": "x".repeat(MAX_WORKER_TEXT_BYTES + 1)},
    ]);
    assert!(validate_extraction(&big, &sha).is_err());
    let mut partial = valid_response(&sha);
    partial["coverage"] = serde_json::json!("partial");
    partial["missing"] = serde_json::json!(["page 3"]);
    assert_eq!(
        validate_extraction(&partial, &sha)
            .expect("partial")
            .coverage,
        ExtractionCoverage::Partial {
            missing: vec!["page 3".to_owned()]
        }
    );
    let mut unknown = valid_response(&sha);
    unknown["coverage"] = serde_json::json!("mystery");
    assert!(matches!(
        validate_extraction(&unknown, &sha)
            .expect("unknown coverage")
            .coverage,
        ExtractionCoverage::TextUnavailable { .. }
    ));
}

#[test]
fn worker_config_defaults_to_unconfigured() {
    if std::env::var_os(WORKER_ENV).is_none() && std::env::var_os(WORKER_OPT_IN_ENV).is_none() {
        assert!(worker_config_from_env().is_none());
    }
}

struct FakeWorker {
    dir: PathBuf,
}

impl FakeWorker {
    fn new(response: &[u8], mode: &str) -> Self {
        static NEXT_WORKER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = loop {
            let sequence = NEXT_WORKER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let candidate = std::env::temp_dir().join(format!(
                "magnesium-docs-worker-{}-{sequence}",
                std::process::id(),
            ));
            match std::fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create fake worker dir: {error}"),
            }
        };
        let response_path = dir.join("response.bin");
        std::fs::write(&response_path, response).expect("fake response");
        let script = match mode {
            "success" | "junk" => {
                format!("cat \"{}\"\n", response_path.display())
            }
            "fail" => "exit 3\n".to_owned(),
            "sleep" => "sleep 60\n".to_owned(),
            "helper-hold" => format!(
                "cat \"{}\"\n(sleep 2; touch \"{}\") &\nexit 0\n",
                response_path.display(),
                dir.join("helper-done").display(),
            ),
            "helper-hold-cancelled" => format!(
                "(sleep 60) &\ncat \"{}\"\nexit 0\n",
                response_path.display(),
            ),
            "helper-hold-running" => format!(
                "(sleep 60) &\ntouch \"{}\"\ncat \"{}\"\nexit 0\n",
                dir.join("request-consumed").display(),
                response_path.display(),
            ),
            _ => unreachable!("fake worker mode"),
        };
        let path = dir.join("worker.sh");
        std::fs::write(&path, format!("#!/bin/sh\ncat >/dev/null\n{script}"))
            .expect("fake worker script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .expect("fake worker executable");
        }
        Self { dir }
    }

    fn framed(body: &[u8]) -> Vec<u8> {
        let mut framed = format!("{WORKER_PROTOCOL_MARKER}{}\n", body.len()).into_bytes();
        framed.extend_from_slice(body);
        framed
    }

    fn configure(&self) -> WorkerConfig {
        WorkerConfig {
            executable: self.dir.join("worker.sh"),
        }
    }

    fn snapshot(&self) -> PathBuf {
        let path = self.dir.join("snapshot.pdf");
        std::fs::write(&path, b"%PDF snapshot").expect("fake snapshot");
        path
    }
}

impl Drop for FakeWorker {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
#[cfg(unix)]
fn worker_round_trip_validates_and_returns_blocks() {
    let sha = "a".repeat(64);
    let body = serde_json::to_vec(&valid_response(&sha)).expect("response JSON");
    let worker = FakeWorker::new(&FakeWorker::framed(&body), "success");
    let config = worker.configure();
    let snapshot = worker.snapshot();
    let extraction = convert_with_worker(&config, &snapshot, &sha, "pdf", 1, 2, None)
        .expect("worker conversion");
    assert_eq!(extraction.method, "fake-worker");
    assert_eq!(extraction.blocks.len(), 2);
}

#[test]
#[cfg(unix)]
fn large_valid_response_drains_while_the_worker_runs() {
    let sha = "a".repeat(64);
    let big_text = "x".repeat(256 * 1024);
    let body = serde_json::to_vec(&serde_json::json!({
        "version": 1,
        "method": "fake-worker",
        "method_version": "test-1",
        "source_sha256": sha,
        "blocks": [
            {"anchor_kind": "page", "anchor_start": 1, "anchor_end": 1, "text": big_text},
        ],
        "coverage": "complete",
    }))
    .expect("large response JSON");
    assert!(body.len() > 64 * 1024, "response must exceed pipe capacity");
    let worker = FakeWorker::new(&FakeWorker::framed(&body), "success");
    let config = worker.configure();
    let snapshot = worker.snapshot();
    let started = Instant::now();
    let extraction = convert_with_worker(&config, &snapshot, &sha, "pdf", 1, 1, None)
        .expect("large valid response must succeed without deadlock");
    assert!(started.elapsed() < WORKER_DEADLINE, "no deadline kill");
    assert_eq!(extraction.blocks.len(), 1);
    assert_eq!(extraction.blocks[0].text.len(), 256 * 1024);
}

#[test]
#[cfg(unix)]
fn helper_held_stdout_completes_within_the_deadline() {
    let sha = "a".repeat(64);
    let body = serde_json::to_vec(&valid_response(&sha)).expect("response JSON");
    let worker = FakeWorker::new(&FakeWorker::framed(&body), "helper-hold");
    let config = worker.configure();
    let snapshot = worker.snapshot();
    let started = Instant::now();
    let extraction = convert_with_worker(&config, &snapshot, &sha, "pdf", 1, 2, None)
        .expect("held-stdout conversion must complete");
    let elapsed = started.elapsed();
    assert_eq!(extraction.blocks.len(), 2);
    assert!(
        elapsed >= Duration::from_secs(2),
        "completion waited for the held output, not just the exit: {elapsed:?}"
    );
    assert!(
        elapsed < WORKER_DEADLINE,
        "completion stayed inside the deadline: {elapsed:?}"
    );
    assert!(
        worker.dir.join("helper-done").exists(),
        "helper ran to completion after the direct worker exited"
    );
}

#[test]
#[cfg(unix)]
fn cancelled_conversion_reaps_helper_held_stdout() {
    let sha = "a".repeat(64);
    let body = serde_json::to_vec(&valid_response(&sha)).expect("response JSON");
    let worker = FakeWorker::new(&FakeWorker::framed(&body), "helper-hold-cancelled");
    let config = worker.configure();
    let snapshot = worker.snapshot();
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let cancellation = crate::retrieval::OperationCancellation::with_flag(flag);
    let started = Instant::now();
    assert!(
        matches!(
            convert_with_worker(&config, &snapshot, &sha, "pdf", 1, 2, Some(&cancellation)),
            Err(WorkerError::Cancelled)
        ),
        "cancelled conversion must abort despite the held pipe"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "cancellation does not wait out the helper: {:?}",
        started.elapsed()
    );
}

#[test]
#[cfg(unix)]
fn worker_failures_are_typed_never_silent() {
    let sha = "a".repeat(64);
    let junk = FakeWorker::new(b"not framed at all", "junk");
    let junk_config = junk.configure();
    let snapshot = junk.snapshot();
    let junk_result = convert_with_worker(&junk_config, &snapshot, &sha, "pdf", 1, 1, None);
    assert!(
        matches!(junk_result, Err(WorkerError::InvalidOutput(_))),
        "junk output must fail validation: {junk_result:?}"
    );
    let failing = FakeWorker::new(b"", "fail");
    let failing_config = failing.configure();
    let failing_result = convert_with_worker(&failing_config, &snapshot, &sha, "pdf", 1, 1, None);
    assert!(
        matches!(failing_result, Err(WorkerError::Failed(_))),
        "nonzero exit must fail: {failing_result:?}"
    );
    assert!(matches!(
        convert_with_worker(&junk_config, &snapshot, &sha, "txt", 1, 1, None),
        Err(WorkerError::Unavailable(_))
    ));
    assert!(matches!(
        convert_with_worker(&junk_config, &snapshot, &sha, "pdf", 0, 1, None),
        Err(WorkerError::Unavailable(_))
    ));
}

#[test]
#[cfg(unix)]
fn cancelled_conversion_kills_the_worker() {
    let sha = "a".repeat(64);
    let sleeper = FakeWorker::new(b"", "sleep");
    let sleeper_config = sleeper.configure();
    let snapshot = sleeper.snapshot();
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let cancellation = crate::retrieval::OperationCancellation::with_flag(flag);
    assert!(
        matches!(
            convert_with_worker(
                &sleeper_config,
                &snapshot,
                &sha,
                "pdf",
                1,
                1,
                Some(&cancellation)
            ),
            Err(WorkerError::Cancelled)
        ),
        "cancelled conversion must abort"
    );
}

#[test]
#[cfg(unix)]
fn cancellation_after_worker_consumes_request_reaps_running_helper() {
    let sha = "a".repeat(64);
    let body = serde_json::to_vec(&valid_response(&sha)).unwrap();
    let worker = FakeWorker::new(&FakeWorker::framed(&body), "helper-hold-running");
    let config = worker.configure();
    let snapshot = worker.snapshot();
    let marker = worker.dir.join("request-consumed");
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let trigger = flag.clone();
    let cancel = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !marker.exists() {
            assert!(
                Instant::now() < deadline,
                "worker never consumed its request"
            );
            std::thread::yield_now();
        }
        trigger.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let token = crate::retrieval::OperationCancellation::with_flag(flag);
    let started = Instant::now();
    let result = convert_with_worker(&config, &snapshot, &sha, "pdf", 1, 2, Some(&token));
    cancel.join().unwrap();
    assert!(matches!(result, Err(WorkerError::Cancelled)), "{result:?}");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "running helper must not hold cancellation open"
    );
}

#[test]
fn worker_status_reports_fixed_contract_fields() {
    let status = worker_status();
    assert_eq!(status["version"], DOCUMENT_EXTRACTION_VERSION);
    assert_eq!(status["fixed_sample_gate"], "not_run");
    assert!(status["opted_in"].is_boolean());
    assert!(status["available"].is_boolean());
    assert!(
        status["source_kinds"]
            .as_array()
            .is_some_and(|kinds| kinds.iter().any(|kind| kind == "pdf"))
    );
}
