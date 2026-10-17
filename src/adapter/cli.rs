//! CLI runner for `magnesium adapter call <operation>`.

use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process::ExitCode;

use serde_json::Value;

use super::SUPPORTED_PROTOCOL_VERSION;

const MAX_JSON_INPUT_BYTES: u64 = 4 * 1024 * 1024;

pub(crate) fn run_cli_call(operation: &str, protocol_version: u64, json_input: &str) -> ExitCode {
    if protocol_version != SUPPORTED_PROTOCOL_VERSION {
        eprintln!(
            "adapter protocol mismatch: requested {protocol_version}, supported {SUPPORTED_PROTOCOL_VERSION}"
        );
        return ExitCode::from(1);
    }
    let input_bytes = match read_json_input(json_input) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("error reading json-input: {error}");
            return ExitCode::from(1);
        }
    };
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(error) => {
            eprintln!("cannot determine current working directory: {error}");
            return ExitCode::from(1);
        }
    };
    match dispatch_operation(operation, &cwd, &input_bytes) {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(serialized) => {
                println!("{serialized}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("adapter operation `{operation}` output serialization failed: {error}");
                ExitCode::from(1)
            }
        },
        Err(error) => {
            eprintln!("adapter operation `{operation}` failed: {error}");
            ExitCode::from(1)
        }
    }
}

fn read_json_input(source: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    if source == "-" {
        io::stdin()
            .take(MAX_JSON_INPUT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
    } else {
        let file = fs::File::open(source).map_err(|e| e.to_string())?;
        file.take(MAX_JSON_INPUT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
    }
    if bytes.len() as u64 > MAX_JSON_INPUT_BYTES {
        return Err(format!(
            "json input exceeds {MAX_JSON_INPUT_BYTES}-byte bound"
        ));
    }
    Ok(bytes)
}

fn dispatch_operation(operation: &str, cwd: &Path, input: &[u8]) -> Result<Value, String> {
    let request = serde_json::from_slice(input).map_err(|error| error.to_string())?;
    super::dispatch(cwd, operation, request)
}
