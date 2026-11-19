use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(crate) const DOCUMENT_EXTRACTION_VERSION: u32 = 1;
pub(crate) const WORKER_PROTOCOL_MARKER: &str = "MAGNESIUM_DOCS_V1 ";
pub(crate) const WORKER_ENV: &str = "MAGNESIUM_DOCS_WORKER";
pub(crate) const WORKER_OPT_IN_ENV: &str = "MAGNESIUM_DOCS_WORKER_OPT_IN";
pub(crate) const WORKER_DEADLINE: Duration = Duration::from_secs(30);
pub(crate) const MAX_WORKER_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_WORKER_TEXT_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_WORKER_BLOCKS: usize = 512;
pub(crate) const WORKER_SOURCE_KINDS: &[&str] = &["pdf", "docx", "pptx", "epub"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExtractionBlock {
    pub(crate) anchor_kind: String,
    pub(crate) anchor_start: u32,
    pub(crate) anchor_end: u32,
    pub(crate) text: String,
    pub(crate) warning: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ExtractionCoverage {
    Complete,
    Partial { missing: Vec<String> },
    TextUnavailable { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConverterProvenance {
    pub(crate) method: String,
    pub(crate) method_version: String,
    pub(crate) coverage: String,
    pub(crate) missing: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

impl DocumentExtraction {
    pub(crate) fn provenance(&self) -> ConverterProvenance {
        let (coverage, missing) = match &self.coverage {
            ExtractionCoverage::Complete => ("complete".to_owned(), Vec::new()),
            ExtractionCoverage::Partial { missing } => ("partial".to_owned(), missing.clone()),
            ExtractionCoverage::TextUnavailable { reason } => {
                ("text_unavailable".to_owned(), vec![reason.clone()])
            }
        };
        ConverterProvenance {
            method: self.method.clone(),
            method_version: self.method_version.clone(),
            coverage,
            missing,
            warnings: self.warnings.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DocumentExtraction {
    pub(crate) version: u32,
    pub(crate) method: String,
    pub(crate) method_version: String,
    pub(crate) blocks: Vec<ExtractionBlock>,
    pub(crate) warnings: Vec<String>,
    pub(crate) coverage: ExtractionCoverage,
}

#[derive(Debug)]
pub(crate) enum WorkerError {
    Unavailable(String),
    Timeout,
    Cancelled,
    InvalidOutput(String),
    Failed(String),
}

impl std::fmt::Display for WorkerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(reason) => {
                write!(formatter, "conversion worker unavailable: {reason}")
            }
            Self::Timeout => write!(formatter, "conversion worker timed out"),
            Self::Cancelled => write!(formatter, "conversion worker cancelled"),
            Self::InvalidOutput(reason) => {
                write!(formatter, "conversion worker output invalid: {reason}")
            }
            Self::Failed(reason) => write!(formatter, "conversion worker failed: {reason}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkerConfig {
    pub(crate) executable: PathBuf,
}

pub(crate) fn worker_config_from_env() -> Option<WorkerConfig> {
    if std::env::var_os(WORKER_OPT_IN_ENV).is_none_or(|value| value != "1") {
        return None;
    }
    let executable = std::env::var_os(WORKER_ENV).map(PathBuf::from)?;
    if !executable.is_file() {
        return None;
    }
    Some(WorkerConfig { executable })
}

pub(crate) fn validate_extraction(
    value: &serde_json::Value,
    source_sha256: &str,
) -> Result<DocumentExtraction, WorkerError> {
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| WorkerError::InvalidOutput("missing version".to_owned()))?;
    if version != u64::from(DOCUMENT_EXTRACTION_VERSION) {
        return Err(WorkerError::InvalidOutput(format!(
            "unsupported extraction version {version}"
        )));
    }
    let echoed = value
        .get("source_sha256")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if echoed != source_sha256 {
        return Err(WorkerError::InvalidOutput(
            "extraction names a different source".to_owned(),
        ));
    }
    let method = value
        .get("method")
        .and_then(serde_json::Value::as_str)
        .filter(|method| !method.is_empty() && method.len() <= 128)
        .ok_or_else(|| WorkerError::InvalidOutput("missing method".to_owned()))?
        .to_owned();
    let method_version = value
        .get("method_version")
        .and_then(serde_json::Value::as_str)
        .filter(|version| !version.is_empty() && version.len() <= 256)
        .ok_or_else(|| WorkerError::InvalidOutput("missing method_version".to_owned()))?
        .to_owned();
    let blocks_value = value
        .get("blocks")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| WorkerError::InvalidOutput("missing blocks".to_owned()))?;
    if blocks_value.len() > MAX_WORKER_BLOCKS {
        return Err(WorkerError::InvalidOutput("too many blocks".to_owned()));
    }
    let mut blocks = Vec::with_capacity(blocks_value.len());
    let mut text_bytes = 0usize;
    for block in blocks_value {
        let anchor_kind = block
            .get("anchor_kind")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| WorkerError::InvalidOutput("block misses anchor_kind".to_owned()))?;
        if !matches!(anchor_kind, "page" | "section") {
            return Err(WorkerError::InvalidOutput(
                "block anchor_kind must be page or section".to_owned(),
            ));
        }
        let anchor_start = block
            .get("anchor_start")
            .and_then(serde_json::Value::as_u64)
            .and_then(|start| u32::try_from(start).ok())
            .filter(|start| *start >= 1)
            .ok_or_else(|| WorkerError::InvalidOutput("block misses anchor_start".to_owned()))?;
        let anchor_end = block
            .get("anchor_end")
            .and_then(serde_json::Value::as_u64)
            .and_then(|end| u32::try_from(end).ok())
            .filter(|end| *end >= anchor_start)
            .ok_or_else(|| WorkerError::InvalidOutput("block misses anchor_end".to_owned()))?;
        let text = block
            .get("text")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| WorkerError::InvalidOutput("block misses text".to_owned()))?;
        if text.contains('\0') {
            return Err(WorkerError::InvalidOutput(
                "block text contains NUL".to_owned(),
            ));
        }
        text_bytes = text_bytes
            .checked_add(text.len())
            .ok_or_else(|| WorkerError::InvalidOutput("text size overflowed".to_owned()))?;
        if text_bytes > MAX_WORKER_TEXT_BYTES {
            return Err(WorkerError::InvalidOutput(
                "converted text exceeds the bound".to_owned(),
            ));
        }
        let warning = block
            .get("warning")
            .and_then(serde_json::Value::as_str)
            .filter(|warning| !warning.is_empty())
            .map(|warning| warning.chars().take(512).collect::<String>());
        blocks.push(ExtractionBlock {
            anchor_kind: anchor_kind.to_owned(),
            anchor_start,
            anchor_end,
            text: text.to_owned(),
            warning,
        });
    }
    let coverage = match value
        .get("coverage")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("complete")
    {
        "complete" => ExtractionCoverage::Complete,
        "partial" => {
            let missing = value
                .get("missing")
                .and_then(serde_json::Value::as_array)
                .map(|missing| {
                    missing
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .take(64)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            ExtractionCoverage::Partial { missing }
        }
        _ => {
            let reason = value
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("conversion incomplete")
                .chars()
                .take(240)
                .collect();
            ExtractionCoverage::TextUnavailable { reason }
        }
    };
    let warnings = value
        .get("warnings")
        .and_then(serde_json::Value::as_array)
        .map(|warnings| {
            warnings
                .iter()
                .filter_map(serde_json::Value::as_str)
                .take(16)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Ok(DocumentExtraction {
        version: DOCUMENT_EXTRACTION_VERSION,
        method,
        method_version,
        blocks,
        warnings,
        coverage,
    })
}

pub(crate) fn convert_with_worker(
    config: &WorkerConfig,
    snapshot_path: &Path,
    source_sha256: &str,
    source_kind: &str,
    anchor_start: u32,
    anchor_end: u32,
    cancellation: Option<&crate::retrieval::OperationCancellation>,
) -> Result<DocumentExtraction, WorkerError> {
    if !WORKER_SOURCE_KINDS.contains(&source_kind) {
        return Err(WorkerError::Unavailable(format!(
            "conversion worker does not handle `{source_kind}`"
        )));
    }
    if anchor_start < 1 || anchor_end < anchor_start {
        return Err(WorkerError::Unavailable(
            "invalid conversion range".to_owned(),
        ));
    }
    if snapshot_path.as_os_str().len() > 4096 {
        return Err(WorkerError::Unavailable(
            "snapshot path is outside the bound".to_owned(),
        ));
    }
    let request = serde_json::json!({
        "version": DOCUMENT_EXTRACTION_VERSION,
        "source_kind": source_kind,
        "source_sha256": source_sha256,
        "snapshot_path": snapshot_path.display().to_string(),
        "anchor_start": anchor_start,
        "anchor_end": anchor_end,
    });
    let request_bytes =
        serde_json::to_vec(&request).map_err(|error| WorkerError::Failed(error.to_string()))?;
    if cancellation.is_some_and(|cancellation| cancellation.is_cancelled()) {
        return Err(WorkerError::Cancelled);
    }
    let mut child = spawn_worker(&config.executable)
        .map_err(|error| WorkerError::Failed(format!("spawn conversion worker: {error}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| WorkerError::Failed("conversion worker stdin is unavailable".to_owned()))?;
    if writeln!(stdin, "{WORKER_PROTOCOL_MARKER}{}", request_bytes.len()).is_err()
        || stdin.write_all(&request_bytes).is_err()
    {
        kill_worker_tree(&mut child);
        let _ = child.wait();
        return Err(WorkerError::Failed("send conversion request".to_owned()));
    }
    drop(stdin);
    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        let Some(mut stdout) = stdout else {
            return Ok(output);
        };
        let mut limited = (&mut stdout).take((MAX_WORKER_MESSAGE_BYTES + 1) as u64);
        match limited.read_to_end(&mut output) {
            Ok(_) => Ok(output),
            Err(error) => Err(error.to_string()),
        }
    });
    let started = Instant::now();
    loop {
        if cancellation.is_some_and(|cancellation| cancellation.is_cancelled()) {
            kill_worker_tree(&mut child);
            let _ = child.wait();
            let _ = drain_reader(reader, started, cancellation);
            return Err(WorkerError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => match drain_reader(reader, started, cancellation) {
                Ok(Ok(output)) => {
                    if !status.success() {
                        return Err(WorkerError::Failed(format!(
                            "conversion worker exited with {status}"
                        )));
                    }
                    if output.len() > MAX_WORKER_MESSAGE_BYTES {
                        return Err(WorkerError::InvalidOutput(
                            "conversion worker output exceeds the bound".to_owned(),
                        ));
                    }
                    return parse_worker_output(&output, source_sha256);
                }
                Ok(Err(error)) => return Err(WorkerError::InvalidOutput(error)),
                Err(DrainTimeout::Expired) => {
                    kill_worker_tree(&mut child);
                    let _ = child.wait();
                    return Err(WorkerError::Timeout);
                }
                Err(DrainTimeout::Cancelled) => {
                    kill_worker_tree(&mut child);
                    let _ = child.wait();
                    return Err(WorkerError::Cancelled);
                }
            },
            Ok(None) => {
                if started.elapsed() > WORKER_DEADLINE {
                    kill_worker_tree(&mut child);
                    let _ = child.wait();
                    let _ = drain_reader(reader, started, cancellation);
                    return Err(WorkerError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                kill_worker_tree(&mut child);
                let _ = child.wait();
                let _ = drain_reader(reader, started, cancellation);
                return Err(WorkerError::Failed(format!(
                    "poll conversion worker: {error}"
                )));
            }
        }
    }
}

fn spawn_worker(executable: &Path) -> std::io::Result<std::process::Child> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        let mut command = Command::new(executable);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0);
        command.spawn()
    }
    #[cfg(not(unix))]
    {
        Command::new(executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
    }
}

fn kill_worker_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let pid = nix::unistd::Pid::from_raw(child.id() as i32);
        let _ = nix::sys::signal::killpg(pid, nix::sys::signal::Signal::SIGKILL);
    }
    let _ = child.kill();
}

fn drain_reader(
    reader: std::thread::JoinHandle<Result<Vec<u8>, String>>,
    started: Instant,
    cancellation: Option<&crate::retrieval::OperationCancellation>,
) -> Result<Result<Vec<u8>, String>, DrainTimeout> {
    loop {
        if cancellation.is_some_and(|cancellation| cancellation.is_cancelled()) {
            return Err(DrainTimeout::Cancelled);
        }
        if reader.is_finished() {
            return Ok(reader
                .join()
                .unwrap_or(Err("reader thread failed".to_owned())));
        }
        if started.elapsed() > WORKER_DEADLINE {
            return Err(DrainTimeout::Expired);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[derive(Debug, Eq, PartialEq)]
enum DrainTimeout {
    Expired,
    Cancelled,
}

fn parse_worker_output(
    output: &[u8],
    source_sha256: &str,
) -> Result<DocumentExtraction, WorkerError> {
    let text = std::str::from_utf8(output)
        .map_err(|_| WorkerError::InvalidOutput("worker output is not UTF-8".to_owned()))?;
    let (marker, body) = text
        .split_once('\n')
        .ok_or_else(|| WorkerError::InvalidOutput("worker output misses the frame".to_owned()))?;
    let Some(length) = marker
        .strip_prefix(WORKER_PROTOCOL_MARKER)
        .and_then(|length| length.trim().parse::<usize>().ok())
    else {
        return Err(WorkerError::InvalidOutput(
            "worker output misses the protocol marker".to_owned(),
        ));
    };
    if length != body.len() {
        return Err(WorkerError::InvalidOutput(
            "worker frame length mismatches".to_owned(),
        ));
    }
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| WorkerError::InvalidOutput("worker output is not JSON".to_owned()))?;
    validate_extraction(&value, source_sha256)
}

pub(crate) fn worker_status() -> serde_json::Value {
    let configured = worker_config_from_env().is_some();
    serde_json::json!({
        "version": DOCUMENT_EXTRACTION_VERSION,
        "protocol": WORKER_PROTOCOL_MARKER.trim(),
        "opted_in": std::env::var_os(WORKER_OPT_IN_ENV).is_some_and(|value| value == "1"),
        "available": configured,
        "source_kinds": WORKER_SOURCE_KINDS,
        "fixed_sample_gate": "not_run",
        "deadline_secs": WORKER_DEADLINE.as_secs(),
        "max_message_bytes": MAX_WORKER_MESSAGE_BYTES,
        "max_text_bytes": MAX_WORKER_TEXT_BYTES,
    })
}

#[cfg(test)]
mod tests;
