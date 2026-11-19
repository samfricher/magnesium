use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::plan::PlanPart;
use crate::runtime::Mark;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    Ready {
        session_id: String,
        problems: Vec<String>,
        plugin_missing: bool,
        #[serde(default)]
        model: Option<String>,
    },
    TurnStarted,
    TextDelta {
        text: String,
    },
    TextDone {
        text: String,
    },
    Working {
        doing: Option<String>,
    },
    Permission {
        id: String,
        title: String,
        detail: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exact: Option<String>,
        can_always: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        always: Option<String>,
    },
    Question {
        id: String,
        questions: Vec<Question>,
    },
    Withdrawn {
        id: String,
    },
    TurnEnded {
        ok: bool,
        interrupted: bool,
        error: Option<String>,
    },
    Notice {
        kind: NoticeKind,
        text: String,
    },
    Exited {
        expected: bool,
        detail: Option<String>,
    },
    PlanChanged {
        what: Vec<PlanPart>,
    },
    Marked {
        mark: Mark,
    },
    OpenTask {
        task: Option<String>,
        posed: bool,
        #[serde(default)]
        fallback: bool,
    },
    Closing {
        lines: Vec<String>,
    },
    StepBegun {
        n: usize,
        of: usize,
        name: String,
    },
    RouteRead {
        steps: Option<Vec<crate::runtime::Step>>,
    },
    QueuedDropped {
        count: u64,
    },
    SittingEnded,
    Settled,
    Models {
        models: Option<Vec<crate::models::ModelOption>>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Talk,
    Answer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    UsageLimit,
    SignIn,
    Retry,
    Turn,
    Other,
    RecordUnopened,
    RecordUnopenedAtEffort,
    ModelUnavailable,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    pub question: String,
    pub chosen: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Payment {
    #[default]
    Plan,
    ApiKey,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permissions {
    #[default]
    Auto,
    Ask,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Account {
    pub name: String,
    pub installed: bool,
    pub signed_in: bool,
    pub paid_with: Option<Payment>,
    pub detail: Option<String>,
    pub problem: Option<String>,
    pub install_page: Option<String>,
}

#[derive(Clone, Debug)]
pub struct StartOptions {
    pub folder: PathBuf,
    pub resume: Option<String>,
    pub payment: Payment,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permissions: Permissions,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Applied {
    pub model: String,
    pub effort: Option<String>,
}

pub type EventSink = Arc<dyn Fn(AgentEvent) + Send + Sync>;

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

pub trait Harness: Send + Sync {
    fn locate(&self) -> AgentResult<PathBuf>;
    fn start(&self, options: StartOptions, sink: EventSink) -> AgentResult<Box<dyn Conversation>>;
    fn new_lesson(&self, goal: &str) -> String;
    fn papers(&self, request: &str) -> String;
    fn account(&self, payment: Payment) -> Account;
    fn sign_in(&self, payment: Payment) -> AgentResult<()>;
    fn models(&self, _payment: Payment) -> AgentResult<Option<Vec<crate::models::ModelOption>>> {
        Err(AgentError("This agent does not list its models.".into()))
    }
}

pub trait Conversation: Send {
    fn send(&mut self, text: &str) -> AgentResult<()>;
    fn send_with(&mut self, text: &str, intent: Option<Intent>) -> AgentResult<()> {
        let _ = intent;
        self.send(text)
    }
    fn interrupt(&mut self) -> AgentResult<()>;
    fn decide(&mut self, id: &str, decision: Decision) -> AgentResult<()>;
    fn answer(&mut self, id: &str, answers: &[Answer]) -> AgentResult<()>;
    fn change_model(&mut self, model: &str, effort: Option<&str>) -> AgentResult<Applied> {
        let _ = (model, effort);
        Err(AgentError(
            "This tutor cannot change its model while it runs.".into(),
        ))
    }
    fn set_permissions(&mut self, permissions: Permissions) -> AgentResult<()> {
        let _ = permissions;
        Ok(())
    }
    fn applied(&mut self) -> AgentResult<Applied> {
        Err(AgentError(
            "This tutor does not say which model it runs on.".into(),
        ))
    }
    fn close(&mut self);
    fn wait_saved(&self, _limit: std::time::Duration) -> bool {
        true
    }
    fn saved_waiter(&self) -> Option<SavedWaiter> {
        None
    }
}
