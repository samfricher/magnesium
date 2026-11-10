//! The app's own `magnesium` program, installed on first open and replacing
//! another copy only when the learner agrees.
//!
//! Stand-in programs answer `--version`, so no runtime is built.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use mg_core::install::{self, RuntimeState};
use mg_core::{runtime, Store};

fn program(path: &Path, version: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("#!/bin/sh\necho 'magnesium {version}'\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn the_apps_copy_is_installed_once_and_another_is_replaced_only_when_agreed() {
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
