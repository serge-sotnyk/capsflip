//! The configuration file: parsed and validated once, trusted afterwards.

use std::fmt;

use serde::Deserialize;

use crate::exclusions::Exclusions;

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
