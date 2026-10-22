//! Deny-only recovery for controls whose canonical SQLite write fails.
//! No learner text, history, expiry, permissions, or interpretation lives here.
//! Short host/thread admission locks serialize actual side effects with controls.
use crate::{learner::HostContext, workspace::Workspace};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
const DIRECTORY: &str = "control-safety";
#[cfg(unix)]
const ENTRY_FLUSHED: &str = "directory-entry.flushed";
const RECORDING: u8 = 1;
const LEARNING: u8 = 2;
pub(crate) struct Guard {
    _lock: fs::File,
    marker: PathBuf,
    flags: u8,
    initial_flags: u8,
}
fn failure(error: impl std::fmt::Display) -> String {
    format!(
        "control_safety_unavailable: {error}; capture remains denied until the metadata can be checked"
    )
}
impl Guard {
    pub(crate) fn acquire(workspace: &Workspace, host: &HostContext) -> Result<Self, String> {
        let directory = workspace
            .private_dir(Path::new(DIRECTORY))
            .map_err(failure)?;
        // Atomic marker publication flushes this directory, but a newly created
        // directory also needs its entry in the control directory flushed. Do this even when
        // another process won creation, before claiming a durable shared deny.
        #[cfg(unix)]
        ensure_entry_flushed(workspace.control(), &directory)?;
        let key = Sha256::digest(
            serde_json::to_vec(&[&host.host_kind, &host.host_thread_id]).map_err(failure)?,
        )
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
        let lock_path = directory.join(format!("{key}.admission.lock"));
        crate::workspace::reject_symlink_components(&lock_path, workspace.control())
            .map_err(failure)?;
        let lock = crate::workspace::open_private_lock(&lock_path).map_err(failure)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(error) => {
                    let error: io::Error = error.into();
                    if error.kind() != io::ErrorKind::WouldBlock || Instant::now() >= deadline {
                        return Err(failure("admission lock busy"));
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
        let marker = directory.join(format!("{key}.deny"));
        crate::workspace::reject_symlink_components(&marker, workspace.control())
            .map_err(failure)?;
        let flags = match fs::symlink_metadata(&marker) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
            Err(error) => return Err(failure(error)),
            Ok(metadata) => {
                if !metadata.file_type().is_file() || metadata.len() != 2 {
                    return Err(failure("invalid deny metadata"));
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if metadata.permissions().mode() & 0o077 != 0 {
                        return Err(failure("deny metadata must be owner-private"));
                    }
                }
                #[cfg(windows)]
                crate::workspace::validate_windows_private_path(&marker).map_err(failure)?;
                let mut bytes = Vec::new();
                fs::File::open(&marker)
                    .map_err(failure)?
                    .take(3)
                    .read_to_end(&mut bytes)
                    .map_err(failure)?;
                if bytes.len() != 2 || bytes[0] != 1 || bytes[1] & !(RECORDING | LEARNING) != 0 {
                    return Err(failure("invalid deny metadata"));
                }
                bytes[1]
            }
        };
        Ok(Self {
            _lock: lock,
            marker,
            flags,
            initial_flags: flags,
        })
    }
    pub(crate) fn recording_blocked(&self) -> bool {
        self.flags & RECORDING != 0
    }
    pub(crate) fn learning_blocked(&self) -> bool {
        self.flags & LEARNING != 0
    }
    pub(crate) fn capture_blocked(&self) -> bool {
        self.flags != 0
    }
    /// Persist the deny before attempting canonical stop/off. Keep the guard alive
    /// through the SQL outcome so a concurrent hook cannot admit stale settings.
    pub(crate) fn before_control(&mut self, action: &str) -> Result<(), String> {
        let bit = match action {
            "recording_off" => RECORDING,
            "pause" | "stop" => LEARNING,
            "recording_on" => return Ok(()),
            _ => return Err(failure("invalid control")),
        };
        self.write(self.flags | bit)
    }
    /// SQLite committed: its setting is canonical, so remove this operation's
    /// recovery deny. A cleanup error stays fail-closed and must be disclosed.
    pub(crate) fn after_control(&mut self, action: &str) -> Result<(), String> {
        let bit = match action {
            "recording_off" | "recording_on" => RECORDING,
            "pause" | "stop" => LEARNING,
            _ => return Err(failure("invalid control")),
        };
        self.write(self.flags & !bit)
    }
    /// Receipt replay has no new authority. Undo only this invocation's
    /// provisional deny, preserving every deny that existed on admission.
    pub(crate) fn after_replay(&mut self) -> Result<(), String> {
        if self.flags == self.initial_flags {
            return Ok(());
        }
        self.write(self.initial_flags)
    }
    pub(crate) fn after_resume(&mut self) -> Result<(), String> {
        self.write(self.flags & !LEARNING)
    }
    fn write(&mut self, flags: u8) -> Result<(), String> {
        if flags == 0 {
            #[cfg(unix)]
            {
                match fs::remove_file(&self.marker) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(failure(error)),
                }
                sync_directory(
                    self.marker
                        .parent()
                        .ok_or_else(|| failure("invalid marker path"))?,
                )?;
            }
            // Portable deletion has no write-through equivalent. Keep a bounded,
            // text-free zero-flag marker through the existing atomic writer;
            // Windows uses MoveFileExW(MOVEFILE_WRITE_THROUGH) for publication.
            #[cfg(not(unix))]
            crate::workspace::write_private_atomic(&self.marker, &[1, 0]).map_err(failure)?;
        } else {
            crate::workspace::write_private_atomic(&self.marker, &[1, flags]).map_err(failure)?;
        }
        self.flags = flags;
        Ok(())
    }
}

/// Flush `directory`'s entry in `control` once per directory pair, not on
/// every call. After a flush succeeds, a record inside `directory` names both
/// directories' identities. A process that reads a matching record knows a
/// flush already completed, because the record is written only after one
/// returns. Replacing either directory changes an identity, so the next call
/// flushes again. A missing or unreadable record also flushes again.
#[cfg(unix)]
fn ensure_entry_flushed(control: &Path, directory: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let identity = |path: &Path| {
        fs::symlink_metadata(path)
            .map(|metadata| format!("{}:{}", metadata.dev(), metadata.ino()))
            .map_err(failure)
    };
    let expected = format!("{} {}\n", identity(control)?, identity(directory)?);
    let record = directory.join(ENTRY_FLUSHED);
    crate::workspace::reject_symlink_components(&record, control).map_err(failure)?;
    let mut found = Vec::new();
    let recorded = fs::File::open(&record)
        .and_then(|file| file.take(128).read_to_end(&mut found))
        .is_ok_and(|_| found == expected.as_bytes());
    if !recorded {
        sync_directory(control)?;
        // The flush above is what this call needs. A record that fails to
        // write only means a later call flushes again.
        let _ = crate::workspace::write_private_atomic(&record, expected.as_bytes());
    }
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), String> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(failure)
}
