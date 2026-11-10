//! A paper is found and opened from the study root, which can be a parent of
//! the folder the window opened.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use mg_core::claude::ClaudeCode;
use mg_core::study::Paper;
use mg_core::{runtime, Manager, Store};

#[test]
fn a_paper_opens_from_a_folder_inside_the_study_root() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("course")).unwrap();
    let root = dir.path().join("course").canonicalize().unwrap();
    let opened = root.join("week-3");
    fs::create_dir_all(&opened).unwrap();
    let papers = root.join("Study/exam/papers");
    fs::create_dir_all(&papers).unwrap();
    fs::write(papers.join("Paper B.pdf"), b"%PDF-1.4").unwrap();
    fs::write(papers.join("Paper B key.pdf"), b"%PDF-1.4").unwrap();

    // A stand-in for `magnesium digest`, which reports the study root.
    let program = dir.path().join("magnesium");
    fs::write(
        &program,
        format!(
            "#!/bin/sh\nprintf '%s\\n' '{{\"active\":true,\"study_folder\":\"{}\",\"plans\":[{{\"target\":\"exam\",\"days_left\":5}}],\"record\":{{\"sessions\":[]}}}}'\n",
            root.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var(runtime::PROGRAM_ENV, &program);

    let manager = Manager::new(
        Arc::new(Store::at(dir.path().join("home"))),
        Arc::new(ClaudeCode::with_program(dir.path().join("no-agent"))),
        Arc::new(|_| {}),
    );
    let home = manager.open_folder(&opened).unwrap();
    assert_eq!(home.papers.len(), 1, "{:?}", home.papers);
    let paper = &home.papers[0];
    assert_eq!(
        manager.paper_file(paper).unwrap(),
        papers.join("Paper B.pdf").canonicalize().unwrap()
    );

    // The answer key stays closed, and nothing outside the root opens.
    let key = Paper {
        path: "Study/exam/papers/Paper B key.pdf".into(),
        ..paper.clone()
    };
    assert!(manager.paper_file(&key).is_err());
    let outside = Paper {
        path: "../secret.pdf".into(),
        ..paper.clone()
    };
    fs::write(dir.path().join("secret.pdf"), b"%PDF-1.4").unwrap();
    assert!(manager.paper_file(&outside).is_err());
}
