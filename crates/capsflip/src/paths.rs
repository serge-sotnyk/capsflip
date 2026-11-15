//! Where capsflip keeps its files.

use std::path::PathBuf;

/// `%APPDATA%\capsflip\config.toml`: settings follow a roaming profile.
pub fn config() -> Option<PathBuf> {
    in_profile("APPDATA", "config.toml")
}

/// `%LOCALAPPDATA%\capsflip\capsflip.log`: the log stays on this machine.
#[cfg(not(test))]
pub fn log() -> Option<PathBuf> {
    in_profile("LOCALAPPDATA", "capsflip.log")
}

/// Tests log to the temporary directory, never to the user's log.
#[cfg(test)]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of the function it stands in for"
)]
pub fn log() -> Option<PathBuf> {
    Some(std::env::temp_dir().join("capsflip-tests.log"))
}

fn in_profile(variable: &str, file: &str) -> Option<PathBuf> {
    let base = std::env::var_os(variable)?;
    Some(PathBuf::from(base).join("capsflip").join(file))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_config_lives_in_the_roaming_profile() {
        let path = config().expect("APPDATA is set on Windows");
        let roaming = std::env::var_os("APPDATA").unwrap();
        assert_eq!(path, PathBuf::from(roaming).join("capsflip/config.toml"));
    }

    #[test]
    fn a_profile_path_is_the_capsflip_folder_in_the_variable() {
        let path = in_profile("LOCALAPPDATA", "capsflip.log").expect("set on Windows");
        let local = std::env::var_os("LOCALAPPDATA").unwrap();
        assert_eq!(path, PathBuf::from(local).join("capsflip/capsflip.log"));
    }

    #[test]
    fn tests_never_write_the_users_log() {
        assert!(log().unwrap().starts_with(std::env::temp_dir()));
    }

    #[test]
    fn without_the_profile_variable_there_is_no_path() {
        assert_eq!(in_profile("CAPSFLIP_NO_SUCH_VARIABLE", "file"), None);
    }
}
