//! The only extraction publication owner: prepare without side effects, then
//! compare one source revision before atomically publishing its entire outcome.
use std::path::Path;
use std::time::Instant;

use rusqlite::{Connection, OptionalExtension, params};

use super::convert::ConverterProvenance;
use super::extract::candidate_id;
use super::schema::{INDEX_INITIALIZATION_BUDGET, begin_write};
use super::status::stale_candidates;
use super::{
    AnchorKind, CurrentSource, ManifestError, PublishedSource, SourceCandidate, anchor_name, sqlite,
};
use crate::retrieval::chunking::{ChunkDraft, ChunkKind};
use crate::retrieval::{OperationCancellation, SensitivityClass};

pub(super) enum PreparedOutcome {
    Ready(PreparedExtraction),
    Failed { diagnostic: String },
    ChangedDuringRead,
    Sensitive(SensitivityClass),
}

pub(super) struct PreparedExtraction {
    candidates: Vec<PreparedCandidate>,
    converter: PreparedProvenance,
}

struct PreparedCandidate {
    candidate: SourceCandidate,
    definitions_json: String,
    imports_json: String,
    references_json: String,
    symbols: String,
    ranking_body: String,
}

#[derive(Default)]
struct PreparedProvenance {
    method: Option<String>,
    method_version: Option<String>,
    coverage: Option<String>,
    missing_json: Option<String>,
    warnings_json: Option<String>,
}

impl PreparedExtraction {
    pub(super) fn into_candidates(self) -> Vec<SourceCandidate> {
        self.candidates
            .into_iter()
            .map(|item| item.candidate)
            .collect()
    }
}

/// Navigation detection, canonical IDs, ranking bodies and every JSON field
/// are prepared before acquiring the source-cache writer lock.
pub(super) fn prepare_extraction(
    source: &CurrentSource,
    extracted: Vec<ChunkDraft>,
    converter: Option<&ConverterProvenance>,
) -> Result<PreparedExtraction, ManifestError> {
    let navigation = super::ranking::navigation_lines(
        &extracted
            .iter()
            .map(|chunk| chunk.body.as_str())
            .collect::<Vec<_>>(),
    );
    let candidates = extracted
        .into_iter()
        .map(|chunk| {
            let anchor_kind = if chunk.kind == ChunkKind::Page {
                AnchorKind::Page
            } else {
                AnchorKind::Line
            };
            let candidate_id = candidate_id(
                source.manifest_id,
                anchor_name(anchor_kind),
                chunk.line_start,
                chunk.line_end,
                chunk.kind.as_str(),
                chunk.structural_path.as_deref(),
                &chunk.body,
            );
            let definitions_json = serde_json::to_string(&chunk.definitions)
                .map_err(|error| ManifestError::Invalid(error.to_string()))?;
            let imports_json = serde_json::to_string(&chunk.imports)
                .map_err(|error| ManifestError::Invalid(error.to_string()))?;
            let references_json = serde_json::to_string(&chunk.references)
                .map_err(|error| ManifestError::Invalid(error.to_string()))?;
            let symbols = format!("{definitions_json} {imports_json} {references_json}");
            let ranking_body = super::ranking::ranking_body(&chunk.body, &navigation);
            Ok(PreparedCandidate {
                candidate: SourceCandidate {
                    candidate_id,
                    path: source.display_path.clone(),
                    content_sha256: source.content_sha256.clone(),
                    anchor_kind,
                    anchor_start: chunk.line_start,
                    anchor_end: chunk.line_end,
                    body: chunk.body,
                    chunk_kind: chunk.kind,
                    structural_path: chunk.structural_path,
                    language: chunk.language.map(str::to_owned),
                    definitions: chunk.definitions,
                    imports: chunk.imports,
                    references: chunk.references,
                },
                definitions_json,
                imports_json,
                references_json,
                symbols,
                ranking_body,
            })
        })
        .collect::<Result<Vec<_>, ManifestError>>()?;
    let converter = match converter {
        Some(provenance) => PreparedProvenance {
            method: Some(provenance.method.clone()),
            method_version: Some(provenance.method_version.clone()),
            coverage: Some(provenance.coverage.clone()),
            missing_json: Some(
                serde_json::to_string(&provenance.missing)
                    .map_err(|error| ManifestError::Invalid(error.to_string()))?,
            ),
            warnings_json: Some(
                serde_json::to_string(&provenance.warnings)
                    .map_err(|error| ManifestError::Invalid(error.to_string()))?,
            ),
        },
        None => PreparedProvenance::default(),
    };
    Ok(PreparedExtraction {
        candidates,
        converter,
    })
}

pub(super) fn publish_extraction(
    connection: &mut Connection,
    index_path: &Path,
    expected: &CurrentSource,
    outcome: &PreparedOutcome,
    cancellation: &OperationCancellation,
) -> Result<PublishedSource, ManifestError> {
    #[cfg(test)]
    crate::request::test_checkpoint("source_publication_before_lock");
    cancellation.check()?;
    let transaction = begin_write(
        connection,
        index_path,
        "begin guarded source publication",
        Instant::now() + INDEX_INITIALIZATION_BUDGET,
        cancellation,
    )?;
    #[cfg(test)]
    crate::request::test_checkpoint("source_publication_after_lock");
    cancellation.check()?;
    let current = transaction
        .query_row(
            "SELECT source_revision, present FROM source_manifest WHERE manifest_id = ?1",
            [expected.manifest_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?)),
        )
        .optional()
        .map_err(|error| sqlite(index_path, "read source publication revision", error))?;
    #[cfg(test)]
    crate::request::test_checkpoint("source_publication_after_revision_read");
    cancellation.check()?;
    if current != Some((expected.source_revision, true)) {
        return Err(ManifestError::SourceSuperseded);
    }
    match outcome {
        PreparedOutcome::Ready(prepared) => {
            stale_candidates(&transaction, expected.manifest_id, index_path)?;
            for item in &prepared.candidates {
                cancellation.check()?;
                let candidate = &item.candidate;
                let candidate_no: i64 = transaction.query_row(
                    "INSERT INTO source_candidates(
                        candidate_id, manifest_id, content_sha256, anchor_kind,
                        anchor_start, anchor_end, body, status, chunk_kind, structural_path,
                        language, definitions_json, imports_json, references_json
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'current', ?8, ?9, ?10, ?11, ?12, ?13)
                     ON CONFLICT(candidate_id) DO UPDATE SET
                        content_sha256 = excluded.content_sha256, body = excluded.body,
                        status = 'current', chunk_kind = excluded.chunk_kind,
                        structural_path = excluded.structural_path, language = excluded.language,
                        definitions_json = excluded.definitions_json, imports_json = excluded.imports_json,
                        references_json = excluded.references_json
                     RETURNING candidate_no",
                    params![candidate.candidate_id, expected.manifest_id, expected.content_sha256,
                        candidate.anchor_kind.as_str(), candidate.anchor_start, candidate.anchor_end,
                        candidate.body, candidate.chunk_kind.as_str(), candidate.structural_path,
                        candidate.language, item.definitions_json, item.imports_json, item.references_json],
                    |row| row.get(0),
                ).map_err(|error| sqlite(index_path, "store source candidate", error))?;
                super::ranking::insert_ranked_candidate(
                    &transaction,
                    index_path,
                    candidate_no,
                    &expected.display_path,
                    candidate.structural_path.as_deref(),
                    &item.ranking_body,
                    &item.symbols,
                )?;
                #[cfg(test)]
                crate::request::test_checkpoint("source_publication_between_fts_writes");
                cancellation.check()?;
                transaction.execute(
                    "INSERT INTO source_search(rowid, path, structural_path, body, symbols) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![candidate_no, expected.display_path, candidate.structural_path, candidate.body, item.symbols],
                ).map_err(|error| sqlite(index_path, "index source candidate for search", error))?;
                #[cfg(test)]
                crate::request::test_checkpoint("source_publication_after_candidate");
                cancellation.check()?;
            }
            #[cfg(test)]
            crate::request::test_checkpoint("source_publication_before_manifest");
            cancellation.check()?;
            let provenance = &prepared.converter;
            transaction.execute(
                "UPDATE source_manifest SET index_status = 'indexed', last_error = NULL,
                    extraction_method = ?2, extraction_method_version = ?3,
                    extraction_coverage = ?4, extraction_missing_json = ?5, extraction_warnings_json = ?6
                 WHERE manifest_id = ?1",
                params![expected.manifest_id, provenance.method, provenance.method_version,
                    provenance.coverage, provenance.missing_json, provenance.warnings_json],
            ).map_err(|error| sqlite(index_path, "complete source extraction", error))?;
        }
        PreparedOutcome::Failed { diagnostic } => {
            transaction.execute(
                "UPDATE source_manifest SET index_status = 'failed', last_error = ?2 WHERE manifest_id = ?1",
                params![expected.manifest_id, diagnostic],
            ).map_err(|error| sqlite(index_path, "record source extraction failure", error))?;
        }
        PreparedOutcome::ChangedDuringRead => {
            stale_candidates(&transaction, expected.manifest_id, index_path)?;
            transaction.execute(
                "UPDATE source_manifest SET index_status = 'stale', last_error = 'source changed after reconciliation' WHERE manifest_id = ?1",
                [expected.manifest_id],
            ).map_err(|error| sqlite(index_path, "mark source stale", error))?;
        }
        PreparedOutcome::Sensitive(sensitivity) => {
            stale_candidates(&transaction, expected.manifest_id, index_path)?;
            transaction.execute(
                "UPDATE source_manifest SET source_kind = 'excluded', content_sha256 = NULL,
                    sensitivity_class = ?2, exclusion_reason = 'sensitive_extracted',
                    index_status = 'excluded', last_error = NULL,
                    extraction_method = NULL, extraction_method_version = NULL,
                    extraction_coverage = NULL, extraction_missing_json = NULL, extraction_warnings_json = NULL
                 WHERE manifest_id = ?1",
                params![expected.manifest_id, sensitivity.as_str()],
            ).map_err(|error| sqlite(index_path, "record sensitive-source exclusion", error))?;
        }
    }
    let published = transaction.query_row(
        "SELECT manifest_id, source_revision, content_sha256 FROM source_manifest WHERE manifest_id = ?1",
        [expected.manifest_id],
        |row| Ok(PublishedSource { manifest_id: row.get(0)?, source_revision: row.get(1)?, content_sha256: row.get::<_, Option<String>>(2)?.unwrap_or_default() }),
    ).map_err(|error| sqlite(index_path, "read published source revision", error))?;
    #[cfg(test)]
    crate::request::test_checkpoint("source_publication_before_commit");
    cancellation.check()?;
    transaction
        .commit()
        .map_err(|error| sqlite(index_path, "commit guarded source publication", error))?;
    // A valid committed cache state is complete even when caller cancellation
    // arrives afterwards. Never report rollback after commit or undo the state.
    #[cfg(test)]
    crate::request::test_checkpoint("source_publication_after_commit");
    Ok(published)
}
