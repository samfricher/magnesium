//! Test commands must not inherit the caller's workspace, identity or host capabilities.
use support::TestEnvironment;

const CONTEXT_VARIABLES: [&str; 20] = [
    "MAGNESIUM_WORKSPACE_ROOT",
    "CODEX_THREAD_ID",
    "CLAUDE_CODE_SESSION_ID",
    "OPENCODE_SESSION_ID",
    "CLAUDE_CONFIG_DIR",
    "MAGNESIUM_BIN",
    "OPENCODE_SERVER_PASSWORD",
    "OPENCODE_SERVER_USERNAME",
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
    "CODEX_API_KEY",
    "MAGNESIUM_SUPPORT_UNTRUSTED_PARENT",
];
const TEST_NAME: &str =
    "default_commands_strip_inherited_context_and_keep_explicit_test_configuration";
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
            std::env::var("CODEX_TEST_BINARY").unwrap(),
            "synthetic-codex-test-binary"
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
        let codex_default = command
            .get_envs()
            .find(|(name, _)| *name == "CODEX_TEST_BINARY")
            .and_then(|(_, value)| value)
            .unwrap();
        assert_eq!(
            codex_default,
            environment.root.join("codex-not-installed").as_os_str()
        );
        command
            .env(PHASE, "clean")
            .env("CODEX_TEST_BINARY", "synthetic-codex-test-binary")
            .env(
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
            .env("MAGNESIUM_TEST_BINARY", "synthetic-magnesium-test-binary")
            .env("CODEX_TEST_BINARY", "synthetic-codex-test-binary");
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
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
