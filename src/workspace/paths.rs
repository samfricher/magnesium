use std::io;
use std::path::{Component, Path, PathBuf};

use directories::BaseDirs;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GlobalPaths {
    root: PathBuf,
}

impl GlobalPaths {
    pub(crate) fn resolve() -> io::Result<Self> {
        #[cfg(windows)]
        {
            if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
                if !local_app_data.is_empty() {
                    return Ok(Self::from_data_dir(Path::new(&local_app_data)));
                }
            }
        }
        if let Some(base) = BaseDirs::new() {
            return Ok(Self::from_data_dir(base.data_local_dir()));
        }
        #[cfg(windows)]
        {
            if let Some(app_data) = std::env::var_os("APPDATA") {
                if !app_data.is_empty() {
                    return Ok(Self::from_data_dir(Path::new(&app_data)));
                }
            }
            if let Some(user_profile) = std::env::var_os("USERPROFILE") {
                if !user_profile.is_empty() {
                    return Ok(Self::from_data_dir(
                        &Path::new(&user_profile).join("AppData/Local"),
                    ));
                }
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the operating system did not provide a per-user data directory",
        ))
    }

    pub(crate) fn from_data_dir(data_dir: &Path) -> Self {
        Self {
            root: data_dir.join("magnesium"),
        }
    }

    /// Whether `magnesium init` has written plugin copies on this machine.
    pub(crate) fn installed(&self) -> bool {
        self.root.exists()
    }

    pub(crate) fn marketplace(&self) -> PathBuf {
        self.root.join("marketplace")
    }

    /// Claude Code loads this directory in place as a one-plugin marketplace.
    pub(crate) fn claude_plugin(&self) -> PathBuf {
        self.root.join("claude-plugin")
    }
}

/// Reject symlinks only in the managed portion of a path. Platform aliases
/// above `boundary` (such as `/tmp`) are outside the control boundary.
pub(crate) fn reject_symlink_components(path: &Path, boundary: &Path) -> io::Result<()> {
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("managed path {} contains parent traversal", path.display()),
        ));
    }
    if !path.starts_with(boundary) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "managed path {} is outside {}",
                path.display(),
                boundary.display()
            ),
        ));
    }
    let mut current = path;
    loop {
        match std::fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "managed path {} contains a symbolic link",
                        current.display()
                    ),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if current == boundary {
            return Ok(());
        }
        current = current.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("managed path {} has no boundary", path.display()),
            )
        })?;
    }
}

#[cfg(unix)]
pub(crate) fn encode_path(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
pub(crate) fn decode_path(bytes: &[u8]) -> Result<PathBuf, String> {
    use std::os::unix::ffi::OsStringExt;

    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec())))
}

#[cfg(windows)]
pub(crate) fn encode_path(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(windows)]
pub(crate) fn decode_path(bytes: &[u8]) -> Result<PathBuf, String> {
    use std::os::windows::ffi::OsStringExt;

    if !bytes.len().is_multiple_of(2) {
        return Err("Windows native path has an odd byte length".to_owned());
    }
    let wide: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    Ok(PathBuf::from(std::ffi::OsString::from_wide(&wide)))
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn encode_path(path: &Path) -> Vec<u8> {
    path.as_os_str().to_string_lossy().as_bytes().to_vec()
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn decode_path(bytes: &[u8]) -> Result<PathBuf, String> {
    String::from_utf8(bytes.to_vec())
        .map(PathBuf::from)
        .map_err(|_| "native path is not valid UTF-8".to_owned())
}
