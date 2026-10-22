//! Background indexing after a search. A search indexes only what fits its own
//! small budget, so the MCP server then keeps one thread per workspace indexing
//! until nothing is left. Searches keep their own budget and behavior.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::coordinator::{
    CoordinatorBudget, ReconciliationOutcome, extract_pending_sources,
    prepare_source_query_with_allowlist_and_cancellation,
};
use super::manifest::{IndexingProgress, indexing_progress};
use super::{ManifestError, OperationCancellation};
use crate::request::enter_request_cancellation;

/// A cancelled worker kills its PDF extractor within one 10 ms poll. The host
/// allows the server about a second to exit after its input closes.
const SHUTDOWN_WAIT: Duration = Duration::from_millis(500);
const SHUTDOWN_POLL: Duration = Duration::from_millis(5);
const CONTENDED_PAUSE: Duration = Duration::from_millis(50);

/// One discovery batch. Small, so its files are extracted and searchable before
/// the next batch hashes them. The deadline leaves room to hash 32 files at the
/// 64 MB PDF limit on a slow disk, so a batch is never rolled back. The batch
/// extracts nothing; extraction publishes one file at a time.
const DISCOVERY: CoordinatorBudget = CoordinatorBudget {
    max_metadata_entries: 32,
    max_duration: Duration::from_secs(120),
    max_on_demand: 0,
};

/// What one read of the source index says catch-up does next. Extraction
/// comes first, so the files one discovery batch found become searchable
/// before the next batch.
#[derive(PartialEq, Eq)]
enum CatchUpStep {
    Extract(Vec<PathBuf>),
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

/// Index until nothing is left. Reading the same step twice in a row means the
/// pass between them changed nothing, such as a file that never finishes, so
/// catch-up stops rather than repeat it. An error also stops it; the next
/// search schedules it again.
pub(super) fn catch_up(
    workspace_root: &Path,
    cancellation: &OperationCancellation,
) -> Result<(), ManifestError> {
    let index_path = crate::workspace::source_index_path(workspace_root);
    let extraction = CoordinatorBudget::for_catch_up();
    let mut previous = None;
    loop {
        cancellation.check()?;
        let step = CatchUpStep::from(indexing_progress(&index_path, extraction.max_on_demand)?);
        match &step {
            CatchUpStep::Settled => return Ok(()),
            _ if previous.as_ref() == Some(&step) => return Ok(()),
            CatchUpStep::Extract(pending) => extract_pending_sources(
                workspace_root,
                &index_path,
                pending,
                extraction,
                cancellation,
            )?,
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
    running: HashMap<PathBuf, JoinHandle<()>>,
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
    let root = workspace_root.to_path_buf();
    let cancellation = OperationCancellation::with_flag(Arc::clone(&workers.stopped));
    let spawned = thread::Builder::new()
        .name("magnesium-catch-up".to_owned())
        .spawn(move || {
            let _scope = enter_request_cancellation(&cancellation);
            let _ = catch_up(&root, &cancellation);
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
