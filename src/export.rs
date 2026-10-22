//! Private, consistent snapshots of the workspace learner model.
use crate::{
    learner::store::LearnerStore,
    workspace::{Workspace, create_private_dir, reject_symlink_components, restrict_private_path},
};
use rusqlite::{Connection, types::ValueRef};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};
#[derive(Clone, Copy, Debug)]
pub(crate) enum ExportFormat {
    Dir,
    Zip,
}
pub(crate) struct ExportOptions {
    pub destination: PathBuf,
    pub format: ExportFormat,
    pub include_source_copies: bool,
    pub overwrite: bool,
}
enum CopyFailure {
    Unavailable(String),
    Source(crate::retrieval::ManifestError),
}
impl From<crate::retrieval::ManifestError> for CopyFailure {
    fn from(error: crate::retrieval::ManifestError) -> Self {
        Self::Source(error)
    }
}
impl From<crate::retrieval::DiscoveryError> for CopyFailure {
    fn from(error: crate::retrieval::DiscoveryError) -> Self {
        Self::Source(error.into())
    }
}

const EXPORT_MARKER_CONTENT: &[u8] = b"magnesium-export-v2\n";

const TABLES: &[&str] = &[
    "workspace_meta",
    "learning_sessions",
    "host_bindings",
    "learning_events",
    "observations",
    "observation_events",
    "learner_memories",
    "memory_topics",
    "memory_observations",
    "memory_changes",
    "operation_receipts",
    "review_items",
    "context_receipts",
];
fn workspace(root: &Path) -> Result<Workspace, String> {
    Workspace::discover(root)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "workspace_not_initialized".into())
}
fn rows(conn: &Connection, table: &str) -> Result<Value, String> {
    // Table identifiers come exclusively from the fixed schema inventory above.
    let mut statement = conn
        .prepare(&format!("SELECT * FROM {table}"))
        .map_err(|e| e.to_string())?;
    let names: Vec<String> = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let result = statement
        .query_map([], |row| {
            let mut object = serde_json::Map::new();
            for (i, name) in names.iter().enumerate() {
                let value = match row.get_ref(i)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(v) => json!(v),
                    ValueRef::Real(v) => json!(v),
                    ValueRef::Text(v) => json!(String::from_utf8_lossy(v)),
                    ValueRef::Blob(_) => Value::Null,
                };
                object.insert(name.clone(), value);
            }
            Ok(Value::Object(object))
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    Ok(json!(result))
}
// Revalidate an existing directory before touching even its reserved marker.
fn validate_export_directory(destination: &Path) -> Result<(), String> {
    let manifest = destination.join("manifest.json");
    let meta = fs::symlink_metadata(&manifest)
        .map_err(|_| "overwrite requires an existing Magnesium export directory")?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 1024 * 1024 {
        return Err("overwrite requires a bounded regular export manifest".into());
    }
    let manifest: Value = serde_json::from_slice(&fs::read(&manifest).map_err(|e| e.to_string())?)
        .map_err(|_| "overwrite requires an existing Magnesium export manifest")?;
    if manifest["schema"] != "learner-model/v2" || manifest["serializer"] != "magnesium-export-v2" {
        return Err("overwrite requires an existing Magnesium export directory".into());
    }
    Ok(())
}

fn protect_previous_export(directory: &Path) -> Result<(), String> {
    use std::io::Write;
    validate_export_directory(directory)?;
    let marker = directory.join(crate::retrieval::EXPORT_BOUNDARY_MARKER);
    match fs::symlink_metadata(&marker) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => return Ok(()),
        Ok(_) => {
            return Err(
                "existing export marker must be a regular file, not a symlink or directory".into(),
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    // A manifest-validated export whose marker was deleted gets it back before
    // the backup rename, so the backup stays outside source discovery. Create
    // only this reserved file: never replace a user file or follow a
    // concurrently added marker.
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&marker).map_err(|error| error.to_string())?;
    restrict_private_path(&marker).map_err(|error| error.to_string())?;
    file.write_all(EXPORT_MARKER_CONTENT)
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

pub(crate) fn export_learner_data(root: &Path, options: &ExportOptions) -> Result<Value, String> {
    let cancellation = crate::retrieval::OperationCancellation::none();
    cancellation.check().map_err(|error| error.to_string())?;
    let w = workspace(root)?;
    let id = w.id().map_err(|e| e.to_string())?;
    reject_symlink_components(&w.store_path(), w.control()).map_err(|e| e.to_string())?;
    let store = LearnerStore::open(&w.store_path(), &id).map_err(|e| e.to_string())?;
    let parent = options
        .destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|e| e.to_string())?;
    let name = options
        .destination
        .file_name()
        .ok_or("invalid destination")?;
    let destination = parent.join(name);
    if destination.starts_with(w.control()) || w.root().starts_with(&destination) {
        return Err(format!(
            "export destination cannot be a workspace root, its ancestor, or inside {}",
            crate::workspace::CONTROL_DIR
        ));
    }
    let existing = match fs::symlink_metadata(&destination) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
                return Err("export destination must not be a symlink or special file".into());
            }
            if !options.overwrite {
                return Err("export destination exists".into());
            }
            if metadata.is_dir() {
                validate_export_directory(&destination)?;
            }
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.to_string()),
    };
    let mut snapshot = Connection::open_in_memory().map_err(|e| e.to_string())?;
    {
        let backup =
            rusqlite::backup::Backup::new(&store.conn, &mut snapshot).map_err(|e| e.to_string())?;
        backup
            .run_to_completion(128, Duration::from_millis(5), None)
            .map_err(|e| e.to_string())?;
    }
    let mut data = serde_json::Map::new();
    for table in TABLES {
        cancellation.check().map_err(|error| error.to_string())?;
        data.insert((*table).into(), rows(&snapshot, table)?);
    }
    let mut readable = String::from(
        "# Magnesium learner-model export\n\nPrivate workspace snapshot. Current descriptions differ from historical revisions and from packets prepared for earlier tutor turns. Context receipts establish prepared output; host traces are needed to establish delivery.\n\n",
    );
    if let Some(memories) = data["learner_memories"].as_array() {
        for memory in memories {
            readable.push_str(&format!(
                "## {} (revision {}, {})\n\nMemory: {}\n\n{}\n\n{}\n\nReview note: {}\n\nSupport: {}\n\n",
                memory["title"].as_str().unwrap_or(""),
                memory["revision"],
                memory["lifecycle"].as_str().unwrap_or(""),
                memory["memory_id"].as_str().unwrap_or(""),
                memory["summary"].as_str().unwrap_or(""),
                memory["state_text"].as_str().unwrap_or(""),
                memory["review_note"].as_str().unwrap_or("none"),
                memory["current_basis_json"].as_str().unwrap_or("")
            ));
        }
    }
    let mut files = vec![
        // Publish the exclusion marker before any private content enters the
        // staging directory. Copies, renames, backups and extracted ZIPs keep
        // the same source boundary without a path registry.
        (
            crate::retrieval::EXPORT_BOUNDARY_MARKER.to_owned(),
            EXPORT_MARKER_CONTENT.to_vec(),
        ),
        (
            "learner.json".to_owned(),
            serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?,
        ),
        ("README.md".to_owned(), readable.into_bytes()),
    ];
    let mut source_coverage = Vec::new();
    let mut copied_paths = std::collections::BTreeSet::new();
    if options.include_source_copies {
        let mut seen = std::collections::BTreeSet::new();
        for observation in data["observations"].as_array().into_iter().flatten() {
            cancellation.check().map_err(|error| error.to_string())?;
            let references: Value =
                serde_json::from_str(observation["source_refs_json"].as_str().unwrap_or("[]"))
                    .map_err(|e| e.to_string())?;
            for reference in references.as_array().into_iter().flatten() {
                #[cfg(test)]
                crate::request::test_checkpoint("export_before_source_copy");
                cancellation.check().map_err(|error| error.to_string())?;
                let Some(path) = reference["path"].as_str() else {
                    continue;
                };
                if !seen.insert((
                    path.to_owned(),
                    reference["hash"].as_str().map(str::to_owned),
                )) {
                    continue;
                }
                let relative = Path::new(path);
                if path.contains('\\')
                    || path.contains(':')
                    || relative.is_absolute()
                    || relative
                        .components()
                        .any(|c| !matches!(c, std::path::Component::Normal(_)))
                {
                    source_coverage.push(json!({"path":path,"status":"invalid_path"}));
                    continue;
                }
                let copy = (|| -> Result<Vec<u8>, CopyFailure> {
                    if crate::retrieval::source_relative_path_is_excluded(w.root(), relative)? {
                        return Err(CopyFailure::Unavailable("source_excluded".into()));
                    }
                    let mut check = |_: &Path| cancellation.check();
                    let opened =
                        crate::retrieval::open_workspace_source(w.root(), relative, &mut check)
                            .map_err(|error| match error {
                                crate::retrieval::ManifestError::Io { ref source, .. }
                                    if source.kind() == std::io::ErrorKind::NotFound =>
                                {
                                    CopyFailure::Unavailable("source_missing".into())
                                }
                                other => CopyFailure::Source(other),
                            })?;
                    if opened.metadata.len() > 16 * 1024 * 1024 {
                        return Err(CopyFailure::Unavailable("source_too_large".into()));
                    }
                    let bytes = crate::adapter::read_bounded_bytes(
                        opened.file,
                        &opened.path,
                        16 * 1024 * 1024,
                        &cancellation,
                    )?;
                    if bytes.len() > 16 * 1024 * 1024 {
                        return Err(CopyFailure::Unavailable("source_too_large".into()));
                    }
                    if crate::retrieval::sensitive_content(&bytes).is_some() {
                        return Err(CopyFailure::Unavailable("sensitive_source_content".into()));
                    }
                    use sha2::Digest;
                    let hash = sha2::Sha256::digest(&bytes)
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>();
                    if let Some(expected) = reference["hash"].as_str()
                        && hash != expected
                    {
                        return Err(CopyFailure::Unavailable("source_changed".into()));
                    }
                    if bytes.starts_with(b"%PDF-")
                        || relative
                            .extension()
                            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
                    {
                        return crate::adapter::read_authorized_pdf(
                            w.root(),
                            path,
                            &hash,
                            &cancellation,
                        )
                        .map_err(|error| match error {
                            crate::adapter::PdfCopyError::Unavailable(reason) => {
                                CopyFailure::Unavailable(reason.into())
                            }
                            crate::adapter::PdfCopyError::Source(error) => {
                                CopyFailure::Source(error)
                            }
                        });
                    }
                    if std::str::from_utf8(&bytes).is_err() {
                        return Err(CopyFailure::Unavailable(
                            "source_encoding_not_supported".into(),
                        ));
                    }
                    if crate::retrieval::source_relative_path_is_excluded(w.root(), relative)? {
                        return Err(CopyFailure::Unavailable("source_excluded".into()));
                    }
                    cancellation.check()?;
                    Ok(bytes)
                })();
                match copy {
                    Ok(bytes) => {
                        cancellation.check().map_err(|error| error.to_string())?;
                        if copied_paths.insert(path.to_owned()){files.push((format!("sources/{path}"), bytes));}
                        source_coverage.push(json!({"path":path,"requested_hash":reference["hash"],"status":"copied_current"}));
                    }
                    Err(CopyFailure::Source(error)) => return Err(error.to_string()),
                    Err(CopyFailure::Unavailable(error)) => source_coverage
                        .push(json!({"path":path,"requested_hash":reference["hash"],"status":"unavailable","reason":error})),
                }
            }
        }
    }
    files.push(("manifest.json".into(),serde_json::to_vec_pretty(&json!({"schema":"learner-model/v2","workspace_id":id,"serializer":"magnesium-export-v2","tables":TABLES,"source_coverage":source_coverage})).map_err(|e|e.to_string())?));
    let stage = parent
        .join(crate::learner::store::new_id(".magnesium-export-stage").map_err(|e| e.to_string())?);
    let publication = (|| -> Result<Option<PathBuf>, String> {
        cancellation.check().map_err(|error| error.to_string())?;
        match options.format {
            ExportFormat::Dir => {
                create_private_dir(&stage).map_err(|e| e.to_string())?;
                for (name, bytes) in &files {
                    #[cfg(test)]
                    crate::request::test_checkpoint("export_before_stage_write");
                    cancellation.check().map_err(|error| error.to_string())?;
                    crate::workspace::write_private_atomic(&stage.join(name), bytes)
                        .map_err(|e| e.to_string())?;
                }
            }
            ExportFormat::Zip => {
                use std::io::Write;
                let mut options = fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = options.open(&stage).map_err(|e| e.to_string())?;
                restrict_private_path(&stage).map_err(|e| e.to_string())?;
                for chunk in build_zip(&files)?.chunks(64 * 1024) {
                    cancellation.check().map_err(|error| error.to_string())?;
                    file.write_all(chunk).map_err(|e| e.to_string())?;
                }
                file.sync_all().map_err(|e| e.to_string())?;
            }
        }
        cancellation.check().map_err(|error| error.to_string())?;
        let backup = if existing {
            // Keep the previous artifact intact, including any user additions.
            let backup = parent.join(
                crate::learner::store::new_id(".magnesium-export-previous")
                    .map_err(|e| e.to_string())?,
            );
            let metadata = fs::symlink_metadata(&destination).map_err(|e| e.to_string())?;
            if metadata.file_type().is_symlink() {
                return Err("export destination changed to a symlink".into());
            }
            if metadata.is_dir() {
                protect_previous_export(&destination)?;
            }
            cancellation.check().map_err(|error| error.to_string())?;
            fs::rename(&destination, &backup).map_err(|e| e.to_string())?;
            Some(backup)
        } else {
            None
        };
        if !existing && fs::symlink_metadata(&destination).is_ok() {
            return Err("export destination appeared during preparation".into());
        }
        #[cfg(test)]
        crate::request::test_checkpoint("export_before_publication");
        let publish = cancellation
            .check()
            .map_err(std::io::Error::other)
            .and_then(|()| match options.format {
                ExportFormat::Dir => fs::rename(&stage, &destination),
                ExportFormat::Zip => {
                    fs::hard_link(&stage, &destination).and_then(|()| fs::remove_file(&stage))
                }
            });
        if let Err(error) = publish {
            if let Some(backup) = &backup
                && fs::symlink_metadata(&destination).is_err()
            {
                fs::rename(backup, &destination).map_err(|restore| {
                    format!(
                        "publication failed: {error}; previous export remains at {}: {restore}",
                        backup.display()
                    )
                })?;
            }
            return Err(error.to_string());
        }
        Ok(backup)
    })();
    if publication.is_err() {
        // Only the fresh, runtime-created staging artifact is cleaned up.
        if stage.is_dir() {
            let _ = fs::remove_dir_all(&stage);
        } else {
            let _ = fs::remove_file(&stage);
        }
    }
    let previous_export_backup = publication?;
    Ok(
        json!({"schema":"learner-model/v2","workspace_id":id,"destination":destination.to_string_lossy(),"previous_export_backup":previous_export_backup.as_ref().map(|p|p.to_string_lossy()),"format":match options.format{ExportFormat::Dir=>"dir",ExportFormat::Zip=>"zip"},"files":files.iter().map(|x|&x.0).collect::<Vec<_>>()}),
    )
}
pub(crate) fn run(
    destination: &Path,
    format: &str,
    include_source_copies: bool,
    overwrite: bool,
) -> ExitCode {
    let result = (|| {
        let root = std::env::current_dir().map_err(|e| e.to_string())?;
        let format = match format {
            "dir" => ExportFormat::Dir,
            "zip" => ExportFormat::Zip,
            _ => return Err("invalid export format".into()),
        };
        export_learner_data(
            &root,
            &ExportOptions {
                destination: destination.to_owned(),
                format,
                include_source_copies,
                overwrite,
            },
        )
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
fn build_zip(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    if files.len() > u16::MAX as usize {
        return Err("export exceeds ZIP32 file-count limit; use directory format".into());
    }
    let total = files
        .iter()
        .try_fold(22u64, |total, (name, bytes)| {
            total.checked_add(bytes.len() as u64 + name.len() as u64 * 2 + 76)
        })
        .ok_or("export size overflow")?;
    if total > u32::MAX as u64 {
        return Err("export exceeds ZIP32 size limit; use directory format".into());
    }
    let mut output = Vec::new();
    let mut central = Vec::new();
    let cancellation = crate::retrieval::OperationCancellation::none();
    for (relative, bytes) in files {
        cancellation.check().map_err(|error| error.to_string())?;
        let name = relative.as_bytes();
        if name.len() > u16::MAX as usize {
            return Err(format!("export path {} is too long for zip", relative));
        }
        let crc = crc32(bytes);
        let header_offset = output.len() as u32;
        // Local file header.
        write_u32(&mut output, 0x0403_4b50);
        write_u16(&mut output, 20);
        write_u16(&mut output, 1 << 11);
        write_u16(&mut output, 0);
        write_u16(&mut output, 0);
        write_u16(&mut output, 0);
        write_u32(&mut output, crc);
        write_u32(&mut output, bytes.len() as u32);
        write_u32(&mut output, bytes.len() as u32);
        write_u16(&mut output, name.len() as u16);
        write_u16(&mut output, 0);
        output.extend_from_slice(name);
        for chunk in bytes.chunks(64 * 1024) {
            cancellation.check().map_err(|error| error.to_string())?;
            output.extend_from_slice(chunk);
        }
        // Central directory entry.
        write_u32(&mut central, 0x0201_4b50);
        write_u16(&mut central, 20);
        write_u16(&mut central, 20);
        write_u16(&mut central, 1 << 11);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u32(&mut central, crc);
        write_u32(&mut central, bytes.len() as u32);
        write_u32(&mut central, bytes.len() as u32);
        write_u16(&mut central, name.len() as u16);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u32(&mut central, 0);
        write_u32(&mut central, header_offset);
        central.extend_from_slice(name);
    }
    let central_offset = output.len() as u32;
    let central_len = central.len() as u32;
    output.extend_from_slice(&central);
    write_u32(&mut output, 0x0605_4b50);
    write_u16(&mut output, 0);
    write_u16(&mut output, 0);
    write_u16(&mut output, files.len() as u16);
    write_u16(&mut output, files.len() as u16);
    write_u32(&mut output, central_len);
    write_u32(&mut output, central_offset);
    write_u16(&mut output, 0);
    Ok(output)
}

fn write_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn write_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learner::{HostContext, RawEvent};
    struct Fixture {
        root: PathBuf,
        workspace: Workspace,
        store: LearnerStore,
        host: HostContext,
        memory: String,
    }
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(crate::learner::store::new_id("consumer-test").unwrap());
            fs::create_dir_all(root.join("workspace").join(crate::workspace::CONTROL_DIR)).unwrap();
            fs::write(
                root.join("workspace")
                    .join(crate::workspace::CONTROL_DIR)
                    .join("config.toml"),
                "schema_version=1\nworkspace_id='consumer-test'\n",
            )
            .unwrap();
            let workspace = workspace(&root.join("workspace")).unwrap();
            let store = LearnerStore::initialize(&workspace.store_path(), "consumer-test").unwrap();
            let host = HostContext {
                host_kind: "test".into(),
                host_thread_id: "monday".into(),
            };
            store.update(&host,json!({"schema":"learner-model/v2","action":"update","request_id":"start","session":{"operation":"start","client_ref":"s","goal":"quadratics"}})).unwrap();
            let event = store
                .capture(
                    &host,
                    &[RawEvent {
                        external_event_key: "q1".into(),
                        speaker: crate::learner::Speaker::Learner,
                        kind: "message".into(),
                        text: "I applied the formula correctly, but what does b stand for?".into(),
                        metadata: Some(crate::learner::EventMetadata::default()),
                        occurred_at_ms: Some(100),
                    }],
                    "model_reported",
                    false,
                )
                .unwrap()
                .remove(0);
            let result=store.update(&host,json!({"schema":"learner-model/v2","action":"update","request_id":"memory","turn_ref":event,"observations":[{"client_ref":"o","kind":"learner_question","description":"Formula substitution is available; coefficient roles uncertain.","events":[{"event_id":event,"role":"learner_response","excerpts":[]}]}],"memory_edits":[{"operation":"create","client_ref":"m","record":{"title":"Coefficient roles","scope":"Quadratics","summary":"Formula application works; symbol roles need explanation.","state_text":"Preserve the substitution strength. The question does not establish a broad algebra difficulty.","current_basis":[{"new_observation":"o"}]},"reason":"Learner question","change_basis":[{"new_observation":"o"}]}],"event_dispositions":[{"event_id":event,"outcome":"processed"}]})).unwrap();
            Self {
                root,
                workspace,
                store,
                host,
                memory: result.references["m"].clone(),
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn export_uses_writer_records_and_exact_prepared_context() {
        let f = Fixture::new();
        let packet = crate::learner::read::test_support::execute(
            &f.store.conn,
            "test",
            "monday",
            json!({"schema":"learner-model/v2","action":"prepare"}),
        )
        .unwrap();
        let later = f
            .store
            .capture(
                &f.host,
                &[RawEvent {
                    external_event_key: "later-challenge".into(),
                    speaker: crate::learner::Speaker::Learner,
                    kind: "message".into(),
                    text: "Please revisit whether I truly understand b.".into(),
                    metadata: Some(crate::learner::EventMetadata::default()),
                    occurred_at_ms: Some(200),
                }],
                "model_reported",
                false,
            )
            .unwrap()
            .remove(0);
        f.store.update(&f.host,json!({"schema":"learner-model/v2","action":"update","request_id":"later-review","turn_ref":later,"memory_edits":[{"operation":"flag","memory_id":f.memory,"expected_revision":1,"review_note":"Recheck coefficient understanding","reason":"Learner requests reconsideration","change_basis":[{"event_id":later}]}],"event_dispositions":[{"event_id":later,"outcome":"processed"}]})).unwrap();
        let destination = f.root.join("export");
        export_learner_data(
            f.workspace.root(),
            &ExportOptions {
                destination: destination.clone(),
                format: ExportFormat::Dir,
                include_source_copies: false,
                overwrite: false,
            },
        )
        .unwrap();
        let data: Value =
            serde_json::from_slice(&fs::read(destination.join("learner.json")).unwrap()).unwrap();
        assert_eq!(data["learner_memories"][0]["memory_id"], f.memory);
        assert_eq!(data["learner_memories"][0]["revision"], 2);
        assert_eq!(data["memory_changes"].as_array().unwrap().len(), 2);
        assert_eq!(data["learning_events"][0]["origin"], "model_reported");
        let saved: Value = serde_json::from_str(
            data["context_receipts"][0]["response_json"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(saved, packet);
        assert_eq!(data["memory_changes"][0]["revision"], 1);
        assert!(!data.as_object().unwrap().contains_key("capability_states"));
        assert!(
            fs::read_to_string(destination.join("README.md"))
                .unwrap()
                .contains("substitution strength")
        );
    }
    #[test]
    fn zip_and_existing_destination_are_safe() {
        let f = Fixture::new();
        let destination = f.root.join("export.zip");
        let options = ExportOptions {
            destination: destination.clone(),
            format: ExportFormat::Zip,
            include_source_copies: false,
            overwrite: false,
        };
        export_learner_data(f.workspace.root(), &options).unwrap();
        let bytes = fs::read(&destination).unwrap();
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        assert!(export_learner_data(f.workspace.root(), &options).is_err());
        assert_eq!(fs::read(&destination).unwrap(), bytes);
    }
    impl Fixture {
        fn add_memory(&self, title: &str, sources: Value) -> String {
            let key = crate::learner::store::new_id("evidence").unwrap();
            let event = self
                .store
                .capture(
                    &self.host,
                    &[RawEvent {
                        external_event_key: key.clone(),
                        speaker: crate::learner::Speaker::Learner,
                        kind: "message".into(),
                        text: format!("My explanation about {title}"),
                        metadata: Some(crate::learner::EventMetadata::default()),
                        occurred_at_ms: None,
                    }],
                    "model_reported",
                    false,
                )
                .unwrap()
                .remove(0);
            let result=self.store.update(&self.host,json!({"schema":"learner-model/v2","action":"update","request_id":key,"turn_ref":event,"observations":[{"client_ref":"o","kind":"learner_question","description":title,"events":[{"event_id":event,"role":"learner_response","excerpts":[]}],"source_refs":sources}],"memory_edits":[{"operation":"create","client_ref":"m","record":{"title":title,"scope":"algebra","summary":title,"state_text":title,"current_basis":[{"new_observation":"o"}]},"reason":"New learner explanation","change_basis":[{"new_observation":"o"}]}],"event_dispositions":[{"event_id":event,"outcome":"processed"}]})).unwrap();
            result.references["m"].clone()
        }
        fn export(&self, name: &str, copies: bool, overwrite: bool) -> Result<Value, String> {
            export_learner_data(
                self.workspace.root(),
                &ExportOptions {
                    destination: self.root.join(name),
                    format: ExportFormat::Dir,
                    include_source_copies: copies,
                    overwrite,
                },
            )
        }
    }

    #[test]
    fn cancelled_copy_read_staging_and_publication_preserve_previous_export() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        for checkpoint in [
            "export_before_source_copy",
            "sources_current_read_chunk",
            "export_before_stage_write",
            "export_before_publication",
        ] {
            let f = Fixture::new();
            fs::write(
                f.workspace.root().join("allowed.txt"),
                "coefficient explanation",
            )
            .unwrap();
            f.add_memory(
                "Source exercise",
                json!([{"path":"allowed.txt","available_form":"original"}]),
            );
            f.export("replace", false, false).unwrap();
            fs::write(f.root.join("replace/user-note.txt"), "keep user addition").unwrap();
            let before = fs::read(f.root.join("replace/learner.json")).unwrap();
            let flag = Arc::new(AtomicBool::new(false));
            let flag_hook = flag.clone();
            let token = crate::retrieval::OperationCancellation::with_flag(flag.clone())
                .with_test_hook(Arc::new(move |observed| {
                    if observed == checkpoint {
                        flag_hook.store(true, Ordering::Relaxed);
                    }
                }));
            let result = {
                let _scope = crate::request::enter_request_cancellation(&token);
                f.export("replace", true, true)
            };
            assert!(
                flag.load(Ordering::Relaxed),
                "checkpoint was reached: {checkpoint}"
            );
            assert!(
                result.unwrap_err().contains("source_operation_cancelled"),
                "{checkpoint}"
            );
            assert_eq!(
                fs::read(f.root.join("replace/learner.json")).unwrap(),
                before
            );
            assert_eq!(
                fs::read_to_string(f.root.join("replace/user-note.txt")).unwrap(),
                "keep user addition"
            );
            assert!(!f.root.join("replace/sources/allowed.txt").exists());
            assert!(!fs::read_dir(&f.root).unwrap().any(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".magnesium-export-stage")
            }));
        }
    }

    #[test]
    fn optional_pdf_copy_propagates_cache_failure_and_halts_later_optional_work() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let f = Fixture::new();
        let pdf = include_bytes!("../tests/fixtures/interaction/statistics/source-pages.pdf");
        fs::write(f.workspace.root().join("source.pdf"), pdf).unwrap();
        fs::write(
            f.workspace.root().join("later.txt"),
            "later optional source",
        )
        .unwrap();
        f.add_memory(
            "Source exercise",
            json!([
                {"path":"missing.txt","available_form":"original"},
                {"path":"source.pdf","available_form":"original"},
                {"path":"later.txt","available_form":"original"}
            ]),
        );
        let index = f
            .workspace
            .root()
            .join(crate::workspace::CONTROL_DIR)
            .join("index/source-search.sqlite");
        fs::create_dir_all(index.parent().unwrap()).unwrap();
        Connection::open(&index).unwrap().execute_batch("CREATE TABLE foreign_data(value TEXT); INSERT INTO foreign_data VALUES ('preserve');").unwrap();
        let cache = fs::read(&index).unwrap();
        f.export("replace", false, false).unwrap();
        let previous = fs::read(f.root.join("replace/learner.json")).unwrap();
        let observed = Arc::new(AtomicUsize::new(0));
        let observed_hook = observed.clone();
        let token = crate::retrieval::OperationCancellation::none().with_test_hook(Arc::new(
            move |checkpoint| {
                if checkpoint == "export_before_source_copy" {
                    observed_hook.fetch_add(1, Ordering::Relaxed);
                }
            },
        ));
        let result = {
            let _scope = crate::request::enter_request_cancellation(&token);
            f.export("replace", true, true)
        };
        assert!(result.unwrap_err().contains("source-search.sqlite"));
        assert_eq!(observed.load(Ordering::Relaxed), 2);
        assert_eq!(fs::read(&index).unwrap(), cache);
        assert_eq!(
            fs::read(f.root.join("replace/learner.json")).unwrap(),
            previous
        );
        assert!(!f.root.join("replace/sources/later.txt").exists());
    }

    #[test]
    fn actual_writer_source_refs_obey_exclusions_versions_and_copy_opt_in() {
        let f = Fixture::new();
        let root = f.workspace.root();
        fs::write(root.join("allowed.txt"), "coefficient explanation").unwrap();
        fs::write(root.join("ignored.txt"), "private lesson").unwrap();
        fs::write(
            root.join(crate::workspace::CONTROL_DIR)
                .join("magnesiumignore"),
            "ignored.txt\n",
        )
        .unwrap();
        fs::write(root.join(".env"), "TOKEN=private-value").unwrap();
        fs::write(
            root.join("secret.txt"),
            "-----BEGIN PRIVATE KEY-----\nprivate\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("nested").join(crate::workspace::CONTROL_DIR)).unwrap();
        fs::write(
            root.join("nested")
                .join(crate::workspace::CONTROL_DIR)
                .join("config.toml"),
            "schema_version=1\nworkspace_id='nested'",
        )
        .unwrap();
        fs::write(
            root.join("nested/private.txt"),
            "nested workspace private data",
        )
        .unwrap();
        let mut refs = vec![];
        for path in [
            "allowed.txt",
            "ignored.txt",
            ".env",
            "secret.txt",
            "nested/private.txt",
            "missing.txt",
        ] {
            refs.push(json!({"path":path,"available_form":"original"}));
        }
        refs.push(json!({"path":"allowed.txt","hash":"0".repeat(64),"available_form":"original"}));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("allowed.txt"), root.join("alias.txt")).unwrap();
            refs.push(json!({"path":"alias.txt","available_form":"original"}));
        }
        f.add_memory("Source exercise", json!(refs));
        f.export("no-copies", false, false).unwrap();
        assert!(!f.root.join("no-copies/sources").exists());
        f.export("with-copies", true, false).unwrap();
        assert_eq!(
            fs::read_to_string(f.root.join("with-copies/sources/allowed.txt")).unwrap(),
            "coefficient explanation"
        );
        for path in [
            "ignored.txt",
            ".env",
            "secret.txt",
            "nested/private.txt",
            "missing.txt",
            "alias.txt",
        ] {
            assert!(
                !f.root.join("with-copies/sources").join(path).exists(),
                "copied excluded {path}"
            );
        }
        let manifest: Value =
            serde_json::from_slice(&fs::read(f.root.join("with-copies/manifest.json")).unwrap())
                .unwrap();
        assert!(
            manifest["source_coverage"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["path"] == "allowed.txt" && entry["reason"] == "source_changed")
        );
    }
    #[test]
    fn overwrite_preserves_old_artifact_and_user_additions_and_rejects_workspace_roots() {
        let f = Fixture::new();
        f.export("replace", false, false).unwrap();
        fs::write(f.root.join("replace/my-note.txt"), "keep my addition").unwrap();
        let updated = f.export("replace", false, true).unwrap();
        let backup = PathBuf::from(updated["previous_export_backup"].as_str().unwrap());
        assert_eq!(
            fs::read_to_string(backup.join("my-note.txt")).unwrap(),
            "keep my addition"
        );
        assert!(!f.root.join("replace/my-note.txt").exists());
        assert!(
            f.export("workspace", false, true)
                .unwrap_err()
                .contains("workspace root")
        );
        fs::create_dir(f.root.join("unrelated")).unwrap();
        assert!(
            f.export("unrelated", false, true)
                .unwrap_err()
                .contains("Magnesium export")
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(f.root.join("replace"), f.root.join("alias")).unwrap();
            assert!(
                f.export("alias", false, true)
                    .unwrap_err()
                    .contains("symlink")
            );
        }
    }
}
