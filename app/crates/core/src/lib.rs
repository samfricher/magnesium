//! Everything Magnesium's window does that is not drawing.
//!
//! - [`agent`] is the one interface to a coding agent, and [`claude`] is its
//!   first implementation.
//! - [`runtime`] runs the `magnesium` program and reads what it prints, and
//!   [`install`] puts the app's own copy of it on this Mac.
//! - [`study`] turns that into what the library, the course page and the
//!   progress page show.
//! - [`activity`] keeps the days the learner studied, and [`find`] looks for
//!   study folders set up in a terminal.
//! - [`material`] copies course material the learner adds into the study
//!   folder and words what search can and cannot read of it.
//! - [`manager`] holds one open study folder and its one conversation.
//!
//! No teaching rule lives here. The runtime and its plugin hold them all, so
//! a lesson behaves the same in this window and in a terminal.

pub mod activity;
pub mod agent;
pub mod claude;
pub mod files;
pub mod find;
pub mod gaplog;
pub mod install;
pub mod manager;
pub mod material;
pub mod paths;
pub mod peek;
pub mod runtime;
pub mod store;
pub mod study;

pub use agent::{Account, AgentEvent, Answer, Decision, EventSink, Harness, Payment};
pub use manager::Manager;
pub use store::Store;
