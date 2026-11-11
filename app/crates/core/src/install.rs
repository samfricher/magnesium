//! The `magnesium` program on this Mac.
//!
//! The Mac download carries the program inside the app, built from the same
//! commit as the window. On first open the window installs it, so the window,
//! the agent's hooks and a terminal all run one copy. A different copy that is
//! already installed is replaced only after the learner agrees, because a
//! newer program upgrades each study folder's record when it opens the folder.
//!
//! The window run from source carries no program, and uses the one installed,
//! as before.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Serialize;

use crate::paths;
use crate::runtime;
use crate::store::Store;

/// Set this to name the program inside the app in place of the one beside the
/// window's own binary. For tests.
pub const BUNDLED_ENV: &str = "MAGNESIUM_APP_BUNDLED";
/// Set this to install a new copy somewhere other than `~/.local/bin`. For
/// tests.
pub const TARGET_ENV: &str = "MAGNESIUM_APP_INSTALL_TO";

/// A `magnesium` program and what `--version` printed for it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Program {
    /// Where it is, with the home folder written as `~`.
    pub shown_path: String,
    #[serde(skip)]
    pub path: PathBuf,
    /// The version, with the commit the download's build adds after a `+`,
    /// such as `0.3.0+1a2b3c…`. Empty when the program did not answer.
    pub version: String,
}

impl Program {
    /// The version without the commit, such as `0.3.0`.
    pub fn number(&self) -> &str {
        self.version.split('+').next().unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    /// The program is installed, and it is the app's own copy or the app
    /// carries none.
    Ready,
    /// No program is installed and the app carries none to install.
    Missing,
    /// Another copy is installed. The window asks before replacing it.
    Different,
    /// The learner chose to keep another copy.
    Kept,
    /// The app's copy could not be installed, or does not run.
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct RuntimeSetup {
    pub state: RuntimeState,
    /// The copy lessons run: the installed one.
    pub installed: Option<Program>,
    /// The copy inside the app. None for the window run from source.
    pub bundled: Option<Program>,
    /// True when the window installed its copy just now.
    pub installed_now: bool,
    pub message: Option<String>,
}

fn shown(path: &Path) -> String {
    let home = paths::home();
    match path.strip_prefix(&home) {
        Ok(rest) if home != Path::new("/") => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

/// The program inside the app: beside the window's own binary, in
/// `Magnesium.app/Contents/MacOS`.
pub fn bundled_path() -> Option<PathBuf> {
    if let Some(named) = env::var_os(BUNDLED_ENV).filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(named)).filter(|p| p.is_file());
    }
    let window = env::current_exe().ok()?;
    let beside = window.parent()?.join("magnesium");
    beside.is_file().then_some(beside)
}

/// Where a new copy goes when none is installed: `~/.local/bin`, where the
/// install guide suggests and where Anthropic's installer puts `claude`. So
/// a terminal that finds `claude` finds `magnesium` too.
pub fn default_target() -> PathBuf {
    if let Some(named) = env::var_os(TARGET_ENV).filter(|v| !v.is_empty()) {
        return PathBuf::from(named);
    }
    paths::home().join(".local/bin/magnesium")
}

/// Ask a program for its version. A program that does not answer as
/// `magnesium <version>` gives an empty version.
pub fn identify(path: &Path) -> Program {
    let mut command = paths::light_command(path);
    command.arg("--version");
    let version = paths::run(command, Duration::from_secs(10))
        .ok()
        .and_then(|out| {
            out.lines()
                .next()?
                .trim()
                .strip_prefix("magnesium ")
                .map(|v| v.trim().to_owned())
        })
        .unwrap_or_default();
    Program {
        shown_path: shown(path),
        path: path.to_path_buf(),
        version,
    }
}

/// What is installed and what the app carries, without changing anything.
fn look(store: &Store) -> RuntimeSetup {
    let installed = runtime::program().map(|path| identify(&path));
    let bundled = bundled_path().map(|path| identify(&path));
    let mut setup = RuntimeSetup {
        state: RuntimeState::Ready,
        installed,
        bundled,
        installed_now: false,
        message: None,
    };
    setup.state = match (&setup.bundled, &setup.installed) {
        (Some(bundled), _) if bundled.version.is_empty() => {
            setup.message = Some(
                "The magnesium program inside the app does not run. Download the app again, or install Magnesium with the install guide."
                    .into(),
            );
            RuntimeState::Failed
        }
        (None, Some(_)) => RuntimeState::Ready,
        (None, None) => {
            setup.message = Some(
                "The magnesium program is not installed, or the window cannot find it. Install Magnesium, then open the window again."
                    .into(),
            );
            RuntimeState::Missing
        }
        (Some(_), None) => RuntimeState::Missing,
        (Some(bundled), Some(installed)) if installed.version == bundled.version => {
            RuntimeState::Ready
        }
        (Some(_), Some(installed))
            if !installed.version.is_empty()
                && store.kept_runtime().as_deref() == Some(installed.version.as_str()) =>
        {
            setup.message = Some(kept_message(installed));
            RuntimeState::Kept
        }
        (Some(_), Some(_)) => RuntimeState::Different,
    };
    setup
}

fn kept_message(installed: &Program) -> String {
    format!(
        "Lessons use the Magnesium already on this Mac, version {}. They teach with that version's rules, and it cannot open a folder that a newer Magnesium has upgraded.",
        installed.number()
    )
}

/// What is installed, after installing the app's copy if there was none. A
/// first copy needs no question: nothing is replaced.
pub fn check(store: &Store) -> RuntimeSetup {
    let setup = look(store);
    match (&setup.state, &setup.bundled) {
        (RuntimeState::Missing, Some(bundled)) => {
            let bundled = bundled.clone();
            finish(store, &bundled, &default_target())
        }
        _ => setup,
    }
}

/// Replace the installed copy with the app's own, in the same place, so one
/// copy stays on this Mac. The learner agreed to this.
pub fn replace(store: &Store) -> RuntimeSetup {
    let setup = look(store);
    let (Some(bundled), Some(installed)) = (&setup.bundled, &setup.installed) else {
        return check(store);
    };
    if setup.state == RuntimeState::Ready {
        return setup;
    }
    let (bundled, target) = (bundled.clone(), installed.path.clone());
    store.set_kept_runtime(None);
    finish(store, &bundled, &target)
}

/// Keep the installed copy. The window does not ask again for this version.
pub fn keep(store: &Store) -> RuntimeSetup {
    let setup = look(store);
    if setup.state != RuntimeState::Different {
        return setup;
    }
    if let Some(installed) = setup.installed.as_ref().filter(|p| !p.version.is_empty()) {
        store.set_kept_runtime(Some(&installed.version));
    }
    look(store)
}

fn finish(store: &Store, bundled: &Program, target: &Path) -> RuntimeSetup {
    match install(&bundled.path, target, &bundled.version) {
        Ok(()) => {
            let mut setup = look(store);
            // The window found a copy after all, but not this one: another
            // folder on the PATH comes first.
            if setup.state != RuntimeState::Ready {
                setup.state = RuntimeState::Failed;
                setup.message = Some(format!(
                    "Magnesium was installed in {}, but another copy at {} comes first on this Mac. Remove that copy, then open the window again.",
                    shown(target),
                    setup.installed.as_ref().map_or(String::new(), |p| p.shown_path.clone())
                ));
            } else {
                setup.installed_now = true;
            }
            setup
        }
        Err(why) => {
            let mut setup = look(store);
            setup.state = RuntimeState::Failed;
            setup.message = Some(format!(
                "The window could not install the magnesium program in {}: {why}",
                shown(target)
            ));
            setup
        }
    }
}

/// Write the program to `target`, whole, and check that the copy runs.
///
/// The bytes are written to a new file, not copied: a copy keeps the file's
/// extended attributes, and with them the download flag that macOS sets on a
/// downloaded app. macOS can then refuse to run the copy when the agent's
/// hooks call it. The new file replaces the old one in one step, so a program
/// that is running is never changed under it.
pub fn install(source: &Path, target: &Path, expected: &str) -> Result<(), String> {
    let folder = target.parent().ok_or("it has no folder")?;
    fs::create_dir_all(folder).map_err(|e| format!("{e}."))?;
    let bytes = fs::read(source).map_err(|e| format!("its copy could not be read ({e})."))?;
    let part = folder.join(format!(".magnesium-{}.part", std::process::id()));
    let _ = fs::remove_file(&part);
    let written = (|| -> std::io::Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o755);
        }
        let mut file = options.open(&part)?;
        file.write_all(&bytes)?;
        // The lint at the root of the repository sends every flush through
        // a helper of the runtime's, so that the runtime's tests can skip
        // it. The window links none of the runtime's code, and this flush is
        // the point: the program must be whole on disk before it takes the
        // place of the one installed.
        #[allow(clippy::disallowed_methods)]
        file.sync_all()
    })();
    if let Err(error) = written {
        let _ = fs::remove_file(&part);
        return Err(format!("{error}."));
    }
    clear_download_flag(&part);
    if let Err(error) = fs::rename(&part, target) {
        let _ = fs::remove_file(&part);
        return Err(format!("{error}."));
    }
    let copy = identify(target);
    if copy.version != expected {
        return Err(if copy.version.is_empty() {
            "the copy does not run.".to_owned()
        } else {
            format!("the copy reports version {}.", copy.version)
        });
    }
    Ok(())
}

/// Remove macOS's download flag, if the file has one.
fn clear_download_flag(path: &Path) {
    if !cfg!(target_os = "macos") {
        return;
    }
    let xattr = Path::new("/usr/bin/xattr");
    if xattr.is_file() {
        let mut command = Command::new(xattr);
        command.args(["-d", "com.apple.quarantine"]).arg(path);
        let _ = paths::run(command, Duration::from_secs(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn program(dir: &Path, name: &str, version: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\necho 'magnesium {version}'\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn a_copy_is_written_whole_runs_and_replaces_a_link_not_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let source = program(dir.path(), "inside", "0.3.0+abc");
        let elsewhere = program(dir.path(), "dev-build", "0.2.0");
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let target = bin.join("magnesium");
        std::os::unix::fs::symlink(&elsewhere, &target).unwrap();

        install(&source, &target, "0.3.0+abc").unwrap();
        assert!(!fs::symlink_metadata(&target)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(identify(&target).version, "0.3.0+abc");
        assert_eq!(
            identify(&elsewhere).version,
            "0.2.0",
            "the link's target is untouched"
        );
        let mode = fs::metadata(&target).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        let leftovers: Vec<_> = fs::read_dir(&bin)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".part"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn a_copy_that_does_not_report_the_version_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let source = program(dir.path(), "inside", "0.3.0+abc");
        let target = dir.path().join("bin/magnesium");
        let refused = install(&source, &target, "0.3.0+def").unwrap_err();
        assert!(refused.contains("0.3.0+abc"), "{refused}");
    }

    #[test]
    fn a_path_in_the_home_folder_is_shown_with_a_tilde() {
        let home = paths::home();
        assert_eq!(
            shown(&home.join(".local/bin/magnesium")),
            "~/.local/bin/magnesium"
        );
        assert_eq!(
            shown(Path::new("/opt/homebrew/bin/magnesium")),
            "/opt/homebrew/bin/magnesium"
        );
    }

    #[test]
    fn the_version_number_leaves_out_the_commit() {
        let program = Program {
            shown_path: String::new(),
            path: PathBuf::new(),
            version: "0.3.0+1a2b".into(),
        };
        assert_eq!(program.number(), "0.3.0");
    }
}
