//! Local v2 invariants replace the former numbered assessment/group graph.
use rusqlite::Connection;
use support::TestEnvironment;

fn state(environment: &TestEnvironment) -> Vec<String> {
    let connection = Connection::open(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("state.sqlite"),
    )
    .unwrap();
    let mut rows = connection.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE '%fts%' ORDER BY name").unwrap();
    let tables: Vec<String> = rows
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    tables
        .into_iter()
        .map(|table| {
            let mut statement = connection
                .prepare(&format!("SELECT * FROM \"{table}\" ORDER BY rowid"))
                .unwrap();
            let width = statement.column_count();
            let values: Vec<String> = statement
                .query_map([], |row| {
                    Ok((0..width)
                        .map(|index| format!("{:?}", row.get_ref(index).unwrap()))
                        .collect::<Vec<_>>()
                        .join("|"))
                })
                .unwrap()
                .map(Result::unwrap)
                .collect();
            format!("{table}: {values:?}")
        })
        .collect()
}

#[test]
fn diagnostics_and_reindex_do_not_change_canonical_learning_state() {
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    assert!(
        environment
            .command()
            .arg("init")
            .output()
            .unwrap()
            .status
            .success()
    );
    let before = state(&environment);
    for arguments in [vec!["info", "--json"], vec!["memory", "reindex"]] {
        let output = environment.command().args(arguments).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(state(&environment), before);
    }
}
