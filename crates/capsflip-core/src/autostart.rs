//! Starting capsflip at login: what to write in the user's `Run` key.

use std::path::Path;

/// The command Windows runs at login: the quoted path of the executable.
#[must_use]
pub fn command(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// Whether the value `registered` in the `Run` key starts `exe`. Paths are
/// compared without regard to case, as Windows does. A value left by a copy
/// in another folder does not count.
#[must_use]
pub fn starts(registered: Option<&str>, exe: &Path) -> bool {
    registered.is_some_and(|value| value.to_lowercase() == command(exe).to_lowercase())
}

/// What a click on "Start at login" does to the `Run` key.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Change {
    Register(String),
    Unregister,
}

#[must_use]
pub fn toggle(registered: Option<&str>, exe: &Path) -> Change {
    if starts(registered, exe) {
        Change::Unregister
    } else {
        Change::Register(command(exe))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Program Files\capsflip\capsflip.exe")
    }

    #[test]
    fn the_command_is_the_quoted_path_of_the_executable() {
        assert_eq!(
            command(&exe()),
            r#""C:\Program Files\capsflip\capsflip.exe""#
        );
    }

    #[test]
    fn only_a_value_that_starts_this_executable_counts() {
        assert!(!starts(None, &exe()));
        assert!(starts(Some(&command(&exe())), &exe()));
        assert!(starts(
            Some(r#""c:\program files\CAPSFLIP\capsflip.exe""#),
            &exe()
        ));
        assert!(!starts(Some(r#""D:\tools\capsflip.exe""#), &exe()));
    }

    #[test]
    fn toggling_registers_this_executable_or_removes_it() {
        let registered = command(&exe());
        assert_eq!(toggle(None, &exe()), Change::Register(registered.clone()));
        assert_eq!(toggle(Some(&registered), &exe()), Change::Unregister);
        assert_eq!(
            toggle(Some(r#""D:\tools\capsflip.exe""#), &exe()),
            Change::Register(registered),
            "a copy that moved points the value at itself"
        );
    }
}
