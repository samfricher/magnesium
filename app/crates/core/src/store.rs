//! What the window itself remembers, kept on this computer and nowhere else:
//! which folders are in the library, which conversation each one was in, the
//! line the learner stopped at, the turns the window showed, the days the
//! learner studied in it, how the learner chose to pay for lessons, and an
//! installed `magnesium` they chose to keep. Never a credential.
//!
//! None of it is learning state. The learner record belongs to the runtime.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::activity;
use crate::agent::Payment;
use crate::paths;

/// How many folders the library keeps.
const LIBRARY_SIZE: usize = 24;

/// Set this to keep the window's files somewhere else. For tests.
pub const HOME_ENV: &str = "MAGNESIUM_APP_HOME";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PaperSitting {
    pub target: String,
    pub name: String,
    pub path: Option<String>,
    pub minutes: u32,
    pub started_ms: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FolderState {
    /// The agent's id of the conversation this folder was last in.
    #[serde(default)]
    pub session_id: Option<String>,
    /// The engine that started that conversation. None for one an earlier
    /// window started, which was the plugin engine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<Engine>,
    /// The learner turned recording off in a direct lesson in this study
    /// folder, and has not turned it on again. Kept by study root.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recording_off: bool,
    #[serde(default)]
    pub stopped_at: Option<String>,
    #[serde(default)]
    pub stopped_ms: Option<i64>,
    #[serde(default)]
    pub paper: Option<PaperSitting>,
    /// What a direct lesson keeps between window starts about its sitting:
    /// when the learner last wrote, what the sitting taught, and the
    /// repairs and checks it listed. Kept by the direct engine, opaque here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Data {
    #[serde(default)]
    recent: Vec<String>,
    #[serde(default)]
    folders: BTreeMap<String, FolderState>,
    #[serde(default)]
    block_minutes: Option<u32>,
    /// Answers the learner sent from the window on each day, by "2026-11-10".
    /// Used for the run only when the runtime cannot say which days had a
    /// marked answer.
    #[serde(default)]
    days: BTreeMap<String, u32>,
    /// Whether `days` was filled once from the turns kept before it existed.
    #[serde(default)]
    days_filled: bool,
    #[serde(default)]
    payment: Option<Payment>,
    /// The engine the learner picked for new conversations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    engine: Option<Engine>,
    /// What `--version` printed for an installed `magnesium` the learner chose
    /// to keep over the one inside the app. Asked again for any other.
    #[serde(default)]
    kept_runtime: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub session: String,
    pub ms: i64,
    pub learner: String,
    pub tutor: String,
    #[serde(default)]
    pub interrupted: bool,
}

/// Which engine runs a lesson.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    /// Claude Code runs the lesson through Magnesium's plugin.
    #[default]
    Plugin,
    /// The window runs the lesson, and Claude Code only reaches the model.
    Direct,
}

pub struct Store {
    root: PathBuf,
    data: Mutex<Data>,
}

fn default_root() -> PathBuf {
    if let Some(named) = env::var_os(HOME_ENV).filter(|v| !v.is_empty()) {
        return PathBuf::from(named);
    }
    let home = paths::home();
    if cfg!(target_os = "macos") {
        // Named after the app's identifier. The runtime keeps its own files
        // in "Application Support/magnesium", and macOS ignores case, so a
        // folder called "Magnesium" would be the same folder.
        home.join("Library/Application Support/dev.magnesium.window")
    } else if let Some(data) = env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        PathBuf::from(data).join("magnesium-window")
    } else {
        home.join(".local/share/magnesium-window")
    }
}

fn key(folder: &Path) -> String {
    folder
        .canonicalize()
        .unwrap_or_else(|_| folder.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// A short stable name for a folder's transcript file.
fn file_name(folder_key: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in folder_key.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let name: String = Path::new(folder_key)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect();
    format!("{name}-{hash:016x}.jsonl")
}

impl Store {
    pub fn open() -> Self {
        Self::at(default_root())
    }

    pub fn at(root: PathBuf) -> Self {
        let data = fs::read_to_string(root.join("state.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            root,
            data: Mutex::new(data),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Data> {
        self.data.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn save(&self, data: &Data) {
        let Ok(text) = serde_json::to_string_pretty(data) else {
            return;
        };
        if fs::create_dir_all(&self.root).is_err() {
            return;
        }
        // Written beside the real file, then moved over it, so a crash leaves
        // the old state whole.
        let temp = self.root.join("state.json.tmp");
        if fs::write(&temp, text).is_ok() {
            let _ = fs::rename(&temp, self.root.join("state.json"));
        }
    }

    pub fn recent(&self) -> Vec<String> {
        self.lock()
            .recent
            .iter()
            .filter(|folder| Path::new(folder).is_dir())
            .cloned()
            .collect()
    }

    pub fn opened(&self, folder: &Path) {
        let key = key(folder);
        let mut data = self.lock();
        data.recent.retain(|other| other != &key);
        data.recent.insert(0, key);
        data.recent.truncate(LIBRARY_SIZE);
        self.save(&data);
    }

    /// Put folders in the library without opening them, after the ones
    /// already there. Returns how many were new.
    pub fn add_folders(&self, folders: &[PathBuf]) -> usize {
        let mut data = self.lock();
        let mut added = 0;
        for folder in folders {
            let key = key(folder);
            if data.recent.len() < LIBRARY_SIZE && !data.recent.contains(&key) {
                data.recent.push(key);
                added += 1;
            }
        }
        if added > 0 {
            self.save(&data);
        }
        added
    }

    /// Take a folder out of the library. The folder itself is not touched,
    /// and what the window remembers of it stays, so a lesson continues if
    /// the folder is opened again.
    pub fn forget(&self, folder: &Path) {
        let key = key(folder);
        let plain = folder.to_string_lossy();
        let mut data = self.lock();
        data.recent.retain(|other| other != &key && *other != plain);
        self.save(&data);
    }

    pub fn folder(&self, folder: &Path) -> FolderState {
        self.lock()
            .folders
            .get(&key(folder))
            .cloned()
            .unwrap_or_default()
    }

    pub fn update(&self, folder: &Path, change: impl FnOnce(&mut FolderState)) {
        let mut data = self.lock();
        change(data.folders.entry(key(folder)).or_default());
        self.save(&data);
    }

    pub fn block_minutes(&self) -> u32 {
        self.lock().block_minutes.unwrap_or(50).clamp(5, 240)
    }

    pub fn set_block_minutes(&self, minutes: u32) {
        let mut data = self.lock();
        data.block_minutes = Some(minutes.clamp(5, 240));
        self.save(&data);
    }

    /// Count one answer the learner sent on `day`.
    pub fn count_turn(&self, day: NaiveDate) {
        let mut data = self.lock();
        self.fill_days(&mut data);
        *data.days.entry(activity::key(day)).or_default() += 1;
        self.save(&data);
    }

    /// The days on which the learner sent an answer from the window.
    pub fn days(&self) -> BTreeSet<NaiveDate> {
        let mut data = self.lock();
        if !data.days_filled {
            self.fill_days(&mut data);
            self.save(&data);
        }
        data.days
            .iter()
            .filter(|(_, answers)| **answers > 0)
            .filter_map(|(day, _)| activity::parse(day))
            .collect()
    }

    /// Once, count the days from the turns the window kept before it counted
    /// days, so a learner's first run is not lost.
    fn fill_days(&self, data: &mut Data) {
        if data.days_filled {
            return;
        }
        data.days_filled = true;
        let Ok(files) = fs::read_dir(self.root.join("transcripts")) else {
            return;
        };
        for file in files.filter_map(Result::ok) {
            let Ok(text) = fs::read_to_string(file.path()) else {
                continue;
            };
            for turn in text
                .lines()
                .filter_map(|line| serde_json::from_str::<Turn>(line).ok())
                .filter(|turn| !turn.learner.trim().is_empty())
            {
                if let Some(time) = chrono::DateTime::from_timestamp_millis(turn.ms) {
                    let day = time.with_timezone(&Local).date_naive();
                    *data.days.entry(activity::key(day)).or_default() += 1;
                }
            }
        }
    }

    pub fn payment(&self) -> Payment {
        self.lock().payment.unwrap_or_default()
    }

    pub fn set_payment(&self, payment: Payment) {
        let mut data = self.lock();
        data.payment = Some(payment);
        self.save(&data);
    }

    pub fn engine(&self) -> Engine {
        self.lock().engine.unwrap_or_default()
    }

    pub fn set_engine(&self, engine: Engine) {
        let mut data = self.lock();
        data.engine = Some(engine);
        self.save(&data);
    }

    pub fn kept_runtime(&self) -> Option<String> {
        self.lock().kept_runtime.clone()
    }

    pub fn set_kept_runtime(&self, version: Option<&str>) {
        let mut data = self.lock();
        data.kept_runtime = version.map(str::to_owned);
        self.save(&data);
    }

    fn transcript(&self, folder: &Path) -> PathBuf {
        self.root.join("transcripts").join(file_name(&key(folder)))
    }

    /// Keep one finished turn. The agent keeps its own full transcript; this
    /// copy only lets the window show earlier turns after it is reopened.
    pub fn append_turn(&self, folder: &Path, turn: &Turn) {
        let path = self.transcript(folder);
        let Some(parent) = path.parent() else { return };
        if fs::create_dir_all(parent).is_err() {
            return;
        }
        let Ok(mut line) = serde_json::to_string(turn) else {
            return;
        };
        line.push('\n');
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = file.write_all(line.as_bytes());
        }
    }

    /// Every turn the window kept for a folder, of every conversation, oldest
    /// first: what the workspace lists as earlier lessons.
    pub fn all_turns(&self, folder: &Path) -> Vec<Turn> {
        let Ok(text) = fs::read_to_string(self.transcript(folder)) else {
            return Vec::new();
        };
        text.lines()
            .filter_map(|line| serde_json::from_str::<Turn>(line).ok())
            .collect()
    }

    /// The turns the window showed in one conversation, oldest first.
    pub fn turns(&self, folder: &Path, session: &str) -> Vec<Turn> {
        let Ok(text) = fs::read_to_string(self.transcript(folder)) else {
            return Vec::new();
        };
        let mut turns: Vec<Turn> = text
            .lines()
            .filter_map(|line| serde_json::from_str::<Turn>(line).ok())
            .filter(|turn| turn.session == session)
            .collect();
        let extra = turns.len().saturating_sub(400);
        turns.drain(..extra);
        turns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_survives_a_reopen() {
        let home = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let store = Store::at(home.path().to_path_buf());
        store.opened(folder.path());
        store.update(folder.path(), |state| {
            state.session_id = Some("abc".into());
            state.stopped_at = Some("step 3".into());
        });
        store.append_turn(
            folder.path(),
            &Turn {
                session: "abc".into(),
                ms: 1,
                learner: "hi".into(),
                tutor: "hello".into(),
                interrupted: false,
            },
        );
        store.append_turn(
            folder.path(),
            &Turn {
                session: "other".into(),
                ms: 2,
                learner: "x".into(),
                tutor: "y".into(),
                interrupted: false,
            },
        );

        let again = Store::at(home.path().to_path_buf());
        assert_eq!(again.recent().len(), 1);
        let state = again.folder(folder.path());
        assert_eq!(state.session_id.as_deref(), Some("abc"));
        assert_eq!(state.stopped_at.as_deref(), Some("step 3"));
        let turns = again.turns(folder.path(), "abc");
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].tutor, "hello");
    }

    #[test]
    fn the_window_keeps_its_files_apart_from_the_runtimes() {
        let root = default_root().to_string_lossy().to_ascii_lowercase();
        assert!(!root.ends_with("/magnesium"), "{root}");
    }

    #[test]
    fn a_folder_can_be_added_and_forgotten_without_losing_its_lesson() {
        let home = tempfile::tempdir().unwrap();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let store = Store::at(home.path().to_path_buf());
        store.opened(first.path());
        store.update(first.path(), |state| state.session_id = Some("abc".into()));

        // A found folder goes after the opened one, and only once.
        let found = [second.path().to_path_buf(), first.path().to_path_buf()];
        assert_eq!(store.add_folders(&found), 1);
        assert_eq!(store.add_folders(&found), 0);
        assert_eq!(store.recent(), vec![key(first.path()), key(second.path())]);

        store.forget(first.path());
        assert_eq!(store.recent(), vec![key(second.path())]);
        assert!(first.path().is_dir());
        // Opened again, the lesson is still there.
        store.opened(first.path());
        assert_eq!(
            store.folder(first.path()).session_id.as_deref(),
            Some("abc")
        );
    }

    #[test]
    fn studied_days_are_counted_and_earlier_turns_are_counted_once() {
        let home = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let day = |text: &str| NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap();
        let at_noon = |text: &str| {
            day(text)
                .and_hms_opt(12, 0, 0)
                .unwrap()
                .and_local_timezone(Local)
                .unwrap()
                .timestamp_millis()
        };
        let turn = |ms: i64, learner: &str| Turn {
            session: "abc".into(),
            ms,
            learner: learner.into(),
            tutor: "…".into(),
            interrupted: false,
        };
        // Turns kept by a window that did not count days yet.
        let before = Store::at(home.path().to_path_buf());
        before.append_turn(folder.path(), &turn(at_noon("2026-11-08"), "an answer"));
        before.append_turn(folder.path(), &turn(at_noon("2026-11-09"), "another"));
        // The tutor's own opening line, with no words from the learner.
        before.append_turn(folder.path(), &turn(at_noon("2026-11-05"), " "));

        let store = Store::at(home.path().to_path_buf());
        store.count_turn(day("2026-11-10"));
        let days = store.days();
        assert_eq!(
            days.into_iter().collect::<Vec<_>>(),
            [day("2026-11-08"), day("2026-11-09"), day("2026-11-10")]
        );
        let activity = activity::summarize(&store.days(), day("2026-11-10"));
        assert_eq!(activity.run, 3);
        assert!(activity.studied_today);

        // Reopened, the earlier turns are not counted a second time.
        let again = Store::at(home.path().to_path_buf());
        again.count_turn(day("2026-11-10"));
        assert_eq!(again.lock().days["2026-11-08"], 1);
        assert_eq!(again.lock().days["2026-11-10"], 2);
    }

    #[test]
    fn a_broken_state_file_starts_fresh() {
        let home = tempfile::tempdir().unwrap();
        fs::write(home.path().join("state.json"), "{not json").unwrap();
        let store = Store::at(home.path().to_path_buf());
        assert!(store.recent().is_empty());
        assert_eq!(store.block_minutes(), 50);
    }
}
