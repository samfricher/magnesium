use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, Transaction};

use super::schema::open_index;
use super::{ExistingRecord, ManifestCoverage, ManifestError, sqlite};
use crate::retrieval::OperationCancellation;
use crate::workspace::{decode_native_path, native_path_key};

pub(crate) fn source_status_and_size(
    index_path: &Path,
    relative_path: &Path,
) -> Result<Option<(String, u64)>, ManifestError> {
    let connection = open_index(index_path)?;
    connection
        .query_row(
            "SELECT index_status, file_size FROM source_manifest
             WHERE path_key = ?1 AND present = 1 AND source_kind IN ('text', 'pdf')",
            [native_path_key(relative_path)],
            |row| {
                let size: i64 = row.get(1)?;
                Ok((row.get(0)?, u64::try_from(size).unwrap_or(u64::MAX)))
            },
        )
        .optional()
        .map_err(|source| sqlite(index_path, "read source status", source))
}

/// Sources a search may extract, in path order. Failed sources are not listed:
/// extracting them again fails again until the file changes and
/// reconciliation marks it pending. An explicit request still retries one.
pub(crate) fn pending_source_paths(
    index_path: &Path,
    query_scope: &Path,
    maximum: usize,
) -> Result<Vec<PathBuf>, ManifestError> {
    if maximum == 0 {
        return Ok(Vec::new());
    }
    let connection = open_index(index_path)?;
    let mut statement = connection
        .prepare(
            "SELECT path_key FROM source_manifest
             WHERE present = 1 AND source_kind IN ('text', 'pdf') AND index_status = 'pending'
             ORDER BY display_path COLLATE BINARY, manifest_id",
        )
        .map_err(|source| sqlite(index_path, "prepare pending-source query", source))?;
    let paths = statement
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(|source| sqlite(index_path, "query pending sources", source))?;
    let mut selected = Vec::with_capacity(maximum);
    for path_key in paths {
        if selected.len() == maximum {
            break;
        }
        let relative = decode_native_path(
            &path_key.map_err(|source| sqlite(index_path, "read pending source", source))?,
        )
        .map_err(ManifestError::Invalid)?;
        if query_scope.as_os_str().is_empty() || relative.starts_with(query_scope) {
            selected.push(relative);
        }
    }
    Ok(selected)
}

/// What catch-up needs before each step, read from one snapshot.
pub(crate) struct IndexingProgress {
    /// Present text and PDF sources whose current content waits for
    /// extraction, in path order. Failed sources are not listed: extracting
    /// them again fails again.
    pub(crate) pending: Vec<PathBuf>,
    /// Whether any full scan has completed.
    pub(crate) scanned: bool,
    /// The saved cursor of the scan in progress, if one is in progress.
    pub(crate) scan_cursor: Option<String>,
}

pub(crate) fn indexing_progress(
    index_path: &Path,
    maximum: usize,
) -> Result<IndexingProgress, ManifestError> {
    let connection = open_index(index_path)?;
    let snapshot = connection
        .unchecked_transaction()
        .map_err(|source| sqlite(index_path, "begin indexing-progress snapshot", source))?;
    let mut statement = snapshot
        .prepare(
            "SELECT path_key FROM source_manifest
             WHERE present = 1 AND source_kind IN ('text', 'pdf') AND index_status = 'pending'
             ORDER BY display_path COLLATE BINARY, manifest_id
             LIMIT ?1",
        )
        .map_err(|source| sqlite(index_path, "prepare pending-source query", source))?;
    let pending = statement
        .query_map([i64::try_from(maximum).unwrap_or(i64::MAX)], |row| {
            row.get::<_, Vec<u8>>(0)
        })
        .map_err(|source| sqlite(index_path, "query pending sources", source))?
        .map(|path_key| {
            decode_native_path(
                &path_key.map_err(|source| sqlite(index_path, "read pending source", source))?,
            )
            .map_err(ManifestError::Invalid)
        })
        .collect::<Result<_, _>>()?;
    let (scanned, scan_cursor) = snapshot
        .query_row(
            "SELECT
                 (SELECT generation > 0 FROM source_index_metadata WHERE singleton = 1),
                 (SELECT cursor_json FROM source_reconciliation_state WHERE singleton = 1)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|source| sqlite(index_path, "read scan progress", source))?;
    Ok(IndexingProgress {
        pending,
        scanned,
        scan_cursor,
    })
}

/// Last fully reconciled index generation. A deferred pass reports this
/// generation because it describes the snapshot the caller may actually
/// search, never the generation of a pass that failed to commit.
pub(crate) fn current_generation(index_path: &Path) -> Result<i64, ManifestError> {
    let connection = open_index(index_path)?;
    connection
        .query_row(
            "SELECT generation FROM source_index_metadata WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|source| sqlite(index_path, "read current source generation", source))
}

/// True when the index holds at least one indexed source, no source waits for
/// its first extraction, and no recheck is in progress. Failed sources do not
/// count: a smaller recheck does not help them. A pass that ran out of time
/// leaves its recheck in progress, so the next call falls back to the
/// smaller recheck and commits progress. Any read problem answers false.
pub(crate) fn indexed_with_nothing_pending(index_path: &Path) -> bool {
    let Ok(connection) = open_index(index_path) else {
        return false;
    };
    connection
        .query_row(
            "SELECT
                 EXISTS(SELECT 1 FROM source_manifest WHERE present = 1 AND index_status = 'indexed'),
                 EXISTS(SELECT 1 FROM source_manifest WHERE present = 1 AND index_status = 'pending'),
                 EXISTS(SELECT 1 FROM source_reconciliation_state WHERE singleton = 1)",
            [],
            |row| {
                Ok(row.get::<_, bool>(0)? && !row.get::<_, bool>(1)? && !row.get::<_, bool>(2)?)
            },
        )
        .unwrap_or(false)
}

pub(crate) fn manifest_coverage(index_path: &Path) -> Result<ManifestCoverage, ManifestError> {
    let connection = open_index(index_path)?;
    let scan_active = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM source_reconciliation_state WHERE singleton = 1)",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|source| sqlite(index_path, "read reconciliation coverage", source))?
        == 1;
    let (pending, stale): (i64, i64) = connection
        .query_row(
            "SELECT
                 coalesce(sum(CASE WHEN present = 1 AND index_status IN ('pending', 'failed') THEN 1 ELSE 0 END), 0),
                 coalesce(sum(CASE WHEN present = 1 AND index_status = 'stale' THEN 1 ELSE 0 END), 0)
             FROM source_manifest",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|source| sqlite(index_path, "count source-index coverage", source))?;
    Ok(ManifestCoverage {
        scan_active,
        pending: usize::try_from(pending).unwrap_or(usize::MAX),
        stale: usize::try_from(stale).unwrap_or(usize::MAX),
    })
}

pub(crate) fn load_existing(
    transaction: &Transaction<'_>,
    index_path: &Path,
    cancellation: &OperationCancellation,
) -> Result<HashMap<Vec<u8>, ExistingRecord>, ManifestError> {
    let mut statement = transaction
        .prepare(
            "SELECT manifest_id, path_key, source_kind, file_size, modified_unix_ns,
                    content_sha256, present
             FROM source_manifest",
        )
        .map_err(|source| sqlite(index_path, "prepare existing manifest query", source))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(1)?,
                ExistingRecord {
                    manifest_id: row.get(0)?,
                    source_kind: row.get(2)?,
                    file_size: row.get(3)?,
                    modified_unix_ns: row.get(4)?,
                    content_sha256: row.get(5)?,
                    present: row.get::<_, i64>(6)? == 1,
                },
            ))
        })
        .map_err(|source| sqlite(index_path, "query existing manifest", source))?;
    let mut existing = HashMap::new();
    for row in rows {
        cancellation.check()?;
        let (key, record) =
            row.map_err(|source| sqlite(index_path, "read existing manifest", source))?;
        existing.insert(key, record);
    }
    Ok(existing)
}

pub(crate) fn stale_candidates(
    transaction: &Transaction<'_>,
    manifest_id: i64,
    index_path: &Path,
) -> Result<(), ManifestError> {
    transaction
        .execute(
            "DELETE FROM source_search WHERE rowid IN (
                 SELECT candidate_no FROM source_candidates WHERE manifest_id = ?1
             )",
            [manifest_id],
        )
        .map_err(|source| sqlite(index_path, "remove stale source search rows", source))?;
    transaction
        .execute(
            "DELETE FROM source_search_ranked WHERE rowid IN (
                 SELECT candidate_no FROM source_candidates WHERE manifest_id = ?1
             )",
            [manifest_id],
        )
        .map_err(|source| sqlite(index_path, "remove stale ranked search rows", source))?;
    transaction
        .execute(
            "UPDATE source_candidates
             SET status = 'stale', body = NULL, structural_path = NULL,
                 definitions_json = '[]', imports_json = '[]', references_json = '[]'
             WHERE manifest_id = ?1 AND status = 'current'",
            [manifest_id],
        )
        .map_err(|source| sqlite(index_path, "stale source candidates", source))?;
    Ok(())
}

pub(crate) const SOURCES_EXCERPT_CHAR_CAP: usize = 65_536;

pub(crate) struct ManifestRow {
    pub(crate) manifest_id: i64,
    pub(crate) source_revision: i64,
    pub(crate) display_path: String,
    pub(crate) source_kind: String,
    pub(crate) content_sha256: Option<String>,
    pub(crate) index_status: String,
    pub(crate) file_size: Option<i64>,
    pub(crate) exclusion_reason: Option<String>,
    pub(crate) last_error: Option<String>,
    pub(crate) present: bool,
    pub(crate) extraction_method: Option<String>,
    pub(crate) extraction_method_version: Option<String>,
    pub(crate) extraction_coverage: Option<String>,
    pub(crate) extraction_missing_json: Option<String>,
    pub(crate) extraction_warnings_json: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Interval {
    pub(crate) start: u32,
    pub(crate) end: u32,
}

pub(crate) struct SpanFact {
    pub(crate) interval: Interval,
    pub(crate) readable: bool,
}

#[derive(Default)]
pub(crate) struct CandidateFacts {
    pub(crate) spans: Vec<SpanFact>,
    pub(crate) observed_end: u32,
    pub(crate) heading: Option<String>,
    pub(crate) excerpt: Option<String>,
    pub(crate) excerpt_ranges: Vec<Interval>,
    pub(crate) excerpt_truncated: bool,
}

pub(crate) enum FactsInterest {
    IdentityOnly,
    Range {
        start: u32,
        end: Option<u32>,
        include_excerpt: bool,
    },
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PreparationDisposition {
    Complete,
    DeferredBudget,
    DeferredConcurrentUpdate,
    PdfContentFailure,
}

pub(crate) struct SelectedSourceSnapshot {
    pub(crate) preparation: PreparationDisposition,
    pub(crate) row: Option<ManifestRow>,
    pub(crate) candidates: CandidateFacts,
}

pub(crate) fn read_selected_facts(
    index_path: &Path,
    relative: &Path,
    interest: FactsInterest,
    preparation: PreparationDisposition,
    published: Option<&super::PublishedPdfContentFailure>,
    cancellation: &OperationCancellation,
) -> Result<SelectedSourceSnapshot, ManifestError> {
    cancellation.check()?;
    let mut connection = open_index(index_path)?;
    let snapshot = connection
        .transaction()
        .map_err(|error| sqlite(index_path, "start selected source snapshot", error))?;
    let row = read_manifest_row_from(&snapshot, index_path, relative)?;
    #[cfg(test)]
    crate::request::test_checkpoint("sources_manifest_read");
    cancellation.check()?;
    if let Some(published) = published
        && !row.as_ref().is_some_and(|row| {
            row.present
                && row.source_kind == "pdf"
                && row.index_status == "failed"
                && row.manifest_id == published.source.manifest_id
                && row.source_revision == published.source.source_revision
                && row.content_sha256.as_deref() == Some(&published.source.content_sha256)
        })
    {
        return Err(ManifestError::SourceSuperseded);
    }
    let candidates = match (&row, interest) {
        (
            Some(row),
            FactsInterest::Range {
                start,
                end,
                include_excerpt,
            },
        ) if row.present && matches!(row.source_kind.as_str(), "text" | "pdf") => {
            load_current_candidates(
                &snapshot,
                index_path,
                row,
                start,
                end,
                include_excerpt,
                cancellation,
            )?
        }
        _ => CandidateFacts::default(),
    };
    snapshot
        .commit()
        .map_err(|error| sqlite(index_path, "close selected source snapshot", error))?;
    cancellation.check()?;
    Ok(SelectedSourceSnapshot {
        preparation,
        row,
        candidates,
    })
}

#[cfg(test)]
pub(crate) fn read_manifest_row(
    index_path: &Path,
    relative: &Path,
) -> Result<Option<ManifestRow>, ManifestError> {
    let connection = open_index(index_path)?;
    read_manifest_row_from(&connection, index_path, relative)
}

pub(crate) fn read_manifest_row_from(
    connection: &rusqlite::Connection,
    index_path: &Path,
    relative: &Path,
) -> Result<Option<ManifestRow>, ManifestError> {
    let key = native_path_key(relative);
    let mut statement = connection
        .prepare(
            "SELECT manifest_id, display_path, source_kind, content_sha256,
                    index_status, file_size, exclusion_reason, last_error, present,
                    extraction_method, extraction_method_version, extraction_coverage,
                    extraction_missing_json, extraction_warnings_json, source_revision
             FROM source_manifest WHERE path_key = ?1",
        )
        .map_err(|error| sqlite(index_path, "read selected source manifest", error))?;
    statement
        .query_row([key], |row| {
            Ok(ManifestRow {
                manifest_id: row.get(0)?,
                display_path: row.get(1)?,
                source_kind: row.get(2)?,
                content_sha256: row.get(3)?,
                index_status: row.get(4)?,
                file_size: row.get(5)?,
                exclusion_reason: row.get(6)?,
                last_error: row.get(7)?,
                present: row.get::<_, i64>(8)? == 1,
                extraction_method: row.get(9)?,
                extraction_method_version: row.get(10)?,
                extraction_coverage: row.get(11)?,
                extraction_missing_json: row.get(12)?,
                extraction_warnings_json: row.get(13)?,
                source_revision: row.get(14)?,
            })
        })
        .optional()
        .map_err(|error| sqlite(index_path, "read selected source manifest", error))
}

pub(crate) fn load_current_candidates(
    connection: &rusqlite::Connection,
    index_path: &Path,
    row: &ManifestRow,
    start: u32,
    end: Option<u32>,
    include_excerpt: bool,
    cancellation: &OperationCancellation,
) -> Result<CandidateFacts, ManifestError> {
    cancellation.check()?;
    let hash = row.content_sha256.as_deref().unwrap_or_default();
    let kind = if row.source_kind == "pdf" {
        "page"
    } else {
        "line"
    };
    let observed_end: Option<u32> = connection.query_row(
        "SELECT max(anchor_end) FROM source_candidates
         WHERE manifest_id = ?1 AND content_sha256 = ?2 AND status = 'current' AND anchor_kind = ?3",
        rusqlite::params![row.manifest_id, hash, kind], |row| row.get(0),
    ).map_err(|error| sqlite(index_path, "read selected source candidates", error))?;
    let mut facts = CandidateFacts {
        observed_end: observed_end.unwrap_or(0),
        excerpt: include_excerpt.then(String::new),
        ..CandidateFacts::default()
    };
    let mut statement = connection
        .prepare(
            "SELECT anchor_start, anchor_end, body, structural_path FROM source_candidates
         WHERE manifest_id = ?1 AND content_sha256 = ?2 AND status = 'current'
           AND anchor_kind = ?3 AND anchor_end >= ?4 AND (?5 IS NULL OR anchor_start <= ?5)
         ORDER BY anchor_start, candidate_id",
        )
        .map_err(|error| sqlite(index_path, "read selected source candidates", error))?;
    let mut rows = statement
        .query(rusqlite::params![row.manifest_id, hash, kind, start, end])
        .map_err(|error| sqlite(index_path, "read selected source candidates", error))?;
    let mut retained_chars = 0;
    while let Some(row) = rows
        .next()
        .map_err(|error| sqlite(index_path, "read selected source candidates", error))?
    {
        #[cfg(test)]
        crate::request::test_checkpoint("selected_candidate_scan");
        cancellation.check()?;
        let interval = Interval {
            start: row
                .get(0)
                .map_err(|error| sqlite(index_path, "read selected source candidates", error))?,
            end: row
                .get(1)
                .map_err(|error| sqlite(index_path, "read selected source candidates", error))?,
        };
        let body = row
            .get::<_, Option<String>>(2)
            .map_err(|error| sqlite(index_path, "read selected source candidates", error))?
            .unwrap_or_default();
        facts.spans.push(SpanFact {
            interval,
            readable: !body.trim().is_empty(),
        });
        if facts.heading.is_none() {
            facts.heading = row
                .get(3)
                .map_err(|error| sqlite(index_path, "read selected source candidates", error))?;
        }
        if let Some(excerpt) = &mut facts.excerpt {
            if !excerpt.is_empty() && retained_chars < SOURCES_EXCERPT_CHAR_CAP {
                excerpt.push('\n');
                retained_chars += 1;
            }
            let remaining = SOURCES_EXCERPT_CHAR_CAP - retained_chars;
            let bounded = body.chars().take(remaining).collect::<String>();
            let added = bounded.chars().count();
            if added > 0 {
                facts.excerpt_ranges.push(interval);
                excerpt.push_str(&bounded);
                retained_chars += added;
            }
            facts.excerpt_truncated |= body.chars().count() > added;
        }
    }
    cancellation.check()?;
    Ok(facts)
}
