use crate::retrieval::MAX_PDF_SOURCE_BYTES;
use std::io::{self, Read, Write};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::{ManifestError, io_error};

pub(crate) const MAX_PDF_PAGES: usize = 2_048;
pub(crate) const MAX_PDF_TEXT_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_PDF_MESSAGE_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const MAX_PDF_ERROR_BYTES: usize = 16 * 1024;
pub(crate) const PDF_DEADLINE: Duration = Duration::from_secs(5);
pub(crate) const PDF_PROTOCOL_MARKER: &str = "MAGNESIUM_PDF_V2 ";

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PdfWorkerReply {
    Pages { pages: Vec<String> },
    ContentFailure { diagnostic: String },
}

pub(crate) fn run_pdf_extractor() -> ExitCode {
    match child_extract_pdf() {
        Ok(pages) => match serde_json::to_vec(&pages) {
            Ok(output) if output.len() <= MAX_PDF_MESSAGE_BYTES => {
                let mut stdout = io::stdout().lock();
                if writeln!(stdout, "{PDF_PROTOCOL_MARKER}{}", output.len()).is_ok()
                    && stdout.write_all(&output).is_ok()
                {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                }
            }
            Ok(_) => {
                eprintln!("PDF extraction output exceeded the private message limit");
                ExitCode::from(1)
            }
            Err(error) => {
                eprintln!("PDF extraction output could not be serialized: {error}");
                ExitCode::from(1)
            }
        },
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

pub(crate) fn child_extract_pdf() -> Result<PdfWorkerReply, ManifestError> {
    let bytes = read_bounded(io::stdin().lock(), MAX_PDF_SOURCE_BYTES)
        .map_err(|error| ManifestError::Pdf(format!("PDF input is invalid: {error}")))?;
    match std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem_by_pages(&bytes)) {
        Err(_) => Err(ManifestError::Pdf(
            "PDF parser panicked on input".to_owned(),
        )),
        Ok(Err(error)) => Ok(PdfWorkerReply::ContentFailure {
            diagnostic: format!("PDF text extraction failed: {error}")
                .chars()
                .take(240)
                .collect(),
        }),
        Ok(Ok(pages)) => validate_pdf_pages(pages).map(|pages| PdfWorkerReply::Pages { pages }),
    }
}

pub(crate) fn validate_pdf_pages(pages: Vec<String>) -> Result<Vec<String>, ManifestError> {
    if pages.len() > MAX_PDF_PAGES {
        return Err(ManifestError::Pdf(format!(
            "PDF has {} pages; the limit is {MAX_PDF_PAGES}",
            pages.len()
        )));
    }
    let output_bytes = pages
        .iter()
        .try_fold(0_usize, |total, page| total.checked_add(page.len()))
        .ok_or_else(|| ManifestError::Pdf("PDF text size overflowed".to_owned()))?;
    if output_bytes > MAX_PDF_TEXT_BYTES {
        return Err(ManifestError::Pdf(format!(
            "PDF extracted {output_bytes} text bytes; the limit is {MAX_PDF_TEXT_BYTES}"
        )));
    }
    Ok(pages)
}

fn pdf_command() -> Result<Command, ManifestError> {
    #[cfg(test)]
    let (executable, args, helper_mode) = {
        if let Some(path) = std::env::var_os("CARGO_BIN_EXE_magnesium")
            .or_else(|| std::env::var_os("MAGNESIUM_TEST_BINARY"))
            .map(PathBuf::from)
        {
            (
                path,
                vec!["_internal".to_owned(), "extract-pdf".to_owned()],
                None,
            )
        } else {
            let current = std::env::current_exe().map_err(|source| {
                io_error(
                    "resolve Magnesium executable",
                    Path::new("magnesium"),
                    source,
                )
            })?;
            if current.to_string_lossy().contains("/deps/") {
                if let Some(target_dir) = current.parent().and_then(Path::parent) {
                    let candidate = target_dir.join(if cfg!(windows) {
                        "magnesium.exe"
                    } else {
                        "magnesium"
                    });
                    if candidate.is_file() {
                        (
                            candidate,
                            vec!["_internal".to_owned(), "extract-pdf".to_owned()],
                            None,
                        )
                    } else {
                        (
                            current,
                            vec![
                                "--ignored".to_owned(),
                                "--exact".to_owned(),
                                "retrieval::manifest::tests::pdf_process_helper".to_owned(),
                                "--nocapture".to_owned(),
                            ],
                            Some("extract"),
                        )
                    }
                } else {
                    (
                        current,
                        vec!["_internal".to_owned(), "extract-pdf".to_owned()],
                        None,
                    )
                }
            } else {
                (
                    current,
                    vec!["_internal".to_owned(), "extract-pdf".to_owned()],
                    None,
                )
            }
        }
    };
    #[cfg(test)]
    let mut command = Command::new(executable);
    #[cfg(test)]
    {
        command.args(&args);
        if let Some(mode) = helper_mode {
            command.env("MAGNESIUM_PDF_TEST_HELPER", mode);
        }
    }
    #[cfg(not(test))]
    let executable = std::env::current_exe().map_err(|source| {
        io_error(
            "resolve Magnesium executable",
            Path::new("magnesium"),
            source,
        )
    })?;
    #[cfg(not(test))]
    let mut command = Command::new(executable);
    #[cfg(not(test))]
    command.args(["_internal", "extract-pdf"]);
    Ok(command)
}

pub(crate) fn extract_pdf_from_file(
    file: std::fs::File,
    cancellation: Option<&crate::retrieval::OperationCancellation>,
) -> Result<Vec<String>, ManifestError> {
    let mut command = pdf_command()?;
    command.stdin(Stdio::from(file));
    match cancellation {
        Some(cancellation) => {
            run_pdf_process_with_cancellation(command, None, PDF_DEADLINE, cancellation)
        }
        None => run_pdf_process(command, None, PDF_DEADLINE),
    }
}

#[cfg(test)]
pub(crate) fn run_pdf_command(
    command: Command,
    bytes: &[u8],
    deadline: Duration,
) -> Result<Vec<String>, ManifestError> {
    if bytes.len() > MAX_PDF_SOURCE_BYTES {
        return Err(ManifestError::Pdf(format!(
            "PDF input exceeds the {MAX_PDF_SOURCE_BYTES}-byte limit"
        )));
    }
    run_pdf_process(command, Some(bytes), deadline)
}

pub(crate) fn run_pdf_process(
    command: Command,
    bytes_to_write: Option<&[u8]>,
    deadline: Duration,
) -> Result<Vec<String>, ManifestError> {
    run_pdf_process_with_cancellation(
        command,
        bytes_to_write,
        deadline,
        &crate::retrieval::OperationCancellation::none(),
    )
}

pub(crate) fn run_pdf_process_with_cancellation(
    mut command: Command,
    bytes_to_write: Option<&[u8]>,
    deadline: Duration,
    cancellation: &crate::retrieval::OperationCancellation,
) -> Result<Vec<String>, ManifestError> {
    if bytes_to_write.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| {
            io_error(
                "start private PDF extractor",
                Path::new("magnesium"),
                source,
            )
        })?;

    let writer = if let Some(bytes) = bytes_to_write {
        let mut stdin = child.stdin.take().expect("piped PDF stdin");
        let input = bytes.to_vec();
        Some(thread::spawn(move || stdin.write_all(&input)))
    } else {
        None
    };
    let stdout = child.stdout.take().expect("piped PDF stdout");
    let output = thread::spawn(move || read_bounded(stdout, MAX_PDF_MESSAGE_BYTES));
    let stderr = child.stderr.take().expect("piped PDF stderr");
    let errors = thread::spawn(move || read_bounded(stderr, MAX_PDF_ERROR_BYTES));
    let started = Instant::now();
    let status = loop {
        // Cancellation at a real boundary terminates the existing child
        // extractor and releases its reader threads; a dropped future alone
        // does not stop this blocking worker.
        if cancellation.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            if let Some(w) = writer {
                let _ = w.join();
            }
            let _ = output.join();
            let _ = errors.join();
            return Err(ManifestError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                if let Some(w) = writer {
                    let _ = w.join();
                }
                let _ = output.join();
                let _ = errors.join();
                return Err(ManifestError::PdfTimeout);
            }
            Err(source) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io_error(
                    "wait for private PDF extractor",
                    Path::new("magnesium"),
                    source,
                ));
            }
        }
    };
    if let Some(w) = writer {
        w.join()
            .map_err(|_| ManifestError::Pdf("PDF input writer panicked".to_owned()))?
            .map_err(|error| ManifestError::Pdf(format!("PDF input writer failed: {error}")))?;
    }
    let output = output
        .join()
        .map_err(|_| ManifestError::Pdf("PDF output reader panicked".to_owned()))?
        .map_err(|error| ManifestError::Pdf(format!("PDF output is invalid: {error}")))?;
    let errors = errors
        .join()
        .map_err(|_| ManifestError::Pdf("PDF error reader panicked".to_owned()))?
        .map_err(|error| ManifestError::Pdf(format!("PDF diagnostics are invalid: {error}")))?;
    if !status.success() {
        let detail = String::from_utf8_lossy(&errors);
        return Err(ManifestError::Pdf(format!(
            "private PDF extractor failed: {}",
            detail.lines().next().unwrap_or("no diagnostic")
        )));
    }
    let payload = framed_pdf_payload(&output)?;
    decode_pdf_reply(payload)
}

fn decode_pdf_reply(payload: &[u8]) -> Result<Vec<String>, ManifestError> {
    let reply: PdfWorkerReply = serde_json::from_slice(payload)
        .map_err(|error| ManifestError::Pdf(format!("PDF output is invalid JSON: {error}")))?;
    match reply {
        PdfWorkerReply::Pages { pages } => validate_pdf_pages(pages),
        PdfWorkerReply::ContentFailure { diagnostic }
            if diagnostic.len() <= MAX_PDF_ERROR_BYTES && !diagnostic.is_empty() =>
        {
            Err(ManifestError::PdfContent(diagnostic))
        }
        PdfWorkerReply::ContentFailure { .. } => Err(ManifestError::Pdf(
            "PDF content diagnostic exceeds protocol bounds".to_owned(),
        )),
    }
}

pub(crate) fn framed_pdf_payload(output: &[u8]) -> Result<&[u8], ManifestError> {
    let marker = output
        .windows(PDF_PROTOCOL_MARKER.len())
        .position(|window| window == PDF_PROTOCOL_MARKER.as_bytes())
        .ok_or_else(|| ManifestError::Pdf("PDF output omitted its protocol marker".to_owned()))?;
    let framed = &output[marker + PDF_PROTOCOL_MARKER.len()..];
    let newline = framed
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or_else(|| ManifestError::Pdf("PDF output omitted its frame length".to_owned()))?;
    let length = std::str::from_utf8(&framed[..newline])
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|length| *length <= MAX_PDF_MESSAGE_BYTES)
        .ok_or_else(|| {
            ManifestError::Pdf("PDF output declared an invalid frame length".to_owned())
        })?;
    let payload = &framed[newline + 1..];
    if payload.len() != length {
        return Err(ManifestError::Pdf(
            "PDF output does not match its declared frame length".to_owned(),
        ));
    }
    Ok(payload)
}

pub(crate) fn read_bounded(reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    read_bounded_with_cancellation(
        reader,
        limit,
        &crate::retrieval::OperationCancellation::none(),
    )
}

pub(crate) fn read_bounded_with_cancellation(
    mut reader: impl Read,
    limit: usize,
    cancellation: &crate::retrieval::OperationCancellation,
) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        cancellation.check().map_err(io::Error::other)?;
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        if read > limit.saturating_sub(bytes.len()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("input exceeds the {limit}-byte limit"),
            ));
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    cancellation.check().map_err(io::Error::other)?;
    Ok(bytes)
}

#[cfg(test)]
mod runtime_bounds_tests {
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
        let error = run_pdf_process_with_cancellation(
            command,
            None,
            Duration::from_secs(30),
            &cancellation,
        )
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
        // helper so the child is running when cancellation arrives. The worker
        // must kill it promptly rather than waiting for the 30s deadline.
        let mut command =
            std::process::Command::new(std::env::current_exe().expect("test executable"));
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
        let error = run_pdf_process_with_cancellation(
            command,
            None,
            Duration::from_secs(30),
            &cancellation,
        )
        .expect_err("cancellation after process start must abort");
        setter.join().expect("cancellation setter thread");
        assert!(
            matches!(error, ManifestError::Cancelled),
            "unexpected cancellation error: {error}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(10),
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
}
