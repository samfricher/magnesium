#![forbid(unsafe_code)]

//! Internal composition boundary for the Magnesium runtime.
//!
//! Composes the native CLI, host adapters, local learner storage and source retrieval.
//! Supported interfaces are the executable commands and host tool protocols; this
//! library target is not a public Rust SDK commitment.

mod adapter;
mod cli;
mod diagnostics;
mod digest;
mod export;
mod hooks;
pub(crate) mod learner;
mod mcp;
mod request;
mod retrieval;
mod update;
mod workspace;

use std::path::PathBuf;
use std::process::ExitCode;

use cli::command;
/// Dispatch the single native executable. This function is an executable composition
/// boundary, not a supported Rust SDK API.
pub fn run() -> ExitCode {
    match command().get_matches().subcommand() {
        Some(("init", matches)) => {
            let path = matches.get_one::<PathBuf>("path").map(PathBuf::as_path);
            let adapter = matches.get_one::<String>("adapter").map(String::as_str);
            let json = matches.get_flag("json");
            let upgrade = matches.get_flag("upgrade-instructions");
            {
                if json {
                    workspace::run_init_json(path, adapter, upgrade)
                } else {
                    workspace::run_init(path, adapter, upgrade)
                }
            }
        }
        Some(("update", _)) => update::run(),
        Some(("info", matches)) => {
            let json = matches.get_flag("json");
            diagnostics::print_info(json)
        }
        Some(("digest", _)) => digest::print_digest(),
        Some(("export", matches)) => {
            let destination = matches
                .get_one::<PathBuf>("destination")
                .expect("clap requires export destination");
            let format = matches
                .get_one::<String>("format")
                .map(String::as_str)
                .unwrap_or("dir");
            let include_source_copies = matches.get_flag("include-source-copies");
            let overwrite = matches.get_flag("overwrite");
            export::run(destination, format, include_source_copies, overwrite)
        }
        Some(("memory", matches)) => {
            let result = (|| -> Result<serde_json::Value, String> {
                let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
                let workspace::WorkspaceResolution::Active(resolved) =
                    workspace::resolve_initialized(&cwd).map_err(|e| e.to_string())?
                else {
                    return Err("Magnesium workspace is not initialized".into());
                };
                match matches.subcommand() {
                    Some(("reindex", _)) => {
                        let store = learner::LearnerStore::open(
                            resolved.store_path(),
                            resolved.workspace_id(),
                        )
                        .map_err(|e| e.to_string())?;
                        learner::search::reindex(store.connection()).map_err(|e| e.to_string())?;
                        Ok(
                            serde_json::json!({"status":"reindexed","workspace_id":resolved.workspace_id()}),
                        )
                    }
                    _ => Err("unknown memory operation".into()),
                }
            })();
            match result {
                Ok(value) => {
                    println!("{value}");
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("magnesium: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        Some(("adapter", matches)) => match matches.subcommand() {
            Some(("call", call_matches)) => {
                let operation = call_matches
                    .get_one::<String>("operation")
                    .expect("clap requires operation");
                let protocol_version = *call_matches
                    .get_one::<u64>("protocol-version")
                    .expect("clap requires protocol-version");
                let json_input = call_matches
                    .get_one::<String>("json-input")
                    .expect("clap requires json-input");
                adapter::cli::run_cli_call(operation, protocol_version, json_input)
            }
            _ => ExitCode::from(2),
        },
        Some(("_internal", matches)) => match matches.subcommand() {
            // MCP binds workspace identity for this host connection.
            Some(("mcp-server", matches)) => mcp::run(
                matches.get_one::<String>("plugin-version").cloned(),
                matches
                    .get_one::<String>("adapter-protocol-version")
                    .cloned(),
                matches.get_one::<String>("host-kind").cloned(),
            ),
            Some(("extract-pdf", _)) => retrieval::run_pdf_extractor(),
            Some(("hook", hook_matches)) => {
                let event = hook_matches
                    .get_one::<String>("event")
                    .expect("clap requires the hook event");
                let plugin_version = hook_matches.get_one::<String>("plugin-version").cloned();
                let protocol_version = hook_matches
                    .get_one::<String>("adapter-protocol-version")
                    .cloned();
                let host_kind = hook_matches.get_one::<String>("host-kind").cloned();
                hooks::run(event, plugin_version, protocol_version, host_kind)
            }
            _ => ExitCode::from(2),
        },
        None => ExitCode::SUCCESS,
        Some(_) => ExitCode::from(2),
    }
}
