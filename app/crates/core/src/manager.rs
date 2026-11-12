//! One open study folder and its one conversation.
//!
//! The window is a layer: every teaching rule stays in the runtime and the
//! plugin. So this sends the learner's plain words and nothing more, and a
//! button such as "I'm lost" sends those two words.

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
    Harness, Intent, Payment, SavedWaiter, StartOptions,
};
use crate::files;
use crate::find;
use crate::install::{self, RuntimeSetup};
use crate::journal;
use crate::material::{self, Added};
use crate::paths;
use crate::plan::{self, Plan, PlanPart};
use crate::runtime;
use crate::store::{Engine, PaperSitting, Store, Turn};
use crate::study::{self, Card, Home, Paper, Progress, Rail};

#[derive(Default)]
struct Buffer {
    session: Option<String>,
    learner: String,
    tutor: Vec<String>,
    /// Words of a block that has not finished. An interrupt can leave some.
    writing: String,
    /// Whether this process has reported what it loaded.
    ready: bool,
    /// Whether this process has ended.
    ended: bool,
    /// Whether the words in `learner` are an answer the learner typed. The
    /// line the window sends to open a sitting is not one.
    typed: bool,
}

struct Inner {
    folder: Option<PathBuf>,
    conversation: Option<Box<dyn Conversation>>,
    buffer: Arc<Mutex<Buffer>>,
    /// The engine of the conversation above.
    engine: Engine,
    /// Handing in a paper opens the next conversation on the plugin engine,
    /// which marks papers: when the page opens it at once, as it does after
    /// a hand-in. A hand-in the page never followed up does not linger.
    papers_next: Option<std::time::Instant>,
}

/// What the manager watches across one message: the record's marks before
/// it was sent, so a row that appears after it is that message's mark.
#[derive(Default)]
struct Watch {
    /// Counts messages sent, so a settle stops when the learner writes again.
    seq: u64,
    /// The ids of the record's marked answers just before the message, or
    /// none when the runtime cannot list them.
    marks_before: Option<HashSet<String>>,
    intent: Option<Intent>,
}

/// The last reading of a folder's plan, which the next turn is compared with.
struct Seen {
    folder: PathBuf,
    plan: Plan,
    /// When the window last saw the goal or its steps change.
    goal_changed_ms: Option<i64>,
}

/// What a turn's settle needs, shared between the manager and each sink.
#[derive(Clone)]
struct Settling {
    watch: Arc<Mutex<Watch>>,
    seen: Arc<Mutex<Option<Seen>>>,
    /// A message was sent and its reply has not ended.
    turn_running: Arc<AtomicBool>,
    /// Settles still reading the record for the last turn.
    pending: Arc<AtomicUsize>,
}

/// How long a settle waits for the direct engine to keep a turn's record:
/// its save budget and the read-back after it, with room to spare.
const SAVE_WAIT: Duration = Duration::from_secs(35);
/// When the record is read for a new mark after a reply ends, counted from
/// the end of the reply (or of its save), as the lesson read it before:
/// the plugin engine's tutor may write the mark just after the reply.
const MARK_LOOKS_MS: [u64; 6] = [0, 1_000, 2_500, 5_000, 10_000, 20_000];
/// Looks after this many no longer keep the window busy for the harness.
const BUSY_LOOKS: usize = 3;

pub struct Manager {
    store: Arc<Store>,
    harness: Arc<dyn Harness>,
    /// The direct engine, when the window offers it.
    direct: Option<Arc<dyn Harness>>,
    sink: EventSink,
    inner: Mutex<Inner>,
    /// Counts conversations. A conversation that was closed or replaced
    /// still runs for a while, and its events must not reach the page.
    current: Arc<Mutex<u64>>,
    settling: Settling,
}

/// What this Mac has for lessons: the `magnesium` program, the agent and how
/// lessons are paid for. The start screen shows it before the first folder.
#[derive(Clone, Debug, Serialize)]
pub struct Setup {
    pub runtime: RuntimeSetup,
    pub account: Account,
    pub payment: Payment,
}

/// What the session screen needs when it opens.
#[derive(Clone, Debug, Serialize)]
pub struct Opened {
    pub resumed: bool,
    pub turns: Vec<Turn>,
    /// The engine running this conversation.
    pub engine: Engine,
}

/// The engine the learner picked for new conversations, and the engine of
/// the open folder's conversation, running or remembered, if it has one.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EngineChoice {
    pub chosen: Engine,
    pub conversation: Option<Engine>,
}

/// How soon after a hand-in the page opens the conversation that marks it.
const PAPERS_OPEN_WITHIN: Duration = Duration::from_secs(60);

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

fn none_open() -> AgentError {
    AgentError("Open a study folder first.".into())
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
        }
    }

    /// Offer the direct engine beside the plugin engine.
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

    /// The engine new conversations start on. Without the direct engine on
    /// offer it is always the plugin engine.
    fn chosen(&self) -> Engine {
        match self.direct {
            Some(_) => self.store.engine(),
            None => Engine::Plugin,
        }
    }

    /// The engine picked, and the open folder's conversation's.
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

    /// Pick the engine for new conversations. A running conversation goes
    /// on with its own; the next one the learner opens uses this one.
    pub fn set_engine(&self, engine: Engine) -> EngineChoice {
        self.store.set_engine(engine);
        self.engine()
    }

    /// Close the conversation, if there is one. From here on nothing it says
    /// reaches the page or changes what the folder remembers.
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

    /// Whether a message's reply is running, or the record is still being
    /// read for what it changed. For the evidence harness.
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

    pub fn recent(&self) -> Vec<String> {
        self.store.recent()
    }

    /// Every folder in the library, most urgent first. It opens none of
    /// them, so a lesson in the open folder goes on.
    pub fn library(&self) -> Vec<Card> {
        study::library(&self.store.recent(), &self.store)
    }

    /// Every course's kept turns and marked answers from `since_ms` on, for
    /// the log. Read without opening a folder.
    pub fn journal(&self, since_ms: i64) -> Vec<journal::Course> {
        journal::read(&self.store.recent(), &self.store, since_ms)
    }

    /// Take a folder out of the library. The folder is not touched. The open
    /// folder cannot be taken out while its conversation runs.
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

    /// Look under the home folder for study folders set up in a terminal,
    /// and put them in the library. Returns how many were new.
    pub fn find_folders(&self) -> usize {
        let found = find::study_folders(&paths::home(), Duration::from_secs(6));
        self.store.add_folders(&found)
    }

    /// The days the learner studied, and the run they make. The days come
    /// from the record of every course in the library: a day counts when the
    /// tutor marked an answer on it, in this window or in a terminal. With a
    /// runtime that cannot say, they are the days an answer was sent from
    /// the window.
    pub fn activity(&self) -> Activity {
        let days = study::answered_days(&self.store.recent()).unwrap_or_else(|| self.store.days());
        activity::summarize(&days, study::today())
    }

    /// Open a study folder. Any conversation in the previous folder is ended;
    /// it stays on disk and continues the next time that folder is opened.
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
        Ok(study::home(&self.need_folder()?, &self.store))
    }

    pub fn progress(&self, target: Option<&str>) -> AgentResult<Progress> {
        Ok(study::progress(&self.need_folder()?, target))
    }

    pub fn rail(&self) -> AgentResult<Rail> {
        Ok(study::rail(&self.need_folder()?))
    }

    /// The open folder's study plan, read fresh.
    pub fn plan(&self) -> AgentResult<Plan> {
        let folder = self.need_folder()?;
        let mut plan = plan::read(&folder);
        if let Some(seen) = lock(&self.settling.seen)
            .as_ref()
            .filter(|seen| seen.folder == folder)
        {
            plan.changed_ms = plan.changed_ms.max(seen.goal_changed_ms);
        }
        Ok(plan)
    }

    /// The answers the runtime has marked in the open folder, oldest first.
    /// None when this runtime has no `attempts` command. The lesson compares
    /// two readings of this and never takes the tutor's word for a mark.
    pub fn marks(&self) -> AgentResult<Option<Vec<runtime::Mark>>> {
        Ok(runtime::marks(&self.need_folder()?))
    }

    /// The files in the open course's material folder.
    pub fn material_list(&self) -> AgentResult<material::Listed> {
        Ok(material::list(&study::root(&self.need_folder()?)))
    }

    /// Set a folder up for Magnesium, which adds a `.magnesium` folder to it.
    pub fn set_up(&self) -> AgentResult<Home> {
        let folder = self.need_folder()?;
        runtime::init(&folder)
            .map_err(|e| AgentError(format!("Magnesium could not set this folder up: {e}.")))?;
        Ok(study::home(&folder, &self.store))
    }

    /// Upgrade a folder an earlier Magnesium set up, after the learner
    /// agreed. The runtime keeps a copy of the record before it changes it.
    pub fn upgrade_folder(&self) -> AgentResult<Home> {
        let folder = self.need_folder()?;
        runtime::upgrade(&study::root(&folder))
            .map_err(|e| AgentError(format!("Magnesium could not upgrade this folder: {e}.")))?;
        Ok(study::home(&folder, &self.store))
    }

    /// Make a new, empty study folder and open it. Its first lesson sets it
    /// up, as in any folder Magnesium has not been set up in.
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

    /// The program, the agent and the way of paying, after installing the
    /// app's own program if none is installed.
    pub fn setup(&self) -> Setup {
        let payment = self.store.payment();
        Setup {
            runtime: install::check(&self.store),
            account: self.harness.account(payment),
            payment,
        }
    }

    /// Replace another installed `magnesium` with the app's own. The learner
    /// agreed to this.
    pub fn replace_runtime(&self) -> RuntimeSetup {
        install::replace(&self.store)
    }

    /// Keep another installed `magnesium` and stop asking about it.
    pub fn keep_runtime(&self) -> RuntimeSetup {
        install::keep(&self.store)
    }

    pub fn account(&self) -> Account {
        self.harness.account(self.store.payment())
    }

    /// Choose how lessons are paid for. The next conversation is started
    /// that way, and the account shows how it would be paid for.
    pub fn set_payment(&self, payment: Payment) -> Account {
        self.store.set_payment(payment);
        self.harness.account(payment)
    }

    /// Start the agent's own sign-in for the chosen way of paying.
    pub fn sign_in(&self) -> AgentResult<()> {
        self.harness.sign_in(self.store.payment())
    }

    /// The sink the agent writes to. It keeps the window's own copy of each
    /// turn and remembers the conversation's id, then passes the event on.
    /// Once the conversation is closed or replaced, it only keeps the copy of
    /// its own turns, which is stored under its own id.
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
            // Held to the end, so the conversation cannot be replaced between
            // this check and the event reaching the page.
            let now = lock(&current);
            let live = *now == generation;
            match &event {
                AgentEvent::Ready { session_id, .. } if !session_id.is_empty() => {
                    let mut buffer = lock(&buffer);
                    buffer.session = Some(session_id.clone());
                    buffer.ready = true;
                    let id = session_id.clone();
                    if live {
                        store.update(&folder, |state| {
                            state.session_id = Some(id);
                            state.engine = Some(engine);
                        });
                    }
                }
                // An agent that ends before it reports anything was most
                // likely asked for a conversation it no longer has. Forget
                // the id, so the next message starts a new conversation.
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
                AgentEvent::TextDelta { text } => lock(&buffer).writing.push_str(text),
                AgentEvent::TextDone { text } => {
                    let mut buffer = lock(&buffer);
                    buffer.writing.clear();
                    buffer.tutor.push(text.clone());
                }
                AgentEvent::TurnEnded { interrupted, .. } => {
                    let mut buffer = lock(&buffer);
                    let unfinished = std::mem::take(&mut buffer.writing);
                    if !unfinished.trim().is_empty() {
                        buffer.tutor.push(unfinished);
                    }
                    let tutor = buffer.tutor.join("\n\n");
                    let learner = std::mem::take(&mut buffer.learner);
                    buffer.tutor.clear();
                    if std::mem::take(&mut buffer.typed) && !learner.trim().is_empty() {
                        store.count_turn(study::today());
                    }
                    if let Some(session) = buffer.session.clone() {
                        if !tutor.trim().is_empty() || !learner.trim().is_empty() {
                            store.append_turn(
                                &folder,
                                &Turn {
                                    session,
                                    ms: Local::now().timestamp_millis(),
                                    learner,
                                    tutor,
                                    interrupted: *interrupted,
                                },
                            );
                        }
                    }
                }
                _ => {}
            }
            let ended = matches!(event, AgentEvent::TurnEnded { .. });
            if live {
                out(event);
            }
            // What the turn changed is read once the reply has ended, and
            // its record has been kept.
            if live && ended {
                settling.turn_running.store(false, Ordering::SeqCst);
                let waiter = lock(&waiter).clone();
                settle(
                    settling.clone(),
                    folder.clone(),
                    waiter,
                    Arc::clone(&out),
                    Arc::clone(&current),
                    generation,
                );
            }
        })
    }

    /// Open the session screen. With `resume`, the folder's last conversation
    /// continues and its earlier turns come back. Without, the next message
    /// starts a new conversation.
    pub fn open_session(&self, resume: bool) -> AgentResult<Opened> {
        let folder = self.need_folder()?;
        // The agent's hooks would open the record, and an earlier version's
        // record would be upgraded without the learner's yes.
        if study::needs_upgrade(&folder) {
            return Err(AgentError(
                "An earlier Magnesium set this folder up. Upgrade it from the start screen first."
                    .into(),
            ));
        }
        let mut inner = lock(&self.inner);
        // A conversation resumes only on its own engine. Paper marking is
        // the plugin engine's, whatever the learner picked.
        let engine = if inner
            .papers_next
            .take()
            .is_some_and(|handed_in| handed_in.elapsed() < PAPERS_OPEN_WITHIN)
        {
            Engine::Plugin
        } else {
            self.chosen()
        };
        let running =
            inner.conversation.is_some() && !lock(&inner.buffer).ended && inner.engine == engine;
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
            .filter(|_| resume && saved.engine.unwrap_or_default() == engine);
        if !resume {
            self.store.update(&folder, |state| {
                state.session_id = None;
                state.stopped_at = None;
                state.stopped_ms = None;
            });
        }
        let buffer = Arc::new(Mutex::new(Buffer {
            session: remembered.clone(),
            ..Buffer::default()
        }));
        let waiter = Arc::new(Mutex::new(None));
        let conversation = self.harness_for(engine).start(
            StartOptions {
                folder: folder.clone(),
                resume: remembered.clone(),
                payment: self.store.payment(),
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
        }
        // What the record and the plan hold before the message, so the turn
        // can be compared with them once it has settled.
        if let Some(folder) = &folder {
            let before = runtime::marks(folder)
                .map(|rows| rows.into_iter().map(|row| row.id).collect::<HashSet<_>>());
            {
                let mut watch = lock(&self.settling.watch);
                watch.seq += 1;
                watch.marks_before = before;
                watch.intent = intent;
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

    /// Send the learner's words exactly as typed, and leave it to the tutor
    /// whether they answer the open task.
    pub fn send(&self, text: &str) -> AgentResult<()> {
        self.send_with(text, None)
    }

    /// Send the learner's words exactly as typed, saying what they are:
    /// talk is never marked, an answer is marked as any answer is.
    pub fn send_with(&self, text: &str, intent: Option<Intent>) -> AgentResult<()> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(());
        }
        self.send_as(text, text, true, intent)
    }

    /// Start a new lesson on what the learner named.
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

    /// Wait, at most `limit`, until the last reply's record is kept: the
    /// direct engine ends a turn when the reply is whole and keeps its
    /// record just after. For the tests.
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

    /// End the sitting. The line the learner stopped at opens the next one.
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

    /// Copy a photo or PDF of the learner's work into the study folder.
    pub fn attach(&self, source: &Path, group: &str) -> AgentResult<String> {
        let folder = self.need_folder()?;
        files::hand_in(&folder, group, source).map_err(AgentError)
    }

    /// Add course material to the study folder: copy what the learner dropped
    /// or picked, then ask the runtime what the tutor's search can read of it.
    pub fn add_material(&self, sources: &[PathBuf]) -> AgentResult<Added> {
        let root = study::root(&self.need_folder()?);
        let (saved, problems) = material::copy_in(&root, sources);
        Ok(self.said(&root, saved, problems, false))
    }

    /// Save pasted text as a text file in the same place.
    pub fn add_pasted_text(&self, text: &str) -> AgentResult<Added> {
        let root = study::root(&self.need_folder()?);
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
        // An answer key is added and never looked at again by the window.
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

    /// A source a reply cited, for the panel beside the lesson. The path is
    /// from the study root, which can be a parent of the open folder.
    pub fn source(&self, path: &str) -> AgentResult<files::Source> {
        files::source(&study::root(&self.need_folder()?), path).map_err(AgentError)
    }

    pub fn source_pdf(&self, path: &str) -> AgentResult<Vec<u8>> {
        files::source_pdf(&study::root(&self.need_folder()?), path).map_err(AgentError)
    }

    pub fn open_file(&self, path: &str) -> AgentResult<()> {
        files::open(&self.need_folder()?, path).map_err(AgentError)
    }

    pub fn image(&self, path: &str) -> AgentResult<String> {
        files::image(&self.need_folder()?, path).map_err(AgentError)
    }

    /// Begin a timed paper. The clock is the start time on disk, so it keeps
    /// running if the window is closed.
    pub fn paper_start(&self, paper: &Paper, minutes: u32) -> AgentResult<PaperSitting> {
        let folder = self.need_folder()?;
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
        // The paper opens in the system viewer so it can be printed. A paper
        // that will not open does not stop the clock: the learner may hold a
        // printed copy.
        let _ = self
            .paper_file(paper)
            .and_then(|file| files::launch(&file).map_err(AgentError));
        Ok(sitting)
    }

    /// The file of a paper, checked as any file the window opens. A paper's
    /// path is inside the study root, which can be a parent of the folder.
    pub fn paper_file(&self, paper: &Paper) -> AgentResult<PathBuf> {
        let root = study::root(&self.need_folder()?);
        files::openable(&root, &paper.path).map_err(AgentError)
    }

    /// End a timed paper. With files, the learner's work is copied into the
    /// study folder and the words that ask the tutor to mark it are returned.
    pub fn paper_end(&self, hand_in: &[PathBuf]) -> AgentResult<Option<(String, String)>> {
        let folder = self.need_folder()?;
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
        // The plugin engine marks papers: the conversation that sends these
        // words is opened on it.
        lock(&self.inner).papers_next = Some(std::time::Instant::now());
        Ok(Some((words.clone(), self.harness.papers(&words))))
    }

    /// Send words the window wrote for the learner: the line that opens a
    /// sitting, or the request to mark a paper. They can be shown one way and
    /// worded another for the agent, and they are not counted as an answer.
    pub fn send_shown(&self, shown: &str, sent: &str) -> AgentResult<()> {
        self.send_as(shown, sent, false, None)
    }

    pub fn set_block_minutes(&self, minutes: u32) {
        self.store.set_block_minutes(minutes);
    }

    /// End the conversation when the window closes.
    pub fn shut_down(&self) {
        self.close_conversation(&mut lock(&self.inner));
    }
}

/// Read what a turn changed, once its record is kept: the plan, compared with
/// the last reading, and the record's marks, compared with those before the
/// message. Sends `plan_changed` and `marked` while the conversation is still
/// the page's. Runs on its own thread, so the page is never held up.
fn settle(
    settling: Settling,
    folder: PathBuf,
    waiter: Option<SavedWaiter>,
    out: EventSink,
    current: Arc<Mutex<u64>>,
    generation: u64,
) {
    let (seq, before, intent) = {
        let mut watch = lock(&settling.watch);
        (watch.seq, watch.marks_before.take(), watch.intent)
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
        // The plan, against the last reading.
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
        // The record's new marks. The direct engine has kept the turn's
        // record by now, so one reading says it all; the plugin engine's
        // tutor may write it a little after the reply, so it is read a few
        // times more. A message sent meanwhile ends the looking.
        if let Some(before) = before {
            let once = kept == Some(true) || (intent == Some(Intent::Talk) && kept.is_some());
            let mut waited = 0;
            for (look, at) in MARK_LOOKS_MS.iter().enumerate() {
                if look >= BUSY_LOOKS {
                    release(&mut counted);
                }
                std::thread::sleep(Duration::from_millis(at - waited));
                waited = *at;
                if lock(&settling.watch).seq != seq || !live() {
                    break;
                }
                let fresh: Vec<runtime::Mark> = runtime::marks(&folder)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|row| !before.contains(&row.id))
                    .collect();
                if !fresh.is_empty() {
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
        }
        release(&mut counted);
    });
}
