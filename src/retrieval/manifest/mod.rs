pub(crate) mod convert;
pub(crate) mod extract;
pub(crate) mod pdf;
pub(crate) mod publication;
#[cfg(test)]
mod publication_tests;
pub(crate) mod ranking;
pub(crate) mod reconcile;
#[cfg(test)]
mod revision_tests;
pub(crate) mod schema;
pub(crate) mod source;
pub(crate) mod status;
#[cfg(test)]
mod tests;

use serde::de::DeserializeOwned;
use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

pub(crate) use extract::extract_source_with_cancellation;
pub(crate) use pdf::run_pdf_extractor;
pub(crate) use reconcile::load_or_start_reconciliation;
#[cfg(test)]
pub(crate) use reconcile::reconcile_batch;
pub(crate) use status::{
    IndexingProgress, current_generation, indexing_progress, manifest_coverage,
    pending_source_paths, source_status_and_size,
};

use crate::retrieval::DiscoveryError;
use crate::retrieval::chunking::{ChunkKind, SourceImport, SymbolDefinition, SymbolReference};
use crate::workspace::reject_symlink_components;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ManifestReport {
    pub(crate) generation: i64,
    pub(crate) created: usize,
    pub(crate) changed: usize,
    pub(crate) moved: usize,
    pub(crate) deleted: usize,
    pub(crate) unchanged: usize,
    pub(crate) metadata_only: usize,
    pub(crate) excluded: usize,
    pub(crate) content_reads: usize,
    pub(crate) content_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReconciliationState {
    pub(crate) generation: i64,
    pub(crate) cursor_json: String,
}

#[derive(Clone, Copy)]
pub(crate) enum MovePolicy {
    MissingOnly,
    Isolated,
}

pub(crate) struct ReconcileProgress<'a> {
    pub(crate) moved_ids: HashSet<i64>,
    pub(crate) moves: MovePolicy,
    pub(crate) cancellation: &'a crate::retrieval::OperationCancellation,
    pub(crate) report: ManifestReport,
    pub(crate) hashed: std::collections::HashMap<Vec<u8>, reconcile::HashedContent>,
    pub(crate) after_open: &'a mut dyn FnMut(&Path) -> Result<(), ManifestError>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AnchorKind {
    Line,
    Page,
}

impl AnchorKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Page => "page",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceCandidate {
    pub(crate) candidate_id: String,
    pub(crate) path: String,
    pub(crate) content_sha256: String,
    pub(crate) anchor_kind: AnchorKind,
    pub(crate) anchor_start: u32,
    pub(crate) anchor_end: u32,
    pub(crate) body: String,
    pub(crate) chunk_kind: ChunkKind,
    pub(crate) structural_path: Option<String>,
    pub(crate) language: Option<String>,
    pub(crate) definitions: Vec<SymbolDefinition>,
    pub(crate) imports: Vec<SourceImport>,
    pub(crate) references: Vec<SymbolReference>,
}

#[derive(Debug)]
pub(crate) enum ManifestError {
    Discovery(DiscoveryError),
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    Sqlite {
        action: &'static str,
        path: PathBuf,
        source: rusqlite::Error,
    },
    Invalid(String),
    ReconciliationConflict,
    SourceSuperseded,
    Cancelled,
    BudgetExhausted,
    Pdf(String),
    PdfContent(String),
    PdfContentPublished(PublishedPdfContentFailure),
    /// The file's content cannot be extracted and extraction already recorded
    /// it as `failed`. The wrapped error is what an explicit request for the
    /// file reports. Worker and I/O errors are not wrapped, so they stay fatal.
    SourceFailurePublished(Box<ManifestError>),
    PdfTimeout,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(error) => error.fmt(formatter),
            Self::Io {
                action,
                path,
                source,
            } => write!(formatter, "cannot {action} {}: {source}", path.display()),
            Self::Sqlite {
                action,
                path,
                source,
            } => write!(formatter, "cannot {action} {}: {source}", path.display()),
            Self::Invalid(message) | Self::Pdf(message) | Self::PdfContent(message) => formatter.write_str(message),
            Self::PdfContentPublished(failure) => formatter.write_str(&failure.diagnostic),
            Self::SourceFailurePublished(error) => error.fmt(formatter),
            Self::ReconciliationConflict => formatter
                .write_str("source reconciliation cursor changed concurrently; retry the query"),
            Self::SourceSuperseded => formatter.write_str("source_superseded: a newer source state won; no old result was published. Repeat the source request if it is still useful"),
            Self::Cancelled => formatter.write_str("source_operation_cancelled: request cancelled; retry only if retrieval is still wanted"),
            Self::BudgetExhausted => formatter.write_str("source_search_budget_exhausted: retrieval is incomplete; narrow the query or path scope and retry; this is not evidence that no source exists"),
            Self::PdfTimeout => {
                formatter.write_str("PDF extraction exceeded the 5-second deadline")
            }
        }
    }
}

impl Error for ManifestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Discovery(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::Sqlite { source, .. } => Some(source),
            Self::SourceFailurePublished(error) => Some(error.as_ref()),
            Self::Invalid(_)
            | Self::ReconciliationConflict
            | Self::SourceSuperseded
            | Self::Cancelled
            | Self::BudgetExhausted
            | Self::Pdf(_)
            | Self::PdfContent(_)
            | Self::PdfContentPublished(_)
            | Self::PdfTimeout => None,
        }
    }
}

impl From<DiscoveryError> for ManifestError {
    fn from(error: DiscoveryError) -> Self {
        Self::Discovery(error)
    }
}

#[derive(Debug)]
pub(crate) struct PublishedSource {
    pub(crate) manifest_id: i64,
    pub(crate) source_revision: i64,
    pub(crate) content_sha256: String,
}

#[derive(Debug)]
pub(crate) struct PublishedPdfContentFailure {
    pub(crate) source: PublishedSource,
    pub(crate) diagnostic: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ExistingRecord {
    pub(crate) manifest_id: i64,
    pub(crate) source_kind: String,
    pub(crate) file_size: Option<i64>,
    pub(crate) modified_unix_ns: Option<i64>,
    pub(crate) content_sha256: Option<String>,
    pub(crate) present: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct CurrentSource {
    pub(crate) manifest_id: i64,
    pub(crate) source_revision: i64,
    pub(crate) display_path: String,
    pub(crate) source_kind: String,
    pub(crate) content_sha256: String,
    pub(crate) index_status: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ManifestCoverage {
    pub(crate) scan_active: bool,
    pub(crate) pending: usize,
    pub(crate) stale: usize,
}

pub(crate) fn reject_managed_symlink(path: &Path) -> Result<(), ManifestError> {
    let boundary = path.parent().and_then(Path::parent).unwrap_or(path);
    reject_symlink_components(path, boundary)
        .map_err(|source| io_error("inspect source-index path", path, source))?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(ManifestError::Invalid(format!(
                "source index {} must be a regular non-symlink file",
                path.display()
            )))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_error("inspect source index", path, source)),
    }
}

pub(crate) fn to_i64(value: Option<u64>) -> Option<i64> {
    value.map(|value| i64::try_from(value).unwrap_or(i64::MAX))
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex_digest(Sha256::digest(bytes))
}

pub(crate) fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn anchor_name(kind: AnchorKind) -> &'static str {
    match kind {
        AnchorKind::Line => "line",
        AnchorKind::Page => "page",
    }
}

pub(crate) fn json_column<T: DeserializeOwned>(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<T> {
    let value = row.get::<_, String>(index)?;
    serde_json::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

pub(crate) fn sqlite(path: &Path, action: &'static str, source: rusqlite::Error) -> ManifestError {
    ManifestError::Sqlite {
        action,
        path: path.to_path_buf(),
        source,
    }
}

pub(crate) fn io_error(action: &'static str, path: &Path, source: io::Error) -> ManifestError {
    ManifestError::Io {
        action,
        path: path.to_path_buf(),
        source,
    }
}
