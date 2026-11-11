//! The library: every folder the window knows, most urgent first.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use mg_core::study::{self, HomeState};
use mg_core::{runtime, Store};

/// A stand-in for `magnesium digest`: it reports an exam as many days away as
/// the file `days` in the folder says, or no plan when there is no such file.
/// A folder with a file `plain` was never set up for Magnesium.
const RUNTIME: &str = r#"#!/bin/sh
if [ -f plain ]; then
  printf '{"active":false}\n'
elif [ -f days ]; then
  printf '{"active":true,"plans":[{"target":"exam","title":"Exam","days_left":%s,"redos_due":2}],"record":{"sessions":[{"goal":"a goal","days_ago":1}]}}\n' "$(cat days)"
else
  printf '{"active":true,"plans":[],"record":{"sessions":[]}}\n'
fi
"#;

#[test]
fn the_library_lists_every_folder_with_the_nearest_exam_first() {
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("magnesium");
    fs::write(&program, RUNTIME).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var(runtime::PROGRAM_ENV, &program);

    let folder = |name: &str, file: Option<(&str, &str)>| {
        let path = dir.path().join(name);
        fs::create_dir_all(&path).unwrap();
        if let Some((file, text)) = file {
            fs::write(path.join(file), text).unwrap();
        }
        path.to_string_lossy().into_owned()
    };
    let latest = folder("opened-last", Some(("days", "20")));
    let none = folder("no-plan", None);
    let past = folder("exam-past", Some(("days", "-2")));
    let soon = folder("exam-soon", Some(("days", "3")));
    let plain = folder("not-set-up", Some(("plain", "")));

    let store = Store::at(dir.path().join("window"));
    let recent = [latest, none, past, soon, plain];
    let cards = study::library(&recent, &store);
    let names: Vec<&str> = cards.iter().map(|card| card.name.as_str()).collect();
    // The exam in 3 days, then the one in 20, then the rest as last opened.
    assert_eq!(
        names,
        [
            "exam-soon",
            "opened-last",
            "no-plan",
            "exam-past",
            "not-set-up"
        ]
    );

    let soon = &cards[0];
    assert_eq!(soon.state, HomeState::Ready);
    assert_eq!(soon.exam.as_ref().and_then(|exam| exam.days_left), Some(3));
    assert_eq!(soon.goal.as_deref(), Some("a goal"));
    assert_eq!(soon.days_ago, Some(1));
    assert_eq!(soon.due, 2);
    assert_eq!(cards[2].state, HomeState::Empty);
    assert_eq!(cards[4].state, HomeState::NotSetUp);
    assert!(study::library(&[], &store).is_empty());
}
