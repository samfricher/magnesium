//! The app's own `magnesium` program, installed on first open and replacing
//! another copy only when the learner agrees.
//!
//! Stand-in programs answer `--version`, and `digest` with the number of the
//! window's needs they meet, so no runtime is built.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;

use mg_core::install::{self, RuntimeState};
use mg_core::{runtime, Store};

/// The tests set the same environment variables, so they run one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// A stand-in that meets the window's needs.
fn program(path: &Path, version: &str) {
    stand_in(path, version, Some(runtime::WINDOW_FEATURES_NEEDED));
}

/// A stand-in whose digest says `features`, or no number at all, as every
/// runtime from before the number does.
fn stand_in(path: &Path, version: &str, features: Option<u64>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let digest = match features {
        Some(number) => format!("{{\"active\": false, \"window_features\": {number}}}"),
        None => "{\"active\": false}".to_owned(),
    };
    fs::write(
        path,
        format!("#!/bin/sh\nif [ \"$1\" = digest ]; then echo '{digest}'; exit 0; fi\necho 'magnesium {version}'\n"),
    )
    .unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn the_apps_copy_is_installed_once_and_another_is_replaced_only_when_agreed() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let inside = dir.path().join("Magnesium.app/Contents/MacOS/magnesium");
    let target = dir.path().join("home/.local/bin/magnesium");
    program(&inside, "0.3.0+abc");
    std::env::set_var(install::BUNDLED_ENV, &inside);
    std::env::set_var(install::TARGET_ENV, &target);
    // The window finds the installed program here, as it would on the PATH.
    std::env::set_var(runtime::PROGRAM_ENV, &target);
    let store = Store::at(dir.path().join("window"));

    // None installed: installed without a question.
    let first = install::check(&store);
    assert_eq!(first.state, RuntimeState::Ready, "{first:?}");
    assert!(first.installed_now);
    assert_eq!(install::identify(&target).version, "0.3.0+abc");
    // Opened again, nothing more happens.
    let again = install::check(&store);
    assert_eq!(again.state, RuntimeState::Ready);
    assert!(!again.installed_now);

    // Another copy, such as the release of 21 October: asked, not replaced.
    program(&target, "0.2.0");
    let asked = install::check(&store);
    assert_eq!(asked.state, RuntimeState::Different, "{asked:?}");
    assert_eq!(install::identify(&target).version, "0.2.0");

    // Kept: the program is as it was, the window says what it cannot do,
    // and it does not ask again for that copy.
    let kept = install::keep(&store);
    assert_eq!(kept.state, RuntimeState::Kept);
    assert!(kept.message.unwrap().contains("0.2.0"));
    assert_eq!(install::check(&store).state, RuntimeState::Kept);
    assert_eq!(install::identify(&target).version, "0.2.0");

    // Agreed: replaced in place.
    let replaced = install::replace(&store);
    assert_eq!(replaced.state, RuntimeState::Ready, "{replaced:?}");
    assert!(replaced.installed_now);
    assert_eq!(install::identify(&target).version, "0.3.0+abc");

    // Another version again: asked again.
    program(&target, "0.2.1");
    assert_eq!(install::check(&store).state, RuntimeState::Different);
}

#[test]
fn a_copy_older_than_the_window_needs_is_refused_and_cannot_be_kept() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let inside = dir.path().join("Magnesium.app/Contents/MacOS/magnesium");
    let target = dir.path().join("home/.local/bin/magnesium");
    program(&inside, "0.3.0+abc");
    std::env::set_var(install::BUNDLED_ENV, &inside);
    std::env::set_var(install::TARGET_ENV, &target);
    std::env::set_var(runtime::PROGRAM_ENV, &target);
    let store = Store::at(dir.path().join("window"));

    // A copy from before the number: refused, with the app's copy offered.
    stand_in(&target, "0.2.0", None);
    let old = install::check(&store);
    assert_eq!(old.state, RuntimeState::TooOld, "{old:?}");
    let said = old.message.unwrap();
    assert!(
        said.contains("older than this window needs") && said.contains("0.3.0"),
        "{said}"
    );
    // It cannot be kept.
    assert_eq!(install::keep(&store).state, RuntimeState::TooOld);
    assert_eq!(store.kept_runtime(), None);
    // A copy kept before it was known to be too old is refused too.
    store.set_kept_runtime(Some("0.2.0"));
    assert_eq!(install::check(&store).state, RuntimeState::TooOld);
    // A lower number is refused like none.
    stand_in(&target, "0.2.0", Some(runtime::WINDOW_FEATURES_NEEDED - 1));
    assert_eq!(install::check(&store).state, RuntimeState::TooOld);
    // Replaced: ready.
    let replaced = install::replace(&store);
    assert_eq!(replaced.state, RuntimeState::Ready, "{replaced:?}");
    assert_eq!(install::identify(&target).version, "0.3.0+abc");

    // The app's own copy too old as well: nothing to offer but a new download.
    stand_in(&inside, "0.3.0+abc", None);
    stand_in(&target, "0.2.0", None);
    let both = install::check(&store);
    assert_eq!(both.state, RuntimeState::Failed, "{both:?}");
    assert!(both.message.unwrap().contains("Download the app again"));

    // The window run from source carries no copy: the same version as this
    // checkout, from an older commit, is refused, and the install guide named.
    std::env::remove_var(install::BUNDLED_ENV);
    stand_in(&target, env!("CARGO_PKG_VERSION"), None);
    let source = install::check(&store);
    assert_eq!(source.state, RuntimeState::TooOld, "{source:?}");
    assert!(source.message.unwrap().contains("docs/install.md"));
    assert_eq!(install::keep(&store).state, RuntimeState::TooOld);
    // A new enough copy is ready again.
    program(&target, env!("CARGO_PKG_VERSION"));
    assert_eq!(install::check(&store).state, RuntimeState::Ready);
}
