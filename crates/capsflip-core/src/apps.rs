//! Lists of applications, by executable file name: the ones capsflip stays
//! out of, and the terminals.

use serde::Deserialize;

/// Programs by executable file name, such as `mstsc.exe`.
#[derive(Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(transparent)]
pub struct Apps(Vec<String>);

impl Apps {
    pub fn new<S: Into<String>>(apps: impl IntoIterator<Item = S>) -> Self {
        Self(apps.into_iter().map(Into::into).collect())
    }

    /// True when `executable` (a file name such as `mstsc.exe`) is listed.
    /// ASCII letter case is ignored, as Windows file names are.
    #[must_use]
    pub fn contains(&self, executable: &str) -> bool {
        self.0
            .iter()
            .any(|app| app.eq_ignore_ascii_case(executable))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apps() -> Apps {
        Apps::new(["mstsc.exe", "VMware.exe"])
    }

    #[test]
    fn a_listed_executable_is_contained() {
        assert!(apps().contains("mstsc.exe"));
    }

    #[test]
    fn the_match_ignores_letter_case() {
        assert!(apps().contains("MSTSC.EXE"));
        assert!(apps().contains("vmware.exe"));
    }

    #[test]
    fn other_executables_are_not_contained() {
        assert!(!apps().contains("notepad.exe"));
        assert!(!apps().contains("mstsc"));
        assert!(!Apps::default().contains("mstsc.exe"));
    }
}
