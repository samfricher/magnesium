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
#[cfg(not(test))]
use std::process::{Command, Stdio};

use crate::workspace::GlobalPaths;

/// The flag the window's program takes to schedule its notification again
/// and exit, with no window.
#[cfg_attr(test, allow(dead_code))]
pub(crate) const REFRESH_FLAG: &str = "--refresh-due-notifications";
/// Where the window's program looks for the runtime to run, before its PATH.
#[cfg_attr(test, allow(dead_code))]
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

/// The window's bundle identifier. A build of the window made with its own
/// identifier, as a test build is so that it never shares the installed
/// app's notifications, adds a part to it: "dev.magnesium.window.<part>".
const WINDOW_IDENTIFIER: &str = "dev.magnesium.window";

/// The string a plist's XML gives a key, as an app bundle's `Info.plist`
/// holds it: `<key>Name</key><string>value</string>`. A binary plist gives
/// none, and so is not accepted.
fn plist_string(plist: &str, key: &str) -> Option<String> {
    let after = plist.split_once(&format!("<key>{key}</key>"))?.1;
    let value = after.trim_start().strip_prefix("<string>")?;
    Some(value.split_once("</string>")?.0.trim().to_owned())
}

/// Only the Magnesium window's own program is ever started: an absolute
/// path to the main program of an app bundle whose `Info.plist` names the
/// window's identifier and that program. The file names something to run,
/// and nothing else may come to be run through it, however the file was
/// written. Any bundle can claim an identifier, so this keeps the runtime
/// from starting the wrong program by mistake; the file itself is written
/// only by the window, through `magnesium _internal window-notifier`.
fn accepted(program: &Path) -> bool {
    let Some(macos) = program.parent() else {
        return false;
    };
    let Some(contents) = macos.parent() else {
        return false;
    };
    if !(program.is_absolute()
        && program.is_file()
        && macos.file_name().is_some_and(|name| name == "MacOS")
        && contents.file_name().is_some_and(|name| name == "Contents"))
    {
        return false;
    }
    let Ok(plist) = fs::read_to_string(contents.join("Info.plist")) else {
        return false;
    };
    let identifier = plist_string(&plist, "CFBundleIdentifier").unwrap_or_default();
    let executable = plist_string(&plist, "CFBundleExecutable").unwrap_or_default();
    let window = identifier == WINDOW_IDENTIFIER
        || identifier
            .strip_prefix(WINDOW_IDENTIFIER)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|part| !part.is_empty() && !part.contains(['/', '<']));
    window
        && !executable.is_empty()
        && program
            .file_name()
            .is_some_and(|name| name == executable.as_str())
}

/// The window's program, and the folder of its own files when it named one:
/// the program schedules from that folder's library, which need not be the
/// one this runtime's home would give it.
#[cfg_attr(test, allow(dead_code))]
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

/// How many times this process's code asked for the window's program; only
/// unit tests count, since they never start it.
#[cfg(test)]
pub(crate) static ASKED_IN_TESTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// The record changed: start the window's program to schedule its
/// notification again, if the learner has it on.
pub(crate) fn record_changed() {
    // A unit test runs this code inside the test program, under the home of
    // whoever runs the tests: it must never read their window's file or
    // start their app. The tests of the built program (`tests/it`) run it
    // with a home of their own.
    #[cfg(test)]
    {
        ASKED_IN_TESTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    #[cfg(not(test))]
    start_named();
}

#[cfg(not(test))]
fn start_named() {
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
    use std::path::Path;

    fn bundle(root: &Path, identifier: &str, executable: &str) -> std::path::PathBuf {
        let contents = root.join(format!("{identifier}.app/Contents"));
        fs::create_dir_all(contents.join("MacOS")).unwrap();
        fs::write(
            contents.join("Info.plist"),
            format!(
                "<?xml version=\"1.0\"?>\n<plist version=\"1.0\"><dict>\n  <key>CFBundleExecutable</key>\n  <string>{executable}</string>\n  <key>CFBundleIdentifier</key>\n  <string>{identifier}</string>\n</dict></plist>\n"
            ),
        )
        .unwrap();
        let program = contents.join("MacOS").join(executable);
        fs::write(&program, "").unwrap();
        program
    }

    #[test]
    fn only_the_window_s_own_program_is_accepted() {
        let dir = std::env::temp_dir().join(format!("mg-notifier-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let root = &dir;
        let window = bundle(root, "dev.magnesium.window", "magnesium-window");
        assert!(accepted(&window));
        let test_build = bundle(
            root,
            "dev.magnesium.window.notify-check",
            "magnesium-window",
        );
        assert!(
            accepted(&test_build),
            "a build of the window with its own identifier"
        );
        let other_app = bundle(root, "com.example.other", "magnesium-window");
        assert!(!accepted(&other_app), "another app's bundle");
        let lookalike = bundle(root, "dev.magnesium.windowmaker", "magnesium-window");
        assert!(
            !accepted(&lookalike),
            "an identifier that only starts the same"
        );
        let second = window.with_file_name("helper");
        fs::write(&second, "").unwrap();
        assert!(!accepted(&second), "another program in the window's bundle");
        assert!(!accepted(Path::new(
            "dev.magnesium.window.app/Contents/MacOS/magnesium-window"
        )));
        let loose = root.join("magnesium-window");
        fs::write(&loose, "").unwrap();
        assert!(!accepted(&loose), "a program outside a bundle");
        fs::write(
            window
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("Info.plist"),
            "bplist00",
        )
        .unwrap();
        assert!(!accepted(&window), "a plist the runtime cannot read");
        let _ = fs::remove_dir_all(dir);
    }
}
