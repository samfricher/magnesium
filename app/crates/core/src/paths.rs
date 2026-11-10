//! Finding the learner's programs.
//!
//! A window opened from the Finder gets a short PATH, without the folders
//! where `claude`, `magnesium` and `tectonic` usually live. Magnesium's hooks
//! run `magnesium` by name and fail silently when it is missing, so the agent
//! must be started with the PATH a terminal would have.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub fn home() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn usual_folders() -> Vec<PathBuf> {
    let home = home();
    let mut folders = vec![
        home.join(".local/bin"),
        home.join(".cargo/bin"),
        home.join(".claude/local"),
        home.join(".bun/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    // The newest Node that nvm installed, where an npm-installed agent lives.
    if let Ok(entries) = std::fs::read_dir(home.join(".nvm/versions/node")) {
        let mut versions: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
        versions.sort();
        if let Some(newest) = versions.last() {
            folders.push(newest.join("bin"));
        }
    }
    folders
}

fn looks_like_a_terminal_path(path: &str) -> bool {
    path.contains("/.local/bin") || path.contains("homebrew") || path.contains("/.cargo/bin")
}

/// The PATH of the learner's login shell, or None when asking took too long.
fn shell_path() -> Option<String> {
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut child = Command::new(shell)
        .args(["-ilc", "printf '\\n__MG_PATH__%s__MG_PATH__\\n' \"$PATH\""])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() > Duration::from_secs(4) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => return None,
        }
    }
    let output = child.wait_with_output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let start = text.find("__MG_PATH__")? + "__MG_PATH__".len();
    let end = text[start..].find("__MG_PATH__")? + start;
    let found = text[start..end].trim();
    (!found.is_empty()).then(|| found.to_owned())
}

/// The PATH given to every program the window starts.
pub fn child_path() -> OsString {
    static PATH: OnceLock<OsString> = OnceLock::new();
    PATH.get_or_init(|| {
        let current = env::var("PATH").unwrap_or_default();
        let mut folders: Vec<PathBuf> = Vec::new();
        let mut add = |folder: PathBuf| {
            if !folder.as_os_str().is_empty() && !folders.contains(&folder) {
                folders.push(folder);
            }
        };
        env::split_paths(&current).for_each(&mut add);
        if !looks_like_a_terminal_path(&current) {
            if let Some(from_shell) = shell_path() {
                env::split_paths(&from_shell).for_each(&mut add);
            }
        }
        usual_folders().into_iter().for_each(&mut add);
        env::join_paths(folders).unwrap_or_else(|_| OsString::from(current))
    })
    .clone()
}

/// The window's own PATH plus the usual folders. Known at once, with no
/// question to the shell, so the entrance does not wait on it.
fn quick_path() -> OsString {
    static PATH: OnceLock<OsString> = OnceLock::new();
    PATH.get_or_init(|| {
        let current = env::var("PATH").unwrap_or_default();
        let mut folders: Vec<PathBuf> = env::split_paths(&current).collect();
        for folder in usual_folders() {
            if !folders.contains(&folder) {
                folders.push(folder);
            }
        }
        env::join_paths(folders).unwrap_or_else(|_| OsString::from(current))
    })
    .clone()
}

/// Work out the terminal's PATH in the background, so it is ready by the
/// time the learner starts a lesson.
pub fn warm_up() {
    std::thread::spawn(|| {
        child_path();
    });
}

/// Where a program is. The usual folders are searched first. Only a program
/// that is in none of them makes the window ask the shell for its PATH.
pub fn find_program(name: &str) -> Option<PathBuf> {
    let direct = Path::new(name);
    if direct.components().count() > 1 {
        return direct.is_file().then(|| direct.to_path_buf());
    }
    let search = |path: &OsString| {
        env::split_paths(path)
            .map(|folder| folder.join(name))
            .find(|candidate| candidate.is_file())
    };
    search(&quick_path()).or_else(|| search(&child_path()))
}

fn without_agent_markers(mut command: Command) -> Command {
    // These tell an agent it is running inside another agent's terminal.
    command.env_remove("CLAUDECODE");
    command.env_remove("CLAUDE_CODE_ENTRYPOINT");
    command
}

/// A command for the agent, with the PATH a terminal would have. The agent's
/// hooks and tools run other programs by name, so it needs the whole PATH.
pub fn command(program: &Path) -> Command {
    let mut command = Command::new(program);
    command.env("PATH", child_path());
    // A study folder names its plugin in its settings. In a terminal the
    // agent installs it when the folder is first opened. Without a terminal
    // it installs in the background, and the first reply could come before
    // the plugin. This documented switch makes the agent finish first.
    command.env("CLAUDE_CODE_SYNC_PLUGIN_INSTALL", "1");
    without_agent_markers(command)
}

/// A command for a short helper such as `magnesium digest`, which needs only
/// to be found. It does not wait for the shell.
pub fn light_command(program: &Path) -> Command {
    let mut command = Command::new(program);
    command.env("PATH", quick_path());
    without_agent_markers(command)
}

/// Run a short command and return its standard output, or a plain error.
pub fn run(mut command: Command, limit: Duration) -> Result<String, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    // Read both pipes on threads, so a chatty program cannot fill one and stall.
    let mut stdout = child.stdout.take().ok_or("no output pipe")?;
    let mut stderr = child.stderr.take().ok_or("no error pipe")?;
    let out = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut stdout, &mut text);
        text
    });
    let err = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut stderr, &mut text);
        text
    });
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if started.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("it took too long".into());
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let out = out.join().unwrap_or_default();
    let err = err.join().unwrap_or_default();
    if status.success() {
        Ok(out)
    } else {
        let message = err.trim();
        Err(if message.is_empty() {
            format!("it ended with {status}")
        } else {
            message.lines().last().unwrap_or(message).to_owned()
        })
    }
}
