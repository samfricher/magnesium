//! Everything Magnesium's window does that is not drawing.
//!
//! - [`agent`] is the one interface to a coding agent, and [`claude`] is its
//!   first implementation.
//! - [`runtime`] runs the `magnesium` program and reads what it prints, and
//!   [`install`] puts the app's own copy of it on this Mac.
//! - [`study`] turns that into what the entrance and progress pages show.
//! - [`manager`] holds one open study folder and its one conversation.
//!
//! No teaching rule lives here. The runtime and its plugin hold them all, so
//! a lesson behaves the same in this window and in a terminal.

pub mod agent;
pub mod claude;
pub mod files;
pub mod gaplog;
pub mod install;
pub mod manager;
pub mod paths;
pub mod peek;
pub mod runtime;
pub mod store;
pub mod study;

pub use agent::{Account, AgentEvent, Answer, Decision, EventSink, Harness, Payment};
pub use manager::Manager;
pub use store::Store;
