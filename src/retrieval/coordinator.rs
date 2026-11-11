//! One resumable state machine coordinates scoped discovery, manifest reconciliation,
//! and explicitly selected lazy extraction. The persisted cursor is rebuildable
//! source-index state; it never grants authority to write canonical learner data.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
#[cfg(test)]
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::workspace::{decode_native_path, native_path_key};

#[cfg(test)]
use super::manifest::reconcile_batch;
use super::manifest::{
    ManifestError, SourceCandidate, extract_source_with_cancellation, load_or_start_reconciliation,
    manifest_coverage, pending_source_paths, source_status_and_size,
};
use super::{
    DiscoveryDisposition, DiscoveryEntry, ExclusionReason, classify_file,
    classify_sensitive_directory, display_relative, excluded_source_boundary,
    export_boundary_exists, is_excluded_directory, read_magnesiumignore, reject_symlinked_control,
    rules_exclude,
};

const INITIAL_CURSOR_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CoordinatorBudget {
    /// Hard per-call limit on newly classified paths and their metadata work.
    /// Re-enumerated `ReadDir` entries remain under the cooperative deadline
    /// because `ReadDir` has no portable durable offset.
    pub(crate) max_metadata_entries: usize,
    pub(crate) max_duration: Duration,
    pub(crate) max_on_demand: usize,
}

impl Default for CoordinatorBudget {
    fn default() -> Self {
        Self {
            max_metadata_entries: 128,
            max_duration: Duration::from_millis(150),
            max_on_demand: 4,
        }
    }
}

impl CoordinatorBudget {
    /// One `search_sources` call. It may index up to 32 files while time
    /// remains. Once nothing is waiting to be indexed and no recheck is in
    /// progress, it may also recheck up to 1,024 entries, so a typical folder
    /// finishes a full recheck in one call and keeps reporting complete
    /// coverage between rechecks.
    pub(crate) fn for_search(index_path: &Path) -> Self {
        let max_metadata_entries =
            if super::manifest::status::indexed_with_nothing_pending(index_path) {
                1_024
            } else {
                128
            };
        Self {
            max_metadata_entries,
            max_on_demand: 32,
            ..Self::default()
        }
    }
}

/// Extractions a call may start whenever its deadline allows. Past these, a
/// call starts another extraction only in the first half of its duration, so
/// indexing more files per call still returns within the budget.
const GUARANTEED_ON_DEMAND: usize = 4;

fn may_start_on_demand(used: usize, cutoff: Instant) -> bool {
    used < GUARANTEED_ON_DEMAND || Instant::now() < cutoff
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IndexCoverage {
    Complete,
    Partial,
    Stale,
    IndexingOnDemand,
}

/// Outcome of the bounded reconciliation pass. `DeferredConcurrentUpdate`
/// means another writer held the reconciliation cursor past the shared run
/// budget; callers may search the already-indexed, currently authorized
/// snapshot and must not report it as complete or indexing on demand.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReconciliationOutcome {
    Reconciled,
    DeferredConcurrentUpdate,
    DeferredBudget,
}

impl ReconciliationOutcome {
    pub(crate) const fn deferred(self) -> bool {
        matches!(self, Self::DeferredConcurrentUpdate | Self::DeferredBudget)
    }

    pub(crate) const fn diagnostic(self) -> Option<&'static str> {
        match self {
            Self::Reconciled => None,
            Self::DeferredConcurrentUpdate => Some("deferred_concurrent_update"),
            Self::DeferredBudget => Some("deferred_budget"),
        }
    }
}

/// Shared per-request budget for one or more bounded reconciliation passes.
/// A retry continues with whatever remains; it never resets the counters.
struct RunBudget<'a> {
    deadline: Instant,
    extraction_cutoff: Instant,
    max_metadata_entries: usize,
    max_on_demand: usize,
    metadata_entries_used: usize,
    on_demand_used: usize,
    cancellation: &'a OperationCancellation,
}

impl<'a> RunBudget<'a> {
    fn new(
        started: Instant,
        budget: CoordinatorBudget,
        cancellation: &'a OperationCancellation,
    ) -> Self {
        Self {
            deadline: started + budget.max_duration,
            extraction_cutoff: started + budget.max_duration / 2,
            max_metadata_entries: budget.max_metadata_entries,
            max_on_demand: budget.max_on_demand,
            metadata_entries_used: 0,
            on_demand_used: 0,
            cancellation,
        }
    }

    fn remaining_metadata(&self) -> usize {
        self.max_metadata_entries
            .saturating_sub(self.metadata_entries_used)
    }

    fn remaining_on_demand(&self) -> usize {
        self.max_on_demand.saturating_sub(self.on_demand_used)
    }

    fn consume_metadata(&mut self) {
        self.metadata_entries_used = self.metadata_entries_used.saturating_add(1);
    }

    fn may_start_on_demand(&self) -> bool {
        self.remaining_on_demand() > 0
            && may_start_on_demand(self.on_demand_used, self.extraction_cutoff)
    }

    fn consume_on_demand(&mut self) {
        self.on_demand_used = self.on_demand_used.saturating_add(1);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn metadata_entries_used(&self) -> usize {
        self.metadata_entries_used
    }

    fn on_demand_used(&self) -> usize {
        self.on_demand_used
    }

    fn deadline_passed(&self) -> bool {
        Instant::now() >= self.deadline
    }

    fn can_retry(&self) -> bool {
        !self.cancellation.is_cancelled()
            && !self.deadline_passed()
            && self.remaining_metadata() > 0
    }
}

impl IndexCoverage {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Stale => "stale",
            Self::IndexingOnDemand => "indexing_on_demand",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CoordinatorMetrics {
    #[cfg(test)]
    pub(crate) metadata_entries_reconciled: usize,
    #[cfg(test)]
    pub(crate) content_reads: usize,
    #[cfg(test)]
    pub(crate) bytes_read: u64,
    #[cfg(test)]
    pub(crate) reconciliation_attempts: usize,
    /// On-demand extraction slots consumed by the whole run, including
    /// retries. Distinct from `indexed_on_demand`, which counts only sources
    /// that actually needed extraction.
    pub(crate) on_demand_used: usize,
    pub(crate) indexed_on_demand: usize,
}

pub(crate) use crate::request::OperationCancellation;
#[cfg(test)]
use crate::request::enter_request_cancellation;

impl OperationCancellation {
    pub(crate) fn check(&self) -> Result<(), super::manifest::ManifestError> {
        if self.is_cancelled() {
            return Err(super::manifest::ManifestError::Cancelled);
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CoordinatorReport {
    pub(crate) generation: i64,
    pub(crate) reconciliation: ReconciliationOutcome,
    /// Production callers work out coverage after their own extractions.
    #[cfg(test)]
    pub(crate) coverage: IndexCoverage,
    #[cfg(test)]
    pub(crate) candidates: Vec<SourceCandidate>,
    pub(crate) metrics: CoordinatorMetrics,
}

/// The complete preparation result consumed by search. Phase work counters
/// stay inside the coordinator and cannot affect ranking or query policy.
pub(crate) struct PreparedSourceQuery {
    pub(crate) generation: i64,
    pub(crate) reconciliation: ReconciliationOutcome,
    pub(crate) coverage: IndexCoverage,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ScanCursor {
    version: u32,
    directories: Vec<DirectoryCursor>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DirectoryCursor {
    relative_path: Vec<u8>,
    processed_entries: Vec<Vec<u8>>,
}

#[derive(Default)]
struct QueryBatch {
    entries: Vec<DiscoveryEntry>,
    keys: HashSet<Vec<u8>>,
    likely_sources: Vec<PathBuf>,
}

#[cfg(test)]
pub(crate) fn coordinate_source_query(
    workspace_root: &Path,
    index_path: &Path,
    query_scope: &Path,
    likely_paths: &[PathBuf],
    budget: CoordinatorBudget,
) -> Result<CoordinatorReport, ManifestError> {
    coordinate_source_query_with_allowlist(
        workspace_root,
        index_path,
        query_scope,
        likely_paths,
        budget,
        None,
    )
}

#[cfg(test)]
pub(crate) fn coordinate_source_query_with_allowlist(
    workspace_root: &Path,
    index_path: &Path,
    query_scope: &Path,
    likely_paths: &[PathBuf],
    budget: CoordinatorBudget,
    allowed_source_paths: Option<&BTreeSet<PathBuf>>,
) -> Result<CoordinatorReport, ManifestError> {
    coordinate_source_query_with_allowlist_and_cancellation(
        workspace_root,
        index_path,
        query_scope,
        likely_paths,
        budget,
        allowed_source_paths,
        &OperationCancellation::none(),
    )
}

#[cfg(test)]
pub(crate) fn coordinate_source_query_with_allowlist_and_cancellation(
    workspace_root: &Path,
    index_path: &Path,
    query_scope: &Path,
    likely_paths: &[PathBuf],
    budget: CoordinatorBudget,
    allowed_source_paths: Option<&BTreeSet<PathBuf>>,
    cancellation: &OperationCancellation,
) -> Result<CoordinatorReport, ManifestError> {
    coordinate_source_query_with_policy(
        workspace_root,
        index_path,
        query_scope,
        likely_paths,
        budget,
        allowed_source_paths,
        cancellation,
    )
}

fn coordinate_source_query_with_policy(
    workspace_root: &Path,
    index_path: &Path,
    query_scope: &Path,
    likely_paths: &[PathBuf],
    budget: CoordinatorBudget,
    allowed_source_paths: Option<&BTreeSet<PathBuf>>,
    cancellation: &OperationCancellation,
) -> Result<CoordinatorReport, ManifestError> {
    if budget.max_metadata_entries == 0 {
        return Err(ManifestError::Invalid(
            "source coordinator max_metadata_entries must be at least one".to_owned(),
        ));
    }
    if let Some(allowed_source_paths) = allowed_source_paths {
        for path in allowed_source_paths {
            validate_relative(path)?;
        }
    }
    let started = Instant::now();
    let workspace_root = fs::canonicalize(workspace_root).map_err(|source| ManifestError::Io {
        action: "resolve coordinator workspace",
        path: workspace_root.to_path_buf(),
        source,
    })?;
    reject_symlinked_control(&workspace_root)?;
    let rules = read_magnesiumignore(&workspace_root)?;
    cancellation.check()?;
    if !query_scope.as_os_str().is_empty() {
        validate_relative(query_scope)?;
    }
    for relative in likely_paths {
        validate_relative(relative)?;
        if !query_scope.as_os_str().is_empty() && !relative.starts_with(query_scope) {
            return Err(ManifestError::Invalid(format!(
                "likely source {} is outside query scope {}",
                relative.display(),
                query_scope.display()
            )));
        }
    }
    let initial = ScanCursor {
        version: INITIAL_CURSOR_VERSION,
        directories: vec![DirectoryCursor {
            relative_path: native_path_key(Path::new("")),
            processed_entries: Vec::new(),
        }],
    };
    let initial_json = serde_json::to_string(&initial).map_err(|error| {
        ManifestError::Invalid(format!("cannot serialize scan cursor: {error}"))
    })?;
    let mut run = RunBudget::new(started, budget, cancellation);
    let mut attempt = 1;
    loop {
        cancellation.check()?;
        match coordinate_one_pass(
            &workspace_root,
            index_path,
            query_scope,
            likely_paths,
            allowed_source_paths,
            &rules,
            &initial_json,
            attempt,
            &mut run,
        ) {
            Ok(report) => return Ok(report),
            // One bounded retry against the reloaded cursor; the shared run
            // budget is not reset and stale cursor advancement is never
            // applied on top of the new state.
            Err(ManifestError::ReconciliationConflict) if attempt == 1 && run.can_retry() => {
                attempt = 2;
            }
            Err(ManifestError::ReconciliationConflict) => {
                return deferred_reconciliation_report(index_path, attempt, &run);
            }
            Err(error) => return Err(error),
        }
    }
}

/// Prepare every source this query may index before ranking begins. Selected
/// paths and fallback paths share one extraction-slot cap; a deferred pass
/// serves only the last committed snapshot.
pub(crate) fn prepare_source_query_with_allowlist_and_cancellation(
    workspace_root: &Path,
    index_path: &Path,
    query_scope: &Path,
    likely_paths: &[PathBuf],
    budget: CoordinatorBudget,
    allowed_source_paths: Option<&BTreeSet<PathBuf>>,
    cancellation: &OperationCancellation,
) -> Result<PreparedSourceQuery, ManifestError> {
    prepare_source_query_with_policy(
        workspace_root,
        index_path,
        query_scope,
        likely_paths,
        budget,
        allowed_source_paths,
        cancellation,
    )
}

pub(crate) fn prepare_explicit_source_query(
    workspace_root: &Path,
    index_path: &Path,
    relative: &Path,
    cancellation: &OperationCancellation,
) -> Result<PreparedSourceQuery, ManifestError> {
    let selected = BTreeSet::from([relative.to_path_buf()]);
    prepare_source_query_with_policy(
        workspace_root,
        index_path,
        Path::new(""),
        &[],
        CoordinatorBudget {
            max_on_demand: 1,
            ..CoordinatorBudget::default()
        },
        Some(&selected),
        cancellation,
    )
}

fn prepare_source_query_with_policy(
    workspace_root: &Path,
    index_path: &Path,
    query_scope: &Path,
    likely_paths: &[PathBuf],
    budget: CoordinatorBudget,
    allowed_source_paths: Option<&BTreeSet<PathBuf>>,
    cancellation: &OperationCancellation,
) -> Result<PreparedSourceQuery, ManifestError> {
    // The same cooperative deadline covers reconciliation, selected-source
    // extraction and the fallback phase after the coordinator returns.
    // A zero duration requests a deferred reconciliation snapshot. Keep that
    // soft-budget contract: no extraction starts, but the call can return its
    // saved cursor instead of reporting caller cancellation.
    let deadline = Instant::now() + budget.max_duration;
    let extraction_cutoff = deadline - budget.max_duration / 2;
    let preparation_cancellation = if budget.max_duration.is_zero() {
        cancellation.clone()
    } else {
        cancellation.with_earlier_deadline(deadline)
    };
    let report = match coordinate_source_query_with_policy(
        workspace_root,
        index_path,
        query_scope,
        likely_paths,
        budget,
        allowed_source_paths,
        &preparation_cancellation,
    ) {
        Ok(report) => report,
        Err(ManifestError::Cancelled)
            if Instant::now() >= deadline && !cancellation.is_cancelled() =>
        {
            return deferred_budget_preparation(index_path);
        }
        Err(error) => return Err(error),
    };
    let mut fallback_count = 0;
    if !budget.max_duration.is_zero()
        && !report.reconciliation.deferred()
        && allowed_source_paths.is_none()
    {
        let remaining = budget
            .max_on_demand
            .saturating_sub(report.metrics.on_demand_used);
        let mut pending = Vec::new();
        for relative in pending_source_paths(index_path, query_scope, remaining)? {
            // A marker can revoke pending work before the bounded scan
            // reaches its row. Skipped paths are not extraction attempts.
            if !super::source_relative_path_is_excluded(workspace_root, &relative)? {
                pending.push(relative);
            }
        }
        let already_used = report.metrics.on_demand_used;
        let (used, outcome) = run_fallback_extractions(
            &pending,
            &preparation_cancellation,
            |used| may_start_on_demand(already_used + used, extraction_cutoff),
            |relative, token| {
                extract_for_search(workspace_root, index_path, relative, token).map(|_| ())
            },
        );
        fallback_count = used;
        match outcome {
            Err(ManifestError::Cancelled)
                if Instant::now() >= deadline && !cancellation.is_cancelled() =>
            {
                return deferred_budget_preparation(index_path);
            }
            other => other?,
        }
    }
    let coverage = if report.reconciliation.deferred() {
        index_coverage(index_path, 0, true)?
    } else {
        index_coverage(
            index_path,
            report.metrics.indexed_on_demand + fallback_count,
            false,
        )?
    };
    Ok(PreparedSourceQuery {
        generation: report.generation,
        reconciliation: report.reconciliation,
        coverage,
    })
}

fn deferred_budget_preparation(index_path: &Path) -> Result<PreparedSourceQuery, ManifestError> {
    Ok(PreparedSourceQuery {
        generation: super::manifest::current_generation(index_path)?,
        reconciliation: ReconciliationOutcome::DeferredBudget,
        coverage: index_coverage(index_path, 0, true)?,
    })
}

/// A file whose extraction failed and was recorded as `failed` contributes no
/// candidates but does not end the search; the other files still answer it.
/// Searches and catch-up also index pending sources in the same path order, so
/// another writer may publish a source first; that newer state stands. Explicit
/// source requests report both outcomes instead.
pub(crate) fn extract_for_search(
    workspace_root: &Path,
    index_path: &Path,
    relative: &Path,
    cancellation: &OperationCancellation,
) -> Result<Vec<SourceCandidate>, ManifestError> {
    match extract_source_with_cancellation(workspace_root, index_path, relative, cancellation) {
        Err(
            ManifestError::PdfContentPublished(_)
            | ManifestError::SourceFailurePublished(_)
            | ManifestError::SourceSuperseded,
        ) => Ok(Vec::new()),
        result => result,
    }
}

/// A started extraction consumes its slot even if it fails or cancellation
/// arrives mid-file. The returned count remains inspectable in tests; the
/// durable manifest keeps unfinished files pending for the next request.
fn run_fallback_extractions(
    pending: &[PathBuf],
    cancellation: &OperationCancellation,
    may_start: impl Fn(usize) -> bool,
    mut extract: impl FnMut(&Path, &OperationCancellation) -> Result<(), ManifestError>,
) -> (usize, Result<(), ManifestError>) {
    let mut used = 0;
    for relative in pending {
        if let Err(error) = cancellation.check() {
            return (used, Err(error));
        }
        if !may_start(used) {
            break;
        }
        used += 1;
        if let Err(error) = extract(relative, cancellation) {
            return (used, Err(error));
        }
    }
    (used, Ok(()))
}

#[allow(clippy::too_many_arguments)]
fn coordinate_one_pass(
    workspace_root: &Path,
    index_path: &Path,
    query_scope: &Path,
    likely_paths: &[PathBuf],
    allowed_source_paths: Option<&BTreeSet<PathBuf>>,
    rules: &[super::IgnoreRule],
    initial_json: &str,
    attempt: usize,
    run: &mut RunBudget<'_>,
) -> Result<CoordinatorReport, ManifestError> {
    let cancellation = run.cancellation;
    let state = load_or_start_reconciliation(index_path, initial_json, cancellation)?;
    let mut cursor: ScanCursor = serde_json::from_str(&state.cursor_json).map_err(|error| {
        ManifestError::Invalid(format!(
            "invalid persisted source reconciliation cursor: {error}"
        ))
    })?;
    if cursor.version != INITIAL_CURSOR_VERSION {
        return Err(ManifestError::Invalid(format!(
            "unsupported source reconciliation cursor version {}",
            cursor.version
        )));
    }
    #[cfg(test)]
    cursor_load_hook(workspace_root, index_path, attempt);

    // An explicit source selection never advances the workspace scan.
    let preserve_cursor = allowed_source_paths.is_some();

    let mut batch = QueryBatch::default();
    if allowed_source_paths.is_none() {
        for relative in likely_paths.iter().take(run.remaining_metadata()) {
            cancellation.check()?;
            if run.remaining_metadata() == 0 || run.deadline_passed() {
                break;
            }
            run.consume_metadata();
            if let Some(entry) = classify_likely_path(workspace_root, relative, rules)? {
                if matches!(entry.disposition, DiscoveryDisposition::Source(_)) {
                    batch.likely_sources.push(entry.relative_path.clone());
                }
                if batch.keys.insert(native_path_key(&entry.relative_path)) {
                    batch.entries.push(entry);
                }
            }
        }
    }
    if let Some(allowed_source_paths) = allowed_source_paths {
        let maximum = run.remaining_metadata();
        for relative in allowed_source_paths.iter().take(maximum) {
            cancellation.check()?;
            if run.deadline_passed() {
                break;
            }
            run.consume_metadata();
            let classified = match classify_scoped_path(workspace_root, relative, rules) {
                Err(ManifestError::Io { source, .. })
                    if source.kind() == io::ErrorKind::NotFound =>
                {
                    Some(ClassifiedPath::Entry(excluded(
                        relative,
                        None,
                        ExclusionReason::Unreadable(io::ErrorKind::NotFound),
                    )))
                }
                result => result?,
            };
            match classified {
                Some(ClassifiedPath::Entry(entry)) => {
                    if matches!(entry.disposition, DiscoveryDisposition::Source(_)) {
                        batch.likely_sources.push(entry.relative_path.clone());
                    }
                    if batch.keys.insert(native_path_key(&entry.relative_path)) {
                        batch.entries.push(entry);
                    }
                }
                Some(ClassifiedPath::Directory) => {
                    return Err(ManifestError::Invalid(format!(
                        "allowlisted source path {} must identify one source file",
                        relative.display()
                    )));
                }
                None => {}
            }
        }
    }
    if allowed_source_paths.is_none() {
        cancellation.check()?;
        collect_query_scope(workspace_root, query_scope, rules, run, &mut batch)?;
    }

    while allowed_source_paths.is_none()
        && run.remaining_metadata() > 0
        && !run.deadline_passed()
        && !cursor.directories.is_empty()
    {
        cancellation.check()?;
        let frame_index = cursor.directories.len() - 1;
        let frame = cursor.directories[frame_index].clone();
        let relative = decode_native_path(&frame.relative_path).map_err(ManifestError::Invalid)?;
        // A saved directory may have become a symlink, an ignored or nested
        // repository directory, or similar since it was queued.
        if !relative.as_os_str().is_empty() {
            match classify_scoped_path(workspace_root, &relative, rules) {
                Ok(Some(ClassifiedPath::Directory)) => {}
                Ok(Some(ClassifiedPath::Entry(entry))) => {
                    cursor.directories.pop();
                    if batch.keys.insert(native_path_key(&entry.relative_path)) {
                        batch.entries.push(entry);
                    }
                    continue;
                }
                Ok(None) => {
                    cursor.directories.pop();
                    continue;
                }
                // Reported as unreadable by the listing below.
                Err(ManifestError::Io { source, .. })
                    if source.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        let directory = workspace_root.join(&relative);
        let iterator = match fs::read_dir(&directory) {
            Ok(iterator) => iterator,
            Err(error) => {
                cursor.directories.pop();
                let entry = DiscoveryEntry {
                    relative_path: display_relative(&relative),
                    size: None,
                    disposition: DiscoveryDisposition::Excluded(ExclusionReason::Unreadable(
                        error.kind(),
                    )),
                };
                if batch.keys.insert(native_path_key(&entry.relative_path)) {
                    batch.entries.push(entry);
                }
                continue;
            }
        };
        let mut processed = frame.processed_entries.into_iter().collect::<BTreeSet<_>>();
        let mut fully_consumed = true;
        let mut discovered_directories = Vec::new();
        for child in iterator {
            cancellation.check()?;
            if run.deadline_passed() {
                fully_consumed = false;
                break;
            }
            let child = match child {
                Ok(child) => child,
                Err(error) => {
                    if run.remaining_metadata() == 0 || run.deadline_passed() {
                        fully_consumed = false;
                        break;
                    }
                    run.consume_metadata();
                    let entry = DiscoveryEntry {
                        relative_path: display_relative(&relative),
                        size: None,
                        disposition: DiscoveryDisposition::Excluded(ExclusionReason::Unreadable(
                            error.kind(),
                        )),
                    };
                    if batch.keys.insert(native_path_key(&entry.relative_path)) {
                        batch.entries.push(entry);
                    }
                    continue;
                }
            };
            let child_relative = relative.join(child.file_name());
            let child_key = native_path_key(&child_relative);
            if processed.contains(&child_key) {
                continue;
            }
            if run.remaining_metadata() == 0 || run.deadline_passed() {
                fully_consumed = false;
                break;
            }
            processed.insert(child_key);
            run.consume_metadata();
            match classify_path(workspace_root, &child_relative, rules)? {
                ClassifiedPath::Directory => discovered_directories.push(DirectoryCursor {
                    relative_path: native_path_key(&child_relative),
                    processed_entries: Vec::new(),
                }),
                ClassifiedPath::Entry(entry) => {
                    if batch.keys.insert(native_path_key(&entry.relative_path)) {
                        batch.entries.push(entry);
                    }
                }
            }
        }
        if fully_consumed {
            cursor.directories.remove(frame_index);
        } else {
            cursor.directories[frame_index].processed_entries = processed.into_iter().collect();
        }
        cursor
            .directories
            .extend(discovered_directories.into_iter().rev());
    }

    let next_cursor_json = if preserve_cursor {
        Some(state.cursor_json.clone())
    } else if cursor.directories.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&cursor).map_err(|error| {
            ManifestError::Invalid(format!("cannot serialize scan cursor: {error}"))
        })?)
    };
    let manifest_report = super::manifest::reconcile::reconcile_batch_with_policy(
        workspace_root,
        index_path,
        std::mem::take(&mut batch.entries),
        &state,
        if preserve_cursor {
            super::manifest::reconcile::ReconciliationAdvance::PreserveSelected
        } else {
            match next_cursor_json.as_deref() {
                Some(cursor) => super::manifest::reconcile::ReconciliationAdvance::Cursor(cursor),
                None => super::manifest::reconcile::ReconciliationAdvance::CompleteScan,
            }
        },
        cancellation,
    )?;

    let mut candidates = Vec::new();
    let mut indexed_on_demand = 0;
    let mut extraction_bytes = 0_u64;
    let mut extraction_paths = Vec::new();
    for relative in batch.likely_sources {
        if allowed_source_paths.is_some() {
            let Some((status, _)) = source_status_and_size(index_path, &relative)? else {
                continue;
            };
            if !matches!(status.as_str(), "pending" | "failed") {
                continue;
            }
        }
        extraction_paths.push(relative);
    }
    for relative in extraction_paths {
        if !run.may_start_on_demand() {
            break;
        }
        cancellation.check()?;
        let Some((status, size)) = source_status_and_size(index_path, &relative)? else {
            continue;
        };
        // A search leaves a failed source alone, as the fallback does; an
        // explicit request retries it.
        let unindexed =
            status == "pending" || (status == "failed" && allowed_source_paths.is_some());
        // Only tests read the candidates of an already indexed source.
        let load_indexed = cfg!(test) && status == "indexed";
        if !unindexed && !load_indexed {
            continue;
        }
        if unindexed {
            run.consume_on_demand();
            indexed_on_demand += 1;
            extraction_bytes = extraction_bytes.saturating_add(size);
        }
        let extracted = if allowed_source_paths.is_some() {
            extract_source_with_cancellation(workspace_root, index_path, &relative, cancellation)
        } else {
            extract_for_search(workspace_root, index_path, &relative, cancellation)
        };
        candidates.extend(extracted?);
    }
    #[cfg(test)]
    let coverage = index_coverage(index_path, indexed_on_demand, false)?;
    #[cfg(not(test))]
    let _ = (attempt, extraction_bytes);
    #[cfg(not(test))]
    let _ = &candidates;
    Ok(CoordinatorReport {
        generation: manifest_report.generation,
        reconciliation: ReconciliationOutcome::Reconciled,
        #[cfg(test)]
        coverage,
        #[cfg(test)]
        candidates,
        metrics: CoordinatorMetrics {
            #[cfg(test)]
            metadata_entries_reconciled: run.metadata_entries_used(),
            #[cfg(test)]
            content_reads: manifest_report.content_reads + indexed_on_demand,
            #[cfg(test)]
            bytes_read: manifest_report
                .content_bytes
                .saturating_add(extraction_bytes),
            #[cfg(test)]
            reconciliation_attempts: attempt,
            on_demand_used: run.on_demand_used(),
            indexed_on_demand,
        },
    })
}

/// Truthful bounded result for a reconciliation that stayed conflicted past
/// the shared run budget: no candidates from the failed pass, coverage from
/// the snapshot the caller can actually search, and an explicit diagnostic.
fn deferred_reconciliation_report(
    index_path: &Path,
    attempt: usize,
    run: &RunBudget<'_>,
) -> Result<CoordinatorReport, ManifestError> {
    #[cfg(test)]
    let coverage = index_coverage(index_path, 0, true)?;
    #[cfg(not(test))]
    let _ = attempt;
    Ok(CoordinatorReport {
        generation: super::manifest::current_generation(index_path)?,
        reconciliation: ReconciliationOutcome::DeferredConcurrentUpdate,
        #[cfg(test)]
        coverage,
        #[cfg(test)]
        candidates: Vec::new(),
        metrics: CoordinatorMetrics {
            #[cfg(test)]
            metadata_entries_reconciled: run.metadata_entries_used(),
            #[cfg(test)]
            content_reads: 0,
            #[cfg(test)]
            bytes_read: 0,
            #[cfg(test)]
            reconciliation_attempts: attempt,
            on_demand_used: run.on_demand_used(),
            indexed_on_demand: 0,
        },
    })
}

fn index_coverage(
    index_path: &Path,
    indexed_on_demand: usize,
    deferred: bool,
) -> Result<IndexCoverage, ManifestError> {
    let coverage = manifest_coverage(index_path)?;
    Ok(if deferred {
        if coverage.stale > 0 {
            IndexCoverage::Stale
        } else {
            IndexCoverage::Partial
        }
    } else if indexed_on_demand > 0 {
        IndexCoverage::IndexingOnDemand
    } else if coverage.stale > 0 {
        IndexCoverage::Stale
    } else if coverage.scan_active || coverage.pending > 0 {
        IndexCoverage::Partial
    } else {
        IndexCoverage::Complete
    })
}

/// Deterministic test synchronization: a hook observed immediately after the
/// persisted cursor is loaded and before any discovery work. Tests install a
/// one-shot gate to force overlapping passes without sleeps. The hook is
/// matched by index path, so unrelated concurrent tests never observe it.
#[cfg(test)]
pub(crate) type CursorLoadHook = dyn Fn(&Path, &Path, usize) + Send + Sync;

#[cfg(test)]
static CURSOR_LOAD_HOOKS: std::sync::Mutex<Vec<(PathBuf, Arc<CursorLoadHook>)>> =
    std::sync::Mutex::new(Vec::new());

#[cfg(test)]
pub(crate) struct CursorLoadHookGuard {
    index_path: PathBuf,
}

#[cfg(test)]
impl Drop for CursorLoadHookGuard {
    fn drop(&mut self) {
        CURSOR_LOAD_HOOKS
            .lock()
            .expect("cursor load hook lock")
            .retain(|(path, _)| *path != self.index_path);
    }
}

#[cfg(test)]
pub(crate) fn install_cursor_load_hook(
    index_path: &Path,
    hook: Arc<CursorLoadHook>,
) -> CursorLoadHookGuard {
    let mut hooks = CURSOR_LOAD_HOOKS.lock().expect("cursor load hook lock");
    hooks.retain(|(path, _)| *path != index_path);
    hooks.push((index_path.to_path_buf(), hook));
    drop(hooks);
    CursorLoadHookGuard {
        index_path: index_path.to_path_buf(),
    }
}

#[cfg(test)]
fn cursor_load_hook(workspace_root: &Path, index_path: &Path, attempt: usize) {
    let hook = CURSOR_LOAD_HOOKS
        .lock()
        .expect("cursor load hook lock")
        .iter()
        .find(|(path, _)| path == index_path)
        .map(|(_, hook)| Arc::clone(hook));
    if let Some(hook) = hook {
        hook(workspace_root, index_path, attempt);
    }
}

/// A gate that opens permanently once `expected` waiters have arrived.
/// Retries pass through without blocking.
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct OnceGate {
    state: std::sync::Mutex<GateState>,
    opened: std::sync::Condvar,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct GateState {
    arrived: usize,
    expected: usize,
}

#[cfg(test)]
impl OnceGate {
    pub(crate) fn new(expected: usize) -> Self {
        Self {
            state: std::sync::Mutex::new(GateState {
                arrived: 0,
                expected,
            }),
            opened: std::sync::Condvar::new(),
        }
    }

    pub(crate) fn wait(&self) {
        let mut state = self.state.lock().expect("gate state lock");
        state.arrived += 1;
        if state.arrived >= state.expected {
            self.opened.notify_all();
            return;
        }
        while state.arrived < state.expected {
            state = self.opened.wait(state).expect("gate wait");
        }
    }
}

fn collect_query_scope(
    root: &Path,
    scope: &Path,
    rules: &[super::IgnoreRule],
    run: &mut RunBudget<'_>,
    batch: &mut QueryBatch,
) -> Result<(), ManifestError> {
    if scope.as_os_str().is_empty() {
        return Ok(());
    }
    validate_relative(scope)?;
    let classified = match classify_scoped_path(root, scope, rules) {
        Ok(classified) => classified,
        Err(ManifestError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let Some(classified) = classified else {
        return Ok(());
    };
    match classified {
        ClassifiedPath::Entry(entry) => {
            if run.remaining_metadata() == 0 {
                return Ok(());
            }
            run.consume_metadata();
            if matches!(entry.disposition, DiscoveryDisposition::Source(_)) {
                batch.likely_sources.push(entry.relative_path.clone());
            }
            if batch.keys.insert(native_path_key(&entry.relative_path)) {
                batch.entries.push(entry);
            }
        }
        ClassifiedPath::Directory => {
            let iterator = fs::read_dir(root.join(scope)).map_err(|source| ManifestError::Io {
                action: "read source query scope",
                path: root.join(scope),
                source,
            })?;
            let maximum = run.remaining_metadata();
            for child in iterator.take(maximum) {
                if run.deadline_passed() {
                    break;
                }
                run.consume_metadata();
                let child = child.map_err(|source| ManifestError::Io {
                    action: "read source query scope entry",
                    path: root.join(scope),
                    source,
                })?;
                let relative = scope.join(child.file_name());
                if let ClassifiedPath::Entry(entry) = classify_path(root, &relative, rules)?
                    && batch.keys.insert(native_path_key(&entry.relative_path))
                {
                    batch.entries.push(entry);
                }
            }
        }
    }
    Ok(())
}

enum ClassifiedPath {
    Directory,
    Entry(DiscoveryEntry),
}

fn classify_likely_path(
    root: &Path,
    relative: &Path,
    rules: &[super::IgnoreRule],
) -> Result<Option<DiscoveryEntry>, ManifestError> {
    match classify_scoped_path(root, relative, rules) {
        Ok(Some(ClassifiedPath::Entry(entry))) => Ok(Some(entry)),
        Ok(Some(ClassifiedPath::Directory) | None) => Ok(None),
        Err(ManifestError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn classify_scoped_path(
    root: &Path,
    relative: &Path,
    rules: &[super::IgnoreRule],
) -> Result<Option<ClassifiedPath>, ManifestError> {
    validate_relative(relative)?;
    let mut prefix = PathBuf::new();
    let components = relative.components().collect::<Vec<_>>();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        let Component::Normal(name) = component else {
            return Err(ManifestError::Invalid(format!(
                "source path {} is not workspace-relative",
                relative.display()
            )));
        };
        prefix.push(name);
        if !matches!(
            classify_path(root, &prefix, rules)?,
            ClassifiedPath::Directory
        ) {
            return Ok(None);
        }
    }
    classify_path(root, relative, rules).map(Some)
}

fn classify_path(
    root: &Path,
    relative: &Path,
    rules: &[super::IgnoreRule],
) -> Result<ClassifiedPath, ManifestError> {
    validate_relative(relative)?;
    if export_boundary_exists(root).map_err(|source| ManifestError::Io {
        action: "inspect workspace export boundary",
        path: root.to_path_buf(),
        source,
    })? {
        return Ok(ClassifiedPath::Entry(excluded(
            relative,
            None,
            ExclusionReason::PrivateExport,
        )));
    }
    let path = root.join(relative);
    let metadata = fs::symlink_metadata(&path).map_err(|source| ManifestError::Io {
        action: "inspect source candidate",
        path: path.clone(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Ok(ClassifiedPath::Entry(excluded(
            relative,
            None,
            ExclusionReason::Symlink,
        )));
    }
    if metadata.is_dir() {
        let Some(name) = relative.file_name() else {
            return Ok(ClassifiedPath::Directory);
        };
        let exclusion = if let Some(class) = classify_sensitive_directory(name) {
            Some(ExclusionReason::Sensitive(class))
        } else if is_excluded_directory(name) {
            Some(ExclusionReason::BuiltIn)
        } else if let Some(reason) =
            excluded_source_boundary(&path).map_err(|source| ManifestError::Io {
                action: "inspect excluded source boundary",
                path: path.clone(),
                source,
            })?
        {
            Some(reason)
        } else if rules_exclude(rules, relative, true) {
            Some(ExclusionReason::Magnesiumignore)
        } else {
            None
        };
        if let Some(reason) = exclusion {
            return Ok(ClassifiedPath::Entry(excluded(relative, None, reason)));
        }
        let canonical = fs::canonicalize(&path).map_err(|source| ManifestError::Io {
            action: "resolve source directory",
            path: path.clone(),
            source,
        })?;
        if !canonical.starts_with(root) {
            return Ok(ClassifiedPath::Entry(excluded(
                relative,
                None,
                ExclusionReason::EscapesWorkspace,
            )));
        }
        return Ok(ClassifiedPath::Directory);
    }
    let mut entries = Vec::with_capacity(1);
    if metadata.is_file() {
        classify_file(relative, metadata.len(), rules, &mut entries);
    } else {
        entries.push(excluded(
            relative,
            Some(metadata.len()),
            ExclusionReason::Unsupported,
        ));
    }
    Ok(ClassifiedPath::Entry(
        entries.pop().expect("file classification emits one entry"),
    ))
}

fn excluded(relative: &Path, size: Option<u64>, reason: ExclusionReason) -> DiscoveryEntry {
    DiscoveryEntry {
        relative_path: relative.to_path_buf(),
        size,
        disposition: DiscoveryDisposition::Excluded(reason),
    }
}

fn validate_relative(path: &Path) -> Result<(), ManifestError> {
    if path.is_absolute()
        || path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ManifestError::Invalid(format!(
            "source path {} must contain only workspace-relative normal components",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
