//! The study folder as a workspace of documents: its lessons, the files under
//! `Study/`, and the one write the learner can make from the window, to a
//! plan file.
//!
//! Everything here reads, except [`write_plan`], which writes only a plan file
//! that already exists, and only when it has not changed since the learner
//! read it. The record, the gap log and `.magnesium/` are never touched.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Component, Path};
use std::time::UNIX_EPOCH;

use serde::Serialize;

use crate::store::Turn;
use crate::study::looks_like_a_key;

/// The most files listed, newest first.
const MAX_FILES: usize = 400;
/// The most entries looked at while listing.
const MAX_LOOKED_AT: usize = 5000;
/// The deepest folder looked into under `Study/`.
const MAX_DEPTH: usize = 5;
/// The largest plan the window writes, as the readers read it.
const MAX_PLAN_BYTES: usize = 256 * 1024;

/// One conversation the window kept, as a lesson the learner can open again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LessonInfo {
    pub session: String,
    /// When its first and last turns were kept, in ms since the epoch.
    pub first_ms: i64,
    pub last_ms: i64,
    pub turns: usize,
    /// The first thing the learner said, cut to one line: what it was about.
    pub opening: String,
    /// Whether it is the folder's conversation, the one Continue resumes.
    pub current: bool,
}

/// The lessons in a folder's kept turns, newest first.
pub fn lessons(turns: &[Turn], current: Option<&str>) -> Vec<LessonInfo> {
    let mut by_session: BTreeMap<&str, LessonInfo> = BTreeMap::new();
    for turn in turns {
        if turn.session.is_empty() {
            continue;
        }
        let entry = by_session
            .entry(turn.session.as_str())
            .or_insert_with(|| LessonInfo {
                session: turn.session.clone(),
                first_ms: turn.ms,
                last_ms: turn.ms,
                turns: 0,
                opening: one_line(&turn.learner),
                current: current == Some(turn.session.as_str()),
            });
        entry.first_ms = entry.first_ms.min(turn.ms);
        entry.last_ms = entry.last_ms.max(turn.ms);
        entry.turns += 1;
        if entry.opening.is_empty() {
            entry.opening = one_line(&turn.learner);
        }
    }
    let mut out: Vec<LessonInfo> = by_session.into_values().collect();
    out.sort_by_key(|lesson| std::cmp::Reverse(lesson.last_ms));
    out
}

fn one_line(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let line = line.trim();
    if line.chars().count() > 120 {
        let cut: String = line.chars().take(117).collect();
        format!("{}…", cut.trim_end())
    } else {
        line.to_owned()
    }
}

/// One file under `Study/`, as the window lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FileInfo {
    /// From the study root: "Study/materials/Week 4 slides.pdf".
    pub path: String,
    /// When it last changed, in ms since the epoch.
    pub changed_ms: i64,
}

/// What `Study/` holds, newest first. Hidden files, links and answer keys are
/// left out; the keys are counted, never named.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Files {
    pub files: Vec<FileInfo>,
    pub keys: usize,
    /// More were there than are listed.
    pub more: bool,
}

pub fn files(root: &Path) -> Files {
    let mut found = Vec::new();
    let mut keys = 0;
    let mut looked = 0;
    let study = root.join("Study");
    walk(root, &study, 0, &mut found, &mut keys, &mut looked);
    found.sort_by(|a: &FileInfo, b| b.changed_ms.cmp(&a.changed_ms).then(a.path.cmp(&b.path)));
    let more = found.len() > MAX_FILES || looked >= MAX_LOOKED_AT;
    found.truncate(MAX_FILES);
    Files {
        files: found,
        keys,
        more,
    }
}

fn walk(
    root: &Path,
    dir: &Path,
    depth: usize,
    found: &mut Vec<FileInfo>,
    keys: &mut usize,
    looked: &mut usize,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        *looked += 1;
        if *looked >= MAX_LOOKED_AT {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            walk(root, &path, depth + 1, found, keys, looked);
            continue;
        }
        if !kind.is_file() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        if looks_like_a_key(relative) {
            *keys += 1;
            continue;
        }
        found.push(FileInfo {
            path: relative.to_string_lossy().replace('\\', "/"),
            changed_ms: changed_ms(&path).unwrap_or(0),
        });
    }
}

fn changed_ms(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let ms = modified.duration_since(UNIX_EPOCH).ok()?.as_millis();
    i64::try_from(ms).ok()
}

/// Whether a path from the study root names a plan file: `Study/<plan>/plan.md`.
fn is_plan_path(path: &str) -> bool {
    let parts: Vec<Component> = Path::new(path).components().collect();
    matches!(
        parts.as_slice(),
        [Component::Normal(study), Component::Normal(target), Component::Normal(file)]
            if *study == "Study" && *file == "plan.md" && !target.to_string_lossy().starts_with('.')
    )
}

/// Write the learner's own edit of a plan file. Only `Study/<plan>/plan.md`,
/// only a file that is already there and is not a link, and only when it
/// still has the time it had when the learner read it (`expected_ms`), so an
/// edit the tutor made meanwhile is never written over. Returns the file's new
/// time.
pub fn write_plan(root: &Path, path: &str, text: &str, expected_ms: i64) -> Result<i64, String> {
    if !is_plan_path(path) {
        return Err("Only a plan file can be edited here.".into());
    }
    if text.len() > MAX_PLAN_BYTES {
        return Err("That plan is too long to save.".into());
    }
    let file = root.join(path);
    let meta = fs::symlink_metadata(&file)
        .map_err(|_| "That plan file is not there any more.".to_owned())?;
    if !meta.is_file() {
        return Err("That plan file is not a plain file, so it is left as it is.".into());
    }
    if changed_ms(&file) != Some(expected_ms) {
        return Err("The plan changed since you opened it. Your edit is kept here; open the plan again to see the change.".into());
    }
    let parent = file.parent().ok_or("That plan file has no folder.")?;
    let temporary = parent.join(".plan.md.window-edit");
    {
        let mut out = fs::File::create(&temporary).map_err(|e| e.to_string())?;
        out.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    }
    fs::rename(&temporary, &file).map_err(|e| {
        let _ = fs::remove_file(&temporary);
        e.to_string()
    })?;
    changed_ms(&file).ok_or_else(|| "The plan was saved, but its time could not be read.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(session: &str, ms: i64, learner: &str) -> Turn {
        Turn {
            session: session.into(),
            ms,
            learner: learner.into(),
            tutor: "ok".into(),
            interrupted: false,
        }
    }

    #[test]
    fn lessons_are_grouped_by_conversation_newest_first() {
        let turns = [
            turn("a", 10, "Teach me sampling"),
            turn("a", 20, "b = 5"),
            turn("b", 30, "\n  Confidence intervals please\nmore"),
            turn("", 40, "no session"),
        ];
        let found = lessons(&turns, Some("a"));
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].session, "b");
        assert_eq!(found[0].opening, "Confidence intervals please");
        assert!(!found[0].current);
        assert_eq!(found[1].turns, 2);
        assert_eq!((found[1].first_ms, found[1].last_ms), (10, 20));
        assert!(found[1].current);
    }

    #[test]
    fn files_lists_study_without_keys_hidden_files_or_links() {
        let root = tempfile::tempdir().unwrap();
        let study = root.path().join("Study");
        fs::create_dir_all(study.join("materials/Week 4")).unwrap();
        fs::create_dir_all(study.join("midterm-2/papers")).unwrap();
        fs::write(study.join("materials/Week 4/slides.pdf"), b"x").unwrap();
        fs::write(study.join("midterm-2/plan.md"), b"# Plan").unwrap();
        fs::write(study.join("midterm-2/papers/PaperB-key.pdf"), b"x").unwrap();
        fs::write(study.join("materials/.DS_Store"), b"x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/hosts", study.join("materials/hosts")).unwrap();
        let listed = files(root.path());
        let paths: Vec<&str> = listed.files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"Study/materials/Week 4/slides.pdf"));
        assert!(paths.contains(&"Study/midterm-2/plan.md"));
        assert_eq!(paths.len(), 2);
        assert_eq!(listed.keys, 1);
    }

    #[test]
    fn a_plan_is_written_only_when_unchanged_and_only_a_plan() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Study/midterm-2");
        fs::create_dir_all(&dir).unwrap();
        let plan = dir.join("plan.md");
        fs::write(&plan, "# Midterm 2\n").unwrap();
        let read_at = changed_ms(&plan).unwrap();

        assert!(write_plan(root.path(), "Study/midterm-2/gap-log.md", "x", read_at).is_err());
        assert!(write_plan(root.path(), "Study/../plan.md", "x", read_at).is_err());
        assert!(write_plan(root.path(), "Study/other/plan.md", "x", read_at).is_err());
        assert!(write_plan(root.path(), "Study/midterm-2/plan.md", "x", read_at - 5000).is_err());
        assert_eq!(fs::read_to_string(&plan).unwrap(), "# Midterm 2\n");

        let saved = write_plan(
            root.path(),
            "Study/midterm-2/plan.md",
            "# Midterm 2\n\nMoved.\n",
            read_at,
        )
        .unwrap();
        assert!(saved > 0);
        assert_eq!(
            fs::read_to_string(&plan).unwrap(),
            "# Midterm 2\n\nMoved.\n"
        );
        assert!(!dir.join(".plan.md.window-edit").exists());
    }
}
