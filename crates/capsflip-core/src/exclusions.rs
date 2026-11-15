//! Applications in which capsflip stays out of the way.

use serde::Deserialize;

/// The programs, by executable file name, in which no rule fires.
#[derive(Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Exclusions {
    apps: Vec<String>,
}

impl Exclusions {
    pub fn new<S: Into<String>>(apps: impl IntoIterator<Item = S>) -> Self {
        Self {
            apps: apps.into_iter().map(Into::into).collect(),
        }
    }

    /// True when `executable` (a file name such as `mstsc.exe`) is listed.
    /// ASCII letter case is ignored, as Windows file names are.
    #[must_use]
    pub fn contains(&self, executable: &str) -> bool {
        self.apps
            .iter()
            .any(|app| app.eq_ignore_ascii_case(executable))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exclusions() -> Exclusions {
        Exclusions::new(["mstsc.exe", "VMware.exe"])
    }

    #[test]
    fn a_listed_executable_is_excluded() {
        assert!(exclusions().contains("mstsc.exe"));
    }

    #[test]
    fn the_match_ignores_letter_case() {
        assert!(exclusions().contains("MSTSC.EXE"));
        assert!(exclusions().contains("vmware.exe"));
    }

    #[test]
    fn other_executables_are_not_excluded() {
        assert!(!exclusions().contains("notepad.exe"));
        assert!(!exclusions().contains("mstsc"));
        assert!(!Exclusions::default().contains("mstsc.exe"));
    }
}
