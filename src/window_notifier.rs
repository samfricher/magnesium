//! The study window's daily notification is scheduled ahead with the Mac's
//! own notification centre, which delivers it whether or not the window is
//! open. It names what is due, so it would say something false if the record
//! changed after it was scheduled: an answer marked in a terminal lesson in
//! the evening would leave tomorrow's notification naming a check already
//! answered. So while the notification is on, the window names its own
//! program here, and after each change to the record this runtime commits it
//! starts that program to schedule the notification again from the record.
//!
//! The runtime starts it and does not wait: a lesson never waits on the
//! window. Nothing happens when the file is absent, which is when the
//! notification is off or the window was never installed.
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::workspace::GlobalPaths;

/// The flag the window's program takes to schedule its notification again
/// and exit, with no window.
pub(crate) const REFRESH_FLAG: &str = "--refresh-due-notifications";
/// Where the window's program looks for the runtime to run, before its PATH.
const WINDOW_RUNTIME_ENV: &str = "MAGNESIUM_APP_RUNTIME";

/// Name the window's program and the folder it keeps its own files in, or
/// with None forget them.
pub(crate) fn set(program: Option<&Path>, home: Option<&Path>) -> io::Result<()> {
    let paths = GlobalPaths::resolve()?;
    let file = paths.window_notifier();
    match program {
        Some(program) => {
            if !accepted(program) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "the program is not an app's own program inside its bundle",
                ));
            }
            if let Some(dir) = file.parent() {
                fs::create_dir_all(dir)?;
            }
            let mut text = format!("{}\n", program.to_string_lossy());
            if let Some(home) = home.filter(|home| home.is_absolute()) {
                text.push_str(&format!("{}\n", home.to_string_lossy()));
            }
            fs::write(&file, text)
        }
        None => match fs::remove_file(&file) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        },
    }
}

/// Only an absolute path to a program inside an app bundle's
/// `Contents/MacOS`, next to its `Info.plist`, is ever started: the file
/// names something to run, and nothing else may come to be run through it.
fn accepted(program: &Path) -> bool {
    let Some(macos) = program.parent() else {
        return false;
    };
    program.is_absolute()
        && program.is_file()
        && macos.file_name().is_some_and(|name| name == "MacOS")
        && macos.parent().is_some_and(|contents| {
            contents.file_name().is_some_and(|name| name == "Contents")
                && contents.join("Info.plist").is_file()
        })
}

/// The window's program, and the folder of its own files when it named one:
/// the program schedules from that folder's library, which need not be the
/// one this runtime's home would give it.
fn named() -> Option<(PathBuf, Option<PathBuf>)> {
    let file = GlobalPaths::resolve().ok()?.window_notifier();
    let text = fs::read_to_string(file).ok()?;
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let program = PathBuf::from(lines.next()?);
    let home = lines
        .next()
        .map(PathBuf::from)
        .filter(|home| home.is_absolute() && home.is_dir());
    accepted(&program).then_some((program, home))
}

/// The record changed: start the window's program to schedule its
/// notification again, if the learner has it on.
pub(crate) fn record_changed() {
    let Some((program, home)) = named() else {
        return;
    };
    let mut command = Command::new(program);
    command.arg(REFRESH_FLAG);
    // It reads the record through this very runtime, the one that changed
    // it, not whichever `magnesium` its PATH would find first.
    if let Ok(runtime) = std::env::current_exe() {
        command.env(WINDOW_RUNTIME_ENV, runtime);
    }
    if let Some(home) = home {
        command.arg(home);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Its own process group, so a host that stops this runtime's group when
    // a conversation ends does not stop the scheduling half done.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    if let Ok(mut child) = command.spawn() {
        // Waited for apart, so a long-lived server leaves no finished child.
        std::thread::spawn(move || child.wait());
    }
}

pub(crate) fn print_set(program: Option<&str>, home: Option<&str>) -> std::process::ExitCode {
    match set(program.map(Path::new), home.map(Path::new)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("magnesium: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::accepted;
    use std::fs;

    #[test]
    fn only_an_app_s_own_program_is_accepted() {
        let root = std::env::temp_dir().join(format!("mg-notifier-{}", std::process::id()));
        let macos = root.join("Magnesium.app/Contents/MacOS");
        fs::create_dir_all(&macos).unwrap();
        fs::write(root.join("Magnesium.app/Contents/Info.plist"), "<plist/>").unwrap();
        let program = macos.join("magnesium-window");
        fs::write(&program, "").unwrap();
        assert!(accepted(&program));
        assert!(!accepted(std::path::Path::new(
            "Magnesium.app/Contents/MacOS/magnesium-window"
        )));
        let loose = root.join("magnesium-window");
        fs::write(&loose, "").unwrap();
        assert!(!accepted(&loose), "a program outside a bundle");
        assert!(
            !accepted(&macos.join("missing")),
            "a program that is not there"
        );
        fs::remove_file(root.join("Magnesium.app/Contents/Info.plist")).unwrap();
        assert!(!accepted(&program), "a bundle with no Info.plist");
        let _ = fs::remove_dir_all(root);
    }
}
