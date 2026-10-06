//! Settings shared by the global settings file and every session.
//!
//! A session copies the global settings when it is created and edits its own
//! copy afterwards, so changing the global settings never alters a book that
//! is already being worked on.

use std::fs;
use std::io::{self, Write};
use std::sync::Mutex;

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use specta::Type;

use crate::export::{
    self, ExportFailure, ExportOptions, OutputFormat, RenderLayout, RenderOptions,
    TemplateOverrides,
};
use crate::parser::toc::{TocConfigError, TocSettings};
use crate::workspace::FilterConfig;

#[cfg(test)]
mod tests;

const FILE_NAME: &str = "settings.toml";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Settings {
    pub toc: TocSettings,
    /// Filters run once, when the session is initialized.
    pub filters: Vec<FilterConfig>,
    pub render: RenderSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RenderSettings {
    pub layout: RenderLayout,
    pub language: String,
    pub templates: TemplateOverrides,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            layout: RenderLayout::SingleHtml,
            language: "zh-CN".into(),
            templates: TemplateOverrides::default(),
        }
    }
}

#[derive(Debug, snafu::Snafu)]
pub enum SettingsError {
    #[snafu(context(false), display("{source}"))]
    Toc { source: TocConfigError },
    #[snafu(display("invalid render settings: {source}"))]
    Render { source: ExportFailure },
}

impl Settings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        self.toc.to_config()?.build()?;
        export::check_language(&self.render.language).context(RenderSnafu)?;
        self.render.templates.validate().context(RenderSnafu)
    }

    pub fn export_options(&self) -> ExportOptions {
        ExportOptions {
            render: RenderOptions {
                layout: self.render.layout,
                templates: self.render.templates.clone(),
            },
            format: OutputFormat::Epub,
            language: self.render.language.clone(),
            identifier: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StoredSettings {
    pub settings: Settings,
    /// Why the settings file could not be used. The defaults stand in until
    /// the next save replaces the file.
    pub problem: Option<String>,
}

#[derive(Debug, snafu::Snafu)]
pub enum SaveError {
    #[snafu(context(false), display("{source}"))]
    Invalid { source: SettingsError },
    #[snafu(display("cannot encode settings: {source}"))]
    Encode { source: toml::ser::Error },
    #[snafu(display("cannot write {path}: {source}"))]
    Write {
        path: Utf8PathBuf,
        source: io::Error,
    },
}

#[derive(Debug, snafu::Snafu)]
enum LoadError {
    #[snafu(display("cannot read {path}: {source}"))]
    Read {
        path: Utf8PathBuf,
        source: io::Error,
    },
    #[snafu(display("{path} is not valid TOML: {source}"))]
    Parse {
        path: Utf8PathBuf,
        source: toml::de::Error,
    },
    #[snafu(display("{path} has invalid settings: {message}"))]
    Invalid { path: Utf8PathBuf, message: String },
}

pub struct SettingsStore {
    path: Utf8PathBuf,
    // Also serializes saves, so the file always matches the last stored value.
    state: Mutex<StoredSettings>,
}

impl SettingsStore {
    /// Never fails: a broken file must not keep the app from starting, and
    /// it is left untouched until the user saves.
    pub fn load(config_dir: &Utf8Path) -> Self {
        let path = config_dir.join(FILE_NAME);
        let state = match read(&path) {
            Ok(settings) => StoredSettings {
                settings,
                problem: None,
            },
            Err(error) => {
                tracing::warn!("{error}; using default settings");
                StoredSettings {
                    settings: Settings::default(),
                    problem: Some(error.to_string()),
                }
            }
        };
        Self {
            path,
            state: Mutex::new(state),
        }
    }

    pub fn get(&self) -> StoredSettings {
        self.state.lock().unwrap().clone()
    }

    pub fn current(&self) -> Settings {
        self.state.lock().unwrap().settings.clone()
    }

    pub fn save(&self, settings: Settings) -> Result<(), SaveError> {
        settings.validate()?;
        let text = toml::to_string_pretty(&settings).context(EncodeSnafu)?;
        let mut state = self.state.lock().unwrap();
        write(&self.path, &text).context(WriteSnafu {
            path: self.path.clone(),
        })?;
        *state = StoredSettings {
            settings,
            problem: None,
        };
        Ok(())
    }
}

fn read(path: &Utf8Path) -> Result<Settings, LoadError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(source) => {
            return Err(LoadError::Read {
                path: path.to_owned(),
                source,
            })
        }
    };
    let file: toml::Table = text.parse().context(ParseSnafu { path })?;
    // Fields added in later versions are missing from older files; fill them
    // from the defaults instead of rejecting the whole file.
    let mut merged = toml::Table::try_from(Settings::default()).expect("defaults encode as TOML");
    merge(&mut merged, file);
    let invalid = |message: String| LoadError::Invalid {
        path: path.to_owned(),
        message,
    };
    let settings: Settings = toml::Value::Table(merged)
        .try_into()
        .map_err(|error: toml::de::Error| invalid(error.to_string()))?;
    settings
        .validate()
        .map_err(|error| invalid(error.to_string()))?;
    Ok(settings)
}

fn merge(base: &mut toml::Table, overlay: toml::Table) {
    for (key, value) in overlay {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(base)), toml::Value::Table(overlay)) => merge(base, overlay),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

// Write a sibling file and rename it over the target so a crash never leaves
// a half-written settings file behind.
fn write(path: &Utf8Path, text: &str) -> io::Result<()> {
    let directory = path.parent().expect("settings path has a parent");
    fs::create_dir_all(directory)?;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    file.write_all(text.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}
