//! What the window itself remembers, kept on this computer and nowhere else:
//! which folders are in the library, which conversation each one was in, the
//! line the learner stopped at, the turns the window showed, the days the
//! learner studied in it, how the learner chose to pay for lessons, the
//! model they picked and the last list of models the agent sent, and an
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
use crate::agent::{Intent, Payment};
use crate::models::{ModelOption, Pick};
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
    /// When the learner last opened this folder in the window, in ms since
    /// the epoch. Added after the first windows: an older state has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opened_ms: Option<i64>,
    /// What the window knows of each of this folder's conversations, by the
    /// agent's id: its name, when it was last opened, and its engine.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sessions: BTreeMap<String, SessionNote>,
    /// Fields another window build wrote that this one does not know. They are
    /// kept as they are, so the state file can be shared between builds.
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

/// What the window keeps about one conversation, beside its turns.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionNote {
    /// What it is about: the goal it began with, as the window named it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// When the learner last opened it, in ms since the epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opened_ms: Option<i64>,
    /// The engine it runs on, so it resumes on its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<Engine>,
    /// The model it runs on, so it resumes on its own. None for one an
    /// earlier window began, which ran on `models::EARLIER`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The effort it runs at; none for the agent's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
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
    /// The model and effort new sessions start on. A state an earlier window
    /// kept has no such field, and keeps the model every lesson ran on then.
    /// A new window's state has it empty until the agent first sends its
    /// list, when it takes the agent's default. It is always written, empty
    /// or not, so the two are never confused.
    #[serde(default = "earlier_pick")]
    pick: Option<Pick>,
    /// The last list of models the agent sent, when it was sent, and
    /// whether the last time it was asked it sent none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    models: Option<ModelList>,
    /// What `--version` printed for an installed `magnesium` the learner chose
    /// to keep over the one inside the app. Asked again for any other.
    #[serde(default)]
    kept_runtime: Option<String>,
    /// How the window was left: its open tabs and which one was in front.
    /// Opaque here; the page reads and writes it whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    window: Option<serde_json::Value>,
    /// Fields another window build wrote that this one does not know, kept.
    #[serde(flatten)]
    other: BTreeMap<String, serde_json::Value>,
}

fn earlier_pick() -> Option<Pick> {
    Some(Pick::earlier())
}

/// The agent's list of models as the window last had it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelList {
    #[serde(default)]
    pub models: Vec<ModelOption>,
    /// When the list came, in ms since the epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_ms: Option<i64>,
    /// The last time the agent was asked, it sent no list.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub failed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub session: String,
    pub ms: i64,
    pub learner: String,
    pub tutor: String,
    #[serde(default)]
    pub interrupted: bool,
    /// How the window sent the learner's words: an answer to the open task,
    /// or talk. None for the window's own words, help, or a turn an earlier
    /// window kept. Additive: an older window reads the line without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<Intent>,
    /// The task open after this turn, as the engine said it, and whether this
    /// turn's reply posed it. Kept beside the turns, in a file of its own, as
    /// the engine says it only once the turn's record is kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub posed: bool,
    /// The engine said which task was open after this turn (`task` none
    /// means none was). False for an engine that does not say.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub noted: bool,
    /// The ids of the record's marked answers that appeared for this turn's
    /// message: the marks the window showed under it. Empty when the record
    /// was read and had none; none when it was not read to the end, as for a
    /// turn an earlier window kept. Kept beside the turns, in a file of its
    /// own, as the record is read only once the turn is kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark_ids: Option<Vec<String>>,
}

/// What the engine said of a turn's task, kept beside the turn.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct TaskNote {
    session: String,
    ms: i64,
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    posed: bool,
}

/// Which of the record's marked answers appeared for a turn, kept beside it.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct MarksNote {
    session: String,
    ms: i64,
    #[serde(default)]
    ids: Vec<String>,
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
        data.recent.insert(0, key.clone());
        data.recent.truncate(LIBRARY_SIZE);
        data.folders.entry(key).or_default().opened_ms = Some(Local::now().timestamp_millis());
        self.save(&data);
    }

    /// Change what the window keeps about one of a folder's conversations.
    pub fn note_session(
        &self,
        folder: &Path,
        session: &str,
        change: impl FnOnce(&mut SessionNote),
    ) {
        if session.is_empty() {
            return;
        }
        let mut data = self.lock();
        let state = data.folders.entry(key(folder)).or_default();
        change(state.sessions.entry(session.to_owned()).or_default());
        self.save(&data);
    }

    /// How the window was left, as the page last wrote it.
    pub fn window(&self) -> Option<serde_json::Value> {
        self.lock().window.clone()
    }

    pub fn set_window(&self, value: serde_json::Value) {
        let mut data = self.lock();
        data.window = Some(value);
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

    /// The model and effort new sessions start on; none on a new window
    /// before the agent has sent its list.
    pub fn pick(&self) -> Option<Pick> {
        self.lock().pick.clone()
    }

    pub fn set_pick(&self, pick: Pick) {
        let mut data = self.lock();
        data.pick = Some(pick);
        self.save(&data);
    }

    /// The agent's list of models, as it last sent it.
    pub fn models(&self) -> ModelList {
        self.lock().models.clone().unwrap_or_default()
    }

    /// Keep the list the agent just sent, or note that it sent none: the
    /// last list it did send is kept. A new window with no pick yet takes
    /// the agent's default from the list, by its full id, so that a later
    /// change of that default does not change it.
    pub fn received_models(&self, models: Option<Vec<ModelOption>>) {
        let mut data = self.lock();
        let mut kept = data.models.clone().unwrap_or_default();
        match models {
            Some(models) => {
                if data.pick.is_none() {
                    if let Some(default) = crate::models::default_of(&models) {
                        let shown = crate::models::find(&models, default.id()).unwrap_or(default);
                        data.pick = Some(Pick {
                            model: default.id().to_owned(),
                            name: Some(shown.name.clone()),
                            effort: None,
                        });
                    }
                }
                kept = ModelList {
                    models,
                    at_ms: Some(Local::now().timestamp_millis()),
                    failed: false,
                };
            }
            None => kept.failed = true,
        }
        data.models = Some(kept);
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

    /// The file beside a folder's transcript that holds what the engine said
    /// of each turn's task.
    fn task_notes(&self, folder: &Path) -> PathBuf {
        self.transcript(folder).with_extension("tasks.jsonl")
    }

    /// The file beside a folder's transcript that holds which of the
    /// record's marked answers appeared for each turn.
    fn marks_notes(&self, folder: &Path) -> PathBuf {
        self.transcript(folder).with_extension("marks.jsonl")
    }

    /// Keep what the engine said of a kept turn's task: the task open after
    /// it, and whether the turn's reply posed it.
    pub fn note_task(
        &self,
        folder: &Path,
        session: &str,
        ms: i64,
        task: Option<&str>,
        posed: bool,
    ) {
        let note = TaskNote {
            session: session.to_owned(),
            ms,
            task: task.map(str::to_owned),
            posed,
        };
        append_line(&self.task_notes(folder), &note);
    }

    /// Keep which of the record's marked answers appeared for a kept turn's
    /// message, by their ids: none when the record was read and had no new
    /// row. A reopened lesson shows these rows under the turn, so a mark
    /// stays on the answer it was shown on, also when several answers fall
    /// in one minute. Only ids are kept; the marks are read from the record.
    pub fn note_marks(&self, folder: &Path, session: &str, ms: i64, ids: &[String]) {
        let note = MarksNote {
            session: session.to_owned(),
            ms,
            ids: ids.to_vec(),
        };
        append_line(&self.marks_notes(folder), &note);
    }

    /// Keep one finished turn. The agent keeps its own full transcript; this
    /// copy only lets the window show earlier turns after it is reopened.
    pub fn append_turn(&self, folder: &Path, turn: &Turn) {
        append_line(&self.transcript(folder), turn);
    }

    /// Every turn the window kept for a folder, of every conversation, oldest
    /// first: what a workspace lists as its sessions.
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
        // What the engine said of each turn's task, by the turn's time.
        if let Ok(notes) = fs::read_to_string(self.task_notes(folder)) {
            let by_ms: BTreeMap<i64, TaskNote> = notes
                .lines()
                .filter_map(|line| serde_json::from_str::<TaskNote>(line).ok())
                .filter(|note| note.session == session)
                .map(|note| (note.ms, note))
                .collect();
            for turn in &mut turns {
                if let Some(note) = by_ms.get(&turn.ms) {
                    turn.task.clone_from(&note.task);
                    turn.posed = note.posed;
                    turn.noted = true;
                }
            }
        }
        // Which marked answers appeared for each turn, by the turn's time.
        if let Ok(notes) = fs::read_to_string(self.marks_notes(folder)) {
            let mut by_ms: BTreeMap<i64, Vec<String>> = BTreeMap::new();
            for note in notes
                .lines()
                .filter_map(|line| serde_json::from_str::<MarksNote>(line).ok())
                .filter(|note| note.session == session)
            {
                let ids = by_ms.entry(note.ms).or_default();
                for id in note.ids {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            }
            for turn in &mut turns {
                if let Some(ids) = by_ms.get(&turn.ms) {
                    turn.mark_ids = Some(ids.clone());
                }
            }
        }
        turns
    }
}

/// Add one line of JSON to the end of a file of the window's own, making its
/// folder first. Nothing is shown when it cannot be written: the copy is the
/// window's, and the agent and the runtime keep their own.
fn append_line(path: &Path, value: &impl Serialize) {
    let Some(parent) = path.parent() else { return };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(mut line) = serde_json::to_string(value) else {
        return;
    };
    line.push('\n');
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ModelOption, EARLIER};

    fn listed(value: &str, resolved: &str, name: &str) -> ModelOption {
        ModelOption {
            value: value.into(),
            resolved: Some(resolved.into()),
            name: name.into(),
            description: String::new(),
            efforts: Vec::new(),
            current: true,
        }
    }

    #[test]
    fn a_window_that_kept_state_before_the_pick_keeps_the_model_it_ran_on() {
        let home = tempfile::tempdir().unwrap();
        fs::write(
            home.path().join("state.json"),
            r#"{"recent": [], "engine": "direct"}"#,
        )
        .unwrap();
        let store = Store::at(home.path().to_path_buf());
        assert_eq!(store.pick().unwrap().model, EARLIER);
        // The agent's list, with another default, changes nothing.
        store.received_models(Some(vec![listed(
            "default",
            "claude-opus-5-5",
            "Default (recommended)",
        )]));
        assert_eq!(store.pick().unwrap().model, EARLIER);
    }

    #[test]
    fn a_new_window_takes_the_agents_default_by_its_full_id_once_the_list_comes() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::at(home.path().to_path_buf());
        assert_eq!(store.pick(), None);
        // Asked and sent nothing: still no pick, and the window says so.
        store.received_models(None);
        assert_eq!(store.pick(), None);
        assert!(store.models().failed);
        // Kept empty, a new window is not taken for an older one.
        let again = Store::at(home.path().to_path_buf());
        assert_eq!(again.pick(), None);
        again.received_models(Some(vec![
            listed("default", "claude-opus-5-5", "Default (recommended)"),
            listed("opus", "claude-opus-5-5", "Opus"),
        ]));
        let pick = again.pick().unwrap();
        assert_eq!(pick.model, "claude-opus-5-5");
        assert_eq!(pick.name.as_deref(), Some("Opus"));
        assert!(!again.models().failed);
        // A later list with another default does not move it.
        again.received_models(Some(vec![listed(
            "default",
            "claude-opus-5-6",
            "Default (recommended)",
        )]));
        assert_eq!(again.pick().unwrap().model, "claude-opus-5-5");
        assert_eq!(again.models().models[0].id(), "claude-opus-5-6");
        // A list that does not come keeps the last one that did.
        again.received_models(None);
        let kept = again.models();
        assert!(kept.failed && kept.models.len() == 1);
    }

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
                intent: Some(Intent::Answer),
                ..Turn::default()
            },
        );
        store.append_turn(
            folder.path(),
            &Turn {
                session: "other".into(),
                ms: 2,
                learner: "x".into(),
                tutor: "y".into(),
                ..Turn::default()
            },
        );

        let again = Store::at(home.path().to_path_buf());
        assert_eq!(again.recent().len(), 1);
        let state = again.folder(folder.path());
        assert_eq!(state.session_id.as_deref(), Some("abc"));
        assert_eq!(state.stopped_at.as_deref(), Some("step 3"));
        again.note_task(folder.path(), "abc", 1, Some("What is b?"), true);
        let turns = again.turns(folder.path(), "abc");
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].tutor, "hello");
        assert_eq!(turns[0].intent, Some(Intent::Answer));
        assert_eq!(turns[0].task.as_deref(), Some("What is b?"));
        assert!(turns[0].posed && turns[0].noted);
    }

    #[test]
    fn the_marks_seen_for_a_turn_come_back_with_it_and_an_older_turn_has_none() {
        let home = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let store = Store::at(home.path().to_path_buf());
        let turn = |session: &str, ms: i64| Turn {
            session: session.into(),
            ms,
            learner: "an answer".into(),
            tutor: "…".into(),
            intent: Some(Intent::Answer),
            ..Turn::default()
        };
        for ms in [1, 2, 3] {
            store.append_turn(folder.path(), &turn("abc", ms));
        }
        store.append_turn(folder.path(), &turn("other", 2));
        // The record read after each message: two rows for the first, none
        // for the second, and nothing kept for the third (looking cut short).
        store.note_marks(folder.path(), "abc", 1, &["a-1".into(), "a-2".into()]);
        store.note_marks(folder.path(), "abc", 2, &[]);
        store.note_marks(folder.path(), "other", 2, &["a-9".into()]);

        let turns = Store::at(home.path().to_path_buf()).turns(folder.path(), "abc");
        let ids: Vec<Option<Vec<String>>> = turns.iter().map(|t| t.mark_ids.clone()).collect();
        assert_eq!(
            ids,
            [
                Some(vec!["a-1".to_owned(), "a-2".to_owned()]),
                Some(Vec::new()),
                None
            ]
        );
        // The turn's own line is as an older window wrote it.
        let text = fs::read_to_string(store.transcript(folder.path())).unwrap();
        assert!(!text.contains("mark_ids"), "{text}");
        // And the notes are no turn when the days are counted from the files.
        let notes = fs::read_to_string(store.marks_notes(folder.path())).unwrap();
        assert!(notes
            .lines()
            .all(|line| serde_json::from_str::<Turn>(line).is_err()));
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
            ..Turn::default()
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
    fn new_fields_are_additive_and_unknown_ones_are_kept() {
        let home = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let folder_key = key(folder.path());
        // A state file written by another build, with a field this one does not know.
        let older = serde_json::json!({
            "recent": [folder_key],
            "folders": { folder_key.clone(): { "session_id": "abc", "stopped_at": null, "stopped_ms": null, "paper": null, "later": 7 } },
            "block_minutes": 50,
            "someday": { "x": 1 }
        });
        fs::write(home.path().join("state.json"), older.to_string()).unwrap();
        let store = Store::at(home.path().to_path_buf());
        store.opened(folder.path());
        store.note_session(folder.path(), "abc", |note| {
            note.title = Some("Sampling".into())
        });
        store.set_window(serde_json::json!({ "tabs": [] }));

        let text = fs::read_to_string(home.path().join("state.json")).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(saved["someday"]["x"], 1);
        assert_eq!(saved["folders"][&folder_key]["later"], 7);
        assert_eq!(saved["folders"][&folder_key]["session_id"], "abc");
        assert_eq!(
            saved["folders"][&folder_key]["sessions"]["abc"]["title"],
            "Sampling"
        );
        assert!(saved["folders"][&folder_key]["opened_ms"].is_i64());
        assert_eq!(saved["window"]["tabs"], serde_json::json!([]));

        // What an older build reads: every field it knows, unchanged in kind.
        #[derive(serde::Deserialize)]
        struct OldFolder {
            session_id: Option<String>,
        }
        #[derive(serde::Deserialize)]
        struct Old {
            recent: Vec<String>,
            folders: BTreeMap<String, OldFolder>,
            block_minutes: Option<u32>,
        }
        let old: Old = serde_json::from_str(&text).unwrap();
        assert_eq!(old.recent, vec![folder_key.clone()]);
        assert_eq!(old.folders[&folder_key].session_id.as_deref(), Some("abc"));
        assert_eq!(old.block_minutes, Some(50));
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
