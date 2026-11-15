//! Appending lines to the log file.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::Path;

/// Appends `line` to the file at `path`, creating the file and its
/// directory when they do not exist yet.
///
/// # Errors
///
/// Returns the I/O error when the directory or the file cannot be written.
pub fn append(path: &Path, line: &str) -> std::io::Result<()> {
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")
}

/// Moves the log at `path` aside to `<name>.old.log` once it is larger than
/// `limit` bytes, replacing the one moved before. The log cannot grow
/// without end, and the latest history is still there to read.
///
/// # Errors
///
/// Returns the I/O error when the log cannot be moved.
pub fn rotate(path: &Path, limit: u64) -> std::io::Result<()> {
    if std::fs::metadata(path).is_ok_and(|log| log.len() > limit) {
        std::fs::rename(path, path.with_extension("old.log"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appending_creates_the_directory_and_adds_one_line_per_call() {
        let dir = std::env::temp_dir().join(format!("capsflip-{}-logfile", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("capsflip.log");

        append(&path, "first").unwrap();
        append(&path, "second").unwrap();

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "first
second
"
        );
    }

    #[test]
    fn a_log_past_the_limit_is_moved_aside_replacing_the_older_one() {
        let dir = std::env::temp_dir().join(format!("capsflip-{}-rotate", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("capsflip.log");
        let old = dir.join("capsflip.old.log");

        rotate(&path, 4).unwrap();
        assert!(!old.exists(), "no log yet, nothing to move");

        std::fs::write(&path, "1234").unwrap();
        rotate(&path, 4).unwrap();
        assert!(path.exists(), "at the limit the log stays");

        std::fs::write(&old, "older").unwrap();
        std::fs::write(&path, "12345").unwrap();
        rotate(&path, 4).unwrap();
        assert!(!path.exists());
        assert_eq!(std::fs::read_to_string(&old).unwrap(), "12345");
    }

    #[test]
    fn a_path_that_cannot_be_written_is_an_error() {
        assert!(append(&std::env::temp_dir(), "line").is_err());
    }
}
