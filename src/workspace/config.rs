use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, value};

use super::{reject_symlink_components, validate_workspace_id, write_private_atomic};

pub(crate) const CONFIG_SCHEMA_VERSION: i64 = 1;
const MAX_CONFIG_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct WorkspaceConfig {
    path: PathBuf,
    document: DocumentMut,
    workspace_id: String,
}

impl WorkspaceConfig {
    pub(crate) fn new(path: PathBuf, workspace_id: &str) -> Result<Self, ConfigError> {
        validate_workspace_id(workspace_id)
            .map_err(|source| ConfigError::invalid(&path, source.to_string()))?;
        let mut document = DocumentMut::new();
        document["schema_version"] = value(CONFIG_SCHEMA_VERSION);
        document["workspace_id"] = value(workspace_id);
        Ok(Self {
            path,
            document,
            workspace_id: workspace_id.to_owned(),
        })
    }

    pub(crate) fn load(path: &Path) -> Result<Self, ConfigError> {
        let boundary = path.parent().unwrap_or(path);
        reject_symlink_components(path, boundary).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let text = read_bounded(path)?;
        let document = text.parse::<DocumentMut>().map_err(|source| {
            let location = source
                .span()
                .map(|span| format!(" at byte {}..{}", span.start, span.end))
                .unwrap_or_default();
            ConfigError::invalid(
                path,
                format!("invalid TOML{location}: {}", source.message()),
            )
        })?;

        let schema_version =
            optional_integer(&document, path, "schema_version")?.unwrap_or(CONFIG_SCHEMA_VERSION);
        if schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::invalid(
                path,
                format!(
                    "unsupported schema_version {schema_version}; this runtime supports {CONFIG_SCHEMA_VERSION}"
                ),
            ));
        }

        let workspace_id = required_string(&document, path, "workspace_id")?.to_owned();
        validate_workspace_id(&workspace_id)
            .map_err(|source| ConfigError::invalid(path, source.to_string()))?;
        Ok(Self {
            path: path.to_path_buf(),
            document,
            workspace_id,
        })
    }

    pub(crate) fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    pub(crate) fn save(&mut self) -> Result<(), ConfigError> {
        let boundary = self.path.parent().unwrap_or(&self.path);
        reject_symlink_components(&self.path, boundary).map_err(|source| ConfigError::Io {
            path: self.path.clone(),
            source,
        })?;
        self.document["schema_version"] = value(CONFIG_SCHEMA_VERSION);
        self.document["workspace_id"] = value(&self.workspace_id);
        write_private_atomic(&self.path, self.document.to_string().as_bytes()).map_err(|source| {
            ConfigError::Io {
                path: self.path.clone(),
                source,
            }
        })
    }
}

#[derive(Debug)]
pub(crate) enum ConfigError {
    Io { path: PathBuf, source: io::Error },
    Invalid { path: PathBuf, message: String },
}

impl ConfigError {
    fn invalid(path: &Path, message: impl Into<String>) -> Self {
        Self::Invalid {
            path: path.to_path_buf(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    formatter,
                    "cannot read or write {}: {source}",
                    path.display()
                )
            }
            Self::Invalid { path, message } => {
                write!(
                    formatter,
                    "invalid Magnesium config {}: {message}",
                    path.display()
                )
            }
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Invalid { .. } => None,
        }
    }
}

fn read_bounded(path: &Path) -> Result<String, ConfigError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.file_type().is_file() {
        return Err(ConfigError::invalid(path, "file must be a regular file"));
    }
    let file = fs::File::open(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::with_capacity(4096);
    file.take((MAX_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(ConfigError::invalid(
            path,
            format!("file exceeds the {MAX_CONFIG_BYTES}-byte limit"),
        ));
    }
    String::from_utf8(bytes).map_err(|_| ConfigError::invalid(path, "file must contain UTF-8 TOML"))
}

fn required_string<'a>(
    document: &'a DocumentMut,
    path: &Path,
    key: &str,
) -> Result<&'a str, ConfigError> {
    optional_string(document, path, key)?
        .ok_or_else(|| ConfigError::invalid(path, format!("required string `{key}` is missing")))
}

fn optional_string<'a>(
    document: &'a DocumentMut,
    path: &Path,
    key: &str,
) -> Result<Option<&'a str>, ConfigError> {
    match document.get(key) {
        None => Ok(None),
        Some(item) => item
            .as_str()
            .map(Some)
            .ok_or_else(|| ConfigError::invalid(path, format!("`{key}` must be a string"))),
    }
}

fn optional_integer(
    document: &DocumentMut,
    path: &Path,
    key: &str,
) -> Result<Option<i64>, ConfigError> {
    match document.get(key) {
        None => Ok(None),
        Some(item) => item
            .as_integer()
            .map(Some)
            .ok_or_else(|| ConfigError::invalid(path, format!("`{key}` must be an integer"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_config_holds_only_its_schema_and_local_identity() {
        let root = std::env::temp_dir().join(format!(
            "magnesium-config-{}",
            crate::workspace::os_random_hex(16).unwrap()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("config.toml");
        let mut config = WorkspaceConfig::new(path.clone(), "workspace-a").unwrap();
        config.save().unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "schema_version = 1\nworkspace_id = \"workspace-a\"\n"
        );
        assert_eq!(
            WorkspaceConfig::load(&path).unwrap().workspace_id(),
            "workspace-a"
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// The papers skill records optional-tool answers in a `[tools]` table.
    /// `magnesium init` only loads the config; a load and save must still keep it.
    #[test]
    fn a_tools_table_survives_load_and_save() {
        let root = std::env::temp_dir().join(format!(
            "magnesium-config-{}",
            crate::workspace::os_random_hex(16).unwrap()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("config.toml");
        fs::write(
            &path,
            "schema_version = 1\nworkspace_id = \"workspace-a\"\n\n[tools]\ntectonic = \"declined\"\n",
        )
        .unwrap();
        let mut config = WorkspaceConfig::load(&path).unwrap();
        assert_eq!(config.workspace_id(), "workspace-a");
        config.save().unwrap();
        let saved: DocumentMut = fs::read_to_string(&path).unwrap().parse().unwrap();
        assert_eq!(saved["tools"]["tectonic"].as_str(), Some("declined"));
        fs::remove_dir_all(root).unwrap();
    }
}
