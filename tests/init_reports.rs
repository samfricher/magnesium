use std::fs;

use support::TestEnvironment;

#[test]
fn failed_agents_md_adapter_does_not_claim_no_workspace_files_were_replaced() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let control = fs::canonicalize(&environment.workspace)
        .unwrap()
        .join(support::CONTROL_DIR);
    let first = environment
        .command()
        .arg("init")
        .output()
        .expect("run magnesium init");
    assert!(first.status.success(), "{first:?}");
    let instructions = control.join("INSTRUCTIONS.md");
    let installed = fs::read(&instructions).unwrap();
    fs::write(&instructions, b"stale instructions\n").unwrap();
    fs::write(
        environment.workspace.join("AGENTS.md"),
        "<!-- magnesium:begin -->\nunterminated\n",
    )
    .unwrap();

    let output = environment
        .command()
        .args(["init", "--adapter", "agents-md"])
        .output()
        .expect("run magnesium init with mismatched AGENTS.md markers");
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("mismatched Magnesium marker blocks"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("No existing workspace files were replaced"),
        "{stderr}"
    );
    // The failure came after init had already rewritten INSTRUCTIONS.md.
    assert_eq!(fs::read(&instructions).unwrap(), installed);
}
