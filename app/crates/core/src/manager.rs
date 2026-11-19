use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use std::time::Duration;

use chrono::Local;
use serde::Serialize;

use crate::activity::{self, Activity};

use crate::agent::{
    Account, AgentError, AgentEvent, AgentResult, Answer, Conversation, Decision, EventSink,
    Harness, Intent, NoticeKind, Payment, Permissions, SavedWaiter, StartOptions,
};
use crate::files;
use crate::find;
use crate::install::{self, RuntimeSetup};
use crate::material::{self, Added};
use crate::models::{self, ModelOption, Pick};
use crate::paths;
use crate::plan::{self, Plan, PlanPart};
use crate::runtime;
use crate::store::{Engine, FolderState, PaperSitting, StepBegun, Store, Turn};
use crate::study::{self, Card, Home, Paper, Progress, Rail};
use crate::workspace::{self, LessonInfo, Workspace};

#[derive(Default)]
struct Buffer {
    session: Option<String>,
    learner: String,
    tutor: Vec<String>,
    writing: String,
    ready: bool,
    ended: bool,
    typed: bool,
    title: Option<String>,
    intent: Option<Intent>,
    last_ms: Option<i64>,
    sent: u64,
    ended_turns: u64,
    route_for: Option<(String, i64)>,
    model: Option<String>,
    effort: Option<String>,
}

struct Inner {
    folder: Option<PathBuf>,
    conversation: Option<Box<dyn Conversation>>,
    buffer: Arc<Mutex<Buffer>>,
    engine: Engine,
    papers_next: Option<std::time::Instant>,
}

#[derive(Default)]
struct Watch {
    seq: u64,
    marks_before: Option<HashSet<String>>,
    intent: Option<Intent>,
    sent_ms: i64,
    sitting_ended: bool,
}

struct Seen {
    folder: PathBuf,
    plan: Plan,
    goal_changed_ms: Option<i64>,
}

#[derive(Clone)]
struct Settling {
    watch: Arc<Mutex<Watch>>,
    seen: Arc<Mutex<Option<Seen>>>,
    turn_running: Arc<AtomicBool>,
    pending: Arc<AtomicUsize>,
}

const SAVE_WAIT: Duration = Duration::from_secs(35);
const MARK_LOOKS_MS: [u64; 6] = [0, 1_000, 2_500, 5_000, 10_000, 20_000];
const BUSY_LOOKS: usize = 3;

pub struct Manager {
    store: Arc<Store>,
    harness: Arc<dyn Harness>,
    direct: Option<Arc<dyn Harness>>,
    sink: EventSink,
    inner: Mutex<Inner>,
    current: Arc<Mutex<u64>>,
    settling: Settling,
    asking_models: Mutex<()>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Setup {
    pub runtime: RuntimeSetup,
    pub account: Account,
    pub payment: Payment,
    pub permissions: Permissions,
}

#[derive(Clone, Debug, Serialize)]
pub struct Opened {
    pub resumed: bool,
    pub turns: Vec<Turn>,
    pub engine: Engine,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ModelChoice {
    pub models: Vec<ModelOption>,
    pub failed: bool,
    pub pick: Option<Pick>,
    pub not_offered: bool,
    pub on: Option<On>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct On {
    pub model: String,
    pub effort: Option<String>,
    pub live: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EngineChoice {
    pub chosen: Engine,
    pub conversation: Option<Engine>,
}

const PAPERS_OPEN_WITHIN: Duration = Duration::from_secs(60);

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

fn none_open() -> AgentError {
    AgentError("Open a space first.".into())
}

impl Manager {
    pub fn new(store: Arc<Store>, harness: Arc<dyn Harness>, sink: EventSink) -> Self {
        Self {
            store,
            harness,
            direct: None,
            sink,
            inner: Mutex::new(Inner {
                folder: None,
                conversation: None,
                buffer: Arc::new(Mutex::new(Buffer::default())),
                engine: Engine::Plugin,
                papers_next: None,
            }),
            current: Arc::new(Mutex::new(0)),
            settling: Settling {
                watch: Arc::new(Mutex::new(Watch::default())),
                seen: Arc::new(Mutex::new(None)),
                turn_running: Arc::new(AtomicBool::new(false)),
                pending: Arc::new(AtomicUsize::new(0)),
            },
            asking_models: Mutex::new(()),
        }
    }

    pub fn with_direct(mut self, direct: Arc<dyn Harness>) -> Self {
        self.direct = Some(direct);
        self
    }

    fn harness_for(&self, engine: Engine) -> &Arc<dyn Harness> {
        match (engine, &self.direct) {
            (Engine::Direct, Some(direct)) => direct,
            _ => &self.harness,
        }
    }

    fn chosen(&self) -> Engine {
        match self.direct {
            Some(_) => self.store.engine(),
            None => Engine::Plugin,
        }
    }

    pub fn engine(&self) -> EngineChoice {
        let inner = lock(&self.inner);
        let running = inner.conversation.is_some() && !lock(&inner.buffer).ended;
        let conversation = if running {
            Some(inner.engine)
        } else {
            inner.folder.as_ref().and_then(|folder| {
                let state = self.store.folder(folder);
                state.session_id.map(|_| state.engine.unwrap_or_default())
            })
        };
        EngineChoice {
            chosen: self.chosen(),
            conversation,
        }
    }

    pub fn set_engine(&self, engine: Engine) -> EngineChoice {
        self.store.set_engine(engine);
        self.engine()
    }

    pub fn models(&self) -> ModelChoice {
        let list = self.store.models();
        let pick = self.store.pick();
        let not_offered = pick.as_ref().is_some_and(|pick| {
            !list.models.is_empty() && models::find(&list.models, &pick.model).is_none()
        });
        ModelChoice {
            models: list.models,
            failed: list.failed,
            pick,
            not_offered,
            on: None,
        }
    }

    pub fn ask_models(&self) -> ModelChoice {
        let _one = lock(&self.asking_models);
        let kept = self.store.models();
        let fresh = !kept.models.is_empty()
            && !kept.failed
            && kept
                .at_ms
                .is_some_and(|at| Local::now().timestamp_millis() - at < 30_000);
        if !fresh {
            let listed = self.harness_for(self.chosen()).models(self.store.payment());
            self.store.received_models(listed.ok().flatten());
        }
        self.models()
    }

    pub fn set_pick(&self, model: &str, effort: Option<&str>) -> AgentResult<ModelChoice> {
        let pick = self.checked_pick(model, effort)?;
        self.store.set_pick(pick);
        Ok(self.models())
    }

    fn checked_pick(&self, model: &str, effort: Option<&str>) -> AgentResult<Pick> {
        let effort = effort.filter(|e| !e.is_empty());
        if !models::usable_model(model) || effort.is_some_and(|e| !models::usable_effort(e)) {
            return Err(AgentError("That model cannot be picked.".into()));
        }
        let list = self.store.models();
        let row = models::find(&list.models, model);
        if !list.models.is_empty() && row.is_none() {
            return Err(AgentError(
                "That model is not on the agent's list now.".into(),
            ));
        }
        if let (Some(row), Some(effort)) = (row, effort) {
            if !row.efforts.iter().any(|level| level == effort) {
                return Err(AgentError(format!(
                    "{} does not take that effort.",
                    row.name
                )));
            }
        }
        let id = row.map_or(model, ModelOption::id).to_owned();
        let name = models::find(&list.models, &id)
            .or(row)
            .map(|row| row.name.clone())
            .or_else(|| {
                self.store
                    .pick()
                    .filter(|p| p.model == id)
                    .and_then(|p| p.name)
            });
        Ok(Pick {
            model: id,
            name,
            effort: effort.map(str::to_owned),
        })
    }

    pub fn models_at(
        &self,
        folder: Option<&Path>,
        session: Option<&str>,
        live: bool,
    ) -> ModelChoice {
        let mut choice = self.models();
        let Ok(folder) = self.at(folder) else {
            return choice;
        };
        let session = session.filter(|id| !id.is_empty());
        {
            let mut inner = lock(&self.inner);
            if let Some(conversation) = Self::running_for(&mut inner, &folder, session, live) {
                let applied = conversation.applied();
                let buffer = Arc::clone(&inner.buffer);
                let mut buffer = lock(&buffer);
                if let Ok(applied) = applied {
                    buffer.model = Some(self.list_id(&applied.model));
                }
                if let Some(model) = buffer.model.clone() {
                    choice.on = Some(On {
                        model,
                        effort: buffer.effort.clone(),
                        live: true,
                    });
                }
                let kept = (
                    buffer.session.clone(),
                    buffer.model.clone(),
                    buffer.effort.clone(),
                );
                drop(buffer);
                drop(inner);
                if let (Some(id), Some(model)) = (kept.0, kept.1) {
                    self.store.note_session(&folder, &id, |note| {
                        note.model = Some(model);
                        note.effort = kept.2;
                    });
                }
                return choice;
            }
        }
        if let Some(id) = session {
            let saved = self.store.folder(&folder);
            if let Some(note) = saved.sessions.get(id) {
                choice.on = Some(On {
                    model: note.model.clone().unwrap_or_else(|| models::EARLIER.into()),
                    effort: note.effort.clone(),
                    live: false,
                });
            }
        }
        choice
    }

    pub fn set_model_at(
        &self,
        folder: Option<&Path>,
        session: Option<&str>,
        live: bool,
        model: &str,
        effort: Option<&str>,
    ) -> AgentResult<ModelChoice> {
        let pick = self.checked_pick(model, effort)?;
        let folder = self.at(folder)?;
        let session = session.filter(|id| !id.is_empty());
        let mut changed_live = false;
        {
            let mut inner = lock(&self.inner);
            if let Some(conversation) = Self::running_for(&mut inner, &folder, session, live) {
                let result = conversation.change_model(&pick.model, pick.effort.as_deref());
                let applied = match &result {
                    Ok(applied) => Some(applied.clone()),
                    Err(_) => conversation.applied().ok(),
                };
                let buffer = Arc::clone(&inner.buffer);
                let mut buffer = lock(&buffer);
                if let Some(applied) = applied {
                    buffer.model = Some(self.list_id(&applied.model));
                }
                if result.is_ok() {
                    buffer.effort.clone_from(&pick.effort);
                }
                let kept = (
                    buffer.session.clone(),
                    buffer.model.clone(),
                    buffer.effort.clone(),
                );
                drop(buffer);
                drop(inner);
                if let (Some(id), Some(model)) = (kept.0, kept.1) {
                    self.store.note_session(&folder, &id, |note| {
                        note.model = Some(model);
                        note.effort = kept.2;
                    });
                }
                result?;
                changed_live = true;
            }
        }
        if !changed_live {
            if let Some(id) = session {
                self.store.note_session(&folder, id, |note| {
                    note.model = Some(pick.model.clone());
                    note.effort.clone_from(&pick.effort);
                });
            }
        }
        self.store.set_pick(pick);
        Ok(self.models_at(Some(&folder), session, live))
    }

    fn running_for<'a>(
        inner: &'a mut Inner,
        folder: &Path,
        session: Option<&str>,
        live: bool,
    ) -> Option<&'a mut Box<dyn Conversation>> {
        if inner.folder.as_deref() != Some(folder) {
            return None;
        }
        let ours = {
            let buffer = lock(&inner.buffer);
            !buffer.ended
                && match (session, buffer.session.as_deref()) {
                    (Some(id), Some(running)) => id == running,
                    (None, _) => live,
                    (Some(_), None) => false,
                }
        };
        if ours {
            inner.conversation.as_mut()
        } else {
            None
        }
    }

    fn list_id(&self, model: &str) -> String {
        let list = self.store.models().models;
        models::find(&list, model).map_or_else(|| model.to_owned(), |row| row.id().to_owned())
    }

    fn start_on(
        &self,
        saved: &FolderState,
        remembered: Option<&str>,
    ) -> (Option<String>, Option<String>) {
        if let Some(id) = remembered {
            let note = saved.sessions.get(id);
            let model = note
                .and_then(|note| note.model.clone())
                .unwrap_or_else(|| models::EARLIER.into());
            return (Some(model), note.and_then(|note| note.effort.clone()));
        }
        let Some(pick) = self.store.pick() else {
            return (None, None);
        };
        let list = self.store.models();
        if list.models.is_empty() {
            return (Some(pick.model), pick.effort);
        }
        match models::find(&list.models, &pick.model) {
            None => (None, None),
            Some(row) => {
                let effort = pick.effort.filter(|e| row.efforts.contains(e));
                (Some(pick.model), effort)
            }
        }
    }

    fn close_conversation(&self, inner: &mut Inner) -> u64 {
        let generation = {
            let mut current = lock(&self.current);
            *current += 1;
            *current
        };
        if let Some(mut conversation) = inner.conversation.take() {
            conversation.close();
        }
        self.settling.turn_running.store(false, Ordering::SeqCst);
        generation
    }

    pub fn busy(&self) -> bool {
        self.settling.turn_running.load(Ordering::SeqCst)
            || self.settling.pending.load(Ordering::SeqCst) > 0
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn folder(&self) -> Option<PathBuf> {
        lock(&self.inner).folder.clone()
    }

    fn need_folder(&self) -> AgentResult<PathBuf> {
        self.folder().ok_or_else(none_open)
    }

    fn at(&self, folder: Option<&Path>) -> AgentResult<PathBuf> {
        match folder {
            Some(path) => path
                .canonicalize()
                .ok()
                .filter(|p| p.is_dir())
                .ok_or_else(|| AgentError("That folder is not there any more.".into())),
            None => self.need_folder(),
        }
    }

    pub fn visit_folder(&self, path: &Path) -> AgentResult<Home> {
        let folder = self.at(Some(path))?;
        self.store.opened(&folder);
        Ok(study::home(&folder, &self.store))
    }

    pub fn workspaces(&self) -> Vec<Workspace> {
        workspace::workspaces(&self.store)
    }

    pub fn lessons(&self, folder: Option<&Path>) -> AgentResult<Vec<LessonInfo>> {
        let folder = self.at(folder)?;
        let state = self.store.folder(&folder);
        let mut listed = workspace::sessions(
            &self.store.all_turns(&folder),
            state.session_id.as_deref(),
            &state.sessions,
        );
        let root = study::root(&folder);
        for lesson in &mut listed {
            lesson.goal = crate::peek::goal_of_conversation(&root, &lesson.session);
        }
        Ok(listed)
    }

    pub fn lesson_turns(&self, folder: Option<&Path>, session: &str) -> AgentResult<Vec<Turn>> {
        let folder = self.at(folder)?;
        Ok(self.store.turns(&folder, session))
    }

    pub fn rename_session(
        &self,
        folder: Option<&Path>,
        session: &str,
        title: &str,
    ) -> AgentResult<()> {
        let folder = self.at(folder)?;
        let title: String = title.trim().chars().take(200).collect();
        if title.is_empty() {
            return Ok(());
        }
        self.store
            .note_session(&folder, session, |note| note.title = Some(title));
        Ok(())
    }

    pub fn touch_session(&self, folder: Option<&Path>, session: &str) -> AgentResult<()> {
        let folder = self.at(folder)?;
        self.store.opened(&folder);
        let now = Local::now().timestamp_millis();
        self.store
            .note_session(&folder, session, |note| note.opened_ms = Some(now));
        Ok(())
    }

    pub fn window_state(&self) -> Option<serde_json::Value> {
        self.store.window()
    }

    pub fn set_window_state(&self, value: serde_json::Value) {
        self.store.set_window(value);
    }

    pub fn recent(&self) -> Vec<String> {
        self.store.recent()
    }

    pub fn library(&self) -> Vec<Card> {
        study::library(&self.store.recent(), &self.store)
    }

    pub fn forget_folder(&self, path: &Path) -> AgentResult<()> {
        let folder = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let mut inner = lock(&self.inner);
        if inner.folder.as_deref() == Some(folder.as_path()) {
            self.close_conversation(&mut inner);
            inner.folder = None;
        }
        drop(inner);
        self.store.forget(&folder);
        Ok(())
    }

    pub fn find_folders(&self) -> usize {
        let found = find::study_folders(&paths::home(), Duration::from_secs(6));
        self.store.add_folders(&found)
    }

    pub fn activity(&self) -> Activity {
        let days = study::answered_days(&self.store.recent()).unwrap_or_else(|| self.store.days());
        activity::summarize(&days, study::today())
    }

    pub fn open_folder(&self, path: &Path) -> AgentResult<Home> {
        let folder = path
            .canonicalize()
            .ok()
            .filter(|p| p.is_dir())
            .ok_or_else(|| AgentError("That folder does not exist.".into()))?;
        {
            let mut inner = lock(&self.inner);
            self.close_conversation(&mut inner);
            inner.folder = Some(folder.clone());
        }
        self.store.opened(&folder);
        Ok(study::home(&folder, &self.store))
    }

    pub fn home(&self) -> AgentResult<Home> {
        self.home_at(None)
    }

    pub fn home_at(&self, folder: Option<&Path>) -> AgentResult<Home> {
        Ok(study::home(&self.at(folder)?, &self.store))
    }

    pub fn progress(&self, target: Option<&str>) -> AgentResult<Progress> {
        self.progress_at(None, target)
    }

    pub fn progress_at(
        &self,
        folder: Option<&Path>,
        target: Option<&str>,
    ) -> AgentResult<Progress> {
        Ok(study::progress(&self.at(folder)?, target))
    }

    pub fn rail(&self) -> AgentResult<Rail> {
        Ok(study::rail(&self.need_folder()?))
    }

    pub fn saved_turns(&self) -> AgentResult<Vec<Turn>> {
        let folder = self.need_folder()?;
        Ok(self
            .store
            .folder(&folder)
            .session_id
            .map(|id| self.store.turns(&folder, &id))
            .unwrap_or_default())
    }

    pub fn plan(&self) -> AgentResult<Plan> {
        self.plan_at(None)
    }

    pub fn plan_at(&self, folder: Option<&Path>) -> AgentResult<Plan> {
        let folder = self.at(folder)?;
        let mut plan = plan::read(&folder);
        if let Some(seen) = lock(&self.settling.seen)
            .as_ref()
            .filter(|seen| seen.folder == folder)
        {
            plan.changed_ms = plan.changed_ms.max(seen.goal_changed_ms);
        }
        Ok(plan)
    }

    pub fn marks(&self) -> AgentResult<Option<Vec<runtime::Mark>>> {
        self.marks_at(None)
    }

    pub fn marks_at(&self, folder: Option<&Path>) -> AgentResult<Option<Vec<runtime::Mark>>> {
        let folder = self.at(folder)?;
        let mut marks = runtime::marks(&folder);
        if let Some(marks) = marks.as_mut() {
            runtime::note_passes(marks, &folder);
        }
        Ok(marks)
    }

    pub fn material_list(&self) -> AgentResult<material::Listed> {
        self.material_list_at(None)
    }

    pub fn material_list_at(&self, folder: Option<&Path>) -> AgentResult<material::Listed> {
        Ok(material::list(&study::root(&self.at(folder)?)))
    }

    pub fn set_up(&self) -> AgentResult<Home> {
        self.set_up_at(None)
    }

    pub fn set_up_at(&self, folder: Option<&Path>) -> AgentResult<Home> {
        let folder = self.at(folder)?;
        runtime::init(&folder)
            .map_err(|e| AgentError(format!("Magnesium could not set this space up: {e}.")))?;
        Ok(study::home(&folder, &self.store))
    }

    pub fn upgrade_folder(&self) -> AgentResult<Home> {
        self.upgrade_folder_at(None)
    }

    pub fn upgrade_folder_at(&self, folder: Option<&Path>) -> AgentResult<Home> {
        let folder = self.at(folder)?;
        runtime::upgrade(&study::root(&folder))
            .map_err(|e| AgentError(format!("Magnesium could not upgrade this space: {e}.")))?;
        Ok(study::home(&folder, &self.store))
    }

    pub fn new_folder(&self, path: &Path) -> AgentResult<Home> {
        if path.exists() {
            return Err(AgentError(
                "A file or folder with that name is already there.".into(),
            ));
        }
        std::fs::create_dir_all(path)
            .map_err(|e| AgentError(format!("The folder could not be made: {e}.")))?;
        self.open_folder(path)
    }

    pub fn setup(&self) -> Setup {
        let payment = self.store.payment();
        Setup {
            runtime: install::check(&self.store),
            account: self.harness.account(payment),
            payment,
            permissions: self.store.permissions(),
        }
    }

    pub fn replace_runtime(&self) -> RuntimeSetup {
        install::replace(&self.store)
    }

    pub fn keep_runtime(&self) -> RuntimeSetup {
        install::keep(&self.store)
    }

    pub fn account(&self) -> Account {
        self.harness.account(self.store.payment())
    }

    pub fn set_payment(&self, payment: Payment) -> Account {
        self.store.set_payment(payment);
        self.harness.account(payment)
    }

    pub fn set_permissions(&self, permissions: Permissions) -> AgentResult<()> {
        self.store.set_permissions(permissions);
        let mut inner = lock(&self.inner);
        let running = !lock(&inner.buffer).ended;
        match inner.conversation.as_mut() {
            Some(conversation) if running => conversation.set_permissions(permissions),
            _ => Ok(()),
        }
    }

    pub fn sign_in(&self) -> AgentResult<()> {
        self.harness.sign_in(self.store.payment())
    }

    fn sink_for(
        &self,
        folder: PathBuf,
        buffer: Arc<Mutex<Buffer>>,
        generation: u64,
        engine: Engine,
        waiter: Arc<Mutex<Option<SavedWaiter>>>,
    ) -> EventSink {
        let store = Arc::clone(&self.store);
        let out = Arc::clone(&self.sink);
        let current = Arc::clone(&self.current);
        let settling = self.settling.clone();
        Arc::new(move |event: AgentEvent| {
            let now = lock(&current);
            let live = *now == generation;
            let mut kept_turn: Option<(String, i64)> = None;
            let mut also: Option<AgentEvent> = None;
            match &event {
                AgentEvent::Ready {
                    session_id, model, ..
                } if !session_id.is_empty() => {
                    let mut buffer = lock(&buffer);
                    buffer.session = Some(session_id.clone());
                    buffer.ready = true;
                    if let Some(model) = model {
                        let list = store.models().models;
                        buffer.model = Some(
                            models::find(&list, model)
                                .map_or(model.clone(), |row| row.id().to_owned()),
                        );
                    }
                    let id = session_id.clone();
                    if live {
                        store.update(&folder, |state| {
                            state.session_id = Some(id);
                            state.engine = Some(engine);
                        });
                        let title = buffer.title.clone();
                        let (model, effort) = (buffer.model.clone(), buffer.effort.clone());
                        let now = Local::now().timestamp_millis();
                        store.note_session(&folder, session_id, |note| {
                            note.engine = Some(engine);
                            note.opened_ms = Some(now);
                            if note.title.is_none() {
                                note.title = title;
                            }
                            if model.is_some() {
                                note.model = model;
                                note.effort = effort;
                            }
                        });
                    }
                }
                AgentEvent::Exited { expected, .. } => {
                    if live {
                        settling.turn_running.store(false, Ordering::SeqCst);
                    }
                    let mut buffer = lock(&buffer);
                    buffer.ended = true;
                    if live && !expected && !buffer.ready {
                        store.update(&folder, |state| state.session_id = None);
                    }
                }
                AgentEvent::Models { models } => {
                    store.received_models(models.clone());
                    let model = lock(&buffer).model.clone();
                    if let (Some(list), Some(model)) = (models, model) {
                        if models::find(list, &model).is_none() {
                            let name = store
                                .pick()
                                .filter(|pick| pick.model == model)
                                .and_then(|pick| pick.name)
                                .unwrap_or(model);
                            also = Some(AgentEvent::Notice {
                                kind: NoticeKind::Other,
                                text: format!("{name} is not offered on your account now, so this lesson may not start. Start a new session and pick another model on its first line."),
                            });
                        }
                    }
                }
                AgentEvent::TextDelta { text } => lock(&buffer).writing.push_str(text),
                AgentEvent::TextDone { text } => {
                    let mut buffer = lock(&buffer);
                    buffer.writing.clear();
                    buffer.tutor.push(text.clone());
                }
                AgentEvent::TurnEnded {
                    ok, interrupted, ..
                } => {
                    let mut buffer = lock(&buffer);
                    buffer.ended_turns += 1;
                    buffer.route_for = None;
                    let unfinished = std::mem::take(&mut buffer.writing);
                    if !unfinished.trim().is_empty() {
                        buffer.tutor.push(unfinished);
                    }
                    let tutor = buffer.tutor.join("\n\n");
                    let learner = std::mem::take(&mut buffer.learner);
                    buffer.tutor.clear();
                    let unanswered = !*ok && !*interrupted && tutor.trim().is_empty();
                    if std::mem::take(&mut buffer.typed)
                        && !learner.trim().is_empty()
                        && !unanswered
                    {
                        store.count_turn(study::today());
                    }
                    let intent = buffer.intent.take();
                    if let Some(session) = buffer.session.clone().filter(|_| !unanswered) {
                        if !tutor.trim().is_empty() || !learner.trim().is_empty() {
                            let ms = Local::now().timestamp_millis();
                            store.append_turn(
                                &folder,
                                &Turn {
                                    session: session.clone(),
                                    ms,
                                    learner,
                                    tutor,
                                    interrupted: *interrupted,
                                    intent,
                                    ..Turn::default()
                                },
                            );
                            buffer.last_ms = Some(ms);
                            buffer.route_for = Some((session.clone(), ms));
                            kept_turn = Some((session, ms));
                        }
                    }
                }
                AgentEvent::SittingEnded => lock(&settling.watch).sitting_ended = true,
                AgentEvent::QueuedDropped { count } => lock(&buffer).ended_turns += count,
                AgentEvent::RouteRead { steps } => {
                    let (target, page_on_it) = {
                        let mut buffer = lock(&buffer);
                        (buffer.route_for.take(), buffer.ended_turns >= buffer.sent)
                    };
                    if let Some((session, ms)) = target {
                        let step = steps.as_ref().map(|steps| step_in_hand(steps));
                        let began = store.note_step(&folder, &session, ms, step);
                        if let Some(StepBegun { n, of, name }) = began {
                            if live && page_on_it {
                                also = Some(AgentEvent::StepBegun { n, of, name });
                            }
                        }
                    }
                }
                AgentEvent::OpenTask { task, posed, .. } if live => {
                    let buffer = lock(&buffer);
                    if let (Some(session), Some(ms)) = (buffer.session.clone(), buffer.last_ms) {
                        store.note_task(&folder, &session, ms, task.as_deref(), *posed);
                    }
                }
                _ => {}
            }
            let ended = matches!(event, AgentEvent::TurnEnded { .. });
            if live {
                if !matches!(
                    event,
                    AgentEvent::Models { .. }
                        | AgentEvent::SittingEnded
                        | AgentEvent::RouteRead { .. }
                        | AgentEvent::QueuedDropped { .. }
                ) {
                    out(event);
                }
                if let Some(notice) = also {
                    out(notice);
                }
            }
            if live && ended {
                settling.turn_running.store(false, Ordering::SeqCst);
                let waiter = lock(&waiter).clone();
                let task_of = match engine {
                    Engine::Plugin => lock(&buffer).session.clone(),
                    Engine::Direct => None,
                };
                settle(
                    settling.clone(),
                    folder.clone(),
                    waiter,
                    Arc::clone(&out),
                    Arc::clone(&current),
                    generation,
                    kept_turn.map(|(session, ms)| (Arc::clone(&store), session, ms)),
                    task_of,
                );
            }
        })
    }

    pub fn open_session(&self, resume: bool) -> AgentResult<Opened> {
        self.open_session_at(None, resume, None, None)
    }

    pub fn open_session_at(
        &self,
        folder: Option<&Path>,
        resume: bool,
        session: Option<&str>,
        title: Option<&str>,
    ) -> AgentResult<Opened> {
        let folder = self.at(folder)?;
        let title = title
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| t.chars().take(200).collect::<String>());
        {
            let mut inner = lock(&self.inner);
            if inner.folder.as_deref() != Some(folder.as_path()) {
                self.close_conversation(&mut inner);
                inner.folder = Some(folder.clone());
            }
        }
        self.store.opened(&folder);
        if let Some(id) = session.filter(|id| resume && !id.is_empty()) {
            let state = self.store.folder(&folder);
            let known = state
                .sessions
                .get(id)
                .and_then(|note| note.engine)
                .or_else(|| {
                    (state.session_id.as_deref() == Some(id))
                        .then_some(state.engine)
                        .flatten()
                });
            let engine = known.unwrap_or_else(|| self.chosen());
            let now = Local::now().timestamp_millis();
            self.store.update(&folder, |state| {
                if state.session_id.as_deref() != Some(id) {
                    state.stopped_at = None;
                    state.stopped_ms = None;
                }
                state.session_id = Some(id.to_owned());
                state.engine = Some(engine);
            });
            self.store.note_session(&folder, id, |note| {
                note.opened_ms = Some(now);
                if note.title.is_none() {
                    note.title.clone_from(&title);
                }
            });
        }
        self.open_session_in(
            &folder,
            resume,
            session.filter(|id| resume && !id.is_empty()),
            title,
        )
    }

    fn open_session_in(
        &self,
        folder: &Path,
        resume: bool,
        session: Option<&str>,
        title: Option<String>,
    ) -> AgentResult<Opened> {
        let folder = folder.to_path_buf();
        if study::needs_upgrade(&folder) {
            return Err(AgentError(
                "An earlier Magnesium set this space up. Upgrade it from the space’s page first."
                    .into(),
            ));
        }
        let mut inner = lock(&self.inner);
        let saved_state = self.store.folder(&folder);
        let engine = if inner
            .papers_next
            .take()
            .is_some_and(|handed_in| handed_in.elapsed() < PAPERS_OPEN_WITHIN)
        {
            Engine::Plugin
        } else if let Some(engine) = session.and(saved_state.engine) {
            engine
        } else {
            self.chosen()
        };
        let running_session = lock(&inner.buffer).session.clone();
        let running = inner.conversation.is_some()
            && !lock(&inner.buffer).ended
            && inner.engine == engine
            && session.is_none_or(|id| running_session.as_deref() == Some(id));
        if running && resume {
            let session = lock(&inner.buffer).session.clone();
            let turns = session
                .map(|id| self.store.turns(&folder, &id))
                .unwrap_or_default();
            return Ok(Opened {
                resumed: true,
                turns,
                engine,
            });
        }
        let generation = self.close_conversation(&mut inner);
        let saved = self.store.folder(&folder);
        let remembered = saved
            .session_id
            .clone()
            .filter(|_| resume && saved.engine.unwrap_or_default() == engine);
        if !resume {
            self.store.update(&folder, |state| {
                state.session_id = None;
                state.stopped_at = None;
                state.stopped_ms = None;
            });
        }
        if let (Some(id), Some(name)) = (remembered.as_deref(), title.as_ref()) {
            self.store.note_session(&folder, id, |note| {
                if note.title.is_none() {
                    note.title = Some(name.clone());
                }
            });
        }
        let (model, effort) = self.start_on(&saved, remembered.as_deref());
        let buffer = Arc::new(Mutex::new(Buffer {
            session: remembered.clone(),
            title,
            model: model.clone(),
            effort: effort.clone(),
            ..Buffer::default()
        }));
        let waiter = Arc::new(Mutex::new(None));
        let conversation = self.harness_for(engine).start(
            StartOptions {
                folder: folder.clone(),
                resume: remembered.clone(),
                payment: self.store.payment(),
                model,
                effort,
                permissions: self.store.permissions(),
            },
            self.sink_for(
                folder.clone(),
                Arc::clone(&buffer),
                generation,
                engine,
                Arc::clone(&waiter),
            ),
        )?;
        *lock(&waiter) = conversation.saved_waiter();
        inner.conversation = Some(conversation);
        inner.buffer = buffer;
        inner.engine = engine;
        let turns = remembered
            .as_deref()
            .map(|id| self.store.turns(&folder, id))
            .unwrap_or_default();
        Ok(Opened {
            resumed: remembered.is_some(),
            turns,
            engine,
        })
    }

    fn with_conversation<T>(
        &self,
        act: impl FnOnce(&mut dyn Conversation) -> AgentResult<T>,
    ) -> AgentResult<T> {
        let mut inner = lock(&self.inner);
        let ended = lock(&inner.buffer).ended;
        let conversation = inner
            .conversation
            .as_mut()
            .filter(|_| !ended)
            .ok_or_else(|| {
                AgentError("The tutor is not running. Send your message again to start it.".into())
            })?;
        act(conversation.as_mut())
    }

    fn send_as(
        &self,
        shown: &str,
        sent: &str,
        typed: bool,
        intent: Option<Intent>,
    ) -> AgentResult<()> {
        let (buffer, folder) = {
            let inner = lock(&self.inner);
            (Arc::clone(&inner.buffer), inner.folder.clone())
        };
        {
            let mut buffer = lock(&buffer);
            buffer.learner = shown.to_owned();
            buffer.typed = typed;
            buffer.sent += 1;
            buffer.intent = intent;
        }
        if let Some(folder) = &folder {
            let before = runtime::marks(folder)
                .map(|rows| rows.into_iter().map(|row| row.id).collect::<HashSet<_>>());
            {
                let mut watch = lock(&self.settling.watch);
                watch.seq += 1;
                watch.marks_before = before;
                watch.intent = intent;
                watch.sent_ms = Local::now().timestamp_millis();
                watch.sitting_ended = false;
            }
            let mut seen = lock(&self.settling.seen);
            if seen.as_ref().is_none_or(|seen| &seen.folder != folder) {
                *seen = Some(Seen {
                    folder: folder.clone(),
                    plan: plan::read(folder),
                    goal_changed_ms: None,
                });
            }
        }
        self.settling.turn_running.store(true, Ordering::SeqCst);
        let sent = self.with_conversation(|conversation| conversation.send_with(sent, intent));
        if sent.is_err() {
            self.settling.turn_running.store(false, Ordering::SeqCst);
        }
        sent
    }

    pub fn send(&self, text: &str) -> AgentResult<()> {
        self.send_with(text, None)
    }

    pub fn send_with(&self, text: &str, intent: Option<Intent>) -> AgentResult<()> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(());
        }
        self.send_as(text, text, true, intent)
    }

    pub fn send_new_lesson(&self, goal: &str) -> AgentResult<()> {
        let goal = goal.trim();
        if goal.is_empty() {
            return Ok(());
        }
        let engine = lock(&self.inner).engine;
        self.send_as(
            goal,
            &self.harness_for(engine).new_lesson(goal),
            false,
            None,
        )
    }

    pub fn wait_saved(&self, limit: Duration) -> bool {
        lock(&self.inner)
            .conversation
            .as_ref()
            .is_none_or(|conversation| conversation.wait_saved(limit))
    }

    pub fn interrupt(&self) -> AgentResult<()> {
        self.with_conversation(|conversation| conversation.interrupt())
    }

    pub fn decide(&self, id: &str, decision: Decision) -> AgentResult<()> {
        self.with_conversation(|conversation| conversation.decide(id, decision))
    }

    pub fn answer(&self, id: &str, answers: &[Answer]) -> AgentResult<()> {
        self.with_conversation(|conversation| conversation.answer(id, answers))
    }

    pub fn stop(&self, stopped_at: Option<&str>) -> AgentResult<()> {
        let folder = self.need_folder()?;
        let line = stopped_at
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned);
        self.store.update(&folder, |state| {
            state.stopped_at = line;
            state.stopped_ms = Some(Local::now().timestamp_millis());
        });
        self.close_conversation(&mut lock(&self.inner));
        Ok(())
    }

    pub fn attach(&self, source: &Path, group: &str) -> AgentResult<String> {
        self.attach_at(None, source, group)
    }

    pub fn attach_at(
        &self,
        folder: Option<&Path>,
        source: &Path,
        group: &str,
    ) -> AgentResult<String> {
        let folder = self.at(folder)?;
        files::hand_in(&folder, group, source).map_err(AgentError)
    }

    pub fn attach_bytes_at(
        &self,
        folder: Option<&Path>,
        name: &str,
        bytes: &[u8],
        group: &str,
    ) -> AgentResult<String> {
        let folder = self.at(folder)?;
        files::hand_in_bytes(&folder, group, name, bytes).map_err(AgentError)
    }

    pub fn add_material(&self, sources: &[PathBuf]) -> AgentResult<Added> {
        self.add_material_at(None, sources)
    }

    pub fn add_material_at(
        &self,
        folder: Option<&Path>,
        sources: &[PathBuf],
    ) -> AgentResult<Added> {
        let root = study::root(&self.at(folder)?);
        let (saved, problems) = material::copy_in(&root, sources);
        Ok(self.said(&root, saved, problems, false))
    }

    pub fn add_pasted_text(&self, text: &str) -> AgentResult<Added> {
        self.add_pasted_text_at(None, text)
    }

    pub fn add_pasted_text_at(&self, folder: Option<&Path>, text: &str) -> AgentResult<Added> {
        let root = study::root(&self.at(folder)?);
        let saved = material::save_text(&root, text).map_err(AgentError)?;
        Ok(self.said(&root, vec![saved], Vec::new(), true))
    }

    fn said(
        &self,
        root: &Path,
        saved: Vec<material::Saved>,
        problems: Vec<String>,
        pasted: bool,
    ) -> Added {
        let named: Vec<String> = saved
            .iter()
            .filter(|file| !file.key)
            .map(|file| file.relative.clone())
            .collect();
        let search = if named.is_empty() {
            None
        } else {
            runtime::searchable(root, &named)
        };
        material::report(&saved, problems, search.as_deref(), pasted)
    }

    pub fn source(&self, path: &str) -> AgentResult<files::Source> {
        self.source_at(None, path)
    }

    pub fn source_at(&self, folder: Option<&Path>, path: &str) -> AgentResult<files::Source> {
        files::source(&study::root(&self.at(folder)?), path).map_err(AgentError)
    }

    pub fn shareable_at(&self, folder: Option<&Path>, path: &str) -> AgentResult<PathBuf> {
        files::shareable(&study::root(&self.at(folder)?), path).map_err(AgentError)
    }

    pub fn source_pdf(&self, path: &str) -> AgentResult<Vec<u8>> {
        self.source_pdf_at(None, path)
    }

    pub fn source_pdf_at(&self, folder: Option<&Path>, path: &str) -> AgentResult<Vec<u8>> {
        files::source_pdf(&study::root(&self.at(folder)?), path).map_err(AgentError)
    }

    pub fn open_file(&self, path: &str) -> AgentResult<()> {
        self.open_file_at(None, path)
    }

    pub fn open_file_at(&self, folder: Option<&Path>, path: &str) -> AgentResult<()> {
        files::open(&self.at(folder)?, path).map_err(AgentError)
    }

    pub fn image(&self, path: &str) -> AgentResult<String> {
        self.image_at(None, path)
    }

    pub fn image_at(&self, folder: Option<&Path>, path: &str) -> AgentResult<String> {
        files::image(&self.at(folder)?, path).map_err(AgentError)
    }

    pub fn paper_start(&self, paper: &Paper, minutes: u32) -> AgentResult<PaperSitting> {
        self.paper_start_at(None, paper, minutes)
    }

    pub fn paper_start_at(
        &self,
        folder: Option<&Path>,
        paper: &Paper,
        minutes: u32,
    ) -> AgentResult<PaperSitting> {
        let folder = self.at(folder)?;
        let sitting = PaperSitting {
            target: paper.target.clone(),
            name: paper.name.clone(),
            path: Some(paper.path.clone()),
            minutes: minutes.clamp(1, 600),
            started_ms: Local::now().timestamp_millis(),
        };
        let saved = sitting.clone();
        self.store
            .update(&folder, |state| state.paper = Some(saved));
        let _ = files::openable(&study::root(&folder), &paper.path)
            .map_err(AgentError)
            .and_then(|file| files::launch(&file).map_err(AgentError));
        Ok(sitting)
    }

    pub fn paper_file(&self, paper: &Paper) -> AgentResult<PathBuf> {
        let root = study::root(&self.need_folder()?);
        files::openable(&root, &paper.path).map_err(AgentError)
    }

    pub fn paper_end(&self, hand_in: &[PathBuf]) -> AgentResult<Option<(String, String)>> {
        self.paper_end_at(None, hand_in)
    }

    pub fn paper_end_at(
        &self,
        folder: Option<&Path>,
        hand_in: &[PathBuf],
    ) -> AgentResult<Option<(String, String)>> {
        let folder = self.at(folder)?;
        let Some(sitting) = self.store.folder(&folder).paper else {
            return Ok(None);
        };
        let mut copied = Vec::new();
        for source in hand_in {
            copied.push(files::hand_in(&folder, &sitting.name, source).map_err(AgentError)?);
        }
        self.store.update(&folder, |state| state.paper = None);
        if copied.is_empty() {
            return Ok(None);
        }
        let used = ((Local::now().timestamp_millis() - sitting.started_ms) / 60_000).max(0);
        let used = used.min(i64::from(sitting.minutes) * 3);
        let words = format!(
            "Mark my {}. I used {} of {} minutes. My work: {}",
            sitting.name,
            used,
            sitting.minutes,
            copied.join(", ")
        );
        lock(&self.inner).papers_next = Some(std::time::Instant::now());
        Ok(Some((words.clone(), self.harness.papers(&words))))
    }

    pub fn send_shown(&self, shown: &str, sent: &str) -> AgentResult<()> {
        self.send_as(shown, sent, false, None)
    }

    pub fn set_block_minutes(&self, minutes: u32) {
        self.store.set_block_minutes(minutes);
    }

    pub fn shut_down(&self) {
        self.close_conversation(&mut lock(&self.inner));
    }
}

fn step_in_hand(steps: &[runtime::Step]) -> Option<StepBegun> {
    let at = steps.iter().position(|step| step.status == "doing")?;
    Some(StepBegun {
        n: at + 1,
        of: steps.len(),
        name: steps[at].step.clone(),
    })
}

#[allow(clippy::too_many_arguments)]
fn settle(
    settling: Settling,
    folder: PathBuf,
    waiter: Option<SavedWaiter>,
    out: EventSink,
    current: Arc<Mutex<u64>>,
    generation: u64,
    turn: Option<(Arc<Store>, String, i64)>,
    task_of: Option<String>,
) {
    let (seq, before, intent, sent_ms) = {
        let mut watch = lock(&settling.watch);
        (
            watch.seq,
            watch.marks_before.take(),
            watch.intent,
            watch.sent_ms,
        )
    };
    settling.pending.fetch_add(1, Ordering::SeqCst);
    std::thread::spawn(move || {
        let mut counted = true;
        let release = |counted: &mut bool| {
            if std::mem::take(counted) {
                settling.pending.fetch_sub(1, Ordering::SeqCst);
            }
        };
        let live = || *lock(&current) == generation;
        let kept = waiter.as_ref().map(|wait| wait(SAVE_WAIT));
        let mut after = plan::read(&folder);
        let what = {
            let mut seen = lock(&settling.seen);
            let (what, mut goal_changed_ms) = match seen.as_ref().filter(|s| s.folder == folder) {
                Some(last) => (plan::changes(&last.plan, &after), last.goal_changed_ms),
                None => (Vec::new(), None),
            };
            if what
                .iter()
                .any(|part| matches!(part, PlanPart::Goal | PlanPart::Steps))
            {
                goal_changed_ms = Some(Local::now().timestamp_millis());
            }
            after.changed_ms = after.changed_ms.max(goal_changed_ms);
            *seen = Some(Seen {
                folder: folder.clone(),
                plan: after,
                goal_changed_ms,
            });
            what
        };
        if !what.is_empty() && live() {
            out(AgentEvent::PlanChanged { what });
        }
        let read_lesson = || -> (Option<Option<runtime::Lesson>>, bool) {
            let Some(conversation) = task_of.as_deref() else {
                return (None, false);
            };
            if lock(&settling.watch).seq != seq {
                return (None, true);
            }
            let lesson = runtime::lesson(&folder, Some(conversation));
            (lesson, lock(&settling.watch).seq != seq)
        };
        let told: Mutex<Option<Option<String>>> = Mutex::new(None);
        let read_task = |told: &Mutex<Option<Option<String>>>,
                         lesson: &Option<Option<runtime::Lesson>>| {
            if task_of.is_none() || lock(&settling.watch).seq != seq || !live() {
                return;
            }
            let Some(Some(lesson)) = lesson else {
                return;
            };
            let open = lesson.open_task.clone();
            let posed = open
                .as_ref()
                .is_some_and(|task| task.posed_at_ms >= sent_ms);
            let task = open.map(|task| task.text);
            let mut told = lock(told);
            if told.as_ref() == Some(&task) {
                return;
            }
            if let Some((store, session, ms)) = &turn {
                store.note_task(&folder, session, *ms, task.as_deref(), posed);
            }
            if lock(&settling.watch).seq == seq && live() {
                let fallback = task.is_none();
                out(AgentEvent::OpenTask {
                    task: task.clone(),
                    posed,
                    fallback,
                });
            }
            *told = Some(task);
        };
        let stepped: Mutex<Option<Option<Option<StepBegun>>>> = Mutex::new(None);
        let read_step = |lesson: &Option<Option<runtime::Lesson>>, moved_on: bool| {
            let Some((store, session, ms)) = &turn else {
                return;
            };
            if task_of.is_none() {
                return;
            }
            let step = if moved_on {
                None
            } else {
                lesson.as_ref().map(|lesson| {
                    lesson
                        .as_ref()
                        .and_then(|lesson| step_in_hand(&lesson.steps))
                })
            };
            let mut stepped = lock(&stepped);
            if stepped.as_ref() == Some(&step) {
                return;
            }
            let began = store.note_step(&folder, session, *ms, step.clone());
            *stepped = Some(step);
            if let Some(StepBegun { n, of, name }) = began {
                if lock(&settling.watch).seq == seq && live() {
                    out(AgentEvent::StepBegun { n, of, name });
                }
            }
        };
        let (lesson, moved_on) = read_lesson();
        read_task(&told, &lesson);
        read_step(&lesson, moved_on);
        let mut closed: Option<Vec<String>> = None;
        let read_closing = |closed: &mut Option<Vec<String>>| {
            {
                let watch = lock(&settling.watch);
                if watch.seq != seq || !watch.sitting_ended || !live() {
                    return;
                }
            }
            let Some(closing) = runtime::closing(&folder) else {
                return;
            };
            if closing.ended_ms.is_none_or(|ended| ended < sent_ms)
                || closing.lines.is_empty()
                || closed.as_ref() == Some(&closing.lines)
            {
                return;
            }
            if let Some((store, session, ms)) = &turn {
                store.note_closing(&folder, session, *ms, &closing.lines);
            }
            out(AgentEvent::Closing {
                lines: closing.lines.clone(),
            });
            *closed = Some(closing.lines);
        };
        read_closing(&mut closed);
        let mut marked_late = false;
        let mut found: Option<Vec<String>> = None;
        if let Some(before) = before {
            let once = kept == Some(true) || (intent == Some(Intent::Talk) && kept.is_some());
            let mut waited = 0;
            let mut read = false;
            let mut cut = false;
            for (look, at) in MARK_LOOKS_MS.iter().enumerate() {
                if look >= BUSY_LOOKS {
                    release(&mut counted);
                }
                std::thread::sleep(Duration::from_millis(at - waited));
                waited = *at;
                if lock(&settling.watch).seq != seq || !live() {
                    cut = true;
                    break;
                }
                let rows = runtime::marks(&folder);
                read |= rows.is_some();
                let mut fresh: Vec<runtime::Mark> = rows
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|row| !before.contains(&row.id))
                    .collect();
                if !fresh.is_empty() {
                    marked_late = look > 0;
                    found = Some(fresh.iter().map(|mark| mark.id.clone()).collect());
                    runtime::note_passes(&mut fresh, &folder);
                    if live() {
                        for mark in fresh {
                            out(AgentEvent::Marked { mark });
                        }
                    }
                    break;
                }
                if once {
                    break;
                }
            }
            if found.is_none() && read && !cut {
                found = Some(Vec::new());
            }
        }
        if let (Some(ids), Some((store, session, ms))) = (&found, &turn) {
            store.note_marks(&folder, session, *ms, ids);
        }
        if found.as_ref().is_some_and(|ids| !ids.is_empty()) && marked_late {
            let (lesson, moved_on) = read_lesson();
            if !moved_on && lesson.is_some() {
                read_task(&told, &lesson);
                read_step(&lesson, false);
            }
        }
        if closed.is_none() || marked_late {
            read_closing(&mut closed);
        }
        if lock(&settling.watch).seq == seq && live() {
            out(AgentEvent::Settled);
        }
        release(&mut counted);
    });
}
