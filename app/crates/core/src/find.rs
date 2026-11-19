use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const MAX_DEPTH: usize = 4;
const MAX_FOLDERS: usize = 30_000;

const SKIPPED: [&str; 14] = [
    "Library",
    "Applications",
    "Music",
    "Movies",
    "Pictures",
    "Public",
    "node_modules",
    "target",
    "build",
    "dist",
    "venv",
    "vendor",
    "Pods",
    "DerivedData",
];

fn is_study_folder(folder: &Path) -> bool {
    let control = folder.join(".magnesium");
    control.join("state.sqlite").is_file() || control.join("config.toml").is_file()
}

pub fn study_folders(start: &Path, limit: Duration) -> Vec<PathBuf> {
    let began = Instant::now();
    let mut found = Vec::new();
    let mut seen = 0usize;
    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::from([(start.to_path_buf(), 0)]);
    while let Some((folder, depth)) = queue.pop_front() {
        if began.elapsed() > limit || seen >= MAX_FOLDERS {
            break;
        }
        seen += 1;
        if depth > 0 && is_study_folder(&folder) {
            found.push(folder);
            continue;
        }
        if depth >= MAX_DEPTH {
            continue;
        }
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        let mut children: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                !name.starts_with('.')
                    && !name.ends_with(".app")
                    && !name.ends_with(".photoslibrary")
                    && !SKIPPED.contains(&name.as_ref())
            })
            .map(|entry| entry.path())
            .collect();
        children.sort();
        queue.extend(children.into_iter().map(|child| (child, depth + 1)));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn study(root: &Path, path: &str) -> PathBuf {
        let folder = root.join(path);
        fs::create_dir_all(folder.join(".magnesium")).unwrap();
        fs::write(folder.join(".magnesium/config.toml"), "").unwrap();
        folder
    }

    #[test]
    fn study_folders_are_found_nearest_first() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let deep = study(root, "School/Year 2/Statistics");
        let near = study(root, "Chemistry");
        fs::create_dir_all(root.join("School/Notes")).unwrap();
        assert_eq!(
            study_folders(root, Duration::from_secs(5)),
            vec![near, deep]
        );
    }

    #[test]
    fn hidden_skipped_and_too_deep_folders_are_not_searched() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        study(root, ".hidden/Course");
        study(root, "Library/Course");
        study(root, "code/node_modules/Course");
        study(root, "a/b/c/d/Course");
        let outer = study(root, "Course");
        study(root, "Course/Inner");
        fs::create_dir_all(root.join("Half/.magnesium")).unwrap();
        assert_eq!(study_folders(root, Duration::from_secs(5)), vec![outer]);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_folder_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        study(elsewhere.path(), "Course");
        std::os::unix::fs::symlink(elsewhere.path(), dir.path().join("link")).unwrap();
        assert!(study_folders(dir.path(), Duration::from_secs(5)).is_empty());
    }

    #[test]
    fn the_start_folder_itself_is_searched_even_when_it_is_a_study_folder() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".magnesium")).unwrap();
        fs::write(dir.path().join(".magnesium/config.toml"), "").unwrap();
        let inner = study(dir.path(), "Course");
        assert_eq!(
            study_folders(dir.path(), Duration::from_secs(5)),
            vec![inner]
        );
    }
}
