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
    fn a_path_that_cannot_be_written_is_an_error() {
        assert!(append(&std::env::temp_dir(), "line").is_err());
    }
}
