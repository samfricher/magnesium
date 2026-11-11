use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use rusqlite::params;

use super::chunking::ChunkKind;
use super::coordinator::{
    CoordinatorBudget, IndexCoverage, OperationCancellation, ReconciliationOutcome,
    prepare_source_query_with_allowlist_and_cancellation,
};
use super::manifest::{AnchorKind, ManifestError, SourceCandidate, json_column, sqlite};

const MAX_QUERY_CHARS: usize = 1_024;
const MAX_QUERY_TERMS: usize = 32;
/// The one bound on source search results, shared with the search_sources tool.
pub(crate) const MAX_RESULTS: usize = 64;

// Kept as one statement so selection can stream a single ranked cursor.
// FTS fields are path, structural_path, body, symbols. Preserve the effective
// weights of the former six-column projections (their extra trailing weight
// was ignored by FTS5): 0, 8, 4, 1.
const SOURCE_QUERY_SQL: &str = r#"WITH ranked_matches AS MATERIALIZED (
                 SELECT rowid AS candidate_no,
                        bm25(source_search_ranked, 0.0, 8.0, 4.0, 1.0) AS score
                 FROM source_search_ranked
                 WHERE source_search_ranked MATCH ?1
             )
             SELECT candidate.candidate_id, manifest.display_path,
                    candidate.content_sha256, candidate.anchor_kind,
                    candidate.anchor_start, candidate.anchor_end, candidate.body,
                    candidate.chunk_kind, candidate.structural_path, candidate.language,
                    candidate.definitions_json, candidate.imports_json,
                    candidate.references_json,
                    manifest.display_path = ?2 AS exact_path_match,
                    coalesce((candidate.structural_path = ?2 COLLATE NOCASE
                     OR EXISTS(
                         SELECT 1 FROM json_each(candidate.definitions_json)
                         WHERE json_extract(value, '$.name') = ?2 COLLATE NOCASE
                     )
                     OR EXISTS(
                         SELECT 1 FROM json_each(candidate.imports_json) AS source_import
                         WHERE json_extract(source_import.value, '$.module') = ?2 COLLATE NOCASE
                            OR EXISTS(
                                SELECT 1 FROM json_each(source_import.value, '$.names')
                                WHERE value = ?2 COLLATE NOCASE
                            )
                     )
                     OR EXISTS(
                         SELECT 1 FROM json_each(candidate.references_json)
                         WHERE json_extract(value, '$.name') = ?2 COLLATE NOCASE
                     )), 0) AS exact_identifier_match,
                    CASE WHEN source_search_ranked.rowid IS NULL THEN source_search.rowid IN (
                        SELECT rowid FROM source_search WHERE source_search MATCH ?9
                    ) ELSE source_search_ranked.rowid IN (
                        SELECT rowid FROM source_search_ranked
                        WHERE source_search_ranked MATCH ?9
                    ) END AS phrase_match,
                    CASE WHEN source_search_ranked.rowid IS NULL THEN source_search.rowid IN (
                        SELECT rowid FROM source_search WHERE source_search MATCH ?10
                    ) ELSE source_search_ranked.rowid IN (
                        SELECT rowid FROM source_search_ranked
                        WHERE source_search_ranked MATCH ?10
                    ) END AS full_token_match
             FROM source_search
             JOIN source_candidates candidate
               ON candidate.candidate_no = source_search.rowid
             JOIN source_manifest manifest
               ON manifest.manifest_id = candidate.manifest_id
             LEFT JOIN source_search_ranked
               ON source_search_ranked.rowid = candidate.candidate_no
             LEFT JOIN ranked_matches
               ON ranked_matches.candidate_no = candidate.candidate_no
             WHERE source_search MATCH ?1
               AND candidate.status = 'current'
               AND manifest.present = 1 AND manifest.index_status = 'indexed'
               AND (?3 = '' OR manifest.display_path = ?3
                    OR manifest.display_path LIKE ?4 ESCAPE '\')
               AND (?5 = '' OR manifest.display_path = ?5
                    OR manifest.display_path LIKE ?6 ESCAPE '\')
               AND (?7 = 0 OR manifest.display_path IN (SELECT value FROM json_each(?8)))
             ORDER BY exact_path_match DESC, exact_identifier_match DESC,
                      phrase_match DESC, full_token_match DESC,
                      coalesce(
                          ranked_matches.score,
                          bm25(source_search, 0.0, 8.0, 4.0, 1.0)
                      ),
                      manifest.display_path COLLATE BINARY,
                      candidate.anchor_start, candidate.candidate_id"#;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceSearchRequest {
    pub(crate) query: String,
    pub(crate) path_prefix: Option<PathBuf>,
    pub(crate) limit: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LexicalMatchSignals {
    pub(crate) exact_path: bool,
    pub(crate) exact_identifier: bool,
    pub(crate) lexical_rank: u32,
    pub(crate) phrase_match: bool,
    pub(crate) full_token_match: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceSearchHit {
    pub(crate) candidate: SourceCandidate,
    pub(crate) signals: LexicalMatchSignals,
}

#[derive(Clone, Debug)]
pub(crate) struct SourceSearchReport {
    pub(crate) generation: i64,
    pub(crate) coverage: IndexCoverage,
    /// Set when a concurrent reconciliation owner could not be outrun within
    /// the shared run budget. Hits then come from the already-indexed,
    /// currently authorized snapshot only.
    pub(crate) reconciliation: ReconciliationOutcome,
    pub(crate) hits: Vec<SourceSearchHit>,
}

#[cfg(test)]
pub(crate) fn search_workspace_sources(
    workspace_root: &Path,
    index_path: &Path,
    query_scope: &Path,
    request: &SourceSearchRequest,
    budget: CoordinatorBudget,
) -> Result<SourceSearchReport, ManifestError> {
    search_workspace_sources_with_allowlist(
        workspace_root,
        index_path,
        query_scope,
        request,
        budget,
        None,
    )
}

#[cfg(test)]
pub(crate) fn search_workspace_sources_with_allowlist(
    workspace_root: &Path,
    index_path: &Path,
    query_scope: &Path,
    request: &SourceSearchRequest,
    budget: CoordinatorBudget,
    allowed_source_paths: Option<&BTreeSet<PathBuf>>,
) -> Result<SourceSearchReport, ManifestError> {
    search_workspace_sources_with_allowlist_and_cancellation(
        workspace_root,
        index_path,
        query_scope,
        request,
        budget,
        allowed_source_paths,
        &super::coordinator::OperationCancellation::none(),
    )
}

pub(crate) fn search_workspace_sources_with_allowlist_and_cancellation(
    workspace_root: &Path,
    index_path: &Path,
    query_scope: &Path,
    request: &SourceSearchRequest,
    budget: CoordinatorBudget,
    allowed_source_paths: Option<&BTreeSet<PathBuf>>,
    cancellation: &super::coordinator::OperationCancellation,
) -> Result<SourceSearchReport, ManifestError> {
    validate_request(workspace_root, index_path, query_scope, request)?;
    cancellation.check()?;
    let likely_paths = if allowed_source_paths.is_some() {
        Vec::new()
    } else {
        likely_path(workspace_root, query_scope, &request.query)
    };
    let prepared = prepare_source_query_with_allowlist_and_cancellation(
        workspace_root,
        index_path,
        query_scope,
        &likely_paths,
        budget,
        allowed_source_paths,
        cancellation,
    )?;
    Ok(SourceSearchReport {
        generation: prepared.generation,
        coverage: prepared.coverage,
        reconciliation: prepared.reconciliation,
        hits: query_index_with_cancellation(
            workspace_root,
            index_path,
            query_scope,
            request,
            allowed_source_paths,
            cancellation,
        )?,
    })
}

fn validate_request(
    workspace_root: &Path,
    index_path: &Path,
    query_scope: &Path,
    request: &SourceSearchRequest,
) -> Result<(), ManifestError> {
    let root = std::fs::canonicalize(workspace_root).map_err(|source| ManifestError::Io {
        action: "resolve search workspace",
        path: workspace_root.to_path_buf(),
        source,
    })?;
    let expected_index = crate::workspace::source_index_path(&root);
    let index_parent = index_path.parent().ok_or_else(|| {
        ManifestError::Invalid(format!(
            "source search index {} has no parent directory",
            index_path.display()
        ))
    })?;
    let index_parent = std::fs::canonicalize(index_parent).map_err(|source| ManifestError::Io {
        action: "resolve source-search index directory",
        path: index_parent.to_path_buf(),
        source,
    })?;
    if index_parent != root.join(crate::workspace::CONTROL_DIR).join("index")
        || index_path.file_name() != expected_index.file_name()
    {
        return Err(ManifestError::Invalid(format!(
            "source search index {} is not the active workspace index {}",
            index_path.display(),
            expected_index.display()
        )));
    }
    validate_relative(query_scope, "query scope")?;
    if let Some(prefix) = &request.path_prefix {
        validate_relative(prefix, "path prefix")?;
        if !query_scope.as_os_str().is_empty() && !prefix.starts_with(query_scope) {
            return Err(ManifestError::Invalid(format!(
                "source search path prefix {} is outside query scope {}",
                prefix.display(),
                query_scope.display()
            )));
        }
    }
    if request.query.trim().is_empty()
        || request.query.chars().count() > MAX_QUERY_CHARS
        || request.query.chars().any(char::is_control)
    {
        return Err(ManifestError::Invalid(
            "source search query must be bounded printable text".to_owned(),
        ));
    }
    if request.limit == 0 || request.limit > MAX_RESULTS {
        return Err(ManifestError::Invalid(format!(
            "source search limit must be between one and {MAX_RESULTS}"
        )));
    }
    compile_query_plan(&request.query)?;
    Ok(())
}

fn validate_relative(path: &Path, label: &str) -> Result<(), ManifestError> {
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ManifestError::Invalid(format!(
            "source search {label} {} must be workspace-relative",
            path.display()
        )));
    }
    Ok(())
}

fn likely_path(workspace_root: &Path, query_scope: &Path, query: &str) -> Vec<PathBuf> {
    let raw = query.trim().trim_matches('"');
    if raw.is_empty() || raw.contains(char::is_whitespace) || raw.ends_with('*') {
        return Vec::new();
    }
    let relative = PathBuf::from(raw);
    if validate_relative(&relative, "query path").is_ok()
        && (query_scope.as_os_str().is_empty() || relative.starts_with(query_scope))
        && workspace_root.join(&relative).is_file()
    {
        vec![relative]
    } else {
        Vec::new()
    }
}

fn query_index_with_cancellation(
    workspace_root: &Path,
    index_path: &Path,
    query_scope: &Path,
    request: &SourceSearchRequest,
    allowed_source_paths: Option<&BTreeSet<PathBuf>>,
    cancellation: &OperationCancellation,
) -> Result<Vec<SourceSearchHit>, ManifestError> {
    query_index_with_budget(
        workspace_root,
        index_path,
        query_scope,
        request,
        allowed_source_paths,
        cancellation,
        std::time::Duration::from_millis(150),
    )
}

fn query_index_with_budget(
    workspace_root: &Path,
    index_path: &Path,
    query_scope: &Path,
    request: &SourceSearchRequest,
    allowed_source_paths: Option<&BTreeSet<PathBuf>>,
    cancellation: &OperationCancellation,
    budget: std::time::Duration,
) -> Result<Vec<SourceSearchHit>, ManifestError> {
    let plan = compile_query_plan(&request.query)?;
    let raw_query = request.query.trim().trim_matches('"');
    let scope = query_scope.to_string_lossy();
    let scope_like = descendant_pattern(&scope);
    let path_prefix = request
        .path_prefix
        .as_deref()
        .map(Path::to_string_lossy)
        .unwrap_or_default();
    let path_prefix_like = descendant_pattern(&path_prefix);
    let allowed_paths = serde_json::to_string(
        &allowed_source_paths
            .into_iter()
            .flatten()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
    )
    .map_err(|error| ManifestError::Invalid(error.to_string()))?;
    cancellation.check()?;
    let deadline = std::time::Instant::now() + budget;
    if std::time::Instant::now() >= deadline {
        return Err(ManifestError::BudgetExhausted);
    }
    let connection =
        super::manifest::schema::open_index_with_budget(index_path, budget, cancellation).map_err(
            |error| {
                if cancellation.is_cancelled() {
                    ManifestError::Cancelled
                } else if std::time::Instant::now() >= deadline {
                    ManifestError::BudgetExhausted
                } else {
                    error
                }
            },
        )?;
    // One snapshot and one ranked cursor cover the entire selection, including
    // rows skipped because a source boundary changed since reconciliation.
    let _snapshot = connection
        .unchecked_transaction()
        .map_err(|source| sqlite(index_path, "begin lexical source search snapshot", source))?;
    // The SQL handler records the cause, not just SQLITE_INTERRUPT. One
    // deadline covers every ranking tier and the complete row collection.
    cancellation.check()?;
    let reason = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
    {
        let flag = cancellation.clone();
        let interrupted = reason.clone();
        let _ = connection.progress_handler(
            1000,
            Some(move || {
                let cause = if flag.is_cancelled() {
                    1
                } else if std::time::Instant::now() >= deadline {
                    2
                } else {
                    0
                };
                if cause != 0 {
                    interrupted.store(cause, std::sync::atomic::Ordering::Relaxed);
                }
                cause != 0
            }),
        );
    }
    let mut statement = connection.prepare(SOURCE_QUERY_SQL).map_err(|source| {
        sqlite_or_interrupted(index_path, "prepare lexical source search", source, &reason)
    })?;
    // Rank once. Cached rows can outlive a source boundary; stream past them
    // under the same deadline and keep only the requested authorized hits.
    #[cfg(test)]
    crate::request::test_checkpoint("source_query_ranking");
    let rows = statement
        .query_map(
            params![
                plan.candidates,
                raw_query,
                scope.as_ref(),
                scope_like,
                path_prefix.as_ref(),
                path_prefix_like,
                allowed_source_paths.is_some(),
                allowed_paths,
                plan.phrase,
                plan.full_tokens,
            ],
            |row| {
                let anchor_kind = row.get::<_, String>(3)?;
                let chunk_kind = row.get::<_, String>(7)?;
                Ok((
                    SourceCandidate {
                        candidate_id: row.get(0)?,
                        path: row.get(1)?,
                        content_sha256: row.get(2)?,
                        anchor_kind: if anchor_kind == "page" {
                            AnchorKind::Page
                        } else {
                            AnchorKind::Line
                        },
                        anchor_start: row.get(4)?,
                        anchor_end: row.get(5)?,
                        body: row.get(6)?,
                        chunk_kind: ChunkKind::parse(&chunk_kind).ok_or_else(|| {
                            rusqlite::Error::InvalidColumnType(
                                7,
                                "chunk_kind".to_owned(),
                                rusqlite::types::Type::Text,
                            )
                        })?,
                        structural_path: row.get(8)?,
                        language: row.get(9)?,
                        definitions: json_column(row, 10)?,
                        imports: json_column(row, 11)?,
                        references: json_column(row, 12)?,
                    },
                    row.get::<_, bool>(13)?,
                    row.get::<_, bool>(14)?,
                    row.get::<_, bool>(15)?,
                    row.get::<_, bool>(16)?,
                ))
            },
        )
        .map_err(|source| {
            sqlite_or_interrupted(index_path, "query lexical source search", source, &reason)
        })?;
    let mut hits = Vec::with_capacity(request.limit);
    for row in rows {
        let (candidate, exact_path, exact_identifier, phrase_match, full_token_match) = row
            .map_err(|source| {
                sqlite_or_interrupted(index_path, "read lexical source search", source, &reason)
            })?;
        cancellation.check()?;
        if std::time::Instant::now() >= deadline {
            return Err(ManifestError::BudgetExhausted);
        }
        if super::source_relative_path_is_excluded(workspace_root, Path::new(&candidate.path))? {
            continue;
        }
        // Filesystem policy checks consume the same budget as SQL execution.
        cancellation.check()?;
        if std::time::Instant::now() >= deadline {
            return Err(ManifestError::BudgetExhausted);
        }
        hits.push(SourceSearchHit {
            candidate,
            signals: LexicalMatchSignals {
                exact_path,
                exact_identifier,
                phrase_match,
                full_token_match,
                lexical_rank: u32::try_from(hits.len() + 1).map_err(|_| {
                    ManifestError::Invalid("source search rank overflow".to_owned())
                })?,
            },
        });
        if hits.len() == request.limit {
            break;
        }
    }
    cancellation.check()?;
    if std::time::Instant::now() >= deadline {
        return Err(ManifestError::BudgetExhausted);
    }
    Ok(hits)
}

fn sqlite_or_interrupted(
    index_path: &Path,
    action: &'static str,
    source: rusqlite::Error,
    reason: &std::sync::atomic::AtomicU8,
) -> ManifestError {
    if source.sqlite_error_code() == Some(rusqlite::ErrorCode::OperationInterrupted) {
        match reason.load(std::sync::atomic::Ordering::Relaxed) {
            1 => return ManifestError::Cancelled,
            2 => return ManifestError::BudgetExhausted,
            _ => {} // An unrelated SQLite interruption has no inferred cause.
        }
    }
    sqlite(index_path, action, source)
}

struct QueryPlan {
    candidates: String,
    phrase: String,
    full_tokens: String,
}

// One SQL statement ranks all tiers before streaming allowed hits. Its progress handler and
// request deadline cover every FTS branch together, with no per-tier reset.
fn compile_query_plan(query: &str) -> Result<QueryPlan, ManifestError> {
    #[cfg(test)]
    crate::request::test_checkpoint("source_query_compilation");
    let mut terms = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in query.trim().chars() {
        match character {
            '"' => {
                if quoted {
                    push_term(&mut terms, &mut current, false)?;
                } else if !current.trim().is_empty() {
                    push_term(&mut terms, &mut current, true)?;
                }
                quoted = !quoted;
            }
            character if character.is_whitespace() && !quoted => {
                push_term(&mut terms, &mut current, true)?;
            }
            character => current.push(character),
        }
    }
    if quoted {
        return Err(ManifestError::Invalid(
            "source search query contains an unterminated phrase".to_owned(),
        ));
    }
    push_term(&mut terms, &mut current, true)?;
    if terms.is_empty() || terms.len() > MAX_QUERY_TERMS {
        return Err(ManifestError::Invalid(format!(
            "source search query must contain one to {MAX_QUERY_TERMS} terms"
        )));
    }
    let full_tokens = terms.join(" AND ");
    let explicit_phrase = query.contains('"');
    let phrase = if explicit_phrase || terms.len() == 1 || query.contains('*') {
        full_tokens.clone()
    } else {
        format!(
            "\"{}\"",
            query.split_whitespace().collect::<Vec<_>>().join(" ")
        )
    };
    Ok(QueryPlan {
        // An explicit phrase is mandatory, never silently relaxed to its words.
        candidates: if explicit_phrase {
            full_tokens.clone()
        } else {
            terms.join(" OR ")
        },
        phrase,
        full_tokens,
    })
}

fn push_term(
    terms: &mut Vec<String>,
    current: &mut String,
    allow_prefix: bool,
) -> Result<(), ManifestError> {
    let value = current.trim();
    if value.is_empty() {
        current.clear();
        return Ok(());
    }
    let prefix = allow_prefix && value.ends_with('*');
    let value = value.strip_suffix('*').unwrap_or(value);
    if value.is_empty() || value.contains('*') {
        return Err(ManifestError::Invalid(
            "source search wildcard is allowed only after a non-empty term".to_owned(),
        ));
    }
    let escaped = value.replace('"', "\"\"");
    terms.push(format!("\"{escaped}\"{}", if prefix { "*" } else { "" }));
    current.clear();
    Ok(())
}

fn descendant_pattern(prefix: &str) -> String {
    if prefix.is_empty() {
        return String::new();
    }
    let escaped = prefix
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let separator = std::path::MAIN_SEPARATOR_STR.replace('\\', "\\\\");
    format!("{escaped}{separator}%")
}

#[cfg(test)]
mod tests;
