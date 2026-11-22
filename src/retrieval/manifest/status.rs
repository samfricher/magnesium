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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PendingSource {
    pub(crate) path: PathBuf,
    pub(crate) file_size: Option<i64>,
    pub(crate) modified_unix_ns: Option<i64>,
}

pub(crate) struct IndexingProgress {
    pub(crate) pending: Vec<PendingSource>,
    pub(crate) scanned: bool,
    pub(crate) scan_cursor: Option<String>,
}

pub(crate) fn indexing_progress(
    index_path: &Path,
    maximum: usize,
    wanted: impl Fn(&PendingSource) -> bool,
) -> Result<IndexingProgress, ManifestError> {
    let connection = open_index(index_path)?;
    let snapshot = connection
        .unchecked_transaction()
        .map_err(|source| sqlite(index_path, "begin indexing-progress snapshot", source))?;
    let mut statement = snapshot
        .prepare(
            "SELECT path_key, file_size, modified_unix_ns FROM source_manifest
             WHERE present = 1 AND source_kind IN ('text', 'pdf') AND index_status = 'pending'
             ORDER BY display_path COLLATE BINARY, manifest_id",
        )
        .map_err(|source| sqlite(index_path, "prepare pending-source query", source))?;
    let mut rows = statement
        .query([])
        .map_err(|source| sqlite(index_path, "query pending sources", source))?;
    let mut pending = Vec::new();
    while pending.len() < maximum
        && let Some(row) = rows
            .next()
            .map_err(|source| sqlite(index_path, "read pending source", source))?
    {
        let read = |source| sqlite(index_path, "read pending source", source);
        let source = PendingSource {
            path: decode_native_path(&row.get::<_, Vec<u8>>(0).map_err(read)?)
                .map_err(ManifestError::Invalid)?,
            file_size: row.get(1).map_err(read)?,
            modified_unix_ns: row.get(2).map_err(read)?,
        };
        if wanted(&source) {
            pending.push(source);
        }
    }
    drop(rows);
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
                    content_sha256, present, exclusion_reason
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
                    exclusion_reason: row.get(7)?,
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
pub(crate) const MAX_LISTED_SECTIONS: usize = 2_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SectionFact {
    pub(crate) heading: String,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

pub(crate) struct FileSections {
    pub(crate) sections: Vec<SectionFact>,
    pub(crate) untitled_anchors: usize,
    pub(crate) truncated: bool,
}

pub(crate) fn read_sections(
    index_path: &Path,
    row: &ManifestRow,
) -> Result<FileSections, ManifestError> {
    let connection = open_index(index_path)?;
    let mut statement = connection
        .prepare(
            "SELECT anchor_start, anchor_end, structural_path, chunk_kind FROM source_candidates
             WHERE manifest_id = ?1 AND content_sha256 = ?2 AND status = 'current'
             ORDER BY anchor_start, anchor_end, candidate_id",
        )
        .map_err(|error| sqlite(index_path, "read file sections", error))?;
    let rows = statement
        .query_map(
            rusqlite::params![
                row.manifest_id,
                row.content_sha256.as_deref().unwrap_or_default()
            ],
            |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .map_err(|error| sqlite(index_path, "read file sections", error))?;
    let mut listed = FileSections {
        sections: Vec::new(),
        untitled_anchors: 0,
        truncated: false,
    };
    for found in rows {
        let (start, end, path, kind) =
            found.map_err(|error| sqlite(index_path, "read file sections", error))?;
        let Some(heading) = path.filter(|path| *path != format!("page {start}")) else {
            listed.untitled_anchors += 1;
            continue;
        };
        let full = listed.sections.len() == MAX_LISTED_SECTIONS;
        match listed.sections.last_mut() {
            Some(last)
                if last.heading == heading
                    && kind != "heading"
                    && (kind != "page" || start <= last.end.saturating_add(1)) =>
            {
                last.end = last.end.max(end);
            }
            _ if full => listed.truncated = true,
            _ => listed.sections.push(SectionFact {
                heading,
                start,
                end,
            }),
        }
    }
    Ok(listed)
}

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

#[derive(Clone, Debug, Default)]
pub(crate) struct FolderFiles {
    pub(crate) indexed: usize,
    pub(crate) readable: usize,
    pub(crate) waiting: usize,
    pub(crate) first_scan_done: bool,
    pub(crate) rechecking: bool,
    pub(crate) ignored: usize,
    pub(crate) unreadable: Vec<UnreadableGroup>,
    pub(crate) page_code: Vec<(String, usize)>,
}

#[derive(Clone, Debug)]
pub(crate) struct UnreadableGroup {
    pub(crate) kind: &'static str,
    pub(crate) count: usize,
    pub(crate) folders: Vec<(String, usize)>,
    pub(crate) examples: Vec<String>,
}

const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "heic", "heif", "tif", "tiff", "bmp", "svg",
];
const DOCUMENT_EXTENSIONS: &[&str] = &[
    "doc", "docx", "ppt", "pptx", "key", "pages", "xls", "xlsx", "numbers", "odt", "odp", "ods",
    "rtf", "epub", "djvu",
];
const PAGE_CODE_EXTENSIONS: &[&str] = &["css", "scss", "less", "js", "mjs", "cjs", "map"];
const PAGE_CODE_FOLDERS: &[&str] = &["_assets", "site_libs", "libs", "assets", "static", "vendor"];
const PRIVATE_GROUPS: &[&str] = &["name_word", "password_like"];
const LISTED_FOLDERS: usize = 3;
const LISTED_EXAMPLES: usize = 3;
const LISTED_PAGE_CODE: usize = 5;
const PAGE_CODE_FLOOR: usize = 5;

fn extension(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn parent(path: &str) -> String {
    Path::new(path)
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .filter(|parent| !parent.is_empty())
        .unwrap_or_else(|| ".".to_owned())
}

fn page_code_folder(path: &str) -> Option<String> {
    let parts = path.split('/').collect::<Vec<_>>();
    parts
        .iter()
        .take(parts.len().saturating_sub(1))
        .position(|part| PAGE_CODE_FOLDERS.contains(part) || part.ends_with("_files"))
        .map(|at| parts[..=at].join("/"))
}

pub(crate) fn folder_files(index_path: &Path) -> Result<FolderFiles, ManifestError> {
    let connection = open_index(index_path)?;
    let mut files = FolderFiles {
        first_scan_done: connection
            .query_row(
                "SELECT generation > 0 FROM source_index_metadata WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|error| sqlite(index_path, "read folder coverage", error))?,
        rechecking: connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM source_reconciliation_state)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| sqlite(index_path, "read folder coverage", error))?,
        ..FolderFiles::default()
    };
    let mut statement = connection
        .prepare(
            "SELECT display_path, source_kind, index_status, coalesce(exclusion_reason, ''),
                    source_kind = 'pdf' AND index_status = 'indexed' AND NOT EXISTS(
                        SELECT 1 FROM source_candidates candidate
                        WHERE candidate.manifest_id = manifest.manifest_id
                          AND candidate.status = 'current'
                          AND length(trim(coalesce(candidate.body, ''))) > 0),
                    source_kind = 'pdf' AND index_status = 'indexed' AND NOT EXISTS(
                        SELECT 1 FROM source_candidates candidate
                        WHERE candidate.manifest_id = manifest.manifest_id
                          AND candidate.status = 'current'
                          AND candidate.structural_path NOT LIKE 'page %')
             FROM source_manifest manifest
             WHERE present = 1 AND file_size IS NOT NULL
             ORDER BY display_path COLLATE BINARY",
        )
        .map_err(|error| sqlite(index_path, "read folder coverage", error))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, bool>(4)?,
                row.get::<_, bool>(5)?,
            ))
        })
        .map_err(|error| sqlite(index_path, "read folder coverage", error))?;
    let mut groups: HashMap<&'static str, Vec<String>> = HashMap::new();
    let mut page_code: HashMap<String, usize> = HashMap::new();
    for row in rows {
        let (path, kind, status, reason, scan, untitled) =
            row.map_err(|error| sqlite(index_path, "read folder coverage", error))?;
        let group = match (kind.as_str(), status.as_str(), reason.as_str()) {
            ("text" | "pdf", status, _) => {
                files.readable += 1;
                match status {
                    "indexed" => {
                        files.indexed += 1;
                        if PAGE_CODE_EXTENSIONS.contains(&extension(&path).as_str()) {
                            *page_code
                                .entry(page_code_folder(&path).unwrap_or_else(|| parent(&path)))
                                .or_insert(0) += 1;
                        }
                        if scan {
                            Some("scan")
                        } else if untitled {
                            Some("no_titles")
                        } else {
                            None
                        }
                    }
                    "failed" => Some("failed"),
                    _ => {
                        files.waiting += 1;
                        None
                    }
                }
            }
            (_, _, "unsupported") => {
                let extension = extension(&path);
                Some(if IMAGE_EXTENSIONS.contains(&extension.as_str()) {
                    "image"
                } else if DOCUMENT_EXTENSIONS.contains(&extension.as_str()) {
                    "document"
                } else {
                    "other"
                })
            }
            (_, _, "sensitive") => Some("name_word"),
            (_, _, "sensitive_content" | "sensitive_extracted") => Some("password_like"),
            (_, _, "too_large") => Some("too_large"),
            (_, _, "symlink") => Some("link"),
            (_, _, "unreadable") => Some("unreadable"),
            (_, _, "magnesiumignore") => {
                files.ignored += 1;
                None
            }
            _ => None,
        };
        if let Some(group) = group {
            groups.entry(group).or_default().push(path);
        }
    }
    let mut unreadable = groups
        .into_iter()
        .map(|(kind, paths)| {
            if PRIVATE_GROUPS.contains(&kind) {
                return UnreadableGroup {
                    kind,
                    count: paths.len(),
                    folders: Vec::new(),
                    examples: Vec::new(),
                };
            }
            let mut folders: HashMap<String, usize> = HashMap::new();
            for path in &paths {
                *folders.entry(parent(path)).or_insert(0) += 1;
            }
            let mut folders = folders.into_iter().collect::<Vec<_>>();
            folders.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            folders.truncate(LISTED_FOLDERS);
            let mut examples = paths
                .iter()
                .filter(|path| page_code_folder(path).is_none())
                .cloned()
                .collect::<Vec<_>>();
            if examples.is_empty() {
                examples = paths.clone();
            }
            examples.sort_by_key(|path| (path.matches('/').count(), path.clone()));
            examples.truncate(LISTED_EXAMPLES);
            UnreadableGroup {
                kind,
                count: paths.len(),
                folders,
                examples,
            }
        })
        .collect::<Vec<_>>();
    unreadable.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.kind.cmp(b.kind)));
    files.unreadable = unreadable;
    let mut page_code = page_code
        .into_iter()
        .filter(|(_, count)| *count >= PAGE_CODE_FLOOR)
        .collect::<Vec<_>>();
    page_code.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    page_code.truncate(LISTED_PAGE_CODE);
    files.page_code = page_code;
    Ok(files)
}

#[derive(Clone, Debug)]
pub(crate) struct IndexedFile {
    pub(crate) path: String,
    pub(crate) source_kind: String,
    pub(crate) index_status: String,
    pub(crate) exclusion_reason: Option<String>,
    pub(crate) content_sha256: Option<String>,
    pub(crate) titled: usize,
    pub(crate) has_text: bool,
}

pub(crate) fn indexed_files(index_path: &Path) -> Result<Vec<IndexedFile>, ManifestError> {
    let connection = open_index(index_path)?;
    let mut statement = connection
        .prepare(
            "SELECT manifest.display_path, manifest.source_kind, manifest.index_status,
                    manifest.exclusion_reason, manifest.content_sha256,
                    (SELECT count(DISTINCT candidate.structural_path) FROM source_candidates candidate
                     WHERE candidate.manifest_id = manifest.manifest_id
                       AND candidate.status = 'current'
                       AND candidate.structural_path IS NOT NULL
                       AND candidate.structural_path NOT LIKE 'page %'),
                    EXISTS(SELECT 1 FROM source_candidates candidate
                     WHERE candidate.manifest_id = manifest.manifest_id
                       AND candidate.status = 'current'
                       AND length(trim(coalesce(candidate.body, ''))) > 0)
             FROM source_manifest manifest
             WHERE manifest.present = 1 AND manifest.file_size IS NOT NULL
             ORDER BY manifest.display_path COLLATE BINARY",
        )
        .map_err(|error| sqlite(index_path, "read indexed files", error))?;
    let rows = statement
        .query_map([], |row| {
            Ok(IndexedFile {
                path: row.get(0)?,
                source_kind: row.get(1)?,
                index_status: row.get(2)?,
                exclusion_reason: row.get(3)?,
                content_sha256: row.get(4)?,
                titled: row.get::<_, i64>(5)?.max(0) as usize,
                has_text: row.get(6)?,
            })
        })
        .map_err(|error| sqlite(index_path, "read indexed files", error))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| sqlite(index_path, "read indexed files", error))
}

pub(crate) fn sections_at(
    index_path: &Path,
    relative: &Path,
) -> Result<Option<(ManifestRow, FileSections)>, ManifestError> {
    let connection = open_index(index_path)?;
    let Some(row) = read_manifest_row_from(&connection, index_path, relative)? else {
        return Ok(None);
    };
    if row.index_status != "indexed" || !row.present {
        let empty = FileSections {
            sections: Vec::new(),
            untitled_anchors: 0,
            truncated: false,
        };
        return Ok(Some((row, empty)));
    }
    drop(connection);
    let sections = read_sections(index_path, &row)?;
    Ok(Some((row, sections)))
}
