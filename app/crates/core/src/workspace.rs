//! A study folder as a workspace: the sessions the window kept turns of, and
//! the library of workspaces with when each was last opened.
//!
//! Everything here reads the window's own store; nothing reads or writes the
//! record. `lessons` is ported from the workspace editor concept (#68).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::store::{SessionNote, Store, Turn};

/// One conversation the window kept, as a session the learner can open again.
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
    /// Its name, as the window kept it when it began: its goal.
    pub title: Option<String>,
    /// When the learner last opened it, in ms since the epoch: its last turn,
    /// or a later look at it.
    pub opened_ms: i64,
    /// The goal of the record's lesson this conversation is bound to, when the
    /// record says; not the folder's last goal, which a lesson in a terminal
    /// may have set.
    pub goal: Option<String>,
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
                title: None,
                opened_ms: turn.ms,
                goal: None,
            });
        entry.first_ms = entry.first_ms.min(turn.ms);
        entry.last_ms = entry.last_ms.max(turn.ms);
        entry.opened_ms = entry.opened_ms.max(turn.ms);
        entry.turns += 1;
        if entry.opening.is_empty() {
            entry.opening = one_line(&turn.learner);
        }
    }
    let mut out: Vec<LessonInfo> = by_session.into_values().collect();
    out.sort_by_key(|lesson| std::cmp::Reverse(lesson.last_ms));
    out
}

/// The lessons, with what the window noted of each: its name and when it was
/// last opened. A conversation the window noted but kept no turn of yet (one
/// just begun) is listed too. Newest opened first.
pub fn sessions(
    turns: &[Turn],
    current: Option<&str>,
    notes: &BTreeMap<String, SessionNote>,
) -> Vec<LessonInfo> {
    let mut out = lessons(turns, current);
    for lesson in &mut out {
        if let Some(note) = notes.get(&lesson.session) {
            lesson.title = note.title.clone().filter(|t| !t.trim().is_empty());
            lesson.opened_ms = lesson.opened_ms.max(note.opened_ms.unwrap_or(0));
        }
    }
    for (id, note) in notes {
        if out.iter().any(|lesson| &lesson.session == id) {
            continue;
        }
        let Some(opened) = note.opened_ms else {
            continue;
        };
        out.push(LessonInfo {
            session: id.clone(),
            first_ms: opened,
            last_ms: opened,
            turns: 0,
            opening: String::new(),
            current: current == Some(id.as_str()),
            title: note.title.clone().filter(|t| !t.trim().is_empty()),
            opened_ms: opened,
            goal: None,
        });
    }
    out.sort_by_key(|lesson| std::cmp::Reverse(lesson.opened_ms));
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

/// One workspace in the library, read from the window's own store alone: no
/// runtime is asked, so the list is drawn at once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Workspace {
    pub folder: String,
    pub name: String,
    /// When the learner last opened it, or its newest session, in ms since
    /// the epoch; none for a folder found but never opened.
    pub opened_ms: Option<i64>,
    /// Its most recently opened session, if it has one.
    pub latest: Option<LessonInfo>,
}

/// Every folder in the library, most recently opened first.
pub fn workspaces(store: &Store) -> Vec<Workspace> {
    let mut out: Vec<Workspace> = store
        .recent()
        .into_iter()
        .map(|folder| {
            let path = Path::new(&folder);
            let state = store.folder(path);
            let latest = sessions(
                &store.all_turns(path),
                state.session_id.as_deref(),
                &state.sessions,
            )
            .into_iter()
            .next();
            let opened_ms = match (state.opened_ms, latest.as_ref().map(|l| l.opened_ms)) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            };
            Workspace {
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| folder.clone()),
                folder,
                opened_ms,
                latest,
            }
        })
        .collect();
    // The library's own order is most recently opened first; a folder whose
    // newest session is more recent than another's opening goes before it.
    out.sort_by_key(|w| std::cmp::Reverse(w.opened_ms.unwrap_or(0)));
    out
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
            ..Turn::default()
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
    fn sessions_take_their_names_and_last_opening_from_the_notes() {
        let turns = [
            turn("a", 10, "Teach me sampling"),
            turn("b", 30, "Intervals"),
        ];
        let mut notes = BTreeMap::new();
        notes.insert(
            "a".to_owned(),
            SessionNote {
                title: Some("Sampling distributions".into()),
                opened_ms: Some(50),
                engine: None,
            },
        );
        // Begun, with no turn kept yet.
        notes.insert(
            "c".to_owned(),
            SessionNote {
                title: Some("Eigenvalues".into()),
                opened_ms: Some(40),
                engine: None,
            },
        );
        let found = sessions(&turns, Some("c"), &notes);
        let order: Vec<&str> = found.iter().map(|s| s.session.as_str()).collect();
        assert_eq!(order, ["a", "c", "b"]);
        assert_eq!(found[0].title.as_deref(), Some("Sampling distributions"));
        assert_eq!(found[0].opened_ms, 50);
        assert!(found[1].current);
        assert_eq!(found[1].turns, 0);
        assert_eq!(found[2].title, None);
    }

    #[test]
    fn workspaces_are_listed_most_recently_opened_first() {
        let home = tempfile::tempdir().unwrap();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let store = Store::at(home.path().to_path_buf());
        store.opened(first.path());
        std::thread::sleep(std::time::Duration::from_millis(5));
        store.opened(second.path());
        store.note_session(first.path(), "s1", |note| {
            note.title = Some("Sampling".into());
            note.opened_ms = Some(i64::MAX / 2);
        });
        let found = workspaces(&store);
        assert_eq!(found.len(), 2);
        // The first folder's session was opened after the second folder.
        assert_eq!(
            found[0].folder,
            first.path().canonicalize().unwrap().to_string_lossy()
        );
        assert_eq!(
            found[0].latest.as_ref().unwrap().title.as_deref(),
            Some("Sampling")
        );
        assert!(found[1].latest.is_none());
        assert!(found[1].opened_ms.is_some());
    }
}
