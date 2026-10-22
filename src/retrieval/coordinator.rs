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
    /// Re-enumerated `ReadDir` entries are reported separately and remain under
    /// the cooperative deadline because `ReadDir` has no portable durable offset.
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

    /// One background catch-up extraction pass. Lasting twice the PDF
    /// deadline means an extraction that starts before the halfway cutoff
    /// still gets the whole PDF deadline, so the pass never cuts a PDF short.
    pub(crate) fn for_catch_up() -> Self {
        Self {
            max_metadata_entries: 0,
            max_duration: super::manifest::pdf::PDF_DEADLINE.saturating_mul(2),
            max_on_demand: 64,
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
    #[cfg_attr(not(test), allow(dead_code))]
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

#[allow(clippy::too_many_arguments)]
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

#[allow(clippy::too_many_arguments)]
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
        report.coverage
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

/// Extract sources that wait for extraction without a reconciliation pass. An
/// allowlist pass would start a recheck when none is in progress; this never
/// reads or writes the scan cursor. Each extraction starts before the halfway
/// cutoff, so it keeps at least half the duration, and one cut short by the
/// deadline stays pending.
pub(crate) fn extract_pending_sources(
    workspace_root: &Path,
    index_path: &Path,
    pending: &[PathBuf],
    budget: CoordinatorBudget,
    cancellation: &OperationCancellation,
) -> Result<(), ManifestError> {
    let deadline = Instant::now() + budget.max_duration;
    let extraction_cutoff = deadline - budget.max_duration / 2;
    let pass_cancellation = cancellation.with_earlier_deadline(deadline);
    let mut eligible = Vec::new();
    for relative in pending.iter().take(budget.max_on_demand) {
        if !super::source_relative_path_is_excluded(workspace_root, relative)? {
            eligible.push(relative.clone());
        }
    }
    let (_, outcome) = run_fallback_extractions(
        &eligible,
        &pass_cancellation,
        |_| Instant::now() < extraction_cutoff,
        |relative, token| {
            extract_for_search(workspace_root, index_path, relative, token).map(|_| ())
        },
    );
    match outcome {
        Err(ManifestError::Cancelled)
            if Instant::now() >= deadline && !cancellation.is_cancelled() =>
        {
            Ok(())
        }
        other => other,
    }
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
fn extract_for_search(
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
        run.consume_on_demand();
        let Some((status, size)) = source_status_and_size(index_path, &relative)? else {
            continue;
        };
        // A search leaves a failed source alone, as the fallback does; an
        // explicit request retries it.
        let unindexed =
            status == "pending" || (status == "failed" && allowed_source_paths.is_some());
        if unindexed {
            indexed_on_demand += 1;
            extraction_bytes = extraction_bytes.saturating_add(size);
        }
        if unindexed || status == "indexed" {
            let extracted = if allowed_source_paths.is_some() {
                extract_source_with_cancellation(
                    workspace_root,
                    index_path,
                    &relative,
                    cancellation,
                )
            } else {
                extract_for_search(workspace_root, index_path, &relative, cancellation)
            };
            candidates.extend(extracted?);
        }
    }
    let coverage = index_coverage(index_path, indexed_on_demand, false)?;
    #[cfg(not(test))]
    let _ = (attempt, extraction_bytes);
    #[cfg(not(test))]
    let _ = &candidates;
    Ok(CoordinatorReport {
        generation: manifest_report.generation,
        reconciliation: ReconciliationOutcome::Reconciled,
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
    let coverage = index_coverage(index_path, 0, true)?;
    Ok(CoordinatorReport {
        generation: super::manifest::current_generation(index_path)?,
        reconciliation: ReconciliationOutcome::DeferredConcurrentUpdate,
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
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use pdf_extract::{Document, Object, Stream, content::Content, content::Operation, dictionary};
    use rusqlite::Connection;

    use super::super::catch_up::catch_up;
    use super::{
        CoordinatorBudget, IndexCoverage, ManifestError, OnceGate, OperationCancellation,
        ReconciliationOutcome, coordinate_source_query, coordinate_source_query_with_allowlist,
        coordinate_source_query_with_allowlist_and_cancellation, enter_request_cancellation,
        install_cursor_load_hook, load_or_start_reconciliation,
        prepare_source_query_with_allowlist_and_cancellation, reconcile_batch,
        run_fallback_extractions,
    };

    static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn relevant_query_discovers_external_pdf_and_indexes_selected_text_on_demand() {
        let fixture = Fixture::new("query");
        fixture.write("lesson.md", b"bounded coordinator\n");
        finish_metadata_scan(&fixture, budget(4));

        let indexed = coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[PathBuf::from("lesson.md")],
            budget(4),
        )
        .expect("index selected text source");
        assert_eq!(indexed.coverage, IndexCoverage::IndexingOnDemand);
        assert_eq!(indexed.metrics.indexed_on_demand, 1);
        assert_eq!(indexed.candidates.len(), 1);
        assert_eq!(indexed.candidates[0].path, "lesson.md");
        assert_eq!(indexed.candidates[0].body, "bounded coordinator\n");

        fixture.write("documents/finder-added.pdf", &pdf_fixture());
        let discovered = coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new("documents"),
            &[],
            CoordinatorBudget {
                max_on_demand: 0,
                ..budget(4)
            },
        )
        .expect("discover externally added PDF on its next relevant query");
        assert_eq!(discovered.coverage, IndexCoverage::Partial);
        let pdf_state: (String, String, i64) = Connection::open(fixture.index())
            .expect("open source index")
            .query_row(
                "SELECT source_kind, index_status, present FROM source_manifest
                 WHERE display_path = 'documents/finder-added.pdf'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("read Finder-added PDF state");
        assert_eq!(pdf_state, ("pdf".to_owned(), "pending".to_owned(), 1));
    }

    #[test]
    fn search_budget_widens_the_recheck_only_once_nothing_waits_for_indexing() {
        let fixture = Fixture::new("search-budget");
        fixture.write("lesson.md", b"bounded coordinator\n");
        let before_index = CoordinatorBudget::for_search(&fixture.index());
        assert_eq!(before_index.max_metadata_entries, 128);
        assert_eq!(before_index.max_on_demand, 32);
        finish_metadata_scan(&fixture, budget(4));
        assert_eq!(
            CoordinatorBudget::for_search(&fixture.index()).max_metadata_entries,
            128,
            "lesson.md still waits for its first extraction"
        );
        coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[PathBuf::from("lesson.md")],
            budget(4),
        )
        .expect("index selected text source");
        assert_eq!(
            CoordinatorBudget::for_search(&fixture.index()).max_metadata_entries,
            1_024
        );

        // A recheck still in progress, such as a widened pass that ran out of
        // time, falls back to the smaller recheck so the next call commits.
        // Unsupported files leave nothing pending, so only the recheck counts.
        for index in 0..3 {
            fixture.write(&format!("added-{index}.bin"), b"\x00binary\n");
        }
        coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            budget(1),
        )
        .expect("start a bounded recheck");
        assert_eq!(scan_active(&fixture.index()), 1);
        assert_eq!(
            CoordinatorBudget::for_search(&fixture.index()).max_metadata_entries,
            128
        );
    }

    #[test]
    fn extractions_past_the_guaranteed_few_start_only_before_the_cutoff() {
        let passed = std::time::Instant::now();
        let ahead = passed + Duration::from_secs(60);
        assert!(super::may_start_on_demand(3, passed));
        assert!(!super::may_start_on_demand(4, passed));
        assert!(super::may_start_on_demand(31, ahead));

        let pending = (0..6)
            .map(|index| PathBuf::from(format!("pending-{index}.md")))
            .collect::<Vec<_>>();
        let mut started = 0;
        let (used, result) = run_fallback_extractions(
            &pending,
            &OperationCancellation::none(),
            |used| super::may_start_on_demand(used, passed),
            |_, _| {
                started += 1;
                Ok(())
            },
        );
        assert!(result.is_ok());
        assert_eq!((used, started), (4, 4), "past the cutoff only four start");
    }

    #[test]
    fn a_search_indexes_more_than_four_files_while_time_remains() {
        let fixture = Fixture::new("more-than-four");
        let likely = (0..10)
            .map(|index| {
                let path = format!("lesson-{index}.md");
                fixture.write(&path, format!("lesson {index}\n").as_bytes());
                PathBuf::from(path)
            })
            .collect::<Vec<_>>();
        finish_metadata_scan(&fixture, budget(64));
        let report = coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &likely,
            CoordinatorBudget {
                max_metadata_entries: 64,
                max_duration: Duration::from_secs(600),
                max_on_demand: 32,
            },
        )
        .expect("index every selected source");
        assert_eq!(report.metrics.on_demand_used, 10);
        assert_eq!(report.metrics.indexed_on_demand, 10);
    }

    #[test]
    fn preparation_shares_selected_and_fallback_slots_and_reports_final_coverage() {
        let fixture = Fixture::new("preparation-slots");
        fixture.write("a-selected.md", b"selected source\n");
        fixture.write("b-pending.md", b"pending source\n");
        finish_metadata_scan(&fixture, budget(8));
        let selected = [PathBuf::from("a-selected.md")];
        coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &selected,
            budget(8),
        )
        .expect("index selected source first");

        let one_slot = CoordinatorBudget {
            max_on_demand: 1,
            ..budget(8)
        };
        let exhausted = prepare_source_query_with_allowlist_and_cancellation(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &selected,
            one_slot,
            None,
            &OperationCancellation::none(),
        )
        .expect("prepare with selected path consuming only slot");
        assert_eq!(exhausted.coverage, IndexCoverage::Partial);
        assert_eq!(source_status(&fixture.index(), "b-pending.md"), "pending");

        let two_slots = CoordinatorBudget {
            max_on_demand: 2,
            ..one_slot
        };
        let prepared = prepare_source_query_with_allowlist_and_cancellation(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &selected,
            two_slots,
            None,
            &OperationCancellation::none(),
        )
        .expect("prepare with one remaining fallback slot");
        assert_eq!(prepared.coverage, IndexCoverage::IndexingOnDemand);
        assert_eq!(source_status(&fixture.index(), "b-pending.md"), "indexed");
    }

    #[test]
    fn preparation_fallback_uses_scope_and_binary_path_order() {
        let fixture = Fixture::new("preparation-fallback");
        fixture.write("docs/b.md", b"second source\n");
        fixture.write("docs/a.md", b"first source\n");
        fixture.write("elsewhere/c.md", b"outside scope\n");
        finish_metadata_scan(&fixture, budget(16));

        let prepared = prepare_source_query_with_allowlist_and_cancellation(
            &fixture.root,
            &fixture.index(),
            Path::new("docs"),
            &[],
            CoordinatorBudget {
                max_on_demand: 1,
                ..budget(16)
            },
            None,
            &OperationCancellation::none(),
        )
        .expect("prepare first scoped pending source");
        assert_eq!(prepared.coverage, IndexCoverage::IndexingOnDemand);
        assert_eq!(source_status(&fixture.index(), "docs/a.md"), "indexed");
        assert_eq!(source_status(&fixture.index(), "docs/b.md"), "pending");
        assert_eq!(source_status(&fixture.index(), "elsewhere/c.md"), "pending");
    }

    #[test]
    fn preparation_deadline_and_caller_cancellation_bound_fallback_slots() {
        let fixture = Fixture::new("preparation-deadline");
        fixture.write("pending.md", b"resumable source\n");
        finish_metadata_scan(&fixture, budget(8));
        let pending = [PathBuf::from("pending.md")];

        let expired =
            OperationCancellation::none().with_earlier_deadline(std::time::Instant::now());
        let (used, result) = run_fallback_extractions(
            &pending,
            &expired,
            |_| true,
            |_, _| panic!("expired preparation must not start fallback extraction"),
        );
        assert_eq!(used, 0);
        assert!(matches!(result, Err(ManifestError::Cancelled)));
        assert_eq!(source_status(&fixture.index(), "pending.md"), "pending");

        let caller_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let token = OperationCancellation::with_flag(caller_flag.clone())
            .with_earlier_deadline(std::time::Instant::now() + Duration::from_secs(10));
        let (used, result) = run_fallback_extractions(
            &pending,
            &token,
            |_| true,
            |_, token| {
                caller_flag.store(true, Ordering::Relaxed);
                token.check()
            },
        );
        assert_eq!(used, 1, "a started extraction consumes its slot");
        assert!(matches!(result, Err(ManifestError::Cancelled)));
        assert_eq!(source_status(&fixture.index(), "pending.md"), "pending");

        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let token = OperationCancellation::none().with_earlier_deadline(deadline);
        let (used, result) = run_fallback_extractions(
            &pending,
            &token,
            |_| true,
            |_, token| {
                while std::time::Instant::now() < deadline {
                    std::thread::yield_now();
                }
                token.check()
            },
        );
        assert_eq!(used, 1, "deadline expired after extraction started");
        assert!(matches!(result, Err(ManifestError::Cancelled)));
        assert_eq!(source_status(&fixture.index(), "pending.md"), "pending");
    }

    #[test]
    fn superseded_fallback_attempt_stays_counted_and_is_not_retried() {
        let fixture = Fixture::new("superseded-fallback");
        fixture.write("lesson.md", b"current same-content source\n");
        finish_metadata_scan(&fixture, budget(8));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (published_tx, published_rx) = std::sync::mpsc::channel();
        let root = fixture.root.clone();
        let index = fixture.index();
        let competitor = std::thread::spawn(move || {
            ready_rx.recv().unwrap();
            super::super::manifest::extract_source_with_cancellation(
                &root,
                &index,
                Path::new("lesson.md"),
                &OperationCancellation::none(),
            )
            .unwrap();
            published_tx.send(()).unwrap();
        });
        let checkpoints = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hook_count = checkpoints.clone();
        let published_rx = std::sync::Mutex::new(published_rx);
        let token =
            OperationCancellation::none().with_test_hook(std::sync::Arc::new(move |checkpoint| {
                if checkpoint == "source_publication_before_lock" {
                    hook_count.fetch_add(1, Ordering::Relaxed);
                    ready_tx.send(()).unwrap();
                    published_rx.lock().unwrap().recv().unwrap();
                }
            }));
        let scope = enter_request_cancellation(&token);
        let mut calls = 0;
        let (used, result) = run_fallback_extractions(
            &[PathBuf::from("lesson.md")],
            &token,
            |_| true,
            |path, token| {
                calls += 1;
                super::extract_source_with_cancellation(
                    &fixture.root,
                    &fixture.index(),
                    path,
                    token,
                )
                .map(|_| ())
            },
        );
        drop(scope);
        competitor.join().unwrap();
        assert!(
            matches!(result, Err(ManifestError::SourceSuperseded)),
            "{result:?}"
        );
        assert_eq!(used, 1, "the superseded attempt still consumes its slot");
        assert_eq!(calls, 1, "no source-conflict internal retry");
        assert_eq!(checkpoints.load(Ordering::Relaxed), 1);
        let current = super::extract_source_with_cancellation(
            &fixture.root,
            &fixture.index(),
            Path::new("lesson.md"),
            &OperationCancellation::none(),
        )
        .unwrap();
        assert_eq!(current[0].body, "current same-content source\n");
    }

    #[test]
    fn catch_up_indexes_a_pdf_that_outlasts_a_search() {
        let fixture = Fixture::new("catch-up-slow-pdf");
        fixture.write("slides/lecture.pdf", &pdf_fixture());
        finish_metadata_scan(&fixture, budget(8));
        let search = CoordinatorBudget::for_search(&fixture.index());
        let slow = OperationCancellation::none().with_test_hook(Arc::new(move |checkpoint| {
            if checkpoint == "source_publication_before_lock" {
                std::thread::sleep(search.max_duration);
            }
        }));
        let _scope = enter_request_cancellation(&slow);

        let searched = prepare_source_query_with_allowlist_and_cancellation(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            search,
            None,
            &slow,
        )
        .expect("a search returns within its budget");
        assert_eq!(
            searched.reconciliation,
            ReconciliationOutcome::DeferredBudget
        );
        assert_eq!(
            source_status(&fixture.index(), "slides/lecture.pdf"),
            "pending"
        );

        catch_up(&fixture.root, &slow).expect("catch up after the search");
        assert_eq!(
            source_status(&fixture.index(), "slides/lecture.pdf"),
            "indexed"
        );
    }

    #[test]
    fn catch_up_makes_files_searchable_before_discovery_finishes() {
        let fixture = Fixture::new("catch-up-progressive");
        for index in 0..100 {
            fixture.write(
                &format!("lesson-{index:03}.md"),
                format!("lesson {index}\n").as_bytes(),
            );
        }
        drop(
            crate::retrieval::initialize_index(
                &fixture.index(),
                Duration::from_secs(2),
                &OperationCancellation::none(),
            )
            .unwrap(),
        );
        let scanning_at_first_publication = Arc::new(std::sync::Mutex::new(None));
        let watch = {
            let observed = Arc::clone(&scanning_at_first_publication);
            let index = fixture.index();
            OperationCancellation::none().with_test_hook(Arc::new(move |checkpoint| {
                if checkpoint == "source_publication_before_lock" {
                    observed
                        .lock()
                        .unwrap()
                        .get_or_insert_with(|| scan_active(&index) == 1);
                }
            }))
        };
        let _scope = enter_request_cancellation(&watch);
        catch_up(&fixture.root, &watch).expect("catch up");
        assert_eq!(
            *scanning_at_first_publication.lock().unwrap(),
            Some(true),
            "the first file became searchable only after discovery finished"
        );
        assert_eq!(scan_active(&fixture.index()), 0);
        for index in [0, 99] {
            assert_eq!(
                source_status(&fixture.index(), &format!("lesson-{index:03}.md")),
                "indexed"
            );
        }
    }

    #[test]
    fn catch_up_never_starts_or_finishes_a_recheck_after_a_full_scan() {
        let idle = Fixture::new("catch-up-no-recheck");
        let rechecking = Fixture::new("catch-up-recheck-in-progress");
        for fixture in [&idle, &rechecking] {
            for index in 0..6 {
                fixture.write(
                    &format!("lesson-{index}.md"),
                    format!("lesson {index}\n").as_bytes(),
                );
            }
            finish_metadata_scan(fixture, budget(16));
        }
        coordinate_source_query(
            &rechecking.root,
            &rechecking.index(),
            Path::new(""),
            &[],
            budget(1),
        )
        .expect("a search starts a recheck");
        let recheck = persisted_cursor(&rechecking.index());

        for fixture in [&idle, &rechecking] {
            catch_up(&fixture.root, &OperationCancellation::none()).expect("catch up");
            for index in 0..6 {
                assert_eq!(
                    source_status(&fixture.index(), &format!("lesson-{index}.md")),
                    "indexed"
                );
            }
        }
        assert_eq!(scan_active(&idle.index()), 0, "catch-up started a recheck");
        assert_eq!(
            persisted_cursor(&rechecking.index()),
            recheck,
            "catch-up moved the recheck in progress"
        );
    }

    #[test]
    fn catch_up_leaves_a_failed_source_and_indexes_the_rest() {
        let fixture = Fixture::new("catch-up-failed");
        fixture.write("a-not-text.md", b"\xff\xfe\xfd\n");
        fixture.write("b-lesson.md", b"lesson\n");
        finish_metadata_scan(&fixture, budget(8));
        super::extract_source_with_cancellation(
            &fixture.root,
            &fixture.index(),
            Path::new("a-not-text.md"),
            &OperationCancellation::none(),
        )
        .expect_err("invalid UTF-8 cannot be indexed");
        assert_eq!(source_status(&fixture.index(), "a-not-text.md"), "failed");

        catch_up(&fixture.root, &OperationCancellation::none()).expect("catch up");
        assert_eq!(source_status(&fixture.index(), "b-lesson.md"), "indexed");
    }

    #[test]
    fn a_search_succeeds_when_catch_up_publishes_its_fallback_source_first() {
        let fixture = Fixture::new("catch-up-race");
        fixture.write("lesson.md", b"one source, two writers\n");
        finish_metadata_scan(&fixture, budget(8));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (published_tx, published_rx) = std::sync::mpsc::channel();
        let root = fixture.root.clone();
        let background = std::thread::spawn(move || {
            ready_rx.recv().unwrap();
            catch_up(&root, &OperationCancellation::none()).unwrap();
            published_tx.send(()).unwrap();
        });
        let published_rx = std::sync::Mutex::new(published_rx);
        let paused = OperationCancellation::none().with_test_hook(Arc::new(move |checkpoint| {
            if checkpoint == "source_publication_before_lock" {
                ready_tx.send(()).unwrap();
                published_rx.lock().unwrap().recv().unwrap();
            }
        }));
        let scope = enter_request_cancellation(&paused);
        let searched = prepare_source_query_with_allowlist_and_cancellation(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            budget(8),
            None,
            &paused,
        );
        drop(scope);
        background.join().unwrap();
        let searched = searched.expect("the search keeps the newer published state");
        assert_eq!(searched.reconciliation, ReconciliationOutcome::Reconciled);
        assert_eq!(source_status(&fixture.index(), "lesson.md"), "indexed");
    }

    #[test]
    fn a_search_that_finds_the_index_writer_busy_defers_within_its_budget() {
        for lock_taken in ["before the search", "after the cursor loads"] {
            let fixture = Fixture::new("busy-writer");
            fixture.write("lesson.md", b"indexed before the writer arrives\n");
            finish_metadata_scan(&fixture, budget(8));
            fixture.write("new-lesson.md", b"waits for the next pass\n");
            let writer = Arc::new(std::sync::Mutex::new(
                Connection::open(fixture.index()).expect("open competing writer"),
            ));
            let take_lock = {
                let writer = Arc::clone(&writer);
                move || {
                    writer
                        .lock()
                        .unwrap()
                        .execute_batch("BEGIN IMMEDIATE")
                        .expect("competing writer takes the lock");
                }
            };
            let _hook = if lock_taken == "before the search" {
                take_lock();
                None
            } else {
                Some(install_cursor_load_hook(
                    &fixture.index(),
                    Arc::new(move |_, _, _| take_lock()),
                ))
            };
            let search = CoordinatorBudget::for_search(&fixture.index());
            let started = std::time::Instant::now();
            let prepared = prepare_source_query_with_allowlist_and_cancellation(
                &fixture.root,
                &fixture.index(),
                Path::new(""),
                &[],
                search,
                None,
                &OperationCancellation::none(),
            )
            .unwrap_or_else(|error| panic!("lock taken {lock_taken}: {error}"));
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "lock taken {lock_taken}: the search waited {:?}",
                started.elapsed()
            );
            assert_eq!(
                prepared.reconciliation,
                ReconciliationOutcome::DeferredBudget,
                "lock taken {lock_taken}"
            );
            writer.lock().unwrap().execute_batch("ROLLBACK").unwrap();
        }
    }

    #[test]
    fn other_writers_can_take_the_lock_while_reconciliation_hashes_a_new_file() {
        let fixture = Fixture::new("hash-outside-lock");
        fixture.write("lesson.md", b"hashed before the write lock\n");
        drop(
            crate::retrieval::initialize_index(
                &fixture.index(),
                Duration::from_secs(2),
                &OperationCancellation::none(),
            )
            .unwrap(),
        );
        let lock_free_while_hashing = Arc::new(std::sync::Mutex::new(Vec::new()));
        let probe = {
            let observed = Arc::clone(&lock_free_while_hashing);
            let index = fixture.index();
            OperationCancellation::none().with_test_hook(Arc::new(move |checkpoint| {
                if checkpoint == "source_hash_read_chunk" {
                    let writer = Connection::open(&index).unwrap();
                    writer.busy_timeout(Duration::ZERO).unwrap();
                    observed
                        .lock()
                        .unwrap()
                        .push(writer.execute_batch("BEGIN IMMEDIATE; ROLLBACK").is_ok());
                }
            }))
        };
        let scope = enter_request_cancellation(&probe);
        coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            budget(8),
        )
        .expect("reconcile the new file");
        drop(scope);
        let observed = lock_free_while_hashing.lock().unwrap();
        assert!(!observed.is_empty(), "the new file was never hashed");
        assert!(
            observed.iter().all(|free| *free),
            "reconciliation held the write lock while hashing: {observed:?}"
        );
        assert_eq!(source_status(&fixture.index(), "lesson.md"), "pending");
    }

    #[test]
    fn searches_extract_a_failed_source_again_only_after_it_changes() {
        let fixture = Fixture::new("failed-not-retried");
        fixture.write("a-garbled.md", b"\xff\xfe not utf-8\n");
        fixture.write("lesson.md", b"lesson\n");
        let search = |likely: &[PathBuf]| {
            prepare_source_query_with_allowlist_and_cancellation(
                &fixture.root,
                &fixture.index(),
                Path::new(""),
                likely,
                budget(8),
                None,
                &OperationCancellation::none(),
            )
            .expect("search")
        };
        search(&[]);
        assert_eq!(source_status(&fixture.index(), "a-garbled.md"), "failed");

        for likely in [vec![], vec![PathBuf::from("a-garbled.md")]] {
            let again = search(&likely);
            assert_ne!(
                again.coverage,
                IndexCoverage::IndexingOnDemand,
                "a search extracted the failed source again (likely paths {likely:?})"
            );
        }
        let explicit = crate::retrieval::prepare_explicit_source(
            &fixture.root,
            Path::new("a-garbled.md"),
            crate::retrieval::FactsInterest::IdentityOnly,
            &OperationCancellation::none(),
        );
        assert!(
            matches!(explicit, Err(ManifestError::Invalid(ref message)) if message.contains("not valid UTF-8")),
            "an explicit request extracts the failed source again: {:?}",
            explicit.err()
        );

        fixture.write("a-garbled.md", b"fixed and readable now\n");
        search(&[]);
        assert_eq!(source_status(&fixture.index(), "a-garbled.md"), "indexed");
    }

    #[test]
    fn preparation_budget_exhaustion_is_partial_but_caller_cancellation_is_an_error() {
        let fixture = Fixture::new("preparation-budget-disposition");
        fixture.write("lesson.md", b"source text\n");
        // Real workspace initialization now provisions the cache explicitly;
        // a deadline-expired read must not create it as a hidden side effect.
        drop(
            crate::retrieval::initialize_index(
                &fixture.index(),
                Duration::from_secs(2),
                &OperationCancellation::none(),
            )
            .unwrap(),
        );
        let exhausted = prepare_source_query_with_allowlist_and_cancellation(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            CoordinatorBudget {
                max_duration: Duration::from_nanos(1),
                ..budget(8)
            },
            None,
            &OperationCancellation::none(),
        )
        .expect("own budget exhaustion keeps a searchable snapshot");
        assert_eq!(
            exhausted.reconciliation,
            super::ReconciliationOutcome::DeferredBudget
        );
        assert_eq!(exhausted.coverage, IndexCoverage::Partial);

        let caller_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let cancelled = prepare_source_query_with_allowlist_and_cancellation(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            CoordinatorBudget {
                max_duration: Duration::from_nanos(1),
                ..budget(8)
            },
            None,
            &OperationCancellation::with_flag(caller_flag),
        );
        assert!(matches!(cancelled, Err(ManifestError::Cancelled)));
    }

    #[test]
    fn resumed_scan_reenumeration_stays_metadata_bounded_and_completes() {
        let fixture = Fixture::new("resume");
        for index in 0..24 {
            fixture.write(&format!("source-{index:02}.md"), b"one bounded file\n");
        }
        let budget = budget(3);
        let mut generation = None;
        for _ in 0..32 {
            let report = coordinate_source_query(
                &fixture.root,
                &fixture.index(),
                Path::new(""),
                &[],
                budget,
            )
            .expect("run bounded metadata batch");
            assert!(report.metrics.metadata_entries_reconciled <= 3);
            assert!(report.metrics.content_reads <= 3);
            generation.get_or_insert(report.generation);
            assert_eq!(generation, Some(report.generation));
            if scan_active(&fixture.index()) == 0 {
                break;
            }
        }
        assert_eq!(scan_active(&fixture.index()), 0, "scan must finish");
        assert_eq!(manifest_count(&fixture.index()), 24);

        fs::remove_file(fixture.root.join("source-23.md")).expect("remove source externally");
        coordinate_source_query(&fixture.root, &fixture.index(), Path::new(""), &[], budget)
            .expect("interrupt replacement scan after one batch");
        fixture.write("finder-mid-scan.pdf", &pdf_fixture());
        assert_eq!(
            present(&fixture.index(), "source-23.md"),
            1,
            "partial scans cannot infer deletion"
        );
        finish_metadata_scan(&fixture, budget);
        assert_eq!(present(&fixture.index(), "source-23.md"), 0);
        assert_eq!(present(&fixture.index(), "finder-mid-scan.pdf"), 1);
    }

    #[test]
    fn bounded_reconciliation_preserves_identity_for_a_verified_rename() {
        let fixture = Fixture::new("rename");
        fixture.write("original.md", b"stable source identity\n");
        fixture.write("duplicate.md", b"stable source identity\n");
        finish_metadata_scan(&fixture, budget(8));
        let original = coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[PathBuf::from("original.md")],
            budget(8),
        )
        .expect("index original source");
        let original_manifest_id = manifest_id(&fixture.index(), "original.md");
        let duplicate_manifest_id = manifest_id(&fixture.index(), "duplicate.md");
        let original_candidate_id = original.candidates[0].candidate_id.clone();

        fs::rename(
            fixture.root.join("original.md"),
            fixture.root.join("renamed.md"),
        )
        .expect("rename source externally");
        let renamed = coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[PathBuf::from("renamed.md")],
            budget(8),
        )
        .expect("reconcile verified rename");

        assert_eq!(
            manifest_id(&fixture.index(), "renamed.md"),
            original_manifest_id
        );
        assert_eq!(
            manifest_id(&fixture.index(), "duplicate.md"),
            duplicate_manifest_id
        );
        assert_ne!(original_manifest_id, duplicate_manifest_id);
        assert_eq!(renamed.candidates[0].candidate_id, original_candidate_id);
        assert_eq!(renamed.candidates[0].path, "renamed.md");
    }

    #[test]
    fn coverage_names_complete_partial_stale_and_indexing_on_demand() {
        let empty = Fixture::new("empty-coverage");
        let empty_partial = coordinate_source_query(
            &empty.root,
            &empty.index(),
            Path::new(""),
            &[],
            CoordinatorBudget {
                max_duration: Duration::ZERO,
                ..budget(1)
            },
        )
        .expect("report empty active scan as partial");
        assert_eq!(empty_partial.coverage, IndexCoverage::Partial);

        let fixture = Fixture::new("coverage");
        fixture.write("only.md", b"coverage states\n");
        let partial = coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            budget(1),
        )
        .expect("start partial scan");
        assert_eq!(partial.coverage, IndexCoverage::Partial);
        assert_eq!(partial.coverage.as_str(), "partial");
        finish_metadata_scan(&fixture, budget(4));

        let on_demand = coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[PathBuf::from("only.md")],
            budget(4),
        )
        .expect("index pending source");
        assert_eq!(on_demand.coverage, IndexCoverage::IndexingOnDemand);
        assert_eq!(on_demand.coverage.as_str(), "indexing_on_demand");
        finish_metadata_scan(&fixture, budget(64));
        let complete = coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            budget(64),
        )
        .expect("complete warm query");
        assert_eq!(complete.coverage, IndexCoverage::Complete);
        assert_eq!(complete.coverage.as_str(), "complete");

        Connection::open(fixture.index())
            .expect("open source index")
            .execute(
                "UPDATE source_manifest SET index_status = 'stale'
                 WHERE display_path = 'only.md'",
                [],
            )
            .expect("mark source stale");
        let stale = coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            CoordinatorBudget {
                max_duration: Duration::ZERO,
                ..budget(1)
            },
        )
        .expect("report stale coverage");
        assert_eq!(stale.coverage, IndexCoverage::Stale);
        assert_eq!(stale.coverage.as_str(), "stale");
    }

    #[test]
    fn cursor_and_manifest_advance_atomically_under_competing_resumes() {
        let fixture = Fixture::new("atomic-cursor");
        let initial =
            r#"{"version":1,"directories":[{"relative_path":[],"processed_entries":[]}]}"#;
        let state =
            load_or_start_reconciliation(&fixture.index(), initial, &OperationCancellation::none())
                .expect("start reconciliation state");
        reconcile_batch(
            &fixture.root,
            &fixture.index(),
            Vec::new(),
            &state,
            Some("{\"version\":1,\"directories\":[]}"),
        )
        .expect("advance first cursor owner");
        let error = reconcile_batch(
            &fixture.root,
            &fixture.index(),
            Vec::new(),
            &state,
            Some("{\"version\":1,\"directories\":[]}"),
        )
        .expect_err("reject stale competing cursor owner");
        assert!(error.to_string().contains("changed concurrently"));
    }

    #[test]
    fn parallel_queries_recover_from_reconciliation_conflicts() {
        let fixture = Fixture::new("parallel-recovery");
        for index in 0..48 {
            fixture.write(
                &format!("source-{index:02}.md"),
                b"parallel recovery source\n",
            );
        }
        // Seed the shared index once, then let all four passes load the same
        // cursor state and race to commit. First-time schema creation is not
        // the race under test and is not claimed as repaired here.
        load_or_start_reconciliation(
            &fixture.index(),
            r#"{"version":1,"directories":[{"relative_path":[],"processed_entries":[]}]}"#,
            &OperationCancellation::none(),
        )
        .expect("initialize shared source index");
        let gate = std::sync::Arc::new(OnceGate::new(4));
        let _hook = install_cursor_load_hook(
            &fixture.index(),
            std::sync::Arc::new({
                let gate = std::sync::Arc::clone(&gate);
                move |_root: &Path, _index: &Path, _attempt: usize| gate.wait()
            }),
        );
        let run_budget = CoordinatorBudget {
            max_metadata_entries: 8,
            max_duration: Duration::from_secs(5),
            max_on_demand: 0,
        };
        let reports = std::thread::scope(|scope| {
            let queries = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        coordinate_source_query(
                            &fixture.root,
                            &fixture.index(),
                            Path::new(""),
                            &[],
                            run_budget,
                        )
                        .expect("a concurrent query must not surface a raw retry instruction")
                    })
                })
                .collect::<Vec<_>>();
            queries
                .into_iter()
                .map(|query| query.join().expect("join concurrent query"))
                .collect::<Vec<_>>()
        });
        assert!(
            reports
                .iter()
                .any(|report| report.reconciliation
                    == ReconciliationOutcome::DeferredConcurrentUpdate),
            "the four-way race must defer at least one reconciliation"
        );
        for report in &reports {
            assert!(
                report.metrics.metadata_entries_reconciled <= run_budget.max_metadata_entries,
                "a run must never exceed its shared metadata budget"
            );
            if report.reconciliation.deferred() {
                assert!(
                    matches!(
                        report.coverage,
                        IndexCoverage::Partial | IndexCoverage::Stale
                    ),
                    "a deferred pass must not report complete or on-demand coverage"
                );
            }
        }
        finish_metadata_scan(&fixture, budget(64));
        assert_eq!(manifest_count(&fixture.index()), 48);
    }

    #[test]
    fn second_conflict_consumes_no_fresh_budget_and_does_not_spin() {
        let fixture = Fixture::new("forced-conflict");
        for index in 0..40 {
            fixture.write(
                &format!("source-{index:02}.md"),
                b"forced conflict source\n",
            );
        }
        let passes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let _hook = install_cursor_load_hook(
            &fixture.index(),
            std::sync::Arc::new({
                let passes = std::sync::Arc::clone(&passes);
                move |root: &Path, index: &Path, _attempt: usize| {
                    passes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    advance_persisted_cursor(root, index);
                }
            }),
        );
        let run_budget = CoordinatorBudget {
            max_metadata_entries: 64,
            max_duration: Duration::from_secs(5),
            max_on_demand: 0,
        };
        let report = coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            run_budget,
        )
        .expect("a repeated conflict returns a bounded deferred result");
        assert!(report.reconciliation.deferred());
        assert_eq!(
            passes.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "a second forced conflict must not retry again"
        );
        assert_eq!(report.metrics.reconciliation_attempts, 2);
        assert!(
            report.metrics.metadata_entries_reconciled <= run_budget.max_metadata_entries,
            "the retry must not receive a fresh metadata budget"
        );
        assert!(matches!(
            report.coverage,
            IndexCoverage::Partial | IndexCoverage::Stale
        ));
    }

    #[test]
    fn completed_scan_cannot_finalize_a_stale_cursor() {
        let fixture = Fixture::new("stale-completion");
        let initial =
            r#"{"version":1,"directories":[{"relative_path":[],"processed_entries":[]}]}"#;
        let state =
            load_or_start_reconciliation(&fixture.index(), initial, &OperationCancellation::none())
                .expect("start reconciliation state");
        reconcile_batch(
            &fixture.root,
            &fixture.index(),
            Vec::new(),
            &state,
            Some("{\"version\":1,\"directories\":[]}"),
        )
        .expect("advance competing cursor");
        let error = reconcile_batch(&fixture.root, &fixture.index(), Vec::new(), &state, None)
            .expect_err("completion must not apply a stale cursor");
        assert!(matches!(error, ManifestError::ReconciliationConflict));
    }

    #[test]
    fn cancelled_query_does_not_retry_or_advance_the_cursor() {
        let fixture = Fixture::new("cancel-retry");
        for index in 0..8 {
            fixture.write(&format!("source-{index}.md"), b"cancelled retry source\n");
        }
        coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            budget(1),
        )
        .expect("start bounded scan");
        let before = persisted_cursor(&fixture.index());
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let passes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let _hook = install_cursor_load_hook(
            &fixture.index(),
            std::sync::Arc::new({
                let flag = std::sync::Arc::clone(&flag);
                let passes = std::sync::Arc::clone(&passes);
                move |_root: &Path, _index: &Path, _attempt: usize| {
                    passes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    flag.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }),
        );
        let cancellation = OperationCancellation::with_flag(std::sync::Arc::clone(&flag));
        let error = coordinate_source_query_with_allowlist_and_cancellation(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            budget(8),
            None,
            &cancellation,
        )
        .expect_err("a cancelled run must abort");
        assert!(matches!(error, ManifestError::Cancelled), "{error}");
        assert_eq!(
            passes.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "cancellation must not retry"
        );
        assert_eq!(
            persisted_cursor(&fixture.index()),
            before,
            "cancellation must not write a later cursor"
        );
    }

    #[test]
    fn deferred_allowlist_pass_keeps_authorization_and_partial_coverage() {
        let fixture = Fixture::new("deferred-allowlist");
        fixture.write("approved.md", b"deferred allowlist approved\n");
        fixture.write("unapproved.md", b"deferred allowlist unapproved\n");
        let allowed = BTreeSet::from([PathBuf::from("approved.md")]);
        let _hook = install_cursor_load_hook(
            &fixture.index(),
            std::sync::Arc::new(|root: &Path, index: &Path, _attempt: usize| {
                advance_persisted_cursor(root, index);
            }),
        );
        let report = coordinate_source_query_with_allowlist_and_cancellation(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            budget(8),
            Some(&allowed),
            &OperationCancellation::none(),
        )
        .expect("a deferred allowlist query returns a bounded result");
        assert!(report.reconciliation.deferred());
        assert!(matches!(
            report.coverage,
            IndexCoverage::Partial | IndexCoverage::Stale
        ));
        assert_no_manifest_row(&fixture.index(), "unapproved.md");
    }

    #[test]
    fn deferred_search_serves_only_the_persisted_snapshot() {
        let fixture = Fixture::new("deferred-snapshot");
        fixture.write("lesson.md", b"persisted deferred source\n");
        let indexed = coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[PathBuf::from("lesson.md")],
            budget(8),
        )
        .expect("index source");
        assert_eq!(indexed.candidates.len(), 1);
        let persisted_hash = indexed.candidates[0].content_sha256.clone();

        // A competing writer changes the source and advances the cursor while
        // this pass is in flight; the failed pass's view must be discarded.
        let _hook = install_cursor_load_hook(
            &fixture.index(),
            std::sync::Arc::new(|root: &Path, index: &Path, _attempt: usize| {
                fs::write(root.join("lesson.md"), b"changed after the failed pass\n")
                    .expect("change source between passes");
                advance_persisted_cursor(root, index);
            }),
        );
        let request = crate::retrieval::search::SourceSearchRequest {
            query: "persisted deferred source".to_owned(),
            path_prefix: None,
            limit: 10,
        };
        let report =
            crate::retrieval::search::search_workspace_sources_with_allowlist_and_cancellation(
                &fixture.root,
                &fixture.index(),
                Path::new(""),
                &request,
                budget(8),
                None,
                &OperationCancellation::none(),
            )
            .expect("a deferred search returns a bounded result");
        assert!(report.reconciliation.deferred());
        assert!(matches!(
            report.coverage,
            IndexCoverage::Partial | IndexCoverage::Stale
        ));
        assert!(
            report
                .hits
                .iter()
                .all(|hit| hit.candidate.content_sha256 == persisted_hash
                    && hit.candidate.body.contains("persisted deferred source")),
            "only the persisted snapshot may be served: {:?}",
            report.hits
        );
    }

    #[test]
    fn empty_index_deferred_reports_partial_not_complete() {
        let fixture = Fixture::new("empty-deferred");
        let _hook = install_cursor_load_hook(
            &fixture.index(),
            std::sync::Arc::new(|root: &Path, index: &Path, _attempt: usize| {
                advance_persisted_cursor(root, index);
            }),
        );
        let report = coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            budget(4),
        )
        .expect("an empty deferred query returns a bounded result");
        assert!(report.reconciliation.deferred(), "{report:?}");
        assert_eq!(report.coverage, IndexCoverage::Partial);
        assert!(report.candidates.is_empty());
        assert_eq!(report.metrics.reconciliation_attempts, 2);
    }

    #[test]
    fn query_scope_cannot_expand_likely_paths_or_cross_nested_repository_boundaries() {
        let fixture = Fixture::new("scope-authority");
        fixture.write("docs/allowed.md", b"allowed\n");
        fixture.write("other/forbidden.md", b"forbidden\n");
        fixture.write("nested/.git/HEAD", b"ref: refs/heads/main\n");
        fixture.write("nested/private.md", b"nested private\n");

        let error = coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new("docs"),
            &[PathBuf::from("other/forbidden.md")],
            budget(4),
        )
        .expect_err("reject likely path outside query scope");
        assert!(error.to_string().contains("outside query scope"));
        assert!(
            !fixture.index().exists(),
            "invalid query must not open the index"
        );

        coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new("nested"),
            &[],
            budget(4),
        )
        .expect("record nested repository exclusion");
        let nested_rows: i64 = Connection::open(fixture.index())
            .expect("open source index")
            .query_row(
                "SELECT count(*) FROM source_manifest
                 WHERE display_path = 'nested/private.md'",
                [],
                |row| row.get(0),
            )
            .expect("count nested private rows");
        assert_eq!(nested_rows, 0);
    }

    #[test]
    fn large_repository_cold_and_warm_queries_stay_within_bounded_costs() {
        let fixture = Fixture::new("large-metrics");
        let body = vec![b'x'; 256];
        for index in 0..512 {
            fixture.write(&format!("source-{index:03}.md"), &body);
        }
        let query_budget = CoordinatorBudget {
            max_metadata_entries: 64,
            max_duration: Duration::from_millis(500),
            max_on_demand: 1,
        };
        let selected = [PathBuf::from("source-511.md")];
        let cold = coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &selected,
            query_budget,
        )
        .expect("run cold large-repository query");
        assert_eq!(cold.coverage, IndexCoverage::IndexingOnDemand);
        assert!(cold.metrics.metadata_entries_reconciled <= 64);
        assert!(cold.metrics.content_reads <= 65);
        assert!(cold.metrics.bytes_read <= 65 * 256);
        assert_eq!(cold.candidates.len(), 1);

        finish_metadata_scan(&fixture, query_budget);
        let warm = coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &selected,
            query_budget,
        )
        .expect("run warm large-repository query");
        assert!(warm.metrics.metadata_entries_reconciled <= 64);
        assert_eq!(warm.metrics.indexed_on_demand, 0);
        assert_eq!(warm.candidates.len(), 1);
    }

    #[test]
    fn allowlisted_reconciliation_invalidates_exact_missing_paths() {
        let fixture = Fixture::new("allowlist-missing");
        for index in 0..2 {
            fixture.write(
                &format!("approved-{index:03}.md"),
                format!("approved source {index}\n").as_bytes(),
            );
        }
        fixture.write(
            "unapproved.md",
            b"must never enter restricted reconciliation\n",
        );
        let allowed = (0..2)
            .map(|index| PathBuf::from(format!("approved-{index:03}.md")))
            .collect::<BTreeSet<_>>();
        let restricted_budget = CoordinatorBudget {
            max_metadata_entries: 128,
            max_duration: Duration::from_secs(10),
            max_on_demand: 4,
        };

        let first = coordinate_source_query_with_allowlist(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            restricted_budget,
            Some(&allowed),
        )
        .expect("reconcile the approved sources");
        assert!(
            first
                .candidates
                .iter()
                .any(|candidate| candidate.path == "approved-000.md"),
            "approved sources are indexed on demand"
        );
        assert_no_manifest_row(&fixture.index(), "unapproved.md");

        fs::remove_file(fixture.root.join("approved-000.md")).expect("remove approved source");
        let missing = coordinate_source_query_with_allowlist(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            restricted_budget,
            Some(&allowed),
        )
        .expect("reconcile missing approved path");
        assert!(missing.metrics.metadata_entries_reconciled <= 128);
        let state: (String, String, i64) = Connection::open(fixture.index())
            .expect("open restricted reconciliation index")
            .query_row(
                "SELECT source_kind, index_status, present
                 FROM source_manifest WHERE display_path = 'approved-000.md'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("read missing approved path state");
        assert_eq!(state.0, "text");
        assert_eq!(state.1, "deleted");
        assert_eq!(state.2, 0);
        let stale_candidates: i64 = Connection::open(fixture.index())
            .expect("reopen restricted reconciliation index")
            .query_row(
                "SELECT count(*) FROM source_candidates candidate
                 JOIN source_manifest manifest ON manifest.manifest_id = candidate.manifest_id
                 WHERE manifest.display_path = 'approved-000.md' AND candidate.status = 'current'",
                [],
                |row| row.get(0),
            )
            .expect("count invalidated candidates");
        assert_eq!(stale_candidates, 0);
        assert_no_manifest_row(&fixture.index(), "unapproved.md");
    }

    #[test]
    fn text_deferred_discovery_before_manifest_resumes_with_exact_source_identity() {
        use crate::retrieval::{RetrievalRequest, SourceSearchRequest, retrieve_workspace_sources};
        use sha2::{Digest, Sha256};
        use std::sync::Arc;
        let fixture = Fixture::new("text-before-manifest");
        let text = b"Readers pin the end mark while writers append frames.\n";
        fixture.write("lesson.md", text);
        fixture.write(
            &format!("{}/config.toml", crate::workspace::CONTROL_DIR),
            b"schema_version = 1\nworkspace_id = \"fixture\"\n",
        );
        // Initialize only the real resumable cursor; no fixture SQL inserts.
        coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            CoordinatorBudget {
                max_duration: Duration::ZERO,
                ..budget(8)
            },
        )
        .unwrap();
        assert_no_manifest_row(&fixture.index(), "lesson.md");
        let mut request = RetrievalRequest {
            active_workspace_id: "fixture".into(),
            lexical: SourceSearchRequest {
                query: "end mark".into(),
                path_prefix: Some(PathBuf::from("lesson.md")),
                limit: 20,
            },
        };
        {
            // Existing deterministic competing-owner hook forces both real
            // passes to lose the cursor CAS. Its normal reconciliation API
            // advances the cursor, without sleeps or direct SQL mutation.
            let _hook = install_cursor_load_hook(
                &fixture.index(),
                Arc::new(|root, index, _attempt| advance_persisted_cursor(root, index)),
            );
            let held = retrieve_workspace_sources(
                &fixture.root,
                &fixture.index(),
                Path::new(""),
                &request,
                budget(8),
            )
            .unwrap();
            assert_eq!(
                held.reconciliation,
                ReconciliationOutcome::DeferredConcurrentUpdate
            );
            assert_eq!(held.coverage, IndexCoverage::Partial);
            assert!(held.hits.is_empty());
            assert_no_manifest_row(&fixture.index(), "lesson.md");
        }
        request.lexical.query = "lesson.md".into();
        let prepared = retrieve_workspace_sources(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &request,
            budget(8),
        )
        .unwrap();
        assert!(!prepared.hits.is_empty());
        let hash = Sha256::digest(text)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert!(
            prepared
                .hits
                .iter()
                .all(|hit| hit.candidate.content_sha256 == hash)
        );
        // The original semantic request remains one final retrieval oracle.
        request.lexical.query = "end mark".into();
        let searched = retrieve_workspace_sources(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &request,
            budget(8),
        )
        .unwrap();
        assert_eq!(searched.hits.len(), 1);
        assert_eq!(searched.hits[0].candidate.path, "lesson.md");
        assert_eq!(searched.hits[0].candidate.content_sha256, hash);
        assert_eq!(searched.hits[0].candidate.anchor_kind.as_str(), "line");
        assert!(searched.hits[0].candidate.body.contains("end mark"));
        fixture.write(
            &format!("{}/magnesiumignore", crate::workspace::CONTROL_DIR),
            b"lesson.md\n",
        );
        let excluded = retrieve_workspace_sources(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &request,
            budget(8),
        )
        .unwrap();
        assert!(excluded.hits.is_empty());
    }

    fn budget(max_metadata_entries: usize) -> CoordinatorBudget {
        CoordinatorBudget {
            max_metadata_entries,
            max_duration: Duration::from_secs(5),
            max_on_demand: 4,
        }
    }

    /// Deterministic competing writer: reload the persisted cursor and advance
    /// it so the pass that loaded the previous value must conflict.
    fn advance_persisted_cursor(root: &Path, index: &Path) {
        let initial = r#"{"version":1,"directories":[]}"#;
        let state = load_or_start_reconciliation(index, initial, &OperationCancellation::none())
            .expect("load competing cursor");
        let mut cursor: serde_json::Value =
            serde_json::from_str(&state.cursor_json).expect("parse competing cursor");
        // Serde ignores this test-only field; it only makes the cursor differ.
        let writes = cursor["competing_writes"].as_u64().unwrap_or(0);
        cursor["competing_writes"] = serde_json::json!(writes + 1);
        reconcile_batch(root, index, Vec::new(), &state, Some(&cursor.to_string()))
            .expect("advance competing cursor");
    }

    fn persisted_cursor(index: &Path) -> (i64, String) {
        Connection::open(index)
            .expect("open source index")
            .query_row(
                "SELECT scan_generation, cursor_json
                 FROM source_reconciliation_state WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read persisted reconciliation cursor")
    }

    #[test]
    fn cancelled_preparation_aborts_before_holding_canonical_work() {
        use std::sync::{Arc, atomic::AtomicBool};
        let fixture = Fixture::new("cancelled-prepare");
        fixture.write("lesson.md", b"cancelled preparation must not index\n");
        let flag = Arc::new(AtomicBool::new(true));
        let cancellation = OperationCancellation::with_flag(flag);
        let error = coordinate_source_query_with_allowlist_and_cancellation(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            budget(4),
            None,
            &cancellation,
        )
        .expect_err("pre-cancelled preparation must abort");
        assert!(
            matches!(error, super::super::manifest::ManifestError::Cancelled),
            "unexpected preparation error: {error}"
        );
    }

    #[test]
    fn expired_deadline_aborts_traversal_without_partial_candidates() {
        let fixture = Fixture::new("expired-deadline");
        fixture.write("lesson.md", b"expired deadline must abort\n");
        let past = std::time::Instant::now() - Duration::from_secs(1);
        let cancellation = OperationCancellation::with_deadline(past);
        let error = coordinate_source_query_with_allowlist_and_cancellation(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[],
            budget(4),
            None,
            &cancellation,
        )
        .expect_err("expired preparation must abort");
        assert!(
            matches!(error, super::super::manifest::ManifestError::Cancelled),
            "unexpected deadline error: {error}"
        );
    }

    #[test]
    fn request_cancellation_scope_is_observed_by_none_tokens_and_restored() {
        use std::sync::{Arc, atomic::AtomicBool};
        let flag = Arc::new(AtomicBool::new(false));
        let request = OperationCancellation::with_flag(Arc::clone(&flag));
        // Outside a worker scope, a helper `none()` token is never cancelled.
        assert!(!OperationCancellation::none().is_cancelled());
        {
            let _scope = enter_request_cancellation(&request);
            assert!(
                !OperationCancellation::none().is_cancelled(),
                "an uncancelled request must not poison nested checks"
            );
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
            assert!(
                OperationCancellation::none().is_cancelled(),
                "nested checks must observe the request cancellation"
            );
            assert!(OperationCancellation::none().check().is_err());
        }
        // The guard restores the previous scope so pooled blocking threads do
        // not inherit stale cancellation.
        assert!(!OperationCancellation::none().is_cancelled());
        let later = Arc::new(AtomicBool::new(false));
        let _later_scope = enter_request_cancellation(&OperationCancellation::with_flag(later));
        assert!(!OperationCancellation::none().is_cancelled());
    }

    fn finish_metadata_scan(fixture: &Fixture, budget: CoordinatorBudget) {
        for _ in 0..128 {
            coordinate_source_query(&fixture.root, &fixture.index(), Path::new(""), &[], budget)
                .expect("resume bounded metadata scan");
            if scan_active(&fixture.index()) == 0 {
                return;
            }
        }
        panic!("bounded metadata scan did not complete");
    }

    fn scan_active(index: &Path) -> i64 {
        Connection::open(index)
            .expect("open source index")
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM source_reconciliation_state)",
                [],
                |row| row.get(0),
            )
            .expect("read scan state")
    }

    fn source_status(index: &Path, display_path: &str) -> String {
        Connection::open(index)
            .expect("open source index")
            .query_row(
                "SELECT index_status FROM source_manifest WHERE display_path = ?1",
                [display_path],
                |row| row.get(0),
            )
            .expect("read source status")
    }

    fn manifest_count(index: &Path) -> i64 {
        Connection::open(index)
            .expect("open source index")
            .query_row(
                "SELECT count(*) FROM source_manifest
                 WHERE present = 1 AND source_kind IN ('text', 'pdf')",
                [],
                |row| row.get(0),
            )
            .expect("count manifest")
    }

    fn manifest_id(index: &Path, display_path: &str) -> i64 {
        Connection::open(index)
            .expect("open source index")
            .query_row(
                "SELECT manifest_id FROM source_manifest
                 WHERE present = 1 AND display_path = ?1",
                [display_path],
                |row| row.get(0),
            )
            .expect("read manifest ID")
    }

    fn present(index: &Path, display_path: &str) -> i64 {
        Connection::open(index)
            .expect("open source index")
            .query_row(
                "SELECT present FROM source_manifest WHERE display_path = ?1",
                [display_path],
                |row| row.get(0),
            )
            .expect("read manifest presence")
    }

    fn assert_no_manifest_row(index: &Path, display_path: &str) {
        let count: i64 = Connection::open(index)
            .expect("open source index")
            .query_row(
                "SELECT count(*) FROM source_manifest WHERE display_path = ?1",
                [display_path],
                |row| row.get(0),
            )
            .expect("count manifest rows");
        assert_eq!(count, 0, "unapproved path entered source manifest");
    }

    fn pdf_fixture() -> Vec<u8> {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let resources_id = document.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 18.into()]),
                Operation::new("Tj", vec![Object::string_literal("Finder PDF")]),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id = document.add_object(Stream::new(
            dictionary! {},
            content.encode().expect("encode PDF fixture"),
        ));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("serialize PDF fixture");
        bytes
    }

    fn exact_snapshot(
        fixture: &Fixture,
        path: &str,
    ) -> super::super::manifest::status::SelectedSourceSnapshot {
        for _ in 0..3 {
            let snapshot = crate::retrieval::prepare_explicit_source(
                &fixture.root,
                Path::new(path),
                crate::retrieval::FactsInterest::IdentityOnly,
                &OperationCancellation::none(),
            )
            .unwrap();
            if matches!(
                snapshot.preparation,
                crate::retrieval::PreparationDisposition::DeferredBudget
                    | crate::retrieval::PreparationDisposition::DeferredConcurrentUpdate
            ) {
                continue;
            }
            return snapshot;
        }
        panic!("exact preparation remained deferred after three bounded calls");
    }

    #[test]
    fn exact_missing_known_source_is_deleted_and_unknown_absence_creates_no_row() {
        let fixture = Fixture::new("exact-deletion");
        fixture.write("selected.md", b"selected exact source\n");
        let original = exact_snapshot(&fixture, "selected.md").row.unwrap();
        assert_eq!(original.index_status, "indexed");
        fs::remove_file(fixture.root.join("selected.md")).unwrap();
        let deleted = exact_snapshot(&fixture, "selected.md").row.unwrap();
        assert_eq!(deleted.manifest_id, original.manifest_id);
        assert_eq!(deleted.content_sha256, original.content_sha256);
        assert_eq!(deleted.source_kind, "text");
        assert_eq!(deleted.index_status, "deleted");
        assert!(!deleted.present);
        assert!(deleted.last_error.is_none());
        assert!(exact_snapshot(&fixture, "never-known.md").row.is_none());
        let connection = Connection::open(fixture.index()).unwrap();
        let facts: (i64, i64, i64, i64) = connection
            .query_row(
                "SELECT (SELECT count(*) FROM source_manifest),
                    (SELECT count(*) FROM source_candidates WHERE status = 'current'),
                    (SELECT count(*) FROM source_search),
                    (SELECT count(*) FROM source_search_ranked)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(facts, (1, 0, 0, 0));
    }

    #[test]
    fn exact_absence_reappearing_at_precommit_rolls_back_the_tombstone() {
        use std::sync::atomic::AtomicBool;
        let fixture = Fixture::new("exact-reappearance");
        fixture.write("selected.md", b"original selected source\n");
        let original = exact_snapshot(&fixture, "selected.md").row.unwrap();
        fs::remove_file(fixture.root.join("selected.md")).unwrap();
        let path = fixture.root.join("selected.md");
        let wrote = Arc::new(AtomicBool::new(false));
        let wrote_hook = wrote.clone();
        let token = OperationCancellation::none().with_test_hook(Arc::new(move |checkpoint| {
            if checkpoint == "source_reconciliation_before_commit"
                && !wrote_hook.swap(true, Ordering::Relaxed)
            {
                fs::write(&path, b"reappeared selected source\n").unwrap();
            }
        }));
        let _scope = enter_request_cancellation(&token);
        let result = crate::retrieval::prepare_explicit_source(
            &fixture.root,
            Path::new("selected.md"),
            crate::retrieval::FactsInterest::IdentityOnly,
            &token,
        );
        assert!(matches!(result, Err(ManifestError::SourceSuperseded)));
        assert!(wrote.load(Ordering::Relaxed));
        let row = super::super::manifest::status::read_manifest_row(
            &fixture.index(),
            Path::new("selected.md"),
        )
        .unwrap()
        .unwrap();
        assert!(row.present);
        assert_eq!(row.index_status, "indexed");
        assert_eq!(row.source_revision, original.source_revision);
        assert_eq!(row.content_sha256, original.content_sha256);
    }

    #[test]
    fn exact_reconciliation_keeps_permission_and_other_io_distinct_from_absence() {
        let fixture = Fixture::new("exact-io");
        fixture.write("selected.md", b"readable selected source\n");
        let original = exact_snapshot(&fixture, "selected.md").row.unwrap();
        for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::Other] {
            let state = load_or_start_reconciliation(
                &fixture.index(),
                "unused",
                &OperationCancellation::none(),
            )
            .unwrap();
            let result = super::super::manifest::reconcile::reconcile_batch_with_policy(
                &fixture.root,
                &fixture.index(),
                vec![super::super::DiscoveryEntry {
                    relative_path: PathBuf::from("selected.md"),
                    size: None,
                    disposition: super::super::DiscoveryDisposition::Excluded(
                        super::super::ExclusionReason::Unreadable(kind),
                    ),
                }],
                &state,
                super::super::manifest::reconcile::ReconciliationAdvance::PreserveSelected,
                &OperationCancellation::none(),
            );
            assert!(
                matches!(result, Err(ManifestError::Io { source, .. }) if source.kind() == kind)
            );
            let row = super::super::manifest::status::read_manifest_row(
                &fixture.index(),
                Path::new("selected.md"),
            )
            .unwrap()
            .unwrap();
            assert_eq!(row.index_status, "indexed");
            assert_eq!(row.source_revision, original.source_revision);
        }
    }

    #[test]
    fn exact_preparation_preserves_active_cursor_and_unrelated_missing_hash_identity() {
        let fixture = Fixture::new("exact-cursor-isolation");
        fixture.write("keeper.md", b"matching source content\n");
        let scan_budget = CoordinatorBudget {
            max_duration: Duration::from_secs(10),
            ..CoordinatorBudget::default()
        };
        coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[PathBuf::from("keeper.md")],
            scan_budget,
        )
        .unwrap();
        let finish_active_scan = || {
            for _ in 0..3 {
                if scan_active(&fixture.index()) == 0 {
                    break;
                }
                coordinate_source_query(
                    &fixture.root,
                    &fixture.index(),
                    Path::new(""),
                    &[],
                    scan_budget,
                )
                .unwrap();
            }
            assert_eq!(
                scan_active(&fixture.index()),
                0,
                "scan remained active after three bounded continuations"
            );
        };
        finish_active_scan();
        let keeper = super::super::manifest::status::read_manifest_row(
            &fixture.index(),
            Path::new("keeper.md"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(keeper.index_status, "indexed");
        let keeper_observed: i64 = Connection::open(fixture.index())
            .unwrap()
            .query_row(
                "SELECT observed_generation FROM source_manifest WHERE manifest_id = ?1",
                [keeper.manifest_id],
                |row| row.get(0),
            )
            .unwrap();
        let initial_cursor = serde_json::to_string(&super::ScanCursor {
            version: super::INITIAL_CURSOR_VERSION,
            directories: vec![super::DirectoryCursor {
                relative_path: crate::workspace::native_path_key(Path::new("")),
                processed_entries: Vec::new(),
            }],
        })
        .unwrap();
        let state = load_or_start_reconciliation(
            &fixture.index(),
            &initial_cursor,
            &OperationCancellation::none(),
        )
        .unwrap();
        assert!(state.generation > keeper_observed);
        fs::remove_file(fixture.root.join("keeper.md")).unwrap();
        fixture.write("selected.md", b"matching source content\n");
        let selected = exact_snapshot(&fixture, "selected.md").row.unwrap();
        assert_ne!(selected.manifest_id, keeper.manifest_id);
        assert_eq!(selected.index_status, "indexed");
        let after = load_or_start_reconciliation(
            &fixture.index(),
            &initial_cursor,
            &OperationCancellation::none(),
        )
        .unwrap();
        assert_eq!(after, state);
        let untouched = super::super::manifest::status::read_manifest_row(
            &fixture.index(),
            Path::new("keeper.md"),
        )
        .unwrap()
        .unwrap();
        assert!(untouched.present);
        assert_eq!(untouched.source_revision, keeper.source_revision);
        let observed: i64 = Connection::open(fixture.index())
            .unwrap()
            .query_row(
                "SELECT observed_generation FROM source_manifest WHERE manifest_id = ?1",
                [selected.manifest_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(observed, state.generation);
        finish_active_scan();
        let completed = super::super::manifest::status::read_manifest_row(
            &fixture.index(),
            Path::new("selected.md"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(completed.manifest_id, selected.manifest_id);
        assert_eq!(completed.index_status, "indexed");
        assert!(completed.present);
        assert_eq!(
            super::super::manifest::status::read_manifest_row(
                &fixture.index(),
                Path::new("keeper.md")
            )
            .unwrap()
            .unwrap()
            .index_status,
            "deleted"
        );
    }

    #[test]
    fn exact_preparation_cancellation_at_prepare_hash_and_candidate_scan_stops_work() {
        use std::sync::atomic::AtomicBool;
        for checkpoint in [
            "explicit_source_before_prepare",
            "source_hash_read_chunk",
            "selected_candidate_scan",
        ] {
            let fixture = Fixture::new("exact-cancel");
            fixture.write("selected.md", b"exact cancellation selected source\n");
            if checkpoint == "selected_candidate_scan" {
                exact_snapshot(&fixture, "selected.md");
            }
            let flag = Arc::new(AtomicBool::new(false));
            let flag_hook = flag.clone();
            let token = OperationCancellation::with_flag(flag.clone()).with_test_hook(Arc::new(
                move |observed| {
                    if observed == checkpoint {
                        flag_hook.store(true, Ordering::Relaxed);
                    }
                },
            ));
            let _scope = enter_request_cancellation(&token);
            let result = crate::retrieval::prepare_explicit_source(
                &fixture.root,
                Path::new("selected.md"),
                crate::retrieval::FactsInterest::Range {
                    start: 1,
                    end: Some(1),
                    include_excerpt: true,
                },
                &token,
            );
            assert!(
                matches!(result, Err(ManifestError::Cancelled)),
                "{checkpoint}"
            );
            assert!(
                flag.load(Ordering::Relaxed),
                "checkpoint was reached: {checkpoint}"
            );
        }
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let sequence = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("test clock after epoch")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "magnesium-coordinator-{}-{name}-{nanos}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(root.join(crate::workspace::CONTROL_DIR).join("index"))
                .expect("create fixture index directory");
            Self { root }
        }

        fn index(&self) -> PathBuf {
            self.root
                .join(crate::workspace::CONTROL_DIR)
                .join("index/source-search.sqlite")
        }

        fn write(&self, relative: &str, contents: &[u8]) {
            let path = self.root.join(relative);
            fs::create_dir_all(path.parent().expect("fixture parent"))
                .expect("create fixture parent");
            fs::write(path, contents).expect("write fixture source");
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
