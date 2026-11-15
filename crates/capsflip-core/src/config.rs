//! The configuration file: parsed and validated once, trusted afterwards.

use std::fmt;
use std::path::Path;

use serde::Deserialize;

use crate::exclusions::Exclusions;
use crate::layout::{LangId, LayoutPrefs};

/// Everything the user can configure. The shape mirrors the TOML file.
#[derive(Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub layout: LayoutConfig,
    pub caps_lock: CapsLockConfig,
    pub exclusions: Exclusions,
}

impl Config {
    /// The file written on first start. It spells out the defaults.
    pub const TEMPLATE: &str = r#"# capsflip configuration. Reload it from the tray menu after editing.

[layout]
# The English layout that Caps Lock switches to.
english = "en-US"
# The layout to pair with English before any other one was used.
# Without it, the first non-English layout installed is taken.
# default_secondary = "uk-UA"

[caps_lock]
# How long Caps Lock must be held to toggle capital letters, in milliseconds.
# Shift+Caps Lock toggles them at once.
hold_threshold_ms = 1000

[exclusions]
# Programs in which Caps Lock is left alone, by executable name,
# for example ["mstsc.exe", "vmware.exe"].
apps = []
"#;

    /// Parses and validates the text of a config file.
    ///
    /// # Errors
    ///
    /// Returns a message that names the problem and its line when the text
    /// is not valid TOML, has an unknown key, or has a value out of range.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        toml::from_str(text).map_err(|error| ConfigError(error.to_string()))
    }

    /// Reads the settings from the config file at `path`, writing the
    /// commented template there first when the file does not exist yet.
    ///
    /// # Errors
    ///
    /// Returns a message that starts with the path when the file cannot be
    /// read or written, or is rejected by [`Self::parse`] or
    /// [`Self::resolve`].
    pub fn load(
        path: &Path,
        lang_of: impl Fn(&str) -> Option<LangId>,
    ) -> Result<Settings, ConfigError> {
        read_or_create(path)
            .map_err(|error| ConfigError(error.to_string()))
            .and_then(|text| Self::parse(&text))
            .and_then(|config| config.resolve(lang_of))
            .map_err(|ConfigError(reason)| ConfigError(format!("{}: {reason}", path.display())))
    }

    /// Turns the locale tags into languages, using the system's `lang_of`.
    ///
    /// # Errors
    ///
    /// Returns a message naming the tag when the system does not know a
    /// locale, or when `layout.english` is not an English locale.
    pub fn resolve(
        self,
        lang_of: impl Fn(&str) -> Option<LangId>,
    ) -> Result<Settings, ConfigError> {
        let resolve = |tag: &LocaleTag| {
            lang_of(tag.as_str()).ok_or_else(|| {
                ConfigError(format!(
                    "Windows does not know the locale `{}`",
                    tag.as_str()
                ))
            })
        };
        let english = resolve(&self.layout.english)?;
        if !english.is_english() {
            return Err(ConfigError(format!(
                "layout.english must be an English locale, got `{}`",
                self.layout.english.as_str()
            )));
        }
        let default_secondary = self
            .layout
            .default_secondary
            .as_ref()
            .map(resolve)
            .transpose()?;
        Ok(Settings {
            layouts: LayoutPrefs {
                english,
                default_secondary,
            },
            hold_threshold: self.caps_lock.hold_threshold,
            exclusions: self.exclusions,
        })
    }
}

fn read_or_create(path: &Path) -> std::io::Result<String> {
    if !path.exists() {
        if let Some(directory) = path.parent() {
            std::fs::create_dir_all(directory)?;
        }
        std::fs::write(path, Config::TEMPLATE)?;
    }
    std::fs::read_to_string(path)
}

/// A config whose locale tags the system has resolved to languages.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Settings {
    pub layouts: LayoutPrefs,
    pub hold_threshold: HoldThreshold,
    pub exclusions: Exclusions,
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LayoutConfig {
    pub english: LocaleTag,
    pub default_secondary: Option<LocaleTag>,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            english: LocaleTag("en-US".into()),
            default_secondary: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CapsLockConfig {
    #[serde(rename = "hold_threshold_ms")]
    pub hold_threshold: HoldThreshold,
}

/// How long a key must stay down to count as held.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(try_from = "u32")]
pub struct HoldThreshold(u32);

impl HoldThreshold {
    const RANGE: std::ops::RangeInclusive<u32> = 100..=5000;

    #[must_use]
    pub const fn as_millis(self) -> u32 {
        self.0
    }
}

impl Default for HoldThreshold {
    fn default() -> Self {
        Self(1000)
    }
}

impl TryFrom<u32> for HoldThreshold {
    type Error = String;

    fn try_from(millis: u32) -> Result<Self, Self::Error> {
        if Self::RANGE.contains(&millis) {
            Ok(Self(millis))
        } else {
            Err(format!(
                "hold_threshold_ms must be between {} and {}, got {millis}",
                Self::RANGE.start(),
                Self::RANGE.end()
            ))
        }
    }
}

/// A locale name such as `en-US`. The shell resolves it to a language.
#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
#[serde(try_from = "String")]
pub struct LocaleTag(String);

impl LocaleTag {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for LocaleTag {
    type Error = String;

    fn try_from(tag: String) -> Result<Self, Self::Error> {
        let well_formed =
            !tag.is_empty() && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        if well_formed {
            Ok(Self(tag))
        } else {
            Err(format!("`{tag}` is not a locale tag such as en-US"))
        }
    }
}

/// Why a config file was rejected, ready to show to the user.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConfigError(String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn error_of(text: &str) -> String {
        Config::parse(text)
            .expect_err("config should be rejected")
            .to_string()
    }

    fn known(tag: &str) -> Option<LangId> {
        match tag {
            "en-US" => Some(LangId::new(0x0409)),
            "en-GB" => Some(LangId::new(0x0809)),
            "uk-UA" => Some(LangId::new(0x0422)),
            _ => None,
        }
    }

    /// A fresh, empty directory for one test.
    fn scratch(test: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("capsflip-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn loading_a_missing_file_writes_the_template_and_gives_the_defaults() {
        let path = scratch("missing").join("capsflip").join("config.toml");
        let settings = Config::load(&path, known).unwrap();
        assert_eq!(settings, Config::default().resolve(known).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), Config::TEMPLATE);
    }

    #[test]
    fn loading_an_existing_file_reads_it_and_leaves_it_alone() {
        let path = scratch("existing").join("config.toml");
        let text = "[layout]
default_secondary = \"uk-UA\"
";
        std::fs::write(&path, text).unwrap();
        let settings = Config::load(&path, known).unwrap();
        assert_eq!(
            settings.layouts.default_secondary,
            Some(LangId::new(0x0422))
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn every_load_error_names_the_file() {
        let dir = scratch("errors");
        let name = |file: &str| dir.join(file).display().to_string();

        std::fs::write(
            dir.join("syntax.toml"),
            "[layout
",
        )
        .unwrap();
        let error = Config::load(&dir.join("syntax.toml"), known)
            .unwrap_err()
            .to_string();
        assert!(error.starts_with(&name("syntax.toml")), "{error}");
        assert!(error.contains("line 1"), "{error}");

        std::fs::write(
            dir.join("locale.toml"),
            "[layout]
english = \"xx-XX\"
",
        )
        .unwrap();
        let error = Config::load(&dir.join("locale.toml"), known)
            .unwrap_err()
            .to_string();
        assert!(error.starts_with(&name("locale.toml")), "{error}");
        assert!(error.contains("xx-XX"), "{error}");

        let error = Config::load(&dir, known).unwrap_err().to_string();
        assert!(error.starts_with(&dir.display().to_string()), "{error}");
    }

    fn resolve_error_of(text: &str) -> String {
        Config::parse(text)
            .unwrap()
            .resolve(known)
            .expect_err("settings should be rejected")
            .to_string()
    }

    #[test]
    fn resolving_turns_locale_tags_into_languages() {
        let settings = Config::parse(
            "[layout]\nenglish = \"en-GB\"\ndefault_secondary = \"uk-UA\"\n\
             [caps_lock]\nhold_threshold_ms = 250\n[exclusions]\napps = [\"a.exe\"]\n",
        )
        .unwrap()
        .resolve(known)
        .unwrap();
        assert_eq!(settings.layouts.english, LangId::new(0x0809));
        assert_eq!(
            settings.layouts.default_secondary,
            Some(LangId::new(0x0422))
        );
        assert_eq!(settings.hold_threshold.as_millis(), 250);
        assert!(settings.exclusions.contains("a.exe"));
    }

    #[test]
    fn resolving_the_defaults_needs_only_the_english_locale() {
        let settings = Config::default().resolve(known).unwrap();
        assert_eq!(settings.layouts.english, LangId::new(0x0409));
        assert_eq!(settings.layouts.default_secondary, None);
    }

    #[test]
    fn a_locale_the_system_does_not_know_is_rejected_by_name() {
        assert!(resolve_error_of("[layout]\nenglish = \"xx-XX\"\n").contains("xx-XX"));
        assert!(resolve_error_of("[layout]\ndefault_secondary = \"yy-YY\"\n").contains("yy-YY"));
    }

    #[test]
    fn the_english_layout_must_be_english() {
        let error = resolve_error_of("[layout]\nenglish = \"uk-UA\"\n");
        assert!(error.contains("must be an English locale"), "{error}");
    }

    #[test]
    fn an_empty_file_gives_the_defaults() {
        let config = Config::parse("").unwrap();
        assert_eq!(config.layout.english.as_str(), "en-US");
        assert_eq!(config.layout.default_secondary, None);
        assert_eq!(config.caps_lock.hold_threshold.as_millis(), 1000);
        assert!(!config.exclusions.contains("mstsc.exe"));
        assert_eq!(config, Config::default());
    }

    #[test]
    fn every_documented_key_is_read() {
        let config = Config::parse(
            r#"
            [layout]
            english = "en-GB"
            default_secondary = "uk-UA"

            [caps_lock]
            hold_threshold_ms = 250

            [exclusions]
            apps = ["mstsc.exe", "vmware.exe"]
            "#,
        )
        .unwrap();
        assert_eq!(config.layout.english.as_str(), "en-GB");
        assert_eq!(
            config
                .layout
                .default_secondary
                .as_ref()
                .map(LocaleTag::as_str),
            Some("uk-UA")
        );
        assert_eq!(config.caps_lock.hold_threshold.as_millis(), 250);
        assert!(config.exclusions.contains("vmware.exe"));
    }

    #[test]
    fn the_template_written_on_first_start_matches_the_defaults() {
        assert_eq!(Config::parse(Config::TEMPLATE).unwrap(), Config::default());
    }

    #[test]
    fn an_unknown_key_is_rejected_with_its_name_and_line() {
        let error = error_of("[layout]\nenglsh = \"en-US\"\n");
        assert!(error.contains("englsh"), "{error}");
        assert!(error.contains("line 2"), "{error}");
    }

    #[test]
    fn an_unknown_section_is_rejected() {
        assert!(error_of("[capslock]\n").contains("capslock"));
    }

    #[test]
    fn a_syntax_error_is_reported_with_its_line() {
        assert!(error_of("[layout]\nenglish = \n").contains("line 2"));
    }

    #[test]
    fn the_hold_threshold_must_be_usable() {
        let with = |ms: u32| Config::parse(&format!("[caps_lock]\nhold_threshold_ms = {ms}\n"));
        assert!(with(100).is_ok());
        assert!(with(5000).is_ok());
        for bad in [0, 99, 5001] {
            let error = with(bad).expect_err("out of range").to_string();
            assert!(error.contains("between 100 and 5000"), "{error}");
            assert!(error.contains("line 2"), "{error}");
        }
    }

    #[test]
    fn a_locale_tag_must_look_like_one() {
        for bad in ["", "en US", "uk_UA", "українська"] {
            let error = error_of(&format!("[layout]\nenglish = \"{bad}\"\n"));
            assert!(error.contains("locale tag"), "{bad:?}: {error}");
        }
        assert!(Config::parse("[layout]\ndefault_secondary = \"sr-Latn-RS\"\n").is_ok());
    }
}
