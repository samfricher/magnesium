//! The one interface between the window and a coding agent.
//!
//! Nothing outside `claude.rs` may assume Claude Code. A second agent (Codex,
//! OpenCode) is a second implementation of [`Harness`] and [`Conversation`].

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

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
    /// building a plan. The window shows one quiet line.
    Working,
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

#[derive(Clone, Debug)]
pub struct StartOptions {
    /// The study folder. The agent runs here, so it loads this folder's
    /// settings, hooks and plugins exactly as a terminal session does.
    pub folder: PathBuf,
    /// The agent's own id of a conversation to continue.
    pub resume: Option<String>,
}

pub type EventSink = Arc<dyn Fn(AgentEvent) + Send + Sync>;

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
}

/// One running conversation with the tutor.
pub trait Conversation: Send {
    /// Send the learner's words. The window sends plain words only, so a
    /// lesson started here can continue in a terminal.
    fn send(&mut self, text: &str) -> AgentResult<()>;
    /// Stop the reply being written.
    fn interrupt(&mut self) -> AgentResult<()>;
    fn decide(&mut self, id: &str, decision: Decision) -> AgentResult<()>;
    fn answer(&mut self, id: &str, answers: &[Answer]) -> AgentResult<()>;
    /// End the process. The conversation can be continued later by its id.
    fn close(&mut self);
}
