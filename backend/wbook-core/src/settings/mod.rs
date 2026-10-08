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
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::export::{
    self, CoverSettings, ExportFailure, ExportOptions, OutputFormat, RenderLayout, RenderOptions,
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
    /// The custom image itself is stored with the book, not here.
    pub cover: CoverSettings,
    /// Used from the global settings only; a session's copy is ignored.
    pub cover_search: CoverSearchSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CoverSearchSettings {
    /// The initial search text; `{title}` and `{author}` are replaced.
    pub query: String,
    pub engines: Vec<SearchEngine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SearchEngine {
    pub name: String,
    /// An http(s) address in which `{query}` is replaced by the search text.
    pub url: String,
}

impl Default for CoverSearchSettings {
    fn default() -> Self {
        let engine = |name: &str, url: &str| SearchEngine {
            name: name.into(),
            url: url.into(),
        };
        Self {
            query: "{title} {author} 封面".into(),
            engines: vec![
                engine("必应", "https://www.bing.com/images/search?q={query}"),
                engine(
                    "百度",
                    "https://image.baidu.com/search/index?tn=baiduimage&word={query}",
                ),
                engine("Google", "https://www.google.com/search?tbm=isch&q={query}"),
                engine(
                    "豆瓣读书",
                    "https://search.douban.com/book/subject_search?search_text={query}",
                ),
            ],
        }
    }
}

impl CoverSearchSettings {
    fn validate(&self) -> Result<(), SettingsError> {
        let invalid = |message: String| Err(SettingsError::CoverSearch { message });
        if self.query.trim().is_empty() {
            return invalid("the search text is empty".into());
        }
        for engine in &self.engines {
            if engine.name.trim().is_empty() {
                return invalid(format!("the search source {} has no name", engine.url));
            }
            let web = engine.url.starts_with("https://") || engine.url.starts_with("http://");
            if !web || !engine.url.contains("{query}") {
                return invalid(format!(
                    "the address of {} must start with http(s):// and contain {{query}}",
                    engine.name
                ));
            }
        }
        Ok(())
    }
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
    #[snafu(display("invalid cover search settings: {message}"))]
    CoverSearch { message: String },
}

impl Settings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        self.toc.to_config()?.build()?;
        export::check_language(&self.render.language).context(RenderSnafu)?;
        self.render.templates.validate().context(RenderSnafu)?;
        self.cover_search.validate()
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
            cover: self.cover,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StoredSettings {
    pub settings: Settings,
    /// Counts saves since startup so concurrent editors cannot overwrite
    /// each other. Not persisted: one process owns the file.
    pub revision: u64,
    /// Why the settings file could not be used. The defaults stand in until
    /// the next save replaces the file.
    pub problem: Option<String>,
}

#[derive(Debug, snafu::Snafu)]
pub enum SaveError {
    #[snafu(context(false), display("{source}"))]
    Invalid { source: SettingsError },
    #[snafu(display("settings changed since revision {expected}; now at {current}"))]
    Stale { expected: u64, current: u64 },
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
    // Serializes saves, so the file always matches the last published value.
    saving: Mutex<()>,
    state: watch::Sender<StoredSettings>,
    // The store outlives every session, so subscribers need their own signal
    // to stop at shutdown instead of holding the HTTP server open.
    closing: CancellationToken,
}

impl SettingsStore {
    /// Never fails: a broken file must not keep the app from starting, and
    /// it is left untouched until the user saves.
    pub fn load(config_dir: &Utf8Path) -> Self {
        let path = config_dir.join(FILE_NAME);
        let state = match read(&path) {
            Ok(settings) => StoredSettings {
                settings,
                revision: 0,
                problem: None,
            },
            Err(error) => {
                tracing::warn!("{error}; using default settings");
                StoredSettings {
                    settings: Settings::default(),
                    revision: 0,
                    problem: Some(error.to_string()),
                }
            }
        };
        Self {
            path,
            saving: Mutex::new(()),
            state: watch::Sender::new(state),
            closing: CancellationToken::new(),
        }
    }

    pub fn get(&self) -> StoredSettings {
        self.state.borrow().clone()
    }

    pub fn current(&self) -> Settings {
        self.state.borrow().settings.clone()
    }

    /// Receives the current settings and every later save.
    pub fn subscribe(&self) -> watch::Receiver<StoredSettings> {
        self.state.subscribe()
    }

    /// Cancelled once the app shuts down.
    pub fn closing(&self) -> CancellationToken {
        self.closing.clone()
    }

    pub fn close(&self) {
        self.closing.cancel();
    }

    /// Saves `settings` unless another save landed after `expected`.
    pub fn save(&self, expected: u64, settings: Settings) -> Result<StoredSettings, SaveError> {
        settings.validate()?;
        let text = toml::to_string_pretty(&settings).context(EncodeSnafu)?;
        let _saving = self.saving.lock().unwrap();
        let current = self.state.borrow().revision;
        if current != expected {
            return StaleSnafu { expected, current }.fail();
        }
        write(&self.path, &text).context(WriteSnafu {
            path: self.path.clone(),
        })?;
        let stored = StoredSettings {
            settings,
            revision: current + 1,
            problem: None,
        };
        self.state.send_replace(stored.clone());
        Ok(stored)
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
