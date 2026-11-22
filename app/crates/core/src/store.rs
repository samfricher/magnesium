use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::activity;
use crate::agent::{Intent, Payment, Permissions};
use crate::models::{ModelOption, Pick};
use crate::paths;

const LIBRARY_SIZE: usize = 24;

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
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<Engine>,
    #[serde(default)]
    pub stopped_at: Option<String>,
    #[serde(default)]
    pub stopped_ms: Option<i64>,
    #[serde(default)]
    pub paper: Option<PaperSitting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opened_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sessions: BTreeMap<String, SessionNote>,
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionNote {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opened_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<Engine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
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
    #[serde(default)]
    days: BTreeMap<String, u32>,
    #[serde(default)]
    days_filled: bool,
    #[serde(default)]
    payment: Option<Payment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    permissions: Option<Permissions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    engine: Option<Engine>,
    #[serde(default = "earlier_pick")]
    pick: Option<Pick>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    models: Option<ModelList>,
    #[serde(default)]
    kept_runtime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    window: Option<serde_json::Value>,
    #[serde(flatten)]
    other: BTreeMap<String, serde_json::Value>,
}

fn earlier_pick() -> Option<Pick> {
    Some(Pick::earlier())
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelList {
    #[serde(default)]
    pub models: Vec<ModelOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_ms: Option<i64>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<Intent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub posed: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub noted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closing: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<StepBegun>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepBegun {
    pub n: usize,
    pub of: usize,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StepNote {
    session: String,
    ms: i64,
    #[serde(default)]
    step: Option<StepBegun>,
    #[serde(default)]
    began: bool,
    #[serde(default = "yes")]
    known: bool,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ClosingNote {
    session: String,
    ms: i64,
    #[serde(default)]
    lines: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TaskNote {
    session: String,
    ms: i64,
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    posed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct MarksNote {
    session: String,
    ms: i64,
    #[serde(default)]
    ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    #[default]
    Plugin,
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

    pub fn window(&self) -> Option<serde_json::Value> {
        self.lock().window.clone()
    }

    pub fn set_window(&self, value: serde_json::Value) {
        let mut data = self.lock();
        data.window = Some(value);
        self.save(&data);
    }

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

    pub fn count_turn(&self, day: NaiveDate) {
        let mut data = self.lock();
        self.fill_days(&mut data);
        *data.days.entry(activity::key(day)).or_default() += 1;
        self.save(&data);
    }

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

    pub fn permissions(&self) -> Permissions {
        self.lock().permissions.unwrap_or_default()
    }

    pub fn set_permissions(&self, permissions: Permissions) {
        let mut data = self.lock();
        data.permissions = Some(permissions);
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

    pub fn pick(&self) -> Option<Pick> {
        self.lock().pick.clone()
    }

    pub fn set_pick(&self, pick: Pick) {
        let mut data = self.lock();
        data.pick = Some(pick);
        self.save(&data);
    }

    pub fn models(&self) -> ModelList {
        self.lock().models.clone().unwrap_or_default()
    }

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

    fn task_notes(&self, folder: &Path) -> PathBuf {
        self.transcript(folder).with_extension("tasks.jsonl")
    }

    fn marks_notes(&self, folder: &Path) -> PathBuf {
        self.transcript(folder).with_extension("marks.jsonl")
    }

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

    pub fn note_marks(&self, folder: &Path, session: &str, ms: i64, ids: &[String]) {
        let note = MarksNote {
            session: session.to_owned(),
            ms,
            ids: ids.to_vec(),
        };
        append_line(&self.marks_notes(folder), &note);
    }

    fn closing_notes(&self, folder: &Path) -> PathBuf {
        self.transcript(folder).with_extension("closing.jsonl")
    }

    pub fn note_closing(&self, folder: &Path, session: &str, ms: i64, lines: &[String]) {
        let note = ClosingNote {
            session: session.to_owned(),
            ms,
            lines: lines.to_vec(),
        };
        append_line(&self.closing_notes(folder), &note);
    }

    fn step_notes(&self, folder: &Path) -> PathBuf {
        self.transcript(folder).with_extension("steps.jsonl")
    }

    fn step_notes_of(&self, folder: &Path, session: &str) -> Vec<StepNote> {
        let Ok(text) = fs::read_to_string(self.step_notes(folder)) else {
            return Vec::new();
        };
        text.lines()
            .filter_map(|line| serde_json::from_str::<StepNote>(line).ok())
            .filter(|note| note.session == session)
            .collect()
    }

    pub fn note_step(
        &self,
        folder: &Path,
        session: &str,
        ms: i64,
        step: Option<Option<StepBegun>>,
    ) -> Option<StepBegun> {
        let known = step.is_some();
        if !known
            && self
                .step_notes_of(folder, session)
                .iter()
                .any(|note| note.ms == ms && note.known)
        {
            return None;
        }
        let step = step.flatten();
        let before = self
            .all_turns(folder)
            .into_iter()
            .filter(|turn| turn.session == session && turn.ms < ms)
            .map(|turn| turn.ms)
            .max();
        let began = known
            && step.is_some()
            && match before {
                None => true,
                Some(before) => self
                    .step_notes_of(folder, session)
                    .into_iter()
                    .rfind(|note| note.ms == before)
                    .is_some_and(|note| {
                        note.known
                            && note.step.as_ref().map(|s| &s.name) != step.as_ref().map(|s| &s.name)
                    }),
            };
        let note = StepNote {
            session: session.to_owned(),
            ms,
            step,
            began,
            known,
        };
        append_line(&self.step_notes(folder), &note);
        if began {
            note.step
        } else {
            None
        }
    }

    pub fn append_turn(&self, folder: &Path, turn: &Turn) {
        append_line(&self.transcript(folder), turn);
    }

    pub fn all_turns(&self, folder: &Path) -> Vec<Turn> {
        let Ok(text) = fs::read_to_string(self.transcript(folder)) else {
            return Vec::new();
        };
        text.lines()
            .filter_map(|line| serde_json::from_str::<Turn>(line).ok())
            .filter(|turn| !reached_no_tutor(turn))
            .collect()
    }

    pub fn turns(&self, folder: &Path, session: &str) -> Vec<Turn> {
        let Ok(text) = fs::read_to_string(self.transcript(folder)) else {
            return Vec::new();
        };
        let mut turns: Vec<Turn> = text
            .lines()
            .filter_map(|line| serde_json::from_str::<Turn>(line).ok())
            .filter(|turn| turn.session == session && !reached_no_tutor(turn))
            .collect();
        let extra = turns.len().saturating_sub(400);
        turns.drain(..extra);
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
        if let Ok(notes) = fs::read_to_string(self.closing_notes(folder)) {
            let by_ms: BTreeMap<i64, Vec<String>> = notes
                .lines()
                .filter_map(|line| serde_json::from_str::<ClosingNote>(line).ok())
                .filter(|note| note.session == session && !note.lines.is_empty())
                .map(|note| (note.ms, note.lines))
                .collect();
            for turn in &mut turns {
                if let Some(lines) = by_ms.get(&turn.ms) {
                    turn.closing = Some(lines.clone());
                }
            }
        }
        let mut began: BTreeMap<i64, Option<StepBegun>> = BTreeMap::new();
        for note in self.step_notes_of(folder, session) {
            began.insert(note.ms, note.step.filter(|_| note.began));
        }
        for turn in &mut turns {
            if let Some(step) = began.get(&turn.ms) {
                turn.step.clone_from(step);
            }
        }
        turns
    }
}

fn reached_no_tutor(turn: &Turn) -> bool {
    let reply = turn.tutor.trim();
    reply.starts_with("Failed to authenticate")
        || reply.starts_with("Invalid API key ·")
        || reply.starts_with("Not logged in ·")
}

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
        store.received_models(None);
        assert_eq!(store.pick(), None);
        assert!(store.models().failed);
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
        again.received_models(Some(vec![listed(
            "default",
            "claude-opus-5-6",
            "Default (recommended)",
        )]));
        assert_eq!(again.pick().unwrap().model, "claude-opus-5-5");
        assert_eq!(again.models().models[0].id(), "claude-opus-5-6");
        again.received_models(None);
        let kept = again.models();
        assert!(kept.failed && kept.models.len() == 1);
    }

    #[test]
    fn a_folder_kept_with_recording_off_by_an_older_window_still_loads() {
        let home = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let state = serde_json::json!({
            "recent": [key(folder.path())],
            "folders": {key(folder.path()): {"session_id": "abc", "recording_off": true}},
        });
        fs::write(home.path().join("state.json"), state.to_string()).unwrap();
        let store = Store::at(home.path().to_path_buf());
        assert_eq!(
            store.folder(folder.path()).session_id.as_deref(),
            Some("abc")
        );
        store.update(folder.path(), |state| {
            state.stopped_at = Some("step 2".into())
        });
        let again = Store::at(home.path().to_path_buf());
        let kept = again.folder(folder.path());
        assert_eq!(kept.session_id.as_deref(), Some("abc"));
        assert_eq!(kept.stopped_at.as_deref(), Some("step 2"));
        assert_eq!(
            kept.other.get("recording_off"),
            Some(&serde_json::json!(true))
        );
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
    fn a_turn_kept_for_a_message_sent_while_signed_out_is_not_read_back() {
        let home = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let store = Store::at(home.path().to_path_buf());
        let turn = |ms: i64, tutor: &str| Turn {
            session: "abc".into(),
            ms,
            learner: "6".into(),
            tutor: tutor.into(),
            intent: Some(Intent::Answer),
            ..Turn::default()
        };
        store.append_turn(folder.path(), &turn(1, "**Your turn.** What is 3 + 3?"));
        let said = "Failed to authenticate: OAuth session expired and could not be refreshed";
        store.append_turn(folder.path(), &turn(2, said));
        store.append_turn(folder.path(), &turn(3, said));
        let turns = store.turns(folder.path(), "abc");
        assert_eq!(turns.len(), 1, "{turns:#?}");
        assert_eq!(turns[0].ms, 1);
        assert_eq!(store.all_turns(folder.path()).len(), 1);
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
        let text = fs::read_to_string(store.transcript(folder.path())).unwrap();
        assert!(!text.contains("mark_ids"), "{text}");
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

        let found = [second.path().to_path_buf(), first.path().to_path_buf()];
        assert_eq!(store.add_folders(&found), 1);
        assert_eq!(store.add_folders(&found), 0);
        assert_eq!(store.recent(), vec![key(first.path()), key(second.path())]);

        store.forget(first.path());
        assert_eq!(store.recent(), vec![key(second.path())]);
        assert!(first.path().is_dir());
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
        let before = Store::at(home.path().to_path_buf());
        before.append_turn(folder.path(), &turn(at_noon("2026-11-08"), "an answer"));
        before.append_turn(folder.path(), &turn(at_noon("2026-11-09"), "another"));
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

    fn step(n: usize, of: usize, name: &str) -> Option<StepBegun> {
        Some(StepBegun {
            n,
            of,
            name: name.into(),
        })
    }

    fn read(n: usize, of: usize, name: &str) -> Option<Option<StepBegun>> {
        Some(step(n, of, name))
    }

    fn kept(store: &Store, folder: &Path, session: &str, ms: i64) {
        store.append_turn(
            folder,
            &Turn {
                session: session.into(),
                ms,
                learner: "words".into(),
                tutor: "reply".into(),
                ..Turn::default()
            },
        );
    }

    #[test]
    fn a_step_begins_where_the_step_in_hand_changes_numbered_as_the_route_was_then() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::at(home.path().to_path_buf());
        let folder = home.path().join("course");
        kept(&store, &folder, "s", 1);
        assert_eq!(store.note_step(&folder, "s", 1, Some(None)), None);
        kept(&store, &folder, "s", 2);
        assert_eq!(
            store.note_step(&folder, "s", 2, read(1, 2, "A")),
            step(1, 2, "A")
        );
        kept(&store, &folder, "s", 3);
        assert_eq!(store.note_step(&folder, "s", 3, read(1, 3, "A")), None);
        kept(&store, &folder, "s", 4);
        assert_eq!(
            store.note_step(&folder, "s", 4, read(2, 3, "B")),
            step(2, 3, "B")
        );
        kept(&store, &folder, "s", 5);
        assert_eq!(
            store.note_step(&folder, "s", 5, read(1, 3, "A")),
            step(1, 3, "A")
        );
        kept(&store, &folder, "s", 6);
        assert_eq!(store.note_step(&folder, "s", 6, read(1, 3, "A")), None);
        assert_eq!(
            store.note_step(&folder, "s", 6, read(3, 3, "C")),
            step(3, 3, "C")
        );
        let heads: Vec<Option<StepBegun>> = store
            .turns(&folder, "s")
            .into_iter()
            .map(|turn| turn.step)
            .collect();
        assert_eq!(
            heads,
            [
                None,
                step(1, 2, "A"),
                None,
                step(2, 3, "B"),
                step(1, 3, "A"),
                step(3, 3, "C")
            ]
        );
    }

    #[test]
    fn a_conversation_kept_before_steps_were_noted_has_no_step_begin_mid_way() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::at(home.path().to_path_buf());
        let folder = home.path().join("course");
        kept(&store, &folder, "old", 1);
        kept(&store, &folder, "old", 2);
        kept(&store, &folder, "old", 3);
        assert_eq!(store.note_step(&folder, "old", 3, read(2, 3, "B")), None);
        kept(&store, &folder, "old", 4);
        assert_eq!(
            store.note_step(&folder, "old", 4, read(3, 3, "C")),
            step(3, 3, "C")
        );
        kept(&store, &folder, "new", 5);
        assert_eq!(
            store.note_step(&folder, "new", 5, read(1, 1, "A")),
            step(1, 1, "A")
        );
    }

    #[test]
    fn a_turn_whose_reading_could_not_tell_passes_no_head_to_the_next() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::at(home.path().to_path_buf());
        let folder = home.path().join("course");
        kept(&store, &folder, "s", 1);
        assert_eq!(
            store.note_step(&folder, "s", 1, read(1, 2, "A")),
            step(1, 2, "A")
        );
        kept(&store, &folder, "s", 2);
        assert_eq!(store.note_step(&folder, "s", 2, None), None);
        kept(&store, &folder, "s", 3);
        assert_eq!(store.note_step(&folder, "s", 3, read(2, 2, "B")), None);
        kept(&store, &folder, "s", 4);
        kept(&store, &folder, "s", 5);
        assert_eq!(store.note_step(&folder, "s", 5, read(1, 2, "A")), None);
        let heads: Vec<Option<StepBegun>> = store
            .turns(&folder, "s")
            .into_iter()
            .map(|turn| turn.step)
            .collect();
        assert_eq!(heads, [step(1, 2, "A"), None, None, None, None]);
    }

    #[test]
    fn a_reading_that_could_not_tell_never_replaces_one_that_could() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::at(home.path().to_path_buf());
        let folder = home.path().join("course");
        kept(&store, &folder, "s", 1);
        assert_eq!(
            store.note_step(&folder, "s", 1, read(1, 2, "A")),
            step(1, 2, "A")
        );
        kept(&store, &folder, "s", 2);
        assert_eq!(
            store.note_step(&folder, "s", 2, read(2, 2, "B")),
            step(2, 2, "B")
        );
        assert_eq!(store.note_step(&folder, "s", 2, None), None);
        kept(&store, &folder, "s", 3);
        assert_eq!(
            store.note_step(&folder, "s", 3, read(1, 2, "A")),
            step(1, 2, "A")
        );
        let heads: Vec<Option<StepBegun>> = store
            .turns(&folder, "s")
            .into_iter()
            .map(|turn| turn.step)
            .collect();
        assert_eq!(heads, [step(1, 2, "A"), step(2, 2, "B"), step(1, 2, "A")]);
    }
}
