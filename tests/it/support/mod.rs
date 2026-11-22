use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

pub mod clock;
pub mod mcp;
pub mod snapshot;
pub mod sources;
pub mod v2;

pub const CONTROL_DIR: &str = ".magnesium";

pub const DEFERRED_RETRY: std::time::Duration = std::time::Duration::from_secs(30);

static NEXT_TEST_ROOT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub struct TestEnvironment {
    pub root: PathBuf,
    pub workspace: PathBuf,
    pub global_data: PathBuf,
    fake_home: PathBuf,
    temp: PathBuf,
}

impl TestEnvironment {
    pub fn from_fixture(name: impl AsRef<Path>) -> io::Result<Self> {
        let root = unique_test_root()?;
        let workspace = root.join("workspace");
        let global_data = root.join("global-data");
        let fake_home = root.join("home");
        let temp = root.join("tmp");

        for directory in [&global_data, &fake_home, &temp] {
            fs::create_dir(directory)?;
        }

        let fixture = repo_root().join("tests/fixtures/workspaces").join(name);
        copy_tree(&fixture, &workspace)?;

        Ok(Self {
            root,
            workspace,
            global_data,
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
            .env("TMPDIR", &self.temp)
            .env("TEMP", &self.temp)
            .env("TMP", &self.temp)
            .env("MAGNESIUM_UNSAFE_NO_SYNC", "1")
            .env("MAGNESIUM_NO_BACKGROUND_INDEX", "1");
        command
    }

    pub fn data_dir(&self) -> PathBuf {
        #[cfg(target_os = "macos")]
        return self.fake_home.join("Library/Application Support/magnesium");

        #[cfg(not(target_os = "macos"))]
        self.global_data.join("magnesium")
    }

    pub fn unmanaged_data_entries(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.data_dir()) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name != "claude-plugin")
            .collect();
        names.sort();
        names
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
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn test_binary() -> PathBuf {
    match env::var_os("MAGNESIUM_TEST_BINARY").map(PathBuf::from) {
        Some(configured) if configured.is_absolute() => configured,
        Some(configured) => repo_root().join(configured),
        None => PathBuf::from(env!("CARGO_BIN_EXE_magnesium")),
    }
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
    run_hook_with_host_flag(environment, &environment.workspace, event, input, "claude")
}

pub fn run_hook_without_host_flag(
    environment: &TestEnvironment,
    event: &str,
    input: Value,
) -> process::Output {
    spawn_hook(environment, &environment.workspace, event, &[], input)
}

pub fn run_hook_with_host_flag(
    environment: &TestEnvironment,
    current_dir: &Path,
    event: &str,
    input: Value,
    host_kind: &str,
) -> process::Output {
    spawn_hook(
        environment,
        current_dir,
        event,
        &["--host-kind", host_kind],
        input,
    )
}

fn spawn_hook(
    environment: &TestEnvironment,
    current_dir: &Path,
    event: &str,
    extra_args: &[&str],
    input: Value,
) -> process::Output {
    let mut child = environment
        .command_with(test_binary(), current_dir)
        .args([
            "_internal",
            "hook",
            event,
            "--plugin-version",
            magnesium_version(),
            "--adapter-protocol-version",
            "2",
        ])
        .args(extra_args)
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

pub fn magnesium_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
