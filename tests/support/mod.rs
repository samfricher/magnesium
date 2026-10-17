use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

pub mod mcp;
pub mod snapshot;
pub mod sources;
pub mod v2;

/// The per-workspace control directory that `magnesium init` creates.
pub const CONTROL_DIR: &str = ".magnesium";

static NEXT_TEST_ROOT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub struct TestEnvironment {
    pub root: PathBuf,
    pub workspace: PathBuf,
    pub global_data: PathBuf,
    pub codex_home: PathBuf,
    fake_home: PathBuf,
    temp: PathBuf,
}

impl TestEnvironment {
    pub fn from_fixture(name: impl AsRef<Path>) -> io::Result<Self> {
        let root = unique_test_root()?;
        let workspace = root.join("workspace");
        let global_data = root.join("global-data");
        let codex_home = root.join("codex-home");
        let fake_home = root.join("home");
        let temp = root.join("tmp");

        for directory in [&global_data, &codex_home, &fake_home, &temp] {
            fs::create_dir(directory)?;
        }

        let fixture = repo_root().join("tests/fixtures/workspaces").join(name);
        copy_tree(&fixture, &workspace)?;

        Ok(Self {
            root,
            workspace,
            global_data,
            codex_home,
            fake_home,
            temp,
        })
    }

    pub fn command(&self) -> Command {
        self.command_in(&self.workspace)
    }

    pub fn command_in(&self, current_dir: &Path) -> Command {
        self.command_with(test_binary(), current_dir)
    }

    pub fn command_with(&self, executable: impl AsRef<Path>, current_dir: &Path) -> Command {
        let mut command = Command::new(executable.as_ref());
        // Start from a small process configuration, never the caller's host identity,
        // provider credentials, workspace binding or optional-worker capabilities.
        command.env_clear();
        for variable in [
            "PATH",
            "PATHEXT",
            "SYSTEMROOT",
            "SystemRoot",
            "WINDIR",
            "COMSPEC",
            "SystemDrive",
            "LANG",
            "LC_ALL",
            "LC_CTYPE",
            "TZ",
            "LD_LIBRARY_PATH",
            "DYLD_LIBRARY_PATH",
            "CARGO_HOME",
            "RUSTUP_HOME",
            "CARGO_BIN_EXE_magnesium",
            "MAGNESIUM_TEST_BINARY",
            "RUST_BACKTRACE",
        ] {
            if let Some(value) = env::var_os(variable) {
                command.env(variable, value);
            }
        }
        command
            .current_dir(current_dir)
            .env("HOME", &self.fake_home)
            .env("USERPROFILE", &self.fake_home)
            .env("XDG_DATA_HOME", &self.global_data)
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("LOCALAPPDATA", &self.global_data)
            .env("APPDATA", &self.global_data)
            .env("CODEX_HOME", &self.codex_home)
            .env("CODEX_TEST_BINARY", self.root.join("codex-not-installed"))
            .env(
                "OPENCODE_TEST_BINARY",
                self.root.join("opencode-not-installed"),
            )
            .env("TMPDIR", &self.temp)
            .env("TEMP", &self.temp)
            .env("TMP", &self.temp);
        command
    }

    /// The per-user Magnesium data directory, where `init` writes the plugin copies.
    pub fn data_dir(&self) -> PathBuf {
        #[cfg(target_os = "macos")]
        return self.fake_home.join("Library/Application Support/magnesium");

        #[cfg(not(target_os = "macos"))]
        self.global_data.join("magnesium")
    }

    /// Entries in the data directory other than the two that `init` manages.
    /// Magnesium keeps learner state in the workspace, never in a global store.
    pub fn unmanaged_data_entries(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.data_dir()) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name != "marketplace" && name != "claude-plugin")
            .collect();
        names.sort();
        names
    }

    pub fn opencode_config_dir(&self) -> PathBuf {
        self.root.join("config/opencode")
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn unique_test_root() -> io::Result<PathBuf> {
    let parent = env::temp_dir().join("magnesium-tests");
    fs::create_dir_all(&parent)?;

    loop {
        let sequence = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!("{}-{sequence}", process::id()));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

pub fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let current_dir = env::current_dir().ok();
    let executable = env::current_exe().ok();
    let mut candidates = manifest_dir
        .ancestors()
        .chain(current_dir.iter().flat_map(|path| path.ancestors()))
        .chain(executable.iter().flat_map(|path| path.ancestors()));
    candidates
        .find(|candidate| candidate.join("tests/fixtures/workspaces").is_dir())
        .expect(
            "could not locate Magnesium package root from manifest, working directory, or test binary",
        )
        .to_path_buf()
}

pub fn test_binary() -> PathBuf {
    let configured = env::var_os("MAGNESIUM_TEST_BINARY")
        .map(PathBuf::from)
        .or_else(|| env::var_os("CARGO_BIN_EXE_magnesium").map(PathBuf::from))
        .unwrap_or_else(|| repo_root().join("target/debug/magnesium"));
    if configured.is_absolute() {
        configured
    } else {
        repo_root().join(configured)
    }
}

#[cfg(windows)]
pub fn sanitize_windows_security_path(path: &Path) -> std::ffi::OsString {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let units: Vec<u16> = path.as_os_str().encode_wide().collect();
    let verbatim = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    let unc = [
        b'\\' as u16,
        b'\\' as u16,
        b'?' as u16,
        b'\\' as u16,
        b'U' as u16,
        b'N' as u16,
        b'C' as u16,
        b'\\' as u16,
    ];
    if units.starts_with(&unc) {
        let mut stripped = vec![b'\\' as u16, b'\\' as u16];
        stripped.extend_from_slice(&units[8..]);
        std::ffi::OsString::from_wide(&stripped)
    } else if units.starts_with(&verbatim) {
        std::ffi::OsString::from_wide(&units[4..])
    } else {
        path.as_os_str().to_os_string()
    }
}

pub fn codex_command(environment: &TestEnvironment) -> Command {
    let executable = env::var_os("CODEX_TEST_BINARY").unwrap_or_else(|| "codex".into());
    environment.command_with(executable, &environment.workspace)
}

pub fn codex_app_server_request(environment: &TestEnvironment, request: Value) -> Value {
    let expected_id = request.get("id").cloned().expect("app-server request ID");
    let mut child = codex_command(environment)
        .args(["app-server", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn Codex app-server");
    let mut stdin = child.stdin.take().expect("app-server stdin");
    let mut stdout = io::BufReader::new(child.stdout.take().expect("app-server stdout"));
    for message in [
        json!({
            "method": "initialize",
            "id": 1,
            "params": {
                "clientInfo": {"name": "magnesium-lifecycle-tests", "version": "1"},
                "capabilities": {}
            }
        }),
        json!({"method": "initialized", "params": {}}),
        request,
    ] {
        serde_json::to_writer(&mut stdin, &message).expect("serialize app-server request");
        stdin
            .write_all(b"\n")
            .expect("terminate app-server request");
        stdin.flush().expect("flush app-server request");
    }

    let response = loop {
        let mut line = String::new();
        let read = stdout
            .read_line(&mut line)
            .expect("read app-server response");
        assert!(read != 0, "app-server closed before its response");
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("id") == Some(&expected_id) {
            break value;
        }
    };
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    response
}

pub fn copy_tree(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

pub fn run_hook(environment: &TestEnvironment, event: &str, input: Value) -> process::Output {
    run_hook_in(environment, &environment.workspace, event, input)
}

pub fn run_hook_in(
    environment: &TestEnvironment,
    current_dir: &Path,
    event: &str,
    input: Value,
) -> process::Output {
    run_hook_in_with(environment, test_binary(), current_dir, event, input)
}

pub fn run_hook_in_with(
    environment: &TestEnvironment,
    executable: impl AsRef<Path>,
    current_dir: &Path,
    event: &str,
    input: Value,
) -> process::Output {
    use std::io::Write;

    let mut child = environment
        .command_with(executable, current_dir)
        .args([
            "_internal",
            "hook",
            event,
            "--plugin-version",
            "0.2.0",
            "--adapter-protocol-version",
            "2",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn hook process");
    child
        .stdin
        .take()
        .expect("piped hook stdin")
        .write_all(&serde_json::to_vec(&input).expect("serialize hook input"))
        .expect("write hook input");
    child.wait_with_output().expect("wait for hook process")
}

/// Hook invocation with the installed `--host-kind` flag, mirroring the
/// Claude Code plugin command line whose official event JSON carries no kind
/// field of its own.
pub fn run_hook_with_host_flag(
    environment: &TestEnvironment,
    current_dir: &Path,
    event: &str,
    input: Value,
    host_kind: &str,
) -> process::Output {
    use std::io::Write;

    let mut child = environment
        .command_with(test_binary(), current_dir)
        .args([
            "_internal",
            "hook",
            event,
            "--plugin-version",
            "0.2.0",
            "--adapter-protocol-version",
            "2",
            "--host-kind",
            host_kind,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn hook process");
    child
        .stdin
        .take()
        .expect("piped hook stdin")
        .write_all(&serde_json::to_vec(&input).expect("serialize hook input"))
        .expect("write hook input");
    child.wait_with_output().expect("wait for hook process")
}
