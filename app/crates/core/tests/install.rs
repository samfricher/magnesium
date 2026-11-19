#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;

use mg_core::install::{self, RuntimeState};
use mg_core::{runtime, Store};

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn program(path: &Path, version: &str) {
    stand_in(path, version, Some(runtime::WINDOW_FEATURES_NEEDED));
}

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
    std::env::set_var(runtime::PROGRAM_ENV, &target);
    let store = Store::at(dir.path().join("window"));

    let first = install::check(&store);
    assert_eq!(first.state, RuntimeState::Ready, "{first:?}");
    assert!(first.installed_now);
    assert_eq!(install::identify(&target).version, "0.3.0+abc");
    let again = install::check(&store);
    assert_eq!(again.state, RuntimeState::Ready);
    assert!(!again.installed_now);

    program(&target, "0.2.0");
    let asked = install::check(&store);
    assert_eq!(asked.state, RuntimeState::Different, "{asked:?}");
    assert_eq!(install::identify(&target).version, "0.2.0");

    let kept = install::keep(&store);
    assert_eq!(kept.state, RuntimeState::Kept);
    assert!(kept.message.unwrap().contains("0.2.0"));
    assert_eq!(install::check(&store).state, RuntimeState::Kept);
    assert_eq!(install::identify(&target).version, "0.2.0");

    let replaced = install::replace(&store);
    assert_eq!(replaced.state, RuntimeState::Ready, "{replaced:?}");
    assert!(replaced.installed_now);
    assert_eq!(install::identify(&target).version, "0.3.0+abc");

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

    stand_in(&target, "0.2.0", None);
    let old = install::check(&store);
    assert_eq!(old.state, RuntimeState::TooOld, "{old:?}");
    let said = old.message.unwrap();
    assert!(
        said.contains("older than this window needs") && said.contains("0.3.0"),
        "{said}"
    );
    assert_eq!(install::keep(&store).state, RuntimeState::TooOld);
    assert_eq!(store.kept_runtime(), None);
    store.set_kept_runtime(Some("0.2.0"));
    assert_eq!(install::check(&store).state, RuntimeState::TooOld);
    stand_in(&target, "0.2.0", Some(runtime::WINDOW_FEATURES_NEEDED - 1));
    assert_eq!(install::check(&store).state, RuntimeState::TooOld);
    let replaced = install::replace(&store);
    assert_eq!(replaced.state, RuntimeState::Ready, "{replaced:?}");
    assert_eq!(install::identify(&target).version, "0.3.0+abc");

    stand_in(&inside, "0.3.0+abc", None);
    stand_in(&target, "0.2.0", None);
    let both = install::check(&store);
    assert_eq!(both.state, RuntimeState::Failed, "{both:?}");
    assert!(both.message.unwrap().contains("Download the app again"));

    std::env::remove_var(install::BUNDLED_ENV);
    stand_in(&target, env!("CARGO_PKG_VERSION"), None);
    let source = install::check(&store);
    assert_eq!(source.state, RuntimeState::TooOld, "{source:?}");
    assert!(source.message.unwrap().contains("docs/install.md"));
    assert_eq!(install::keep(&store).state, RuntimeState::TooOld);
    program(&target, env!("CARGO_PKG_VERSION"));
    assert_eq!(install::check(&store).state, RuntimeState::Ready);
}
