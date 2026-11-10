//! What the window itself remembers, kept on this computer and nowhere else:
//! which folders were opened, which conversation each one was in, the line the
//! learner stopped at, and the turns the window showed.
//!
//! None of it is learning state. The learner record belongs to the runtime.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::paths;

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
    #[serde(default)]
    pub stopped_at: Option<String>,
    #[serde(default)]
    pub stopped_ms: Option<i64>,
    #[serde(default)]
    pub paper: Option<PaperSitting>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Data {
    #[serde(default)]
    recent: Vec<String>,
    #[serde(default)]
    folders: BTreeMap<String, FolderState>,
    #[serde(default)]
    block_minutes: Option<u32>,
    #[serde(default)]
    progress_view: Option<String>,
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
        data.recent.truncate(12);
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

    pub fn progress_view(&self) -> String {
        self.lock()
            .progress_view
            .clone()
            .unwrap_or_else(|| "A".into())
    }

    pub fn set_progress_view(&self, view: &str) {
        if matches!(view, "A" | "B" | "C") {
            let mut data = self.lock();
            data.progress_view = Some(view.to_owned());
            self.save(&data);
        }
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
    fn a_broken_state_file_starts_fresh() {
        let home = tempfile::tempdir().unwrap();
        fs::write(home.path().join("state.json"), "{not json").unwrap();
        let store = Store::at(home.path().to_path_buf());
        assert!(store.recent().is_empty());
        assert_eq!(store.block_minutes(), 50);
    }
}
