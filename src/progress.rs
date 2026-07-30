/// Abstraction over progress and status output.
///
/// Implement this trait to redirect command output to any interface —
/// stderr (CLI), a channel (GUI/TUI), a log file, a test harness, etc.
pub trait Progress {
    /// Emit a status or diagnostic message (replaces `eprintln!`).
    fn log(&mut self, msg: &str);

    /// Report numeric progress for progress-bar use.
    ///
    /// Callers always pair this with a [`Progress::log`] call that carries
    /// the human-readable description, so CLI implementations can safely
    /// leave this as a no-op.
    fn update(&mut self, done: u64, total: u64);
}

/// Default CLI implementation — writes status messages to stderr.
///
/// [`Progress::update`] is a no-op here because the paired `log` call
/// already prints the formatted "done/total" string.
pub struct StderrProgress;

impl Progress for StderrProgress {
    fn log(&mut self, msg: &str) {
        eprintln!("{msg}");
    }

    fn update(&mut self, _done: u64, _total: u64) {
        // CLI progress is carried by the log message; no separate bar needed.
    }
}
