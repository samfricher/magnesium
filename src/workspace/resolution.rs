//! Resolve learner ownership exclusively from the nearest native workspace.
use super::{CONTROL_DIR, Workspace, config::WorkspaceConfig};
use crate::learner::store::LearnerStore;
use std::path::{Path, PathBuf};
use std::{fs, io};

#[derive(Clone, Debug)]
pub(crate) struct ResolvedWorkspace {
    workspace: Workspace,
    workspace_id: String,
    store_path: PathBuf,
}
impl ResolvedWorkspace {
    pub(crate) fn workspace(&self) -> &Workspace {
        &self.workspace
    }
    pub(crate) fn workspace_id(&self) -> &str {
        &self.workspace_id
    }
    pub(crate) fn store_path(&self) -> &Path {
        &self.store_path
    }
}
#[derive(Clone, Debug)]
pub(crate) enum WorkspaceResolution {
    Active(ResolvedWorkspace),
    Inactive,
}

pub(crate) fn resolve_initialized(start: &Path) -> io::Result<WorkspaceResolution> {
    let Some(workspace) = Workspace::discover(start)? else {
        return Ok(WorkspaceResolution::Inactive);
    };
    let config = WorkspaceConfig::load(&workspace.control().join("config.toml"))
        .map_err(io::Error::other)?;
    let workspace_id = config.workspace_id().to_owned();
    let store_path = workspace.store_path();
    super::reject_symlink_components(&store_path, workspace.control())?;
    LearnerStore::open(&store_path, &workspace_id).map_err(io::Error::other)?;
    Ok(WorkspaceResolution::Active(ResolvedWorkspace {
        workspace,
        workspace_id,
        store_path,
    }))
}

pub(super) fn nearest_boundary(start: &Path) -> io::Result<Option<PathBuf>> {
    let metadata = fs::metadata(start)?;
    let directory = if metadata.is_dir() {
        start
    } else {
        start.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "file has no parent directory")
        })?
    };
    let mut current = fs::canonicalize(directory)?;
    loop {
        let control = current.join(CONTROL_DIR);
        match fs::symlink_metadata(&control) {
            Ok(metadata) if metadata.file_type().is_dir() => return Ok(Some(current)),
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "Magnesium control path {} must be a real directory, not a file or symlink",
                        control.display()
                    ),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> io::Result<Self> {
            let path = std::env::temp_dir().join(format!(
                "magnesium-owner-{}",
                crate::workspace::os_random_hex(16)?
            ));
            fs::create_dir(&path)?;
            Ok(Self(path))
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn initialize(root: &Path, id: &str) {
        fs::create_dir_all(root.join(CONTROL_DIR)).unwrap();
        let mut config =
            WorkspaceConfig::new(root.join(CONTROL_DIR).join("config.toml"), id).unwrap();
        config.save().unwrap();
        LearnerStore::initialize(&root.join(CONTROL_DIR).join("state.sqlite"), id).unwrap();
    }
    fn active(path: &Path) -> ResolvedWorkspace {
        let WorkspaceResolution::Active(value) = resolve_initialized(path).unwrap() else {
            panic!("workspace missing")
        };
        value
    }
    #[test]
    fn subjects_share_parent_but_nested_workspace_is_independent() {
        let temp = TempDir::new().unwrap();
        initialize(temp.path(), "parent");
        fs::create_dir_all(temp.path().join("algebra/deep")).unwrap();
        fs::create_dir_all(temp.path().join("statistics")).unwrap();
        let algebra = active(&temp.path().join("algebra/deep"));
        let statistics = active(&temp.path().join("statistics"));
        assert_eq!(algebra.store_path(), statistics.store_path());
        initialize(&temp.path().join("statistics"), "nested");
        let nested = active(&temp.path().join("statistics"));
        assert_eq!(nested.workspace_id(), "nested");
        assert_ne!(algebra.store_path(), nested.store_path());
    }
    #[test]
    fn directory_move_preserves_local_identity_without_registry() {
        let temp = TempDir::new().unwrap();
        let original = temp.path().join("before");
        initialize(&original, "moving");
        let moved = temp.path().join("after");
        fs::rename(original, &moved).unwrap();
        let resolved = active(&moved);
        assert_eq!(resolved.workspace_id(), "moving");
        // Workspace discovery canonicalizes its root. On macOS, the temporary
        // path may use /var while the same directory resolves under /private/var.
        let canonical_moved = fs::canonicalize(&moved).unwrap();
        assert_eq!(
            resolved.store_path(),
            canonical_moved.join(CONTROL_DIR).join("state.sqlite")
        );
    }
    #[test]
    fn wrong_workspace_database_and_unknown_store_are_rejected_without_change() {
        let temp = TempDir::new().unwrap();
        initialize(temp.path(), "owner");
        let config_path = temp.path().join(CONTROL_DIR).join("config.toml");
        let mut config = WorkspaceConfig::new(config_path, "other").unwrap();
        config.save().unwrap();
        assert!(resolve_initialized(temp.path()).is_err());
        let db = temp.path().join(CONTROL_DIR).join("state.sqlite");
        fs::write(&db, b"unknown database").unwrap();
        assert!(resolve_initialized(temp.path()).is_err());
        assert_eq!(fs::read(db).unwrap(), b"unknown database");
    }
    #[cfg(unix)]
    #[test]
    fn managed_directory_cannot_follow_symlinks_or_parent_traversal() {
        use std::os::unix::fs::symlink;
        let temp = TempDir::new().unwrap();
        initialize(temp.path(), "owner");
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, temp.path().join(CONTROL_DIR).join("cache")).unwrap();
        let resolved = active(temp.path());
        assert!(
            resolved
                .workspace()
                .private_dir(Path::new("cache/host-ordinals"))
                .is_err()
        );
        assert!(
            resolved
                .workspace()
                .private_dir(Path::new("../outside"))
                .is_err()
        );
        assert!(!outside.join("host-ordinals").exists());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_control_and_database_symlinks() {
        use std::os::unix::fs::symlink;
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("target");
        initialize(&target, "target");
        let alias = temp.path().join("alias");
        fs::create_dir(&alias).unwrap();
        symlink(target.join(CONTROL_DIR), alias.join(CONTROL_DIR)).unwrap();
        assert!(resolve_initialized(&alias).is_err());
        let db = target.join(CONTROL_DIR).join("state.sqlite");
        let outside = temp.path().join("outside.sqlite");
        fs::rename(&db, &outside).unwrap();
        symlink(outside, db).unwrap();
        assert!(resolve_initialized(&target).is_err());
    }
}
