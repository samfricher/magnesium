use rusqlite::Connection;
use std::fs;
use support::TestEnvironment;

#[test]
fn independent_local_stores_keep_learner_state_out_of_global_data() {
    let env = TestEnvironment::from_fixture("basic").unwrap();
    let nested = env.workspace.join("statistics");
    fs::create_dir(&nested).unwrap();
    for root in [&env.workspace, &nested] {
        let output = env.command_in(root).arg("init").output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let id = |root: &std::path::Path| -> String {
        Connection::open(root.join(support::CONTROL_DIR).join("state.sqlite"))
            .unwrap()
            .query_row("SELECT workspace_id FROM workspace_meta", [], |r| r.get(0))
            .unwrap()
    };
    assert_ne!(id(&env.workspace), id(&nested));
    assert_eq!(env.unmanaged_data_entries(), Vec::<String>::new());
}

#[test]
fn unknown_store_is_preserved_and_requires_owner_fresh_start() {
    let env = TestEnvironment::from_fixture("basic").unwrap();
    assert!(env.command().arg("init").output().unwrap().status.success());
    let path = env
        .workspace
        .join(support::CONTROL_DIR)
        .join("state.sqlite");
    fs::write(&path, b"old learner database sentinel").unwrap();
    let output = env.command().arg("init").output().unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read(&path).unwrap(), b"old learner database sentinel");
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(
        message.contains("fresh") || message.contains("initialize again"),
        "{message}"
    );
}
