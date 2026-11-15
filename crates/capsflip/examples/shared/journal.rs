//! The log of the diagnostics in `examples/`. Lines are only queued where
//! they happen, so a hook callback stays fast even while the console is
//! blocked by a text selection; a timer writes them to the console and to a
//! file under `target/`.

use std::io::Write as _;
use std::sync::Mutex;
use std::time::Instant;

static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static START: Mutex<Option<Instant>> = Mutex::new(None);

/// Starts the clock the lines are stamped with.
pub fn start() {
    *START.lock().unwrap() = Some(Instant::now());
    let _ = std::fs::create_dir_all("target");
}

/// Queues a line, stamped with the milliseconds since [`start`].
pub fn log(tag: &str, text: &str) {
    let elapsed = START.lock().unwrap().map_or(0, |s| s.elapsed().as_millis());
    LINES
        .lock()
        .unwrap()
        .push(format!("{elapsed:>8} {tag:<8} {text}"));
}

/// Writes the queued lines to the console and appends them to `file`.
pub fn flush(file: &str) {
    let lines: Vec<String> = std::mem::take(&mut *LINES.lock().unwrap());
    if lines.is_empty() {
        return;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .ok();
    for line in lines {
        println!("{line}");
        if let Some(file) = file.as_mut() {
            let _ = writeln!(file, "{line}");
        }
    }
}
