//! The one interface between the window and a coding agent.
//!
//! Nothing outside `claude.rs` may assume Claude Code. A second agent is a
//! second implementation of [`Harness`] and [`Conversation`].

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::plan::PlanPart;
use crate::runtime::Mark;

/// What the window shows. Tool calls, saves, token counts and model names are
/// deliberately absent: the window shows the tutor's words and nothing else.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// The agent reported what it loaded. `problems` is empty when the
    /// Magnesium plugin and its tool server are both present.
    Ready {
        session_id: String,
        problems: Vec<String>,
        /// True when the Magnesium plugin itself is absent. The session would
        /// then be a plain chat with no teaching rules, so the window stops it.
        plugin_missing: bool,
    },
    /// The tutor began a reply.
    TurnStarted,
    /// More of the tutor's reply, as it is written.
    TextDelta { text: String },
    /// One finished block of the tutor's reply. It replaces the deltas that
    /// came before it.
    TextDone { text: String },
    /// The tutor is doing something other than writing: reading, saving,
    /// building a plan. The window shows one quiet line. `doing` names the
    /// work in plain words, such as "writing paper 2", when the window can
    /// tell; it never holds a tool's name, a command, or a path outside the
    /// study folder.
    Working { doing: Option<String> },
    /// The tutor wants to do something that needs the learner's yes.
    Permission {
        id: String,
        /// A plain sentence: "Run a command", "Write a file".
        title: String,
        /// The command, path or address.
        detail: String,
        can_always: bool,
    },
    /// The tutor asked the learner to choose.
    Question {
        id: String,
        questions: Vec<Question>,
    },
    /// The agent withdrew a permission request or a question.
    Withdrawn { id: String },
    /// The reply ended. `error` is a plain sentence when it ended badly.
    TurnEnded {
        ok: bool,
        interrupted: bool,
        error: Option<String>,
    },
    /// Something the learner must fix outside the window, in one line.
    Notice { kind: NoticeKind, text: String },
    /// The agent process ended.
    Exited {
        expected: bool,
        detail: Option<String>,
    },
    /// A turn changed the open folder's plan: its exam date, its list of
    /// what to study, the goal, the goal's steps or the plan file's text.
    /// Sent after the turn ends; `plan` reads the plan as it now stands.
    PlanChanged { what: Vec<PlanPart> },
    /// The runtime's record has a new marked answer for the last message.
    /// The mark is the record's row, never the tutor's words.
    Marked { mark: Mark },
    /// The task open after a turn, in the tutor's own words as its record
    /// block wrote it, or none; `posed` when this turn's reply asked for it.
    /// The window puts the field for an answer inside it. Sent by an engine
    /// that knows its open task (Magnesium's); never a mark.
    OpenTask { task: Option<String>, posed: bool },
    /// The record has been read for what the last message changed: any mark
    /// it has for it has been sent. With none, the record kept no mark.
    Settled,
}

/// What the learner's message is. The window says it; the tutor does not
/// guess it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    /// Conversation: a question, a change to the plan, something off the
    /// plan. It is never marked, and the open task stays open.
    Talk,
    /// An answer to the open task, marked as any answer is.
    Answer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    UsageLimit,
    SignIn,
    Retry,
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub question: String,
    pub header: Option<String>,
    pub options: Vec<Choice>,
    pub multi: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    pub label: String,
    pub description: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    AllowOnce,
    AllowAlways,
    Refuse,
}

/// One answered question: the question's text and the chosen labels, or the
/// learner's own words.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    pub question: String,
    pub chosen: Vec<String>,
}

/// How lessons are paid for. The learner chooses, and the window starts the
/// agent so that the choice holds. The window never reads or stores a
/// credential: it shows what the agent itself reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Payment {
    /// The learner's plan with the agent's maker, such as a Claude plan.
    #[default]
    Plan,
    /// An API key, paid per use.
    ApiKey,
}

/// The agent on this computer: whether it is there and signed in, and how a
/// lesson started now would be paid for, as the agent reports it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Account {
    /// The agent's name, such as "Claude Code".
    pub name: String,
    pub installed: bool,
    pub signed_in: bool,
    /// How a lesson started now is paid for. None when no lesson can start.
    pub paid_with: Option<Payment>,
    /// The same in plain words, such as "your Claude Max plan".
    pub detail: Option<String>,
    /// What stands in the way of the learner's choice, in one or two lines.
    pub problem: Option<String>,
    /// The maker's install page, when the agent is missing.
    pub install_page: Option<String>,
}

#[derive(Clone, Debug)]
pub struct StartOptions {
    /// The study folder. The agent runs here, so it loads this folder's
    /// settings, hooks and plugins exactly as a terminal session does.
    pub folder: PathBuf,
    /// The agent's own id of a conversation to continue.
    pub resume: Option<String>,
    /// How the learner chose to pay for lessons.
    pub payment: Payment,
}

pub type EventSink = Arc<dyn Fn(AgentEvent) + Send + Sync>;

/// Waits, at most the given time, until what the agent keeps after a reply
/// has been kept. True when it has.
pub type SavedWaiter = Arc<dyn Fn(std::time::Duration) -> bool + Send + Sync>;

#[derive(Debug)]
pub struct AgentError(pub String);

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AgentError {}

pub type AgentResult<T> = Result<T, AgentError>;

/// A coding agent the learner installed and signed in to.
pub trait Harness: Send + Sync {
    /// Whether the agent's program can be found, and where.
    fn locate(&self) -> AgentResult<PathBuf>;
    /// Start, or continue, a conversation in a study folder.
    fn start(&self, options: StartOptions, sink: EventSink) -> AgentResult<Box<dyn Conversation>>;
    /// The learner's first words for a new lesson. Each agent has its own way
    /// to name the Magnesium skill, so the wording lives here.
    fn new_lesson(&self, goal: &str) -> String;
    /// The learner's words for practice-paper work, such as "mark my Paper B".
    fn papers(&self, request: &str) -> String;
    /// Whether the agent is installed and signed in, and how a lesson started
    /// now with this choice would be paid for. It reads no credential.
    fn account(&self, payment: Payment) -> Account;
    /// Start the agent's own sign-in for this way of paying. It opens the
    /// maker's page in the browser, and the window sees nothing typed there.
    /// It returns while the sign-in waits; `account` then shows the result.
    fn sign_in(&self, payment: Payment) -> AgentResult<()>;
}

/// One running conversation with the tutor.
pub trait Conversation: Send {
    /// Send the learner's words. The window sends plain words only, so a
    /// lesson started here can continue in a terminal.
    fn send(&mut self, text: &str) -> AgentResult<()>;
    /// Send the learner's words, saying what they are. None leaves it to the
    /// tutor, as `send` does. An engine that cannot hold the tutor to it
    /// says so in the words it sends.
    fn send_with(&mut self, text: &str, intent: Option<Intent>) -> AgentResult<()> {
        let _ = intent;
        self.send(text)
    }
    /// Stop the reply being written.
    fn interrupt(&mut self) -> AgentResult<()>;
    fn decide(&mut self, id: &str, decision: Decision) -> AgentResult<()>;
    fn answer(&mut self, id: &str, answers: &[Answer]) -> AgentResult<()>;
    /// End the process. The conversation can be continued later by its id.
    fn close(&mut self);
    /// Wait, at most `limit`, until what the agent keeps after a reply has
    /// been kept. True when it has. An agent that keeps nothing after the
    /// reply ends has nothing to wait for.
    fn wait_saved(&self, _limit: std::time::Duration) -> bool {
        true
    }
    /// The same wait, as a handle that outlives a borrow of the
    /// conversation, so another thread can wait without holding it. None
    /// for an agent that keeps nothing after the reply ends.
    fn saved_waiter(&self) -> Option<SavedWaiter> {
        None
    }
}
