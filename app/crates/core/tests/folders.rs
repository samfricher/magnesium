//! Which folder the window opens when none is named.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use mg_core::{runtime, study};

/// A stand-in for `magnesium digest`: it reports an exam as many days away as
/// the file `days` in the folder says, or no plan when there is no such file.
const RUNTIME: &str = r#"#!/bin/sh
if [ -f days ]; then
  printf '{"active":true,"plans":[{"target":"exam","days_left":%s}],"record":{"sessions":[]}}\n' "$(cat days)"
else
  printf '{"active":true,"plans":[],"record":{"sessions":[]}}\n'
fi
"#;

#[test]
fn the_folder_with_the_nearest_exam_opens_first() {
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("magnesium");
    fs::write(&program, RUNTIME).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var(runtime::PROGRAM_ENV, &program);

    let folder = |name: &str, days: Option<&str>| {
        let path = dir.path().join(name);
        fs::create_dir_all(&path).unwrap();
        if let Some(days) = days {
            fs::write(path.join("days"), days).unwrap();
        }
        path.to_string_lossy().into_owned()
    };
    let latest = folder("opened-last", Some("20"));
    let soon = folder("exam-soon", Some("3"));
    let past = folder("exam-past", Some("-2"));
    let none = folder("no-plan", None);

    let recent = vec![latest.clone(), none.clone(), past.clone(), soon.clone()];
    assert_eq!(study::folder_to_open(&recent), Some(soon));

    // With no exam ahead anywhere, the folder opened last comes first.
    assert_eq!(study::folder_to_open(&[none.clone(), past]), Some(none));
    assert_eq!(study::folder_to_open(&[]), None);
}
