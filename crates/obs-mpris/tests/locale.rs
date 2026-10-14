//! Every text the plugin asks OBS for exists in every locale it ships
//! (B-46), and nothing else does, so a renamed key cannot linger.
//!
//! Only the locale files are read here: the plugin library itself cannot be
//! loaded outside OBS.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use mpris_core::settings::{DefaultValue, FIELDS, Group, Kind};

const LOCALES: [&str; 2] = ["en-US", "fr-FR"];

/// Keys the OBS glue asks for besides the schema ones.
const GLUE_KEYS: [&str; 3] = ["SourceName", "PlayerNone", "PlayerUnavailable"];

/// Every key the plugin looks up: each field labels its property with its
/// own key, a translated default adds its locale key, each choice of a
/// list labels itself with its own, each group titles itself with its key,
/// and the glue adds its own.
fn expected_keys() -> Vec<String> {
    let mut keys: Vec<String> = FIELDS
        .iter()
        .flat_map(|field| {
            let translated = match field.default {
                DefaultValue::Translated { locale_key, .. } => Some(locale_key),
                _ => None,
            };
            let choices = match field.kind {
                Kind::Choice(choices) => choices.iter().map(|choice| choice.locale_key).collect(),
                _ => Vec::new(),
            };
            std::iter::once(field.key).chain(translated).chain(choices)
        })
        .chain(Group::ALL.map(Group::key))
        .chain(GLUE_KEYS)
        .map(str::to_owned)
        .collect();
    keys.sort();
    keys
}

fn locale_path(locale: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("data/locale")
        .join(format!("{locale}.ini"))
}

/// Entries of an OBS locale file. Every line that is neither blank nor a
/// comment must read `Key="Value"`, without spaces around `=`, the only
/// form the OBS parser reads reliably.
fn entries(locale: &str) -> BTreeMap<String, String> {
    let path = locale_path(locale);
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut entries = BTreeMap::new();
    for line in text.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once("=\"")
            .and_then(|(key, rest)| Some((key, rest.strip_suffix('"')?)))
            .unwrap_or_else(|| panic!("{locale}: malformed line {line:?}"));
        assert!(
            !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "{locale}: bad key in {line:?}"
        );
        assert!(
            !value.trim().is_empty(),
            "{locale}: blank value in {line:?}"
        );
        let previous = entries.insert(key.to_owned(), value.to_owned());
        assert!(previous.is_none(), "{locale}: {key} given twice");
    }
    entries
}

#[test]
fn every_locale_has_exactly_the_keys_the_plugin_uses() {
    for locale in LOCALES {
        let keys: Vec<String> = entries(locale).into_keys().collect();
        assert_eq!(keys, expected_keys(), "{locale}");
    }
}

#[test]
fn english_defaults_match_the_schema() {
    let english = entries("en-US");
    for field in FIELDS {
        if let DefaultValue::Translated {
            locale_key,
            english: text,
        } = field.default
        {
            assert_eq!(
                english.get(locale_key).map(String::as_str),
                Some(text),
                "{locale_key}"
            );
        }
    }
}

#[test]
fn unavailable_suffix_is_set_apart_from_the_label() {
    for locale in LOCALES {
        let suffix = &entries(locale)["PlayerUnavailable"];
        assert!(suffix.starts_with(' '), "{locale}: {suffix:?}");
    }
}
