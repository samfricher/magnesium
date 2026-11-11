//! Every integration test, in one test program.
//!
//! Each module starts the built `magnesium` program and drives it the way
//! Claude Code or a learner does. One program links once and runs all of its
//! tests at the same time, so a new test file is a new `mod` line here, never
//! a new file directly under `tests/`.

mod support;

mod adapter_call;
mod attempts;
mod captured_turn_lifecycle;
mod checkpoint_recovery;
mod control_safety_review;
mod conversation_handoff;
mod digest;
mod distribution;
mod export;
mod fresh_workspace_references;
mod host_identity;
mod info;
mod init;
mod init_reports;
mod instruction_bundle_loading;
mod instruction_bundle_placeholders;
mod invariants;
mod learner_read_surface;
mod lifecycle;
mod mcp_headless;
mod mcp_host_metadata;
mod pending_chronology;
mod pending_task_recovery;
mod plugin_install;
mod practice;
mod progress;
mod readings;
mod review_queue;
mod searchable;
mod security;
mod sittings;
mod smoke;
mod source_catch_up;
mod source_search_concurrency;
mod sources_view;
mod support_environment;
mod tool_contract_recipes;
mod tool_recipe_examples;
mod tutor_guidance;
mod update_protocol;
mod workspace_ownership;
