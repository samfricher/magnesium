//! The learner's log across every course in the library: the turns the
//! window kept, and the answers the runtime's record marked. It is read
//! without opening any folder, so a lesson in the open folder goes on.
//!
//! The window draws each day as a page of this. Nothing here is a score: it
//! is what was said and what the record says of it, nothing added.

use std::path::Path;

use serde::Serialize;

use crate::runtime::{self, Mark};
use crate::store::{Store, Turn};

/// How many turns of one course the log keeps, newest kept.
const TURNS_PER_COURSE: usize = 2000;

/// One course's part of the log.
#[derive(Clone, Debug, Serialize)]
pub struct Course {
    pub folder: String,
    pub name: String,
    /// Every turn the window kept for this folder since `since_ms`, in every
    /// conversation, oldest first.
    pub turns: Vec<Turn>,
    /// The record's marked answers, oldest first. None when the runtime
    /// cannot say.
    pub marks: Option<Vec<Mark>>,
}

/// The log of every folder in `folders`, from `since_ms` (ms since the epoch)
/// on. A folder with nothing kept and nothing marked is left out.
pub fn read(folders: &[String], store: &Store, since_ms: i64) -> Vec<Course> {
    std::thread::scope(|scope| {
        let jobs: Vec<_> = folders
            .iter()
            .map(|folder| {
                scope.spawn(move || {
                    let path = Path::new(folder);
                    let mut turns: Vec<Turn> = store
                        .all_turns(path)
                        .into_iter()
                        .filter(|turn| turn.ms >= since_ms)
                        .collect();
                    let extra = turns.len().saturating_sub(TURNS_PER_COURSE);
                    turns.drain(..extra);
                    let marks = runtime::marks(path);
                    Course {
                        folder: folder.clone(),
                        name: path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| folder.clone()),
                        turns,
                        marks,
                    }
                })
            })
            .collect();
        jobs.into_iter()
            .filter_map(|job| job.join().ok())
            .filter(|course| {
                !course.turns.is_empty() || course.marks.as_ref().is_some_and(|m| !m.is_empty())
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_conversation_of_a_folder_is_in_its_log_and_old_turns_are_not() {
        let home = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let store = Store::at(home.path().to_path_buf());
        let turn = |session: &str, ms: i64| Turn {
            session: session.into(),
            ms,
            learner: format!("said at {ms}"),
            tutor: "reply".into(),
            interrupted: false,
        };
        store.append_turn(folder.path(), &turn("a", 1_000));
        store.append_turn(folder.path(), &turn("a", 5_000));
        store.append_turn(folder.path(), &turn("b", 9_000));
        let key = folder.path().to_string_lossy().into_owned();
        let log = read(&[key], &store, 2_000);
        assert_eq!(log.len(), 1);
        let ms: Vec<i64> = log[0].turns.iter().map(|t| t.ms).collect();
        assert_eq!(ms, vec![5_000, 9_000]);
    }

    #[test]
    fn a_folder_with_nothing_kept_is_left_out() {
        let home = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let store = Store::at(home.path().to_path_buf());
        let key = folder.path().to_string_lossy().into_owned();
        assert!(read(&[key], &store, 0).is_empty());
    }
}
