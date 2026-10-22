use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;
use std::time::Instant;

use rusqlite::{OptionalExtension, Transaction, params};

use super::extract::hash_file;
use super::schema::{
    INDEX_INITIALIZATION_BUDGET, begin_write, initialize_index, open_index, open_index_for_write,
};
use super::source::{
    exclusion_reason, manifest_path_is_missing, modified_ns, open_workspace_source,
    required_modified_ns, sensitivity, source_kind,
};
use super::status::{load_existing, stale_candidates};
use super::{
    ExistingRecord, ManifestError, ManifestReport, MovePolicy, ReconcileProgress,
    ReconciliationState, io_error, sqlite, to_i64,
};
use crate::retrieval::{
    DiscoveryDisposition, DiscoveryEntry, DiscoveryReport, ExclusionReason, OperationCancellation,
    SensitivityClass,
};
use crate::workspace::native_path_key;

pub(crate) fn load_or_start_reconciliation(
    index_path: &Path,
    initial_cursor_json: &str,
    cancellation: &OperationCancellation,
) -> Result<ReconciliationState, ManifestError> {
    let connection = initialize_index(index_path, INDEX_INITIALIZATION_BUDGET, cancellation)?;
    // Immediate: the read that decides whether a cursor exists must happen
    // under the write lock, or two writers can both decide to insert and the
    // loser observes a stale snapshot instead of the committed cursor.
    let transaction = begin_write(
        &connection,
        index_path,
        "start reconciliation cursor transaction",
        Instant::now() + INDEX_INITIALIZATION_BUDGET,
        cancellation,
    )?;
    let current = transaction
        .query_row(
            "SELECT scan_generation, cursor_json
             FROM source_reconciliation_state WHERE singleton = 1",
            [],
            |row| {
                Ok(ReconciliationState {
                    generation: row.get(0)?,
                    cursor_json: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(|source| sqlite(index_path, "read reconciliation cursor", source))?;
    let state = if let Some(state) = current {
        state
    } else {
        let generation: i64 = transaction
            .query_row(
                "SELECT generation + 1 FROM source_index_metadata WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|source| sqlite(index_path, "read next reconciliation generation", source))?;
        transaction
            .execute(
                "INSERT INTO source_reconciliation_state(
                     singleton, scan_generation, cursor_json
                 ) VALUES (1, ?1, ?2)",
                params![generation, initial_cursor_json],
            )
            .map_err(|source| {
                sqlite(index_path, "persist initial reconciliation cursor", source)
            })?;
        ReconciliationState {
            generation,
            cursor_json: initial_cursor_json.to_owned(),
        }
    };
    transaction
        .commit()
        .map_err(|source| sqlite(index_path, "commit reconciliation cursor", source))?;
    Ok(state)
}

#[cfg(test)]
pub(crate) fn reconcile_batch(
    workspace_root: &Path,
    index_path: &Path,
    entries: Vec<DiscoveryEntry>,
    state: &ReconciliationState,
    next_cursor_json: Option<&str>,
) -> Result<ManifestReport, ManifestError> {
    reconcile_batch_with_policy(
        workspace_root,
        index_path,
        entries,
        state,
        match next_cursor_json {
            Some(cursor) => ReconciliationAdvance::Cursor(cursor),
            None => ReconciliationAdvance::CompleteScan,
        },
        &OperationCancellation::none(),
    )
}

#[derive(Clone, Copy)]
pub(crate) enum ReconciliationAdvance<'a> {
    Cursor(&'a str),
    CompleteScan,
    PreserveSelected,
}

pub(crate) fn reconcile_batch_with_policy(
    workspace_root: &Path,
    index_path: &Path,
    entries: Vec<DiscoveryEntry>,
    state: &ReconciliationState,
    advance: ReconciliationAdvance<'_>,
    cancellation: &OperationCancellation,
) -> Result<ManifestReport, ManifestError> {
    cancellation.check()?;
    let workspace_root = fs::canonicalize(workspace_root)
        .map_err(|source| io_error("resolve reconciliation workspace", workspace_root, source))?;
    let discovery = DiscoveryReport {
        workspace_root,
        entries,
    };
    let mut after_open = |_: &Path| {
        #[cfg(test)]
        crate::request::test_checkpoint("source_reconciliation_after_open");
        cancellation.check()
    };
    let (hashed, hashed_bytes) =
        hash_before_lock(&discovery, index_path, &mut after_open, cancellation)?;
    let connection = open_index_for_write(index_path)?;
    // Immediate: the compare-and-swap guard must read the persisted cursor
    // under the same write lock it commits with. A deferred transaction can
    // read a stale snapshot and then fail with SQLITE_BUSY_SNAPSHOT instead
    // of a truthful reconciliation conflict.
    let transaction = begin_write(
        &connection,
        index_path,
        "start bounded reconciliation batch",
        Instant::now() + INDEX_INITIALIZATION_BUDGET,
        cancellation,
    )?;
    let persisted: Option<(i64, String)> = transaction
        .query_row(
            "SELECT scan_generation, cursor_json
             FROM source_reconciliation_state WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|source| sqlite(index_path, "verify reconciliation cursor", source))?;
    if persisted.as_ref() != Some(&(state.generation, state.cursor_json.clone())) {
        return Err(ManifestError::ReconciliationConflict);
    }

    let existing = load_existing(&transaction, index_path, cancellation)?;
    let current_keys = discovery
        .entries
        .iter()
        .map(|entry| native_path_key(&entry.relative_path))
        .collect::<HashSet<_>>();
    let mut progress = ReconcileProgress {
        cancellation,
        moved_ids: HashSet::new(),
        moves: match advance {
            ReconciliationAdvance::PreserveSelected => MovePolicy::Isolated,
            _ => MovePolicy::MissingOnly,
        },
        report: ManifestReport {
            content_reads: hashed.len(),
            content_bytes: hashed_bytes,
            ..empty_report(state.generation)
        },
        hashed,
        after_open: &mut after_open,
    };
    for entry in &discovery.entries {
        cancellation.check()?;
        if matches!(advance, ReconciliationAdvance::PreserveSelected)
            && let DiscoveryDisposition::Excluded(ExclusionReason::Unreadable(kind)) =
                entry.disposition
            && kind != io::ErrorKind::NotFound
        {
            return Err(io_error(
                "inspect selected source",
                &discovery.workspace_root.join(&entry.relative_path),
                io::Error::from(kind),
            ));
        }
        reconcile_entry(
            &transaction,
            index_path,
            &discovery,
            entry,
            &existing,
            &current_keys,
            &mut progress,
        )?;
    }

    match advance {
        ReconciliationAdvance::PreserveSelected => (),
        ReconciliationAdvance::Cursor(next_cursor_json) => {
            let updated = transaction
                .execute(
                    "UPDATE source_reconciliation_state SET cursor_json = ?1
                 WHERE singleton = 1 AND scan_generation = ?2 AND cursor_json = ?3",
                    params![next_cursor_json, state.generation, state.cursor_json],
                )
                .map_err(|source| sqlite(index_path, "advance reconciliation cursor", source))?;
            if updated != 1 {
                return Err(ManifestError::ReconciliationConflict);
            }
        }
        ReconciliationAdvance::CompleteScan => {
            let mut statement = transaction
                .prepare(
                    "SELECT manifest_id FROM source_manifest
                 WHERE present = 1 AND observed_generation <> ?1",
                )
                .map_err(|source| sqlite(index_path, "prepare unseen-source query", source))?;
            let unseen = statement
                .query_map([state.generation], |row| row.get::<_, i64>(0))
                .map_err(|source| sqlite(index_path, "query unseen sources", source))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|source| sqlite(index_path, "read unseen sources", source))?;
            drop(statement);
            for manifest_id in &unseen {
                cancellation.check()?;
                tombstone_source(&transaction, index_path, *manifest_id, state.generation)?;
            }
            progress.report.deleted += unseen.len();
            transaction
                .execute(
                    "UPDATE source_index_metadata SET generation = ?1 WHERE singleton = 1",
                    [state.generation],
                )
                .map_err(|source| {
                    sqlite(index_path, "complete reconciliation generation", source)
                })?;
            let cleared = transaction
                .execute(
                    "DELETE FROM source_reconciliation_state
                 WHERE singleton = 1 AND scan_generation = ?1 AND cursor_json = ?2",
                    params![state.generation, state.cursor_json],
                )
                .map_err(|source| {
                    sqlite(index_path, "clear completed reconciliation cursor", source)
                })?;
            if cleared != 1 {
                // A competing owner advanced the cursor while this pass scanned.
                // Rolling back prevents a stale completion from deleting unseen
                // sources or bumping the generation.
                return Err(ManifestError::ReconciliationConflict);
            }
        }
    }
    #[cfg(test)]
    crate::request::test_checkpoint("source_reconciliation_before_commit");
    cancellation.check()?;
    for entry in &discovery.entries {
        if is_missing_entry(entry) {
            confirm_selected_absence(&discovery.workspace_root, &entry.relative_path)?;
        }
    }
    transaction
        .commit()
        .map_err(|source| sqlite(index_path, "commit bounded reconciliation batch", source))?;
    Ok(progress.report)
}

/// A content hash read before the write lock, with the size and modified time
/// the file had then. The locked pass reuses it only while the file still has
/// both, the same evidence that lets it skip an unchanged file.
pub(crate) struct HashedContent {
    file_size: Option<i64>,
    modified_unix_ns: Option<i64>,
    sha256: String,
    sensitivity: Option<SensitivityClass>,
}

/// Hashing reads every byte of a new file and scans it for credentials, which
/// takes seconds for a large PDF, and every other writer waits for the lock
/// meanwhile, including a search with a 150 ms budget. So a batch hashes new
/// and resized sources before it takes the lock. A file whose size stays the
/// same but whose modified time changed is still hashed under the lock.
fn hash_before_lock(
    discovery: &DiscoveryReport,
    index_path: &Path,
    after_open: &mut dyn FnMut(&Path) -> Result<(), ManifestError>,
    cancellation: &OperationCancellation,
) -> Result<(HashMap<Vec<u8>, HashedContent>, u64), ManifestError> {
    let connection = open_index(index_path)?;
    let snapshot = connection
        .unchecked_transaction()
        .map_err(|source| sqlite(index_path, "begin pre-lock manifest snapshot", source))?;
    let existing = load_existing(&snapshot, index_path, cancellation)?;
    drop(snapshot);
    let mut hashed = HashMap::new();
    let mut bytes = 0_u64;
    for entry in &discovery.entries {
        cancellation.check()?;
        let Some(kind) = source_kind(entry) else {
            continue;
        };
        let path_key = native_path_key(&entry.relative_path);
        if existing
            .get(&path_key)
            .is_some_and(|record| record.describes(kind) && record.file_size == to_i64(entry.size))
        {
            continue;
        }
        let source =
            open_workspace_source(&discovery.workspace_root, &entry.relative_path, after_open)?;
        let file_size = Some(i64::try_from(source.metadata.len()).unwrap_or(i64::MAX));
        let modified_unix_ns = Some(required_modified_ns(&source.path, &source.metadata)?);
        bytes = bytes.saturating_add(source.metadata.len());
        let (sha256, sensitivity) = hash_file(source.file, &source.path, kind, cancellation)?;
        hashed.insert(
            path_key,
            HashedContent {
                file_size,
                modified_unix_ns,
                sha256,
                sensitivity,
            },
        );
    }
    Ok((hashed, bytes))
}

fn is_missing_entry(entry: &DiscoveryEntry) -> bool {
    matches!(
        entry.disposition,
        DiscoveryDisposition::Excluded(ExclusionReason::Unreadable(io::ErrorKind::NotFound))
    )
}

fn confirm_selected_absence(root: &Path, relative: &Path) -> Result<(), ManifestError> {
    let path = root.join(relative);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            crate::workspace::reject_symlink_components(&path, root)
                .map_err(|source| io_error("verify selected source absence", &path, source))?;
            Ok(())
        }
        Err(source) => Err(io_error("verify selected source absence", &path, source)),
        Ok(_) => Err(ManifestError::SourceSuperseded),
    }
}

fn tombstone_source(
    transaction: &Transaction<'_>,
    index_path: &Path,
    manifest_id: i64,
    generation: i64,
) -> Result<(), ManifestError> {
    stale_candidates(transaction, manifest_id, index_path)?;
    transaction
        .execute(
            "UPDATE source_manifest SET index_status = 'deleted', present = 0,
         observed_generation = ?2, last_error = NULL WHERE manifest_id = ?1",
            params![manifest_id, generation],
        )
        .map_err(|source| sqlite(index_path, "mark source deleted", source))?;
    Ok(())
}

pub(crate) fn empty_report(generation: i64) -> ManifestReport {
    ManifestReport {
        generation,
        created: 0,
        changed: 0,
        moved: 0,
        deleted: 0,
        unchanged: 0,
        metadata_only: 0,
        excluded: 0,
        content_reads: 0,
        content_bytes: 0,
    }
}

pub(crate) fn reconcile_entry(
    transaction: &Transaction<'_>,
    index_path: &Path,
    discovery: &DiscoveryReport,
    entry: &DiscoveryEntry,
    existing: &HashMap<Vec<u8>, ExistingRecord>,
    current_keys: &HashSet<Vec<u8>>,
    progress: &mut ReconcileProgress<'_>,
) -> Result<(), ManifestError> {
    let path_key = native_path_key(&entry.relative_path);
    let display_path = entry.relative_path.to_string_lossy();
    if is_missing_entry(entry) {
        confirm_selected_absence(&discovery.workspace_root, &entry.relative_path)?;
        if let Some(record) = existing.get(&path_key) {
            tombstone_source(
                transaction,
                index_path,
                record.manifest_id,
                progress.report.generation,
            )?;
            progress.report.deleted += usize::from(record.present);
        }
        return Ok(());
    }
    let Some(source_kind) = source_kind(entry) else {
        let sensitivity = sensitivity(entry);
        let exclusion = exclusion_reason(entry);
        if let Some(record) = existing.get(&path_key)
            && record.source_kind != "excluded"
        {
            stale_candidates(transaction, record.manifest_id, index_path)?;
        }
        transaction
            .execute(
                "INSERT INTO source_manifest(
                     path_key, display_path, source_kind, file_size, modified_unix_ns,
                     content_sha256, sensitivity_class, exclusion_reason, index_status,
                     present, observed_generation, last_error
                 ) VALUES (?1, ?2, 'excluded', ?3, ?4, NULL, ?5, ?6, 'excluded', 1, ?7, NULL)
                 ON CONFLICT(path_key) DO UPDATE SET
                     display_path = excluded.display_path,
                     source_kind = 'excluded',
                     file_size = excluded.file_size,
                     modified_unix_ns = excluded.modified_unix_ns,
                     content_sha256 = NULL,
                     sensitivity_class = excluded.sensitivity_class,
                     exclusion_reason = excluded.exclusion_reason,
                     index_status = 'excluded',
                     present = 1,
                     observed_generation = excluded.observed_generation,
                     last_error = NULL",
                params![
                    path_key,
                    display_path.as_ref(),
                    to_i64(entry.size),
                    modified_ns(&discovery.workspace_root.join(&entry.relative_path)),
                    sensitivity,
                    exclusion,
                    progress.report.generation,
                ],
            )
            .map_err(|source| sqlite(index_path, "record excluded manifest entry", source))?;
        progress.report.excluded += 1;
        return Ok(());
    };

    let source = open_workspace_source(
        &discovery.workspace_root,
        &entry.relative_path,
        progress.after_open,
    )?;
    let file_size_bytes = source.metadata.len();
    let file_size = Some(i64::try_from(file_size_bytes).unwrap_or(i64::MAX));
    let modified = Some(required_modified_ns(&source.path, &source.metadata)?);
    if let Some(record) = existing.get(&path_key)
        && record.describes(source_kind)
        && record.file_size == file_size
        && record.modified_unix_ns == modified
    {
        transaction
            .execute(
                "UPDATE source_manifest SET observed_generation = ?2 WHERE manifest_id = ?1",
                params![record.manifest_id, progress.report.generation],
            )
            .map_err(|source| sqlite(index_path, "refresh unchanged manifest entry", source))?;
        progress.report.unchanged += 1;
        return Ok(());
    }

    let (content_sha256, content_sensitivity) = match progress.hashed.remove(&path_key) {
        Some(hashed) if (hashed.file_size, hashed.modified_unix_ns) == (file_size, modified) => {
            (hashed.sha256, hashed.sensitivity)
        }
        _ => {
            let hashed = hash_file(
                source.file,
                &source.path,
                source_kind,
                progress.cancellation,
            )?;
            progress.report.content_reads += 1;
            progress.report.content_bytes = progress
                .report
                .content_bytes
                .saturating_add(file_size_bytes);
            hashed
        }
    };
    if let Some(sensitivity) = content_sensitivity {
        if let Some(record) = existing.get(&path_key) {
            stale_candidates(transaction, record.manifest_id, index_path)?;
        }
        transaction
            .execute(
                "INSERT INTO source_manifest(
                     path_key, display_path, source_kind, file_size, modified_unix_ns,
                     content_sha256, sensitivity_class, exclusion_reason, index_status,
                     present, observed_generation, last_error
                 ) VALUES (?1, ?2, 'excluded', ?3, ?4, NULL, ?5, 'sensitive_content',
                           'excluded', 1, ?6, NULL)
                 ON CONFLICT(path_key) DO UPDATE SET
                     display_path = excluded.display_path,
                     source_kind = 'excluded',
                     file_size = excluded.file_size,
                     modified_unix_ns = excluded.modified_unix_ns,
                     content_sha256 = NULL,
                     sensitivity_class = excluded.sensitivity_class,
                     exclusion_reason = excluded.exclusion_reason,
                     index_status = 'excluded',
                     present = 1,
                     observed_generation = excluded.observed_generation,
                     last_error = NULL",
                params![
                    path_key,
                    display_path.as_ref(),
                    file_size,
                    modified,
                    sensitivity.as_str(),
                    progress.report.generation,
                ],
            )
            .map_err(|source| sqlite(index_path, "record sensitive manifest entry", source))?;
        progress.report.excluded += 1;
        return Ok(());
    }
    if let Some(record) = existing.get(&path_key) {
        if record.present
            && record.content_sha256.as_deref() == Some(&content_sha256)
            && record.source_kind == source_kind
        {
            transaction
                .execute(
                    "UPDATE source_manifest
                     SET file_size = ?2, modified_unix_ns = ?3, present = 1,
                         observed_generation = ?4, last_error = NULL
                     WHERE manifest_id = ?1",
                    params![
                        record.manifest_id,
                        file_size,
                        modified,
                        progress.report.generation
                    ],
                )
                .map_err(|source| sqlite(index_path, "refresh manifest metadata", source))?;
            progress.report.metadata_only += 1;
        } else {
            stale_candidates(transaction, record.manifest_id, index_path)?;
            transaction
                .execute(
                    "UPDATE source_manifest
                     SET display_path = ?2, source_kind = ?3, file_size = ?4,
                         modified_unix_ns = ?5, content_sha256 = ?6,
                         sensitivity_class = NULL, exclusion_reason = NULL,
                         index_status = 'pending', present = 1, observed_generation = ?7,
                         last_error = NULL
                     WHERE manifest_id = ?1",
                    params![
                        record.manifest_id,
                        display_path.as_ref(),
                        source_kind,
                        file_size,
                        modified,
                        content_sha256,
                        progress.report.generation,
                    ],
                )
                .map_err(|source| sqlite(index_path, "update changed manifest source", source))?;
            progress.report.changed += 1;
        }
        return Ok(());
    }

    let move_matches = existing
        .iter()
        .filter(|(old_key, record)| {
            // In-memory checks first: the filesystem check runs only for a
            // row with the same content, not for every row per new file.
            record.present
                && !current_keys.contains(*old_key)
                && !progress.moved_ids.contains(&record.manifest_id)
                && record.source_kind == source_kind
                && record.content_sha256.as_deref() == Some(&content_sha256)
                && match progress.moves {
                    MovePolicy::MissingOnly => {
                        manifest_path_is_missing(&discovery.workspace_root, old_key)
                    }
                    MovePolicy::Isolated => false,
                }
        })
        .map(|(_, record)| record)
        .collect::<Vec<_>>();
    if let [record] = move_matches.as_slice() {
        transaction
            .execute(
                "UPDATE source_manifest
                 SET path_key = ?2, display_path = ?3, file_size = ?4,
                     modified_unix_ns = ?5, present = 1, observed_generation = ?6,
                     last_error = NULL
                 WHERE manifest_id = ?1",
                params![
                    record.manifest_id,
                    path_key,
                    display_path.as_ref(),
                    file_size,
                    modified,
                    progress.report.generation,
                ],
            )
            .map_err(|source| sqlite(index_path, "move manifest source", source))?;
        transaction
            .execute(
                "UPDATE source_search SET path = ?2 WHERE rowid IN (
                     SELECT candidate_no FROM source_candidates WHERE manifest_id = ?1
                 )",
                params![record.manifest_id, display_path.as_ref()],
            )
            .map_err(|source| sqlite(index_path, "move indexed source path", source))?;
        transaction
            .execute(
                "UPDATE source_search_ranked SET path = ?2 WHERE rowid IN (
                     SELECT candidate_no FROM source_candidates WHERE manifest_id = ?1
                 )",
                params![record.manifest_id, display_path.as_ref()],
            )
            .map_err(|source| sqlite(index_path, "move ranked source path", source))?;
        progress.moved_ids.insert(record.manifest_id);
        progress.report.moved += 1;
        return Ok(());
    }

    transaction
        .execute(
            "INSERT INTO source_manifest(
                 path_key, display_path, source_kind, file_size, modified_unix_ns,
                 content_sha256, sensitivity_class, exclusion_reason, index_status,
                 present, observed_generation, last_error
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, 'pending', 1, ?7, NULL)",
            params![
                path_key,
                display_path.as_ref(),
                source_kind,
                file_size,
                modified,
                content_sha256,
                progress.report.generation,
            ],
        )
        .map_err(|source| sqlite(index_path, "insert manifest source", source))?;
    progress.report.created += 1;
    Ok(())
}
