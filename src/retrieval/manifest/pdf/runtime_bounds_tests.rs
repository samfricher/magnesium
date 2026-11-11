use super::*;
use std::sync::{Arc, atomic::AtomicBool};

#[test]
fn worker_content_failure_requires_a_complete_valid_typed_reply() {
    assert_eq!(
        decode_pdf_reply(br#"{"outcome":"pages","pages":["readable page",""]}"#).unwrap(),
        vec!["readable page", ""]
    );
    assert!(
        matches!(decode_pdf_reply(br#"{"outcome":"content_failure","diagnostic":"parser rejected content"}"#), Err(ManifestError::PdfContent(ref diagnostic)) if diagnostic == "parser rejected content")
    );
    for invalid in [
        br#"["old untyped reply"]"#.as_slice(),
        br#"{"outcome":"content_failure","diagnostic":"parser rejected content","extra":true}"#,
        br#"{"outcome":"unknown"}"#,
        b"not JSON",
    ] {
        assert!(matches!(
            decode_pdf_reply(invalid),
            Err(ManifestError::Pdf(_))
        ));
    }
    let payload = br#"{"outcome":"content_failure","diagnostic":"parser rejected content"}"#;
    let mut frame = format!("{PDF_PROTOCOL_MARKER}{}\n", payload.len()).into_bytes();
    frame.extend_from_slice(payload);
    assert!(matches!(
        decode_pdf_reply(framed_pdf_payload(&frame).unwrap()),
        Err(ManifestError::PdfContent(_))
    ));
    frame.push(b'x');
    assert!(matches!(
        framed_pdf_payload(&frame),
        Err(ManifestError::Pdf(_))
    ));
    frame.truncate(frame.len() - 2);
    assert!(matches!(
        framed_pdf_payload(&frame),
        Err(ManifestError::Pdf(_))
    ));
}

#[test]
fn unsuccessful_worker_cannot_publish_a_content_failure_reply() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "retrieval::manifest::tests::pdf_process_helper",
            "--nocapture",
        ])
        .env("MAGNESIUM_PDF_TEST_HELPER", "failed-content-reply");
    assert!(matches!(
        run_pdf_process(command, None, Duration::from_secs(5)),
        Err(ManifestError::Pdf(_))
    ));
}

#[test]
fn cancelled_pdf_process_terminates_child_without_waiting_for_deadline() {
    // Sleep helper sleeps 30s; cancellation must terminate it promptly,
    // well before the 30s deadline. Mirrors the existing sleep helper
    // pattern in manifest tests without duplicating its harness.
    let mut command = std::process::Command::new("sleep");
    command.arg("30");
    let flag = Arc::new(AtomicBool::new(true));
    let cancellation = crate::retrieval::OperationCancellation::with_flag(flag);
    let started = Instant::now();
    let error =
        run_pdf_process_with_cancellation(command, None, Duration::from_secs(30), &cancellation)
            .expect_err("cancelled PDF process must abort");
    assert!(
        matches!(error, ManifestError::Cancelled),
        "unexpected PDF cancellation error: {error}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "cancelled PDF worker did not release promptly"
    );
}

#[test]
fn cancellation_after_process_start_terminates_running_child() {
    // The production extractor is replaced with the ignored in-test sleep
    // helper so the child is running when cancellation arrives. The helper
    // sleeps 5s, so finishing well under that shows the worker killed it.
    let mut command = std::process::Command::new(std::env::current_exe().expect("test executable"));
    command
        .args([
            "--ignored",
            "--exact",
            "retrieval::manifest::tests::pdf_process_helper",
            "--nocapture",
        ])
        .env("MAGNESIUM_PDF_TEST_HELPER", "sleep");
    let flag = Arc::new(AtomicBool::new(false));
    let cancellation = crate::retrieval::OperationCancellation::with_flag(Arc::clone(&flag));
    let setter = thread::spawn(move || {
        thread::sleep(Duration::from_millis(150));
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let started = Instant::now();
    let error =
        run_pdf_process_with_cancellation(command, None, Duration::from_secs(30), &cancellation)
            .expect_err("cancellation after process start must abort");
    setter.join().expect("cancellation setter thread");
    assert!(
        matches!(error, ManifestError::Cancelled),
        "unexpected cancellation error: {error}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "a running PDF child was not terminated promptly"
    );
}

#[test]
fn pdf_timeout_kills_slow_child_and_reports_timeout() {
    let mut command = std::process::Command::new("sleep");
    command.arg("30");
    let started = Instant::now();
    let error = run_pdf_process(command, None, Duration::from_millis(100))
        .expect_err("slow PDF child must time out");
    assert!(
        matches!(error, ManifestError::PdfTimeout),
        "unexpected PDF timeout error: {error}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "timed-out PDF worker did not release promptly"
    );
}

#[cfg(unix)]
#[test]
fn pdf_child_that_closes_stdout_but_keeps_running_still_times_out() {
    let mut command = std::process::Command::new("sh");
    command.args(["-c", "exec 1>&- 2>&-; sleep 30"]);
    let started = Instant::now();
    let error = run_pdf_process(command, None, Duration::from_millis(200))
        .expect_err("a PDF child that never exits must time out");
    assert!(
        matches!(error, ManifestError::PdfTimeout),
        "unexpected PDF timeout error: {error}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "a PDF child with closed stdout was not released promptly"
    );
}
