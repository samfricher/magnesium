use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
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

pub fn warm_up() {
    std::thread::spawn(|| {
        child_path();
    });
}

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
    command.env_remove("CLAUDECODE");
    command.env_remove("CLAUDE_CODE_ENTRYPOINT");
    command
}

pub fn command(program: &Path) -> Command {
    let mut command = Command::new(program);
    command.env("PATH", child_path());
    command.env("CLAUDE_CODE_SYNC_PLUGIN_INSTALL", "1");
    without_agent_markers(command)
}

pub fn light_command(program: &Path) -> Command {
    let mut command = Command::new(program);
    command.env("PATH", quick_path());
    without_agent_markers(command)
}

pub fn run(command: Command, limit: Duration) -> Result<String, String> {
    let (status, out, err) = run_to_end(command, limit)?;
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

pub fn run_to_end(
    mut command: Command,
    limit: Duration,
) -> Result<(ExitStatus, String, String), String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
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
    Ok((status, out, err))
}
