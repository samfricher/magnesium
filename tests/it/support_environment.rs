//! Test commands must not inherit the caller's workspace, identity or host capabilities.
use crate::support::TestEnvironment;

const CONTEXT_VARIABLES: [&str; 15] = [
    "MAGNESIUM_WORKSPACE_ROOT",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CONFIG_DIR",
    "MAGNESIUM_BIN",
    "MAGNESIUM_DOCS_WORKER",
    "MAGNESIUM_DOCS_WORKER_OPT_IN",
    "OPENAI_API_KEY",
    "OPENAI_ACCESS_TOKEN",
    "ANTHROPIC_API_KEY",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "AZURE_OPENAI_API_KEY",
    "OPENROUTER_API_KEY",
    "MAGNESIUM_SUPPORT_UNTRUSTED_PARENT",
];
const TEST_NAME: &str = "support_environment::default_commands_strip_inherited_context_and_keep_explicit_test_configuration";
const PHASE: &str = "MAGNESIUM_SUPPORT_TEST_PHASE";

#[test]
fn default_commands_strip_inherited_context_and_keep_explicit_test_configuration() {
    let phase = std::env::var(PHASE).ok();
    if phase.as_deref() == Some("clean") {
        for variable in CONTEXT_VARIABLES {
            assert!(std::env::var_os(variable).is_none(), "inherited {variable}");
        }
        assert_eq!(
            std::env::var("MAGNESIUM_TEST_BINARY").unwrap(),
            "synthetic-magnesium-test-binary"
        );
        assert_eq!(
            std::env::var_os("HOME"),
            std::env::var_os("MAGNESIUM_SUPPORT_TEST_EXPECTED_HOME")
        );
        for variable in ["PATH", "CARGO_HOME", "RUSTUP_HOME"] {
            assert_eq!(
                std::env::var_os(variable),
                std::env::var_os(format!("MAGNESIUM_SUPPORT_EXPECTED_{variable}"))
            );
        }
        return;
    }
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    let executable = std::env::current_exe().unwrap();
    let mut command = if phase.as_deref() == Some("poisoned") {
        for variable in CONTEXT_VARIABLES {
            assert!(
                std::env::var_os(variable).is_some(),
                "poison missing for {variable}"
            );
        }
        let mut command = environment.command_with(executable, &environment.workspace);
        command.env(PHASE, "clean").env(
            "MAGNESIUM_SUPPORT_TEST_EXPECTED_HOME",
            environment.root.join("home"),
        );
        for variable in ["PATH", "CARGO_HOME", "RUSTUP_HOME"] {
            let expected = format!("MAGNESIUM_SUPPORT_EXPECTED_{variable}");
            if let Some(value) = std::env::var_os(&expected) {
                command.env(expected, value);
            }
        }
        command
    } else {
        let mut command = std::process::Command::new(executable);
        command
            .env(PHASE, "poisoned")
            .env("MAGNESIUM_TEST_BINARY", "synthetic-magnesium-test-binary");
        // Poison only this subprocess. These paths belong to this synthetic fixture;
        // no global environment mutation or real host/account access is needed.
        for variable in CONTEXT_VARIABLES {
            command.env(
                variable,
                environment.root.join(format!("synthetic-{variable}")),
            );
        }
        for variable in ["PATH", "CARGO_HOME", "RUSTUP_HOME"] {
            if let Some(value) = std::env::var_os(variable) {
                command.env(format!("MAGNESIUM_SUPPORT_EXPECTED_{variable}"), value);
            }
        }
        command
    };
    let output = command
        .args(["--exact", TEST_NAME, "--nocapture"])
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{report}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        report.contains("running 1 test\n") && report.contains("test result: ok. 1 passed;"),
        "the child must run {TEST_NAME} exactly once: {report}"
    );
}
