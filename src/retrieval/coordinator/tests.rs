use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pdf_extract::{Document, Object, Stream, content::Content, content::Operation, dictionary};
use rusqlite::Connection;

use super::super::catch_up::{Skipped, catch_up, catch_up_with};
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
    assert_eq!(exhausted.coverage, IndexCoverage::IndexingOnDemand);
    assert_eq!(source_status(&fixture.index(), "a-selected.md"), "indexed");
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

    let expired = OperationCancellation::none().with_earlier_deadline(std::time::Instant::now());
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
            super::extract_source_with_cancellation(&fixture.root, &fixture.index(), path, token)
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

    catch_up(&fixture.root, &slow, &mut Skipped::new()).expect("catch up after the search");
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
    catch_up(&fixture.root, &watch, &mut Skipped::new()).expect("catch up");
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
fn catch_up_continues_past_files_it_cannot_finish() {
    let fixture = Fixture::new("catch-up-unfinishable");
    fixture.write("a-vanished.md", b"deleted after discovery\n");
    fixture.write("b-changed.md", b"changed after discovery\n");
    fixture.write("c-lesson.md", b"lesson\n");
    finish_metadata_scan(&fixture, budget(8));
    fs::remove_file(fixture.root.join("a-vanished.md")).unwrap();
    fixture.write("b-changed.md", b"changed after discovery, and longer\n");

    catch_up(
        &fixture.root,
        &OperationCancellation::none(),
        &mut Skipped::new(),
    )
    .expect("catch up");
    assert_eq!(source_status(&fixture.index(), "c-lesson.md"), "indexed");
}

#[test]
fn catch_up_reaches_a_file_after_a_full_window_of_newly_excluded_files() {
    let fixture = Fixture::new("catch-up-excluded-window");
    for index in 0..64 {
        fixture.write(&format!("drafts/{index:02}.md"), b"draft\n");
    }
    fixture.write("lesson.md", b"lesson\n");
    finish_metadata_scan(&fixture, budget(128));
    fixture.write(
        &format!("{}/magnesiumignore", crate::workspace::CONTROL_DIR),
        b"drafts/\n",
    );
    assert!(
        crate::retrieval::source_relative_path_is_excluded(
            &fixture.root,
            Path::new("drafts/00.md")
        )
        .unwrap()
    );

    catch_up(
        &fixture.root,
        &OperationCancellation::none(),
        &mut Skipped::new(),
    )
    .expect("catch up");
    assert_eq!(source_status(&fixture.index(), "lesson.md"), "indexed");
}

#[test]
fn catch_up_does_not_retry_an_unchanged_pdf_that_timed_out() {
    let fixture = Fixture::new("catch-up-timeout");
    fixture.write("a-slow.pdf", &pdf_fixture());
    fixture.write("b-lesson.md", b"lesson\n");
    finish_metadata_scan(&fixture, budget(8));
    let mut skipped = Skipped::new();
    let attempts = std::cell::Cell::new(0);
    let times_out = |root: &Path, index: &Path, relative: &Path, token: &OperationCancellation| {
        if relative != Path::new("a-slow.pdf") {
            return super::extract_for_search(root, index, relative, token).map(drop);
        }
        attempts.set(attempts.get() + 1);
        super::super::manifest::extract::extract_source_with_pdf(root, index, relative, |_| {
            Err(ManifestError::PdfTimeout)
        })
        .map(drop)
    };

    for run in 1..=2 {
        catch_up_with(
            &fixture.root,
            &OperationCancellation::none(),
            &mut skipped,
            times_out,
        )
        .unwrap_or_else(|error| panic!("run {run}: {error}"));
        assert_eq!(
            source_status(&fixture.index(), "b-lesson.md"),
            "indexed",
            "run {run}"
        );
        assert_eq!(
            source_status(&fixture.index(), "a-slow.pdf"),
            "pending",
            "run {run}"
        );
        assert_eq!(attempts.get(), 1, "run {run} retried the unchanged PDF");
    }

    let mut changed = pdf_fixture();
    changed.extend_from_slice(b"\n% revised\n");
    fixture.write("a-slow.pdf", &changed);
    coordinate_source_query(
        &fixture.root,
        &fixture.index(),
        Path::new(""),
        &[],
        budget(8),
    )
    .expect("a search's recheck records the change");
    catch_up(&fixture.root, &OperationCancellation::none(), &mut skipped).expect("catch up");
    assert_eq!(source_status(&fixture.index(), "a-slow.pdf"), "indexed");
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
        catch_up(
            &fixture.root,
            &OperationCancellation::none(),
            &mut Skipped::new(),
        )
        .expect("catch up");
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

    catch_up(
        &fixture.root,
        &OperationCancellation::none(),
        &mut Skipped::new(),
    )
    .expect("catch up");
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
        catch_up(&root, &OperationCancellation::none(), &mut Skipped::new()).unwrap();
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
fn a_recheck_leaves_unchanged_files_excluded_for_their_content() {
    let fixture = Fixture::new("recheck-content-exclusion");
    fixture.write("a-notes.md", b"password = hunter2\n");
    fixture.write("b-slides.pdf", &pdf_fixture());
    fixture.write("c-lesson.md", b"lesson\n");
    finish_metadata_scan(&fixture, budget(8));
    super::super::manifest::extract::extract_source_with_pdf(
        &fixture.root,
        &fixture.index(),
        Path::new("b-slides.pdf"),
        |_| Ok(vec!["password = hunter2".to_owned()]),
    )
    .expect("extracted text excludes the PDF");
    let exclusions = || {
        ["a-notes.md", "b-slides.pdf"].map(|path| {
            Connection::open(fixture.index())
                .unwrap()
                .query_row(
                    "SELECT index_status, exclusion_reason FROM source_manifest
                         WHERE display_path = ?1",
                    [path],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .unwrap()
        })
    };
    let excluded = exclusions();
    assert_eq!(
        excluded,
        [
            ("excluded".to_owned(), Some("sensitive_content".to_owned())),
            (
                "excluded".to_owned(),
                Some("sensitive_extracted".to_owned())
            ),
        ]
    );

    let recheck = coordinate_source_query(
        &fixture.root,
        &fixture.index(),
        Path::new(""),
        &[],
        budget(8),
    )
    .expect("recheck");
    assert_eq!(scan_active(&fixture.index()), 0, "the recheck finished");
    assert_eq!(
        recheck.metrics.content_reads, 0,
        "the recheck read an unchanged file again"
    );
    assert_eq!(exclusions(), excluded);
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
        let report =
            coordinate_source_query(&fixture.root, &fixture.index(), Path::new(""), &[], budget)
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

/// Run passes until the scan has saved `nested` on its cursor with some
/// of its entries still to list.
fn leave_nested_directory_unfinished(fixture: &Fixture, budget: CoordinatorBudget) {
    for _ in 0..8 {
        coordinate_source_query(&fixture.root, &fixture.index(), Path::new(""), &[], budget)
            .expect("run bounded metadata batch");
        let (_, cursor) = persisted_cursor(&fixture.index());
        let cursor: serde_json::Value =
            serde_json::from_str(&cursor).expect("parse persisted cursor");
        let directories = cursor["directories"]
            .as_array()
            .expect("cursor directories");
        if directories.len() == 1
            && directories[0]["relative_path"] == serde_json::json!(b"nested".to_vec())
            && !directories[0]["processed_entries"]
                .as_array()
                .expect("processed entries")
                .is_empty()
        {
            return;
        }
    }
    panic!("scan never saved an unfinished nested directory");
}

#[cfg(unix)]
#[test]
fn resumed_scan_skips_a_saved_directory_replaced_by_an_outside_symlink() {
    let fixture = Fixture::new("resume-symlink");
    for index in 0..8 {
        fixture.write(&format!("nested/inside-{index}.md"), b"inside note\n");
    }
    let budget = budget(3);
    leave_nested_directory_unfinished(&fixture, budget);

    let outside = fixture.root.with_extension("outside");
    fs::create_dir_all(&outside).expect("create outside directory");
    fs::write(outside.join("outside.md"), b"outside note\n").expect("write outside note");
    fs::remove_dir_all(fixture.root.join("nested")).expect("remove nested directory");
    std::os::unix::fs::symlink(&outside, fixture.root.join("nested"))
        .expect("replace nested directory with symlink");

    let resumed =
        coordinate_source_query(&fixture.root, &fixture.index(), Path::new(""), &[], budget);
    fs::remove_dir_all(&outside).expect("remove outside directory");
    resumed.expect("resume after the directory became a symlink");
    finish_metadata_scan(&fixture, budget);
    assert_no_manifest_row(&fixture.index(), "nested/outside.md");
    assert_eq!(manifest_count(&fixture.index()), 0);
}

#[test]
fn resumed_scan_skips_a_saved_directory_that_became_ignored() {
    let fixture = Fixture::new("resume-ignored");
    for index in 0..8 {
        fixture.write(&format!("nested/inside-{index}.md"), b"inside note\n");
    }
    let budget = budget(3);
    leave_nested_directory_unfinished(&fixture, budget);
    // Files this scan already listed stay until the next scan.
    let listed_before = manifest_count(&fixture.index());
    assert!(listed_before < 8);

    fixture.write("nested/.git/HEAD", b"ref: refs/heads/main\n");
    finish_metadata_scan(&fixture, budget);
    assert_eq!(manifest_count(&fixture.index()), listed_before);
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
    let initial = r#"{"version":1,"directories":[{"relative_path":[],"processed_entries":[]}]}"#;
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
            .any(|report| report.reconciliation == ReconciliationOutcome::DeferredConcurrentUpdate),
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
    let initial = r#"{"version":1,"directories":[{"relative_path":[],"processed_entries":[]}]}"#;
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
    assert_eq!(warm.metrics.on_demand_used, 0);
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
        assert!(matches!(result, Err(ManifestError::Io { source, .. }) if source.kind() == kind));
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
    let keeper =
        super::super::manifest::status::read_manifest_row(&fixture.index(), Path::new("keeper.md"))
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
    let untouched =
        super::super::manifest::status::read_manifest_row(&fixture.index(), Path::new("keeper.md"))
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
        super::super::manifest::status::read_manifest_row(&fixture.index(), Path::new("keeper.md"))
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
        fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture parent");
        fs::write(path, contents).expect("write fixture source");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
