//! Everything Magnesium's window does that is not drawing.
//!
//! - [`agent`] is the one interface to a coding agent, and [`claude`] is its
//!   first implementation.
//! - [`runtime`] runs the `magnesium` program and reads what it prints.
//! - [`study`] turns that into what the entrance and progress pages show.
//! - [`manager`] holds one open study folder and its one conversation.
//!
//! No teaching rule lives here. The runtime and its plugin hold them all, so
//! a lesson behaves the same in this window and in a terminal.

pub mod agent;
pub mod claude;
pub mod files;
pub mod gaplog;
pub mod manager;
pub mod paths;
pub mod peek;
pub mod runtime;
pub mod store;
pub mod study;

pub use agent::{AgentEvent, Answer, Decision, EventSink, Harness};
pub use manager::Manager;
pub use store::Store;
