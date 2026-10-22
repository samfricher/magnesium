#[cfg(unix)]
use std::collections::BTreeSet;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[cfg(not(unix))]
use atomicwrites::{AllowOverwrite, AtomicFile};

mod config;
mod init;
pub(crate) mod instruction_bundle;
mod paths;
mod resolution;

pub(crate) use init::{
    install_opencode_adapter, materialize_bundled_marketplace, refresh_and_verify_codex_plugin,
    refresh_claude_plugin, run_init, run_init_json,
};
pub(crate) use paths::{GlobalPaths, reject_symlink_components};
pub(crate) use paths::{decode_path as decode_native_path, encode_path as native_path_key};
pub(crate) use resolution::{ResolvedWorkspace, WorkspaceResolution, resolve_initialized};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};

const MAX_WORKSPACE_ID_BYTES: usize = 128;

/// The per-workspace control directory. Every control path derives from it.
pub(crate) const CONTROL_DIR: &str = ".magnesium";

pub(crate) fn source_index_path(root: &Path) -> PathBuf {
    root.join(CONTROL_DIR)
        .join("index")
        .join("source-search.sqlite")
}

#[derive(Clone, Debug)]
pub(crate) struct Workspace {
    root: PathBuf,
    control: PathBuf,
}

impl Workspace {
    pub(crate) fn discover(start: &Path) -> io::Result<Option<Self>> {
        Ok(resolution::nearest_boundary(start)?.map(|root| Self {
            control: root.join(CONTROL_DIR),
            root,
        }))
    }

    pub(crate) fn store_path(&self) -> PathBuf {
        self.control.join("state.sqlite")
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn control(&self) -> &Path {
        &self.control
    }

    pub(crate) fn id(&self) -> io::Result<String> {
        config::WorkspaceConfig::load(&self.control.join("config.toml"))
            .map(|config| config.workspace_id().to_owned())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub(crate) fn private_dir(&self, relative: &Path) -> io::Result<PathBuf> {
        let path = self.control.join(relative);
        reject_symlink_components(&path, &self.control)?;
        create_private_dir_all(&path)?;
        Ok(path)
    }
}

fn validate_workspace_id(value: &str) -> io::Result<()> {
    if value.is_empty()
        || value.len() > MAX_WORKSPACE_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "workspace ID must contain 1-128 ASCII letters, digits, '-' or '_'",
        ));
    }
    Ok(())
}

pub(crate) fn create_private_dir_all(path: &Path) -> io::Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(path)?;

    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    #[cfg(windows)]
    restrict_windows_path(path)?;
    Ok(())
}

/// Create exactly one private directory. Unlike `create_private_dir_all`, an
/// existing path is an error, which prevents a caller allocating an
/// unpredictable temporary path from accidentally reusing an attacker-created
/// directory.
pub(crate) fn create_private_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let mut builder = DirBuilder::new();
        builder.mode(0o700);
        builder.create(path)?;
    }
    #[cfg(not(unix))]
    fs::create_dir(path)?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    #[cfg(windows)]
    restrict_windows_path(path)?;
    Ok(())
}

pub(crate) fn os_random_hex(byte_count: usize) -> io::Result<String> {
    os_random_hex_with(byte_count, |bytes| {
        getrandom::getrandom(bytes).map_err(|error| io::Error::other(error.to_string()))
    })
}

fn os_random_hex_with<F>(byte_count: usize, fill: F) -> io::Result<String>
where
    F: FnOnce(&mut [u8]) -> io::Result<()>,
{
    let mut bytes = vec![0_u8; byte_count];
    fill(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Atomic file replacements whose directory flushes wait for `finish`.
///
/// Each file is written to a temporary file in its own directory, flushed,
/// and renamed over the target, so a reader sees the old or the new file and
/// never a partial one. `finish` flushes each directory once, after all of
/// its files are in place: every file is durable when `finish` returns, as
/// with one flushed replacement per file. A batch dropped on an error path
/// still flushes the directories of the files it already replaced. Windows
/// keeps `atomicwrites`, whose write-through move needs no directory flush.
#[derive(Default)]
#[must_use = "replaced files are durable only after `finish`"]
pub(crate) struct AtomicWrites {
    #[cfg(unix)]
    directories: BTreeSet<PathBuf>,
}

impl AtomicWrites {
    /// Replace `path` with an owner-only (0600) file.
    pub(crate) fn replace(
        &mut self,
        path: &Path,
        write: impl FnOnce(&mut fs::File) -> io::Result<()>,
    ) -> io::Result<()> {
        self.replace_with_mode(path, 0o600, write)
    }

    /// Replace `path` with a file created with `mode`, which the umask
    /// filters as for any new file. Windows ignores `mode`.
    pub(crate) fn replace_with_mode(
        &mut self,
        path: &Path,
        #[cfg_attr(not(unix), allow(unused_variables))] mode: u32,
        write: impl FnOnce(&mut fs::File) -> io::Result<()>,
    ) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let directory = match path.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => parent,
                _ => Path::new("."),
            };
            let temporary = directory.join(format!(".magnesium-write-{}.tmp", os_random_hex(8)?));
            let written = (|| {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(mode)
                    .open(&temporary)?;
                write(&mut file)?;
                file.sync_all()?;
                drop(file);
                fs::rename(&temporary, path)
            })();
            if written.is_err() {
                let _ = fs::remove_file(&temporary);
            }
            written?;
            self.directories.insert(directory.to_path_buf());
            Ok(())
        }
        #[cfg(not(unix))]
        AtomicFile::new(path, AllowOverwrite)
            .write(write)
            .map_err(io::Error::from)
    }

    /// `replace` with owner-only permissions, creating private parent
    /// directories as needed.
    pub(crate) fn write_private(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file has no parent"))?;
        create_private_dir_all(parent)?;
        self.replace(path, |file| file.write_all(bytes))?;
        restrict_private_path(path)
    }

    pub(crate) fn finish(self) -> io::Result<()> {
        #[cfg(unix)]
        {
            let mut writes = self;
            for directory in std::mem::take(&mut writes.directories) {
                fs::File::open(&directory)?.sync_all()?;
            }
        }
        Ok(())
    }
}

impl Drop for AtomicWrites {
    fn drop(&mut self) {
        #[cfg(unix)]
        for directory in &self.directories {
            let _ = fs::File::open(directory).and_then(|directory| directory.sync_all());
        }
    }
}

pub(crate) fn write_private_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut writes = AtomicWrites::default();
    writes.write_private(path, bytes)?;
    writes.finish()
}

pub(crate) fn write_user_owned_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    #[cfg(unix)]
    let existing_mode = if path.try_exists()? {
        use std::os::unix::fs::PermissionsExt;
        Some(fs::symlink_metadata(path)?.permissions().mode())
    } else {
        None
    };

    #[cfg(windows)]
    let existing_descriptor = if path.try_exists()? {
        use windows_permissions::constants::{SeObjectType, SecurityInformation};
        use windows_permissions::wrappers::GetNamedSecurityInfo;
        let sec_path = sanitize_windows_security_path(path);
        Some(GetNamedSecurityInfo(
            &sec_path,
            SeObjectType::SE_FILE_OBJECT,
            SecurityInformation::Dacl,
        )?)
    } else {
        None
    };

    // A new user-owned file gets the umask default, as any editor would
    // create it; an existing file keeps its mode below.
    let mut writes = AtomicWrites::default();
    writes.replace_with_mode(path, 0o666, |file| file.write_all(bytes))?;
    writes.finish()?;

    #[cfg(unix)]
    if let Some(mode) = existing_mode {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }

    #[cfg(windows)]
    if let Some(ref descriptor) = existing_descriptor {
        use windows_permissions::constants::{SeObjectType, SecurityInformation};
        if let Some(dacl) = descriptor.dacl() {
            let sec_path = sanitize_windows_security_path(path);
            windows_permissions::wrappers::SetNamedSecurityInfo(
                &sec_path,
                SeObjectType::SE_FILE_OBJECT,
                SecurityInformation::Dacl,
                None,
                None,
                Some(dacl),
                None,
            )?;
        }
    }

    Ok(())
}

pub(crate) fn open_private_lock(path: &Path) -> io::Result<fs::File> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file has no parent"))?;
    create_private_dir_all(parent)?;

    let mut create = OpenOptions::new();
    create.read(true).write(true).create_new(true);
    #[cfg(unix)]
    create.mode(0o600);
    match create.open(path) {
        Ok(file) => {
            restrict_private_path(path)?;
            Ok(file)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let mut existing = OpenOptions::new();
            existing.read(true).write(true);
            let file = existing.open(path)?;
            // Creation applies the Windows DACL. Rewriting it while another
            // process holds the lock file fails with ERROR_ACCESS_DENIED.
            #[cfg(unix)]
            restrict_private_path(path)?;
            Ok(file)
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn restrict_private_path(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    #[cfg(windows)]
    restrict_windows_path(path)?;
    Ok(())
}

#[cfg(windows)]
pub(crate) fn validate_windows_private_path(path: &Path) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    use windows_permissions::Trustee;
    use windows_permissions::constants::{
        AccessRights, AceFlags, AceType, SeObjectType, SecurityInformation,
    };
    use windows_permissions::utilities::current_process_sid;
    use windows_permissions::wrappers::GetNamedSecurityInfo;

    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || metadata.file_attributes() & 0x0000_0400 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "private Windows path cannot be a symbolic link or reparse point",
        ));
    }
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "private Windows path is not a regular file or directory",
        ));
    }
    let sid = current_process_sid()?;
    let sec_path = sanitize_windows_security_path(path);
    let descriptor = GetNamedSecurityInfo(
        &sec_path,
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Dacl,
    )?;
    let dacl = descriptor.dacl().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "private Windows path has no DACL",
        )
    })?;
    if dacl.len() != 1 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "private Windows path has unexpected inherited or broad permissions",
        ));
    }
    let ace = dacl
        .get_ace(0)
        .ok_or_else(|| io::Error::other("private Windows DACL entry is missing"))?;
    let flags = ace.flags();
    let expected_inheritance = AceFlags::ContainerInherit | AceFlags::ObjectInherit;
    let valid_flags = if metadata.is_dir() {
        flags == expected_inheritance || flags == expected_inheritance | AceFlags::Inherited
    } else {
        flags.is_empty() || flags == AceFlags::Inherited
    };
    let owner_ace = ace.ace_type() == AceType::ACCESS_ALLOWED_ACE_TYPE
        && ace.sid() == Some(sid.as_ref())
        && ace.mask() == AccessRights::FileAllAccess;
    let trustee: Trustee<'_> = sid.as_ref().into();
    if !owner_ace
        || !valid_flags
        || !dacl
            .effective_rights(&trustee)?
            .contains(AccessRights::FileAllAccess)
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "private Windows path does not grant only the current user the required effective permissions",
        ));
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn sanitize_windows_security_path(path: &Path) -> std::ffi::OsString {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let units: Vec<u16> = path.as_os_str().encode_wide().collect();
    std::ffi::OsString::from_wide(&strip_verbatim_wide(&units))
}

#[cfg(windows)]
fn strip_verbatim_wide(units: &[u16]) -> Vec<u16> {
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
        stripped
    } else if units.starts_with(&verbatim) {
        units[4..].to_vec()
    } else {
        units.to_vec()
    }
}

#[cfg(windows)]
fn restrict_windows_path(path: &Path) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    use windows_permissions::constants::{SeObjectType, SecurityInformation};
    use windows_permissions::utilities::current_process_sid;
    use windows_permissions::wrappers::SetNamedSecurityInfo;
    use windows_permissions::{LocalBox, SecurityDescriptor};

    let sid = current_process_sid()?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || metadata.file_attributes() & 0x0000_0400 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "private Windows path cannot be a symbolic link or reparse point",
        ));
    }
    let is_dir = metadata.is_dir();
    let (sddl, sec_info) = if is_dir {
        (
            format!("D:P(A;OICI;FA;;;{sid})"),
            SecurityInformation::Dacl | SecurityInformation::ProtectedDacl,
        )
    } else {
        (
            format!("D:P(A;;FA;;;{sid})"),
            SecurityInformation::Dacl | SecurityInformation::ProtectedDacl,
        )
    };
    let descriptor: LocalBox<SecurityDescriptor> = sddl.parse()?;
    let dacl = descriptor
        .dacl()
        .ok_or_else(|| io::Error::other("private Windows DACL is missing"))?;
    let sec_path = sanitize_windows_security_path(path);
    SetNamedSecurityInfo(
        &sec_path,
        SeObjectType::SE_FILE_OBJECT,
        sec_info,
        None,
        None,
        Some(dacl),
        None,
    )?;
    validate_windows_private_path(path)
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::os_random_hex_with;

    #[test]
    fn os_random_failure_is_propagated_without_a_fallback() {
        let error = os_random_hex_with(16, |bytes| {
            bytes[..4].fill(0x5a);
            Err(io::Error::other("test CSPRNG unavailable"))
        })
        .expect_err("CSPRNG failure must fail closed");
        assert_eq!(error.to_string(), "test CSPRNG unavailable");
    }

    #[cfg(unix)]
    #[test]
    fn private_file_creation_sets_owner_only_mode_before_sensitive_write() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::sync::atomic::{AtomicU64, Ordering};

        use super::{create_private_dir, write_private_atomic};

        static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

        let root = std::env::temp_dir().join(format!(
            "magnesium-private-write-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        create_private_dir(&root).expect("create private fixture directory");
        assert!(
            create_private_dir(&root).is_err(),
            "private temporary allocation must not reuse an existing path"
        );
        let file = root.join("secret.json");
        write_private_atomic(&file, b"private learner bytes").expect("write private file");
        assert_eq!(
            fs::metadata(&file)
                .expect("stat private file")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::remove_dir_all(root).expect("remove private write fixture");
    }

    #[cfg(unix)]
    #[test]
    fn atomic_writes_replace_whole_files_and_leave_no_temporary_files() {
        use std::fs;
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        use super::{AtomicWrites, create_private_dir, write_private_atomic};

        let root = std::env::temp_dir().join(format!(
            "magnesium-atomic-writes-{}-{}",
            std::process::id(),
            super::os_random_hex(4).expect("random fixture suffix")
        ));
        create_private_dir(&root).expect("create atomic write fixture");
        let one = root.join("notes/one.md");
        let two = root.join("papers/two.md");
        let mut writes = AtomicWrites::default();
        writes.write_private(&one, b"first").expect("write one");
        writes.write_private(&two, b"second").expect("write two");
        writes.finish().expect("flush both directories");
        write_private_atomic(&one, b"replaced").expect("replace one");

        let mut writes = AtomicWrites::default();
        writes
            .replace(&one, |file| {
                file.write_all(b"partial")?;
                Err(io::Error::other("write interrupted"))
            })
            .expect_err("a failed write must not replace the file");
        writes.finish().expect("nothing to flush");

        assert_eq!(fs::read(&one).expect("read one"), b"replaced");
        assert_eq!(fs::read(&two).expect("read two"), b"second");
        for (directory, name) in [("notes", "one.md"), ("papers", "two.md")] {
            let names = fs::read_dir(root.join(directory))
                .expect("list directory")
                .map(|entry| entry.expect("directory entry").file_name())
                .collect::<Vec<_>>();
            assert_eq!(names, [name], "{directory} holds a temporary file");
        }
        for path in [&one, &two] {
            let mode = fs::metadata(path).expect("stat note").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        fs::remove_dir_all(root).expect("remove atomic write fixture");
    }
}
