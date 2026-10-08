use std::fs;

use camino::Utf8PathBuf;

use super::{ExportLocation, ExportSettings, SaveError, Settings, SettingsStore};
use crate::export::{RenderLayout, TemplateOverrides};
use crate::parser::toc::{TocMode, VolumeSplit};
use crate::workspace::FilterConfig;

fn directory() -> (tempfile::TempDir, Utf8PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(directory.path().join("config")).unwrap();
    (directory, path)
}

fn custom() -> Settings {
    let mut settings = Settings::default();
    settings.toc.mode = TocMode::Chapters;
    settings.toc.chapter_marks = vec!["话".into()];
    settings.filters = vec![FilterConfig::Ad];
    settings.render.layout = RenderLayout::SplitChapters;
    settings.render.templates.stylesheet = Some("p {\n  margin: 0;\n}\n".into());
    settings
}

#[test]
fn missing_file_loads_defaults_without_a_problem() {
    let (_directory, path) = directory();
    let stored = SettingsStore::load(&path).get();
    assert_eq!(stored.settings, Settings::default());
    assert_eq!(stored.problem, None);
}

#[test]
fn saved_settings_survive_a_reload() {
    let (_directory, path) = directory();
    SettingsStore::load(&path).save(0, custom()).unwrap();
    let stored = SettingsStore::load(&path).get();
    assert_eq!(stored.settings, custom());
    assert_eq!(stored.problem, None);
}

#[test]
fn partial_file_takes_missing_fields_from_defaults() {
    let (_directory, path) = directory();
    fs::create_dir_all(&path).unwrap();
    fs::write(
        path.join("settings.toml"),
        "[toc]\nvolume_split = \"None\"\n",
    )
    .unwrap();
    let stored = SettingsStore::load(&path).get();
    assert_eq!(stored.problem, None);
    let mut expected = Settings::default();
    expected.toc.volume_split = VolumeSplit::None;
    assert_eq!(stored.settings, expected);
}

#[test]
fn broken_files_fall_back_to_defaults_and_are_left_alone() {
    for text in [
        "[toc\n",
        "[toc]\nmode = \"Sideways\"\n",
        "[toc]\nchapter_marks = []\n",
        "[render]\nlanguage = \"not a tag!\"\n",
    ] {
        let (_directory, path) = directory();
        fs::create_dir_all(&path).unwrap();
        let file = path.join("settings.toml");
        fs::write(&file, text).unwrap();
        let store = SettingsStore::load(&path);
        let stored = store.get();
        assert_eq!(stored.settings, Settings::default(), "{text}");
        assert!(stored.problem.is_some(), "{text}");
        assert_eq!(fs::read_to_string(&file).unwrap(), text);

        store.save(0, custom()).unwrap();
        assert_eq!(store.get().problem, None);
    }
}

#[test]
fn invalid_settings_are_rejected_before_writing() {
    let (_directory, path) = directory();
    let store = SettingsStore::load(&path);
    let mut invalid = custom();
    invalid.render.templates = TemplateOverrides {
        paragraph: Some("{{ text".into()),
        ..TemplateOverrides::default()
    };
    assert!(store.save(0, invalid).is_err());
    assert!(!path.join("settings.toml").exists());
    assert_eq!(store.current(), Settings::default());
    assert_eq!(store.get().revision, 0);
}

#[test]
fn saves_from_a_stale_revision_are_rejected_without_writing() {
    let (_directory, path) = directory();
    let store = SettingsStore::load(&path);
    assert_eq!(store.save(0, custom()).unwrap().revision, 1);
    let file = path.join("settings.toml");
    let written = fs::read_to_string(&file).unwrap();

    let error = store.save(0, Settings::default()).unwrap_err();
    assert!(
        matches!(
            error,
            SaveError::Stale {
                expected: 0,
                current: 1
            }
        ),
        "{error}"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), written);
    assert_eq!(store.current(), custom());
    assert_eq!(store.save(1, Settings::default()).unwrap().revision, 2);
}

#[test]
fn subscribers_see_the_current_value_and_each_save() {
    let (_directory, path) = directory();
    let store = SettingsStore::load(&path);
    let mut receiver = store.subscribe();
    assert_eq!(receiver.borrow_and_update().revision, 0);

    store.save(0, custom()).unwrap();
    assert!(receiver.has_changed().unwrap());
    let stored = receiver.borrow_and_update().clone();
    assert_eq!((stored.revision, stored.settings), (1, custom()));

    // A rejected save publishes nothing.
    assert!(store.save(0, Settings::default()).is_err());
    assert!(!receiver.has_changed().unwrap());
}

#[test]
fn export_options_follow_render_settings() {
    let settings = custom();
    let options = settings.export_options();
    assert_eq!(options.render.layout, RenderLayout::SplitChapters);
    assert_eq!(options.render.templates, settings.render.templates);
    assert_eq!(options.language, "zh-CN");
    assert_eq!(options.identifier, None);
}

#[test]
fn cover_search_sources_need_a_web_address_with_a_query() {
    use super::SearchEngine;
    let with = |name: &str, url: &str| {
        let mut settings = Settings::default();
        settings.cover_search.engines.push(SearchEngine {
            name: name.into(),
            url: url.into(),
        });
        settings.validate()
    };
    assert!(Settings::default().validate().is_ok());
    assert!(with("自定义", "https://example.com/?q={query}").is_ok());
    assert!(with("", "https://example.com/?q={query}").is_err());
    assert!(with("无占位", "https://example.com/").is_err());
    assert!(with("本地", "file:///C:/{query}").is_err());
    let mut settings = Settings::default();
    settings.cover_search.query = " ".into();
    assert!(settings.validate().is_err());
    settings = Settings::default();
    settings.cover_search.engines.clear();
    assert!(settings.validate().is_ok());
}

#[test]
fn files_without_cover_search_get_the_default_sources() {
    let (_directory, path) = directory();
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("settings.toml"), "[render]\nlanguage = \"en\"\n").unwrap();
    let stored = SettingsStore::load(&path).get();
    assert_eq!(stored.problem, None);
    assert_eq!(stored.settings.cover_search, Default::default());
}

#[test]
fn export_directory_follows_the_chosen_location() {
    let source = Utf8PathBuf::from("books/novel.txt");
    let data = Utf8PathBuf::from("data");
    let custom = Utf8PathBuf::from_path_buf(std::env::temp_dir()).unwrap();
    let at = |location| {
        ExportSettings {
            location,
            custom_directory: format!(" {custom} "),
        }
        .directory(&source, &data)
    };
    assert_eq!(at(ExportLocation::SourceFolder), "books");
    assert_eq!(at(ExportLocation::DataFolder), data.join("exports"));
    assert_eq!(at(ExportLocation::Custom), custom);
}

#[test]
fn a_custom_export_directory_must_be_absolute() {
    let with = |location, directory: &str| {
        Settings {
            export: ExportSettings {
                location,
                custom_directory: directory.into(),
            },
            ..Default::default()
        }
        .validate()
    };
    let absolute = std::env::temp_dir();
    assert!(with(ExportLocation::Custom, absolute.to_str().unwrap()).is_ok());
    assert!(with(ExportLocation::Custom, "").is_err());
    assert!(with(ExportLocation::Custom, "books").is_err());
    // An unused custom directory is kept as typed.
    assert!(with(ExportLocation::SourceFolder, "books").is_ok());
}
