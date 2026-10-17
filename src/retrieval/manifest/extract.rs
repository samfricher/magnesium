use std::fs;
use std::io::Read;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use super::pdf::{extract_pdf_from_file, read_bounded_with_cancellation};
use super::publication::{PreparedOutcome, prepare_extraction, publish_extraction};
use super::schema::open_index_for_write;
use super::source::{OpenSource, open_workspace_source};
use super::{
    AnchorKind, CurrentSource, ManifestError, SourceCandidate, hex_digest, io_error, json_column,
    sha256_hex, sqlite,
};
use crate::retrieval::chunking::{ChunkDraft, ChunkKind, chunk_text, page_chunk};
use crate::retrieval::{MAX_PDF_SOURCE_BYTES, MAX_SOURCE_BYTES, SensitivityClass};
use crate::workspace::native_path_key;

const SENSITIVE_ASSIGNMENT_KEYS: &[&str] = &[
    "password",
    "passwd",
    "api_key",
    "api-key",
    "secret",
    "secret_key",
    "secret-key",
    "access_token",
    "access-token",
    "client_secret",
    "client-secret",
    "token",
];

static SNAPSHOT_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct SnapshotGuard<'a>(&'a Path);

impl Drop for SnapshotGuard<'_> {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0);
    }
}

#[cfg(test)]
pub(crate) fn extract_source(
    workspace_root: &Path,
    index_path: &Path,
    relative_path: &Path,
) -> Result<Vec<SourceCandidate>, ManifestError> {
    extract_source_with_cancellation(
        workspace_root,
        index_path,
        relative_path,
        &crate::retrieval::OperationCancellation::none(),
    )
}

pub(crate) fn extract_source_with_cancellation(
    workspace_root: &Path,
    index_path: &Path,
    relative_path: &Path,
    cancellation: &crate::retrieval::OperationCancellation,
) -> Result<Vec<SourceCandidate>, ManifestError> {
    cancellation.check()?;
    let mut after_open = |_: &Path| {
        cancellation.check()?;
        Ok(())
    };
    extract_source_with_hooks(
        workspace_root,
        index_path,
        relative_path,
        None,
        Some(cancellation),
        &mut after_open,
    )
}

#[cfg(test)]
pub(crate) fn extract_source_with_pdf(
    workspace_root: &Path,
    index_path: &Path,
    relative_path: &Path,
    pdf_extractor: impl FnOnce(&[u8]) -> Result<Vec<String>, ManifestError> + 'static,
) -> Result<Vec<SourceCandidate>, ManifestError> {
    let mut after_open = |_: &Path| Ok(());
    extract_source_with_hooks(
        workspace_root,
        index_path,
        relative_path,
        Some(Box::new(pdf_extractor)),
        None,
        &mut after_open,
    )
}

pub(crate) type CustomPdfExtractor = Box<dyn FnOnce(&[u8]) -> Result<Vec<String>, ManifestError>>;

pub(crate) fn extract_source_with_hooks(
    workspace_root: &Path,
    index_path: &Path,
    relative_path: &Path,
    custom_pdf_extractor: Option<CustomPdfExtractor>,
    cancellation: Option<&crate::retrieval::OperationCancellation>,
    after_open: &mut impl FnMut(&Path) -> Result<(), ManifestError>,
) -> Result<Vec<SourceCandidate>, ManifestError> {
    // Pending and already-indexed records can survive bounded reconciliation.
    // Their cached status never overrides the current shared source policy.
    if crate::retrieval::source_relative_path_is_excluded(workspace_root, relative_path)? {
        return Err(ManifestError::Invalid(format!(
            "{} is excluded by current source policy",
            relative_path.display()
        )));
    }
    let mut connection = open_index_for_write(index_path)?;
    let key = native_path_key(relative_path);
    let source = connection
        .query_row(
            "SELECT manifest_id, display_path, source_kind, content_sha256, index_status, source_revision
             FROM source_manifest WHERE path_key = ?1 AND present = 1",
            [key],
            |row| {
                Ok(CurrentSource {
                    manifest_id: row.get(0)?,
                    display_path: row.get(1)?,
                    source_kind: row.get(2)?,
                    content_sha256: row.get(3)?,
                    index_status: row.get(4)?,
                    source_revision: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(|source| sqlite(index_path, "read manifest source", source))?
        .ok_or_else(|| {
            ManifestError::Invalid(format!(
                "{} is not a current supported manifest source",
                relative_path.display()
            ))
        })?;
    if source.index_status == "indexed" {
        let candidates = load_candidates(&connection, index_path, &source)?;
        if candidates.is_empty() {
            return Err(ManifestError::Invalid(format!(
                "{} is marked indexed but has no current source candidate",
                relative_path.display()
            )));
        }
        return Ok(candidates);
    }
    if !matches!(source.index_status.as_str(), "pending" | "failed") {
        return Err(ManifestError::Invalid(format!(
            "{} has source-index status {} and requires reconciliation",
            relative_path.display(),
            source.index_status
        )));
    }

    let fallback_cancellation = crate::retrieval::OperationCancellation::none();
    let cancellation = cancellation.unwrap_or(&fallback_cancellation);
    // Authorization/open-boundary failures are not extracted-content outcomes.
    // In particular a symlink swap must fail before reading or publishing.
    let source_file = open_workspace_source(workspace_root, relative_path, after_open)?;
    cancellation.check()?;
    let extracted = prepare_source_content(
        workspace_root,
        relative_path,
        &source,
        source_file,
        custom_pdf_extractor,
        Some(cancellation),
    );
    let (outcome, converter) = match extracted {
        Ok(extracted) => extracted,
        // Resource limits are incomplete work, never a defective source.
        Err(
            error @ (ManifestError::Cancelled
            | ManifestError::BudgetExhausted
            | ManifestError::PdfTimeout),
        ) => return Err(error),
        Err(error) => {
            let diagnostic = error.to_string().chars().take(240).collect();
            let published = publish_extraction(
                &mut connection,
                index_path,
                &source,
                &PreparedOutcome::Failed { diagnostic },
                cancellation,
            )?;
            return Err(match error {
                ManifestError::PdfContent(diagnostic) => {
                    ManifestError::PdfContentPublished(super::PublishedPdfContentFailure {
                        source: published,
                        diagnostic,
                    })
                }
                other => other,
            });
        }
    };
    cancellation.check()?;
    match outcome {
        PdfOutcome::Drafts(drafts) => {
            let prepared = prepare_extraction(&source, drafts, converter.as_ref())?;
            let ready = PreparedOutcome::Ready(prepared);
            publish_extraction(&mut connection, index_path, &source, &ready, cancellation)?;
            let PreparedOutcome::Ready(prepared) = ready else {
                unreachable!()
            };
            Ok(prepared.into_candidates())
        }
        PdfOutcome::Sensitive(sensitivity) => {
            publish_extraction(
                &mut connection,
                index_path,
                &source,
                &PreparedOutcome::Sensitive(sensitivity),
                cancellation,
            )?;
            Ok(Vec::new())
        }
        PdfOutcome::ChangedDuringRead => {
            publish_extraction(
                &mut connection,
                index_path,
                &source,
                &PreparedOutcome::ChangedDuringRead,
                cancellation,
            )?;
            Err(ManifestError::Invalid(format!(
                "{} changed after manifest reconciliation; reconcile before extraction",
                relative_path.display()
            )))
        }
    }
}

fn prepare_source_content(
    workspace_root: &Path,
    relative_path: &Path,
    source: &CurrentSource,
    source_file: OpenSource,
    custom_pdf_extractor: Option<CustomPdfExtractor>,
    cancellation: Option<&crate::retrieval::OperationCancellation>,
) -> Result<(PdfOutcome, Option<super::convert::ConverterProvenance>), ManifestError> {
    let fallback_read_cancellation = crate::retrieval::OperationCancellation::none();
    let read_cancellation = cancellation.unwrap_or(&fallback_read_cancellation);
    if let Some(cancellation) = cancellation {
        // Re-check after the (possibly slow) open hook and before any
        // extraction work begins.
        cancellation.check()?;
    }
    match source.source_kind.as_str() {
        "text" => {
            let bytes = read_bounded_with_cancellation(
                source_file.file,
                MAX_SOURCE_BYTES,
                read_cancellation,
            )
            .map_err(|source| {
                read_cancellation
                    .check()
                    .err()
                    .unwrap_or_else(|| io_error("read source", &source_file.path, source))
            })?;
            let observed_hash = sha256_hex(&bytes);
            if observed_hash != source.content_sha256 {
                return Ok((PdfOutcome::ChangedDuringRead, None));
            }
            if let Some(sensitivity) = sensitive_content(&bytes) {
                return Ok((PdfOutcome::Sensitive(sensitivity), None));
            }
            String::from_utf8(bytes)
                .map(|text| chunk_text(relative_path, &text))
                .map_err(|_| {
                    ManifestError::Invalid(format!(
                        "{} is not valid UTF-8 text",
                        relative_path.display()
                    ))
                })
                .map(|drafts| (PdfOutcome::Drafts(drafts), None))
        }
        "pdf" => {
            if let Some(custom) = custom_pdf_extractor {
                let bytes = read_bounded_with_cancellation(
                    source_file.file,
                    MAX_PDF_SOURCE_BYTES,
                    read_cancellation,
                )
                .map_err(|source| {
                    read_cancellation
                        .check()
                        .err()
                        .unwrap_or_else(|| io_error("read source", &source_file.path, source))
                })?;
                let observed_hash = sha256_hex(&bytes);
                if observed_hash != source.content_sha256 {
                    return Ok((PdfOutcome::ChangedDuringRead, None));
                }
                if let Some(cancellation) = cancellation {
                    cancellation.check()?;
                }
                let pages = custom(&bytes);
                if let Some(cancellation) = cancellation {
                    // The custom extractor does not poll cancellation itself;
                    // observe it before any candidate is stored.
                    cancellation.check()?;
                }
                pages
                    .and_then(native_pages_to_drafts)
                    .map(|outcome| (outcome, None))
            } else {
                use std::io::{Seek, SeekFrom, Write};
                let scratch_dir = workspace_root
                    .join(crate::workspace::CONTROL_DIR)
                    .join("scratch");
                crate::workspace::reject_symlink_components(&scratch_dir, workspace_root)
                    .map_err(|source| io_error("validate scratch dir", &scratch_dir, source))?;
                crate::workspace::create_private_dir_all(&scratch_dir)
                    .map_err(|source| io_error("create scratch dir", &scratch_dir, source))?;
                let unique_id = SNAPSHOT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let pid = std::process::id();
                let snapshot_path = scratch_dir.join(format!(
                    "pdf-snapshot-{}-{}-{}.tmp",
                    source.manifest_id, pid, unique_id
                ));
                let mut snapshot_file = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .open(&snapshot_path)
                    .map_err(|source| io_error("create pdf snapshot", &snapshot_path, source))?;
                let _guard = SnapshotGuard(&snapshot_path);

                let mut file = source_file.file;
                let mut hasher = Sha256::new();
                let mut buf = [0u8; 64 * 1024];
                let mut total_read = 0usize;
                loop {
                    if let Some(cancellation) = cancellation {
                        cancellation.check()?;
                    }
                    let n = file
                        .read(&mut buf)
                        .map_err(|source| io_error("read pdf source", &source_file.path, source))?;
                    if n == 0 {
                        break;
                    }
                    total_read += n;
                    if total_read > MAX_PDF_SOURCE_BYTES {
                        return Err(ManifestError::Pdf(format!(
                            "PDF input exceeds the {MAX_PDF_SOURCE_BYTES}-byte limit"
                        )));
                    }
                    hasher.update(&buf[..n]);
                    snapshot_file
                        .write_all(&buf[..n])
                        .map_err(|source| io_error("write pdf snapshot", &snapshot_path, source))?;
                }
                if let Some(cancellation) = cancellation {
                    cancellation.check()?;
                }
                let observed_hash = hex_digest(hasher.finalize());
                if observed_hash != source.content_sha256 {
                    return Ok((PdfOutcome::ChangedDuringRead, None));
                }
                snapshot_file
                    .seek(SeekFrom::Start(0))
                    .map_err(|source| io_error("rewind pdf snapshot", &snapshot_path, source))?;
                let native = extract_pdf_from_file(snapshot_file, cancellation);
                // #208: a text layer with no readable page is the exact case
                // an optional local converter exists for (scanned pages,
                // complex layout). The worker runs only under explicit
                // opt-in; any worker failure preserves this native result
                // and the last valid index generation. Worker-assisted
                // generations record converter provenance beside the
                // derived index (see publication); the fixed
                // 12-document gate and the packaged worker itself remain.
                let worker = super::convert::worker_config_from_env();
                match try_worker_for_blank_pdf(
                    source,
                    &native,
                    &snapshot_path,
                    worker.as_ref(),
                    cancellation,
                )? {
                    Some((outcome, provenance)) => Ok((outcome, provenance)),
                    None => match native {
                        Ok(pages) => native_pages_to_drafts(pages).map(|outcome| (outcome, None)),
                        Err(error) => Err(error),
                    },
                }
            }
        }
        _ => Err(ManifestError::Invalid(format!(
            "{} is not extractable",
            relative_path.display()
        ))),
    }
}

/// Classification only. Conversion never changes manifests or projections;
/// every outcome is published later against the captured source revision.
#[derive(Debug)]
pub(super) enum PdfOutcome {
    Sensitive(SensitivityClass),
    ChangedDuringRead,
    Drafts(Vec<ChunkDraft>),
}

/// Map native PDF text-layer pages to page-anchored drafts, preserving
/// the existing sensitivity exclusion.
fn native_pages_to_drafts(pages: Vec<String>) -> Result<PdfOutcome, ManifestError> {
    if let Some(sensitivity) = pages
        .iter()
        .find_map(|page| sensitive_content(page.as_bytes()))
    {
        return Ok(PdfOutcome::Sensitive(sensitivity));
    }
    Ok(PdfOutcome::Drafts(
        pages
            .into_iter()
            .enumerate()
            .map(|(index, text)| {
                let page = u32::try_from(index + 1).expect("PDF page limit fits u32");
                page_chunk(page, text)
            })
            .collect(),
    ))
}

/// Attempt optional worker conversion for a PDF whose native text layer
/// is entirely blank (scanned pages, complex layout). Returns `None`
/// whenever the native result stands: readable pages, no opt-in, no
/// worker, worker failure, or a `text_unavailable` conversion. The
/// native result and the last valid index generation are always
/// preserved; cancellation propagates like the native path.
pub(super) fn try_worker_for_blank_pdf(
    source: &CurrentSource,
    native: &Result<Vec<String>, ManifestError>,
    snapshot_path: &Path,
    worker: Option<&super::convert::WorkerConfig>,
    cancellation: Option<&crate::retrieval::OperationCancellation>,
) -> Result<Option<(PdfOutcome, Option<super::convert::ConverterProvenance>)>, ManifestError> {
    let Ok(pages) = native else {
        return Ok(None);
    };
    if pages.iter().any(|page| !page.trim().is_empty()) {
        return Ok(None);
    }
    let Some(worker) = worker else {
        return Ok(None);
    };
    let page_count = u32::try_from(pages.len().max(1)).expect("PDF page limit fits u32");
    let extraction = match super::convert::convert_with_worker(
        worker,
        snapshot_path,
        &source.content_sha256,
        "pdf",
        1,
        page_count,
        cancellation,
    ) {
        Ok(extraction) => extraction,
        Err(super::convert::WorkerError::Cancelled) => return Err(ManifestError::Cancelled),
        Err(_) => return Ok(None),
    };
    if extraction.blocks.is_empty()
        || matches!(
            extraction.coverage,
            super::convert::ExtractionCoverage::TextUnavailable { .. }
        )
    {
        return Ok(None);
    }
    if let Some(sensitivity) = extraction
        .blocks
        .iter()
        .find_map(|block| sensitive_content(block.text.as_bytes()))
    {
        return Ok(Some((PdfOutcome::Sensitive(sensitivity), None)));
    }
    // Every converted block links to its original anchor: page blocks keep
    // their physical file index, section blocks fall back to their start
    // page with the converter warning intact for the #206 original view.
    // Provenance travels beside the drafts so the derived index records
    // which converter produced this generation.
    let provenance = extraction.provenance();
    Ok(Some((
        PdfOutcome::Drafts(
            extraction
                .blocks
                .into_iter()
                .map(|block| page_chunk(block.anchor_start, block.text))
                .collect(),
        ),
        Some(provenance),
    )))
}

#[cfg(test)]
pub(crate) fn store_candidates(
    connection: &mut Connection,
    index_path: &Path,
    source: &CurrentSource,
    extracted: Vec<ChunkDraft>,
    converter: Option<&super::convert::ConverterProvenance>,
) -> Result<Vec<SourceCandidate>, ManifestError> {
    let outcome = PreparedOutcome::Ready(prepare_extraction(source, extracted, converter)?);
    publish_extraction(
        connection,
        index_path,
        source,
        &outcome,
        &crate::retrieval::OperationCancellation::none(),
    )?;
    let PreparedOutcome::Ready(prepared) = outcome else {
        unreachable!()
    };
    Ok(prepared.into_candidates())
}

pub(crate) fn load_candidates(
    connection: &Connection,
    index_path: &Path,
    source: &CurrentSource,
) -> Result<Vec<SourceCandidate>, ManifestError> {
    let mut statement = connection
        .prepare(
            "SELECT candidate_id, anchor_kind, anchor_start, anchor_end, body,
                    chunk_kind, structural_path, language,
                    definitions_json, imports_json, references_json
             FROM source_candidates
             WHERE manifest_id = ?1 AND content_sha256 = ?2 AND status = 'current'
             ORDER BY anchor_start, candidate_id",
        )
        .map_err(|error| sqlite(index_path, "prepare source candidate query", error))?;
    let rows = statement
        .query_map(params![source.manifest_id, source.content_sha256], |row| {
            let kind: String = row.get(1)?;
            let chunk_kind = row.get::<_, String>(5)?;
            Ok(SourceCandidate {
                candidate_id: row.get(0)?,
                path: source.display_path.clone(),
                content_sha256: source.content_sha256.clone(),
                anchor_kind: if kind == "page" {
                    AnchorKind::Page
                } else {
                    AnchorKind::Line
                },
                anchor_start: row.get(2)?,
                anchor_end: row.get(3)?,
                body: row.get(4)?,
                chunk_kind: ChunkKind::parse(&chunk_kind).ok_or_else(|| {
                    rusqlite::Error::InvalidColumnType(
                        5,
                        "chunk_kind".to_owned(),
                        rusqlite::types::Type::Text,
                    )
                })?,
                structural_path: row.get(6)?,
                language: row.get(7)?,
                definitions: json_column(row, 8)?,
                imports: json_column(row, 9)?,
                references: json_column(row, 10)?,
            })
        })
        .map_err(|error| sqlite(index_path, "query source candidates", error))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| sqlite(index_path, "read source candidates", error))
}

pub(crate) fn hash_file(
    mut file: fs::File,
    path: &Path,
    source_kind: &str,
    cancellation: &crate::retrieval::OperationCancellation,
) -> Result<(String, Option<SensitivityClass>), ManifestError> {
    let mut hasher = Sha256::new();
    let mut total = 0_usize;
    let mut sensitivity = None;
    let mut scan_tail = Vec::new();
    let mut buffer = [0_u8; 64 * 1024];
    let limit = if source_kind == "pdf" {
        MAX_PDF_SOURCE_BYTES
    } else {
        MAX_SOURCE_BYTES
    };
    loop {
        #[cfg(test)]
        crate::request::test_checkpoint("source_hash_read_chunk");
        cancellation.check()?;
        let read = file
            .read(&mut buffer)
            .map_err(|source| io_error("hash source", path, source))?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read);
        if total > limit {
            return Err(ManifestError::Invalid(format!(
                "source {} exceeds the {limit}-byte extraction limit",
                path.display()
            )));
        }
        if sensitivity.is_none() {
            let mut scan_window = scan_tail.clone();
            scan_window.extend_from_slice(&buffer[..read]);
            sensitivity = sensitive_content(&scan_window);
            let keep = scan_window.len().min(128);
            scan_tail = scan_window[scan_window.len() - keep..].to_vec();
        }
        hasher.update(&buffer[..read]);
    }
    cancellation.check()?;
    Ok((hex_digest(hasher.finalize()), sensitivity))
}

pub(crate) fn sensitive_content(bytes: &[u8]) -> Option<SensitivityClass> {
    let text = String::from_utf8_lossy(bytes).to_ascii_lowercase();
    if text.contains("-----begin ") && text.contains(" private key-----") {
        return Some(SensitivityClass::PrivateKey);
    }
    if [
        "aws_secret_access_key=",
        "github_token=",
        "github_pat_",
        "ghp_",
        "xoxb-",
        "xoxp-",
        "authorization: bearer ",
    ]
    .iter()
    .any(|marker| text.contains(marker))
    {
        return Some(SensitivityClass::Credential);
    }
    for key in SENSITIVE_ASSIGNMENT_KEYS {
        let quoted = format!("\"{key}\"");
        for (offset, _) in text.match_indices(&quoted) {
            let rest = &text[offset + quoted.len()..];
            let rest = rest.trim_start();
            if rest
                .strip_prefix(':')
                .or_else(|| rest.strip_prefix('='))
                .is_some_and(|value| !value.trim().is_empty())
            {
                return Some(SensitivityClass::Credential);
            }
        }
    }
    for line in text.lines() {
        let line = line.trim_start();
        for key in SENSITIVE_ASSIGNMENT_KEYS {
            let Some(rest) = line.strip_prefix(key) else {
                continue;
            };
            let rest = rest.trim_start();
            let Some(value) = rest.strip_prefix('=').or_else(|| rest.strip_prefix(':')) else {
                continue;
            };
            if !value.trim().is_empty() {
                return Some(SensitivityClass::Credential);
            }
        }
    }
    None
}

pub(crate) fn candidate_id(
    manifest_id: i64,
    anchor_kind: &str,
    start: u32,
    end: u32,
    chunk_kind: &str,
    structural_path: Option<&str>,
    body: &str,
) -> String {
    sha256_hex(
        format!(
            "source-candidate\0{manifest_id}\0{anchor_kind}\0{start}\0{end}\0{chunk_kind}\0{}\0{}",
            structural_path.unwrap_or(""),
            sha256_hex(body.as_bytes())
        )
        .as_bytes(),
    )
}
