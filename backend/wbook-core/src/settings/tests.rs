use std::fs;

use camino::Utf8PathBuf;

use super::{Settings, SettingsStore};
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
    SettingsStore::load(&path).save(custom()).unwrap();
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

        store.save(custom()).unwrap();
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
    assert!(store.save(invalid).is_err());
    assert!(!path.join("settings.toml").exists());
    assert_eq!(store.current(), Settings::default());
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
