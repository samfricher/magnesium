//! Background indexing after a search. A search indexes only what fits its own
//! small budget, so the MCP server then keeps one thread per workspace indexing
//! until nothing is left.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::coordinator::{
    CoordinatorBudget, ReconciliationOutcome, extract_for_search,
    prepare_source_query_with_allowlist_and_cancellation,
};
use super::manifest::{IndexingProgress, PendingSource, indexing_progress};
use super::{ManifestError, OperationCancellation};
use crate::request::enter_request_cancellation;

/// A cancelled worker kills its PDF extractor within one 10 ms poll. The host
/// allows the server about a second to exit after its input closes.
const SHUTDOWN_WAIT: Duration = Duration::from_millis(500);
const SHUTDOWN_POLL: Duration = Duration::from_millis(5);
const CONTENDED_PAUSE: Duration = Duration::from_millis(50);
const EXTRACT_WINDOW: usize = 64;

/// One discovery batch. Small, so its files are extracted and searchable before
/// the next batch hashes them. The deadline leaves room to hash 32 files at the
/// 64 MB PDF limit on a slow disk, so a batch is never rolled back. The batch
/// extracts nothing; extraction publishes one file at a time.
const DISCOVERY: CoordinatorBudget = CoordinatorBudget {
    max_metadata_entries: 32,
    max_duration: Duration::from_secs(120),
    max_on_demand: 0,
};

/// Pending sources catch-up could not finish: a PDF slower than its deadline,
/// or a file that vanished, changed or became excluded after discovery. Each
/// stays pending, so without this set every search would restart catch-up on
/// the same file. The key holds the manifest row's size and modified time, so
/// a file is tried again once a recheck records a change.
pub(super) type Skipped = HashSet<PendingSource>;

/// What one read of the source index says catch-up does next. Extraction
/// comes first, so the files one discovery batch found become searchable
/// before the next batch.
#[derive(PartialEq, Eq)]
enum CatchUpStep {
    Extract(Vec<PendingSource>),
    /// No full scan has completed yet. The saved cursor tells a pass that
    /// moved the scan from one that did not. Once a scan has completed,
    /// catch-up never starts or moves a recheck, so searches keep their usual
    /// recheck pattern.
    Discover {
        cursor: Option<String>,
    },
    Settled,
}

impl From<IndexingProgress> for CatchUpStep {
    fn from(progress: IndexingProgress) -> Self {
        if !progress.pending.is_empty() {
            Self::Extract(progress.pending)
        } else if !progress.scanned {
            Self::Discover {
                cursor: progress.scan_cursor,
            }
        } else {
            Self::Settled
        }
    }
}

pub(super) fn catch_up(
    workspace_root: &Path,
    cancellation: &OperationCancellation,
    skipped: &mut Skipped,
) -> Result<(), ManifestError> {
    catch_up_with(
        workspace_root,
        cancellation,
        skipped,
        |root, index, relative, token| extract_for_search(root, index, relative, token).map(drop),
    )
}

/// Index until nothing is left. Discovering the same cursor twice in a row
/// means the pass between them changed nothing, so catch-up stops rather than
/// repeat it. A file that fails without publishing is skipped; an error from
/// the index itself or shutdown stops catch-up, and the next search schedules
/// it again.
pub(super) fn catch_up_with(
    workspace_root: &Path,
    cancellation: &OperationCancellation,
    skipped: &mut Skipped,
    mut extract: impl FnMut(&Path, &Path, &Path, &OperationCancellation) -> Result<(), ManifestError>,
) -> Result<(), ManifestError> {
    let index_path = crate::workspace::source_index_path(workspace_root);
    let mut previous = None;
    loop {
        cancellation.check()?;
        let progress = indexing_progress(&index_path, EXTRACT_WINDOW, |source| {
            !skipped.contains(source)
        })?;
        let step = CatchUpStep::from(progress);
        match &step {
            CatchUpStep::Settled => return Ok(()),
            _ if previous.as_ref() == Some(&step) => return Ok(()),
            CatchUpStep::Extract(pending) => {
                for source in pending {
                    cancellation.check()?;
                    match extract(workspace_root, &index_path, &source.path, cancellation) {
                        Ok(()) => {}
                        Err(error @ ManifestError::Sqlite { .. }) => return Err(error),
                        Err(error) if cancellation.is_cancelled() => return Err(error),
                        Err(_) => {
                            skipped.insert(source.clone());
                        }
                    }
                }
            }
            CatchUpStep::Discover { .. } => {
                let prepared = prepare_source_query_with_allowlist_and_cancellation(
                    workspace_root,
                    &index_path,
                    Path::new(""),
                    &[],
                    DISCOVERY,
                    None,
                    cancellation,
                )?;
                if prepared.reconciliation == ReconciliationOutcome::DeferredConcurrentUpdate {
                    thread::sleep(CONTENDED_PAUSE);
                }
            }
        }
        previous = Some(step);
    }
}

struct Workers {
    /// Set once by `shutdown`. It cancels every worker and refuses new ones.
    stopped: Arc<AtomicBool>,
    /// A finished worker hands its skipped sources to the next one.
    running: HashMap<PathBuf, JoinHandle<Skipped>>,
}

fn workers() -> &'static Mutex<Workers> {
    static WORKERS: OnceLock<Mutex<Workers>> = OnceLock::new();
    WORKERS.get_or_init(|| {
        Mutex::new(Workers {
            stopped: Arc::default(),
            running: HashMap::new(),
        })
    })
}

/// Start catch-up for `workspace_root` unless it is already running or the
/// server is stopping. A plain thread, so runtime shutdown never waits for it.
pub(crate) fn schedule(workspace_root: &Path) {
    let mut workers = workers().lock().unwrap_or_else(PoisonError::into_inner);
    if workers.stopped.load(Ordering::Relaxed)
        || workers
            .running
            .get(workspace_root)
            .is_some_and(|worker| !worker.is_finished())
    {
        return;
    }
    let mut skipped = workers
        .running
        .remove(workspace_root)
        .and_then(|finished| finished.join().ok())
        .unwrap_or_default();
    let root = workspace_root.to_path_buf();
    let cancellation = OperationCancellation::with_flag(Arc::clone(&workers.stopped));
    let spawned = thread::Builder::new()
        .name("magnesium-catch-up".to_owned())
        .spawn(move || {
            let _scope = enter_request_cancellation(&cancellation);
            let _ = catch_up(&root, &cancellation, &mut skipped);
            skipped
        });
    if let Ok(worker) = spawned {
        workers.running.insert(workspace_root.to_path_buf(), worker);
    }
}

/// Cancel every worker and wait briefly for them to return. A cancelled worker
/// kills its PDF extractor before returning, so no extractor outlives the server.
pub(crate) fn shutdown() {
    let running = {
        let mut workers = workers().lock().unwrap_or_else(PoisonError::into_inner);
        workers.stopped.store(true, Ordering::Relaxed);
        std::mem::take(&mut workers.running)
    };
    let deadline = Instant::now() + SHUTDOWN_WAIT;
    for worker in running.into_values() {
        while !worker.is_finished() && Instant::now() < deadline {
            thread::sleep(SHUTDOWN_POLL);
        }
    }
}
