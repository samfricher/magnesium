use std::collections::{HashMap, HashSet};
use std::fs;
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

const SHUTDOWN_WAIT: Duration = Duration::from_millis(500);
const SHUTDOWN_POLL: Duration = Duration::from_millis(5);
const CONTENDED_PAUSE: Duration = Duration::from_millis(50);
const EXTRACT_WINDOW: usize = 64;
const SETTLE_PAUSE: Duration = Duration::from_millis(200);
const LOCK_NAME: &str = "indexer.lock";

const DISCOVERY: CoordinatorBudget = CoordinatorBudget {
    max_metadata_entries: 32,
    max_duration: Duration::from_secs(120),
    max_on_demand: 0,
};

pub(super) type Skipped = HashSet<PendingSource>;

#[derive(PartialEq, Eq)]
enum CatchUpStep {
    Extract(Vec<PendingSource>),
    Discover { cursor: Option<String> },
    Settled,
}

impl From<IndexingProgress> for CatchUpStep {
    fn from(progress: IndexingProgress) -> Self {
        if !progress.pending.is_empty() {
            Self::Extract(progress.pending)
        } else if !progress.scanned || progress.scan_cursor.is_some() {
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
    rescan: bool,
) -> Result<(), ManifestError> {
    catch_up_with(
        workspace_root,
        cancellation,
        skipped,
        rescan,
        |root, index, relative, token| extract_for_search(root, index, relative, token).map(drop),
    )
}

pub(super) fn catch_up_with(
    workspace_root: &Path,
    cancellation: &OperationCancellation,
    skipped: &mut Skipped,
    rescan: bool,
    mut extract: impl FnMut(&Path, &Path, &Path, &OperationCancellation) -> Result<(), ManifestError>,
) -> Result<(), ManifestError> {
    let index_path = crate::workspace::source_index_path(workspace_root);
    let mut previous = None;
    let mut rescan = rescan;
    loop {
        cancellation.check()?;
        let progress = indexing_progress(&index_path, EXTRACT_WINDOW, |source| {
            !skipped.contains(source)
        })?;
        let step = if rescan && progress.pending.is_empty() {
            rescan = false;
            CatchUpStep::Discover {
                cursor: progress.scan_cursor,
            }
        } else {
            CatchUpStep::from(progress)
        };
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
    stopped: Arc<AtomicBool>,
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

pub(crate) fn settled(workspace_root: &Path, skipped: &Skipped) -> Result<bool, ManifestError> {
    let index_path = crate::workspace::source_index_path(workspace_root);
    let progress = indexing_progress(&index_path, EXTRACT_WINDOW, |source| {
        !skipped.contains(source)
    })?;
    Ok(CatchUpStep::from(progress) == CatchUpStep::Settled)
}

fn indexer_lock(workspace_root: &Path) -> Option<fs::File> {
    let control = workspace_root.join(crate::workspace::CONTROL_DIR);
    let path = control.join("index").join(LOCK_NAME);
    crate::workspace::reject_symlink_components(&path, &control).ok()?;
    let file = crate::workspace::open_private_lock(&path).ok()?;
    file.try_lock().ok()?;
    Some(file)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IndexOutcome {
    Settled,
    AlreadyRunning,
    OutOfTime,
}

impl IndexOutcome {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Settled => "settled",
            Self::AlreadyRunning => "already_running",
            Self::OutOfTime => "out_of_time",
        }
    }
}

pub(crate) fn index_folder(
    workspace_root: &Path,
    limit: Duration,
) -> Result<IndexOutcome, ManifestError> {
    let Some(_lock) = indexer_lock(workspace_root) else {
        return Ok(IndexOutcome::AlreadyRunning);
    };
    let cancellation = OperationCancellation::none().with_earlier_deadline(Instant::now() + limit);
    let _scope = enter_request_cancellation(&cancellation);
    let mut skipped = Skipped::default();
    let mut rescan = true;
    loop {
        match catch_up(
            workspace_root,
            &cancellation,
            &mut skipped,
            std::mem::take(&mut rescan),
        ) {
            Ok(()) => {}
            Err(_) if cancellation.is_cancelled() => return Ok(IndexOutcome::OutOfTime),
            Err(error @ ManifestError::Sqlite { .. }) => return Err(error),
            Err(_) => {}
        }
        if settled(workspace_root, &skipped)? {
            return Ok(IndexOutcome::Settled);
        }
        if cancellation.is_cancelled() {
            return Ok(IndexOutcome::OutOfTime);
        }
        thread::sleep(SETTLE_PAUSE);
    }
}

pub(crate) fn schedule(workspace_root: &Path, rescan: bool) {
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
            let Some(_lock) = indexer_lock(&root) else {
                return skipped;
            };
            let _scope = enter_request_cancellation(&cancellation);
            let _ = catch_up(&root, &cancellation, &mut skipped, rescan);
            skipped
        });
    if let Ok(worker) = spawned {
        workers.running.insert(workspace_root.to_path_buf(), worker);
    }
}

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
