//! A plain text log: standard error plus a file the user can send along.

use capsflip_core::logfile;
use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::paths;

/// Writes one timestamped line. A failure to write the file is ignored: a
/// log that cannot be written must not take the keyboard down with it.
pub fn log(line: &str) {
    let line = format!("{} {line}", timestamp());
    eprintln!("{line}");
    if let Some(path) = paths::log() {
        let _ = logfile::append(&path, &line);
    }
}

/// The size at which the log is moved aside on start.
const LIMIT: u64 = 1024 * 1024;

/// Moves a log that has grown past [`LIMIT`] aside, once at start.
pub fn rotate() {
    if let Some(path) = paths::log() {
        let _ = logfile::rotate(&path, LIMIT);
    }
}

fn timestamp() -> String {
    let t = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_timestamp_is_a_local_date_and_time_with_milliseconds() {
        let stamp = timestamp();
        let shape: String = stamp
            .chars()
            .map(|c| if c.is_ascii_digit() { '0' } else { c })
            .collect();
        assert_eq!(shape, "0000-00-00 00:00:00.000", "{stamp}");
    }
}
