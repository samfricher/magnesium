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
    "attempts",
    "practice_items",
    "practice_events",
    "gap_logs",
    "attempt_corrections",
    "help_levels",
    "imported_readings",
    "anchors",
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
/// The sittings kept beside the learner record, when this study folder has
/// any. A store from before #30 also kept its quick checks there.
fn sittings_data(w: &Workspace) -> Result<Option<Value>, String> {
    let path = w.control().join(crate::sittings::STORE_FILE);
    if !path.try_exists().map_err(|e| e.to_string())? {
        return Ok(None);
    }
    reject_symlink_components(&path, w.control()).map_err(|e| e.to_string())?;
    let conn = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    conn.busy_timeout(Duration::from_secs(2))
        .map_err(|e| e.to_string())?;
    let snapshot = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let mut data = serde_json::Map::new();
    // Table identifiers come from this fixed list, as for the learner tables.
    for table in ["items", "attempts", "sittings", "sitting_items"] {
        let exists: bool = snapshot
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists {
            data.insert(table.into(), rows(&snapshot, table)?);
        }
    }
    Ok(Some(Value::Object(data)))
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
    crate::workspace::sync_to_disk(&file).map_err(|error| error.to_string())
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
    readable.push_str(&format!(
        "## Answers\n\n{} answers marked. Each is a row of `attempts` in learner.json, and `magnesium attempts` lists them.\n\n",
        data["attempts"].as_array().map_or(0, Vec::len)
    ));
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
    if let Some(sittings) = sittings_data(&w)? {
        files.push((
            "sittings.json".to_owned(),
            serde_json::to_vec_pretty(&sittings).map_err(|e| e.to_string())?,
        ));
    }
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
                crate::workspace::sync_to_disk(&file).map_err(|e| e.to_string())?;
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
mod tests;
