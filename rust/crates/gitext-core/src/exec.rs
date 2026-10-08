//! Process execution: port of `Executable`, `GitCommandRunner` and `CommandLog`.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use crate::args::GitArgs;

/// Output of a finished process.
#[derive(Debug, Clone, Default)]
pub struct ExecResult {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl ExecResult {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }

    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn stderr_str(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }

    /// Combined stdout and stderr, as shown to the user in process dialogs.
    pub fn all_output(&self) -> String {
        let mut s = self.stdout_str();
        let e = self.stderr_str();
        if !e.is_empty() {
            if !s.is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            s.push_str(&e);
        }
        s
    }
}

/// Error from running git.
#[derive(Debug)]
pub enum GitError {
    /// The process could not be started.
    Io(io::Error),
    /// Git exited with an error.
    Failed { args: String, exit_code: i32, stderr: String },
    /// Output could not be parsed or the request is invalid.
    Invalid(String),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::Io(e) => write!(f, "failed to run git: {e}"),
            GitError::Failed { args, exit_code, stderr } => {
                write!(f, "git {args} failed with exit code {exit_code}")?;
                if !stderr.trim().is_empty() {
                    write!(f, ":\n{}", stderr.trim())?;
                }
                Ok(())
            }
            GitError::Invalid(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for GitError {}

impl From<io::Error> for GitError {
    fn from(e: io::Error) -> Self {
        GitError::Io(e)
    }
}

pub type GitResult<T> = Result<T, GitError>;

/// One entry of the git command log (`FormGitCommandLog`).
#[derive(Debug, Clone)]
pub struct CommandLogEntry {
    pub file_name: String,
    pub arguments: String,
    pub working_dir: String,
    pub start: SystemTime,
    pub duration: Option<Duration>,
    pub exit_code: Option<i32>,
    pub is_on_main_thread: bool,
}

impl CommandLogEntry {
    /// One-line description (port of `CommandLogEntry.ToString`).
    pub fn column_line(&self) -> String {
        let start: chrono::DateTime<chrono::Local> = self.start.into();
        let duration = self.duration.map(|d| format!("{:>6}ms", d.as_millis())).unwrap_or_else(|| "running".to_string());
        let exit = self.exit_code.map(|c| c.to_string()).unwrap_or_default();
        format!("{} {} {:>3} {} {}", start.format("%H:%M:%S%.3f"), duration, exit, self.file_name, self.arguments)
    }
}

const MAX_LOG_ENTRIES: usize = 500;

fn command_log() -> &'static Mutex<Vec<CommandLogEntry>> {
    static LOG: OnceLock<Mutex<Vec<CommandLogEntry>>> = OnceLock::new();
    LOG.get_or_init(|| Mutex::new(Vec::new()))
}

/// A snapshot of the command log.
pub fn command_log_entries() -> Vec<CommandLogEntry> {
    command_log().lock().unwrap().clone()
}

pub fn clear_command_log() {
    command_log().lock().unwrap().clear();
}

fn log_start(file_name: &str, args: &str, working_dir: &Path) -> usize {
    let mut log = command_log().lock().unwrap();
    if log.len() >= MAX_LOG_ENTRIES {
        log.remove(0);
    }
    log.push(CommandLogEntry {
        file_name: file_name.to_string(),
        arguments: args.to_string(),
        working_dir: working_dir.display().to_string(),
        start: SystemTime::now(),
        duration: None,
        exit_code: None,
        is_on_main_thread: false,
    });
    log.len() - 1
}

fn log_end(index: usize, args: &str, started: Instant, exit_code: i32) {
    let mut log = command_log().lock().unwrap();
    // The index may have shifted if entries were evicted; search from the end.
    let i = if log.get(index).is_some_and(|e| e.arguments == args && e.exit_code.is_none()) {
        Some(index)
    } else {
        log.iter().rposition(|e| e.arguments == args && e.exit_code.is_none())
    };
    if let Some(i) = i {
        log[i].duration = Some(started.elapsed());
        log[i].exit_code = Some(exit_code);
    }
}

/// Path of the git executable (configurable in settings).
fn git_command_path() -> &'static Mutex<String> {
    static PATH: OnceLock<Mutex<String>> = OnceLock::new();
    PATH.get_or_init(|| Mutex::new("git".to_string()))
}

pub fn set_git_command(path: &str) {
    *git_command_path().lock().unwrap() = if path.trim().is_empty() { "git".to_string() } else { path.to_string() };
}

pub fn git_command() -> String {
    git_command_path().lock().unwrap().clone()
}

/// Lines (or progress updates terminated by `\r`) emitted by a streaming process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputEvent {
    Stdout(String),
    Stderr(String),
    /// A progress line (terminated by carriage return) which replaces the previous one.
    Progress(String),
    Exited(i32),
}

/// A running process whose output is streamed through a channel.
pub struct RunningProcess {
    pub events: Receiver<OutputEvent>,
    child: Arc<Mutex<Option<Child>>>,
    cancelled: Arc<AtomicBool>,
}

impl RunningProcess {
    /// Kills the process.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(child) = self.child.lock().unwrap().as_mut() {
            let _ = child.kill();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// A process with raw access to its stdout.
pub struct RawProcess {
    pub stdout: std::process::ChildStdout,
    pub child: Mutex<Child>,
}

/// Runs git (or another executable) in a working directory.
#[derive(Debug, Clone)]
pub struct Executable {
    pub file_name: String,
    pub working_dir: PathBuf,
    pub env: Vec<(String, String)>,
}

impl Executable {
    pub fn git(working_dir: impl Into<PathBuf>) -> Self {
        Executable { file_name: git_command(), working_dir: working_dir.into(), env: Vec::new() }
    }

    pub fn new(file_name: impl Into<String>, working_dir: impl Into<PathBuf>) -> Self {
        Executable { file_name: file_name.into(), working_dir: working_dir.into(), env: Vec::new() }
    }

    fn command(&self, args: &[String]) -> Command {
        let mut cmd = Command::new(&self.file_name);
        cmd.args(args);
        if !self.working_dir.as_os_str().is_empty() && self.working_dir.is_dir() {
            cmd.current_dir(&self.working_dir);
        }
        // Never block on an interactive terminal prompt; credentials go through askpass helpers.
        cmd.env("GIT_TERMINAL_PROMPT", "0");
        cmd.env_remove("GIT_DIR");
        cmd.env_remove("GIT_WORK_TREE");
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd
    }

    /// Runs the process to completion, capturing output. Optional `stdin` input.
    pub fn run_with_input(&self, args: &GitArgs, stdin: Option<&[u8]>) -> GitResult<ExecResult> {
        let display = args.to_string();
        let log_index = log_start(&self.file_name, &display, &self.working_dir);
        let started = Instant::now();
        let mut cmd = self.command(args.as_slice());
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                log_end(log_index, &display, started, -1);
                return Err(GitError::Io(e));
            }
        };
        if let Some(input) = stdin {
            let mut pipe = child.stdin.take().unwrap();
            let input = input.to_vec();
            std::thread::spawn(move || {
                let _ = pipe.write_all(&input);
            });
        }
        let output = child.wait_with_output()?;
        let exit_code = output.status.code().unwrap_or(-1);
        log_end(log_index, &display, started, exit_code);
        Ok(ExecResult { exit_code, stdout: output.stdout, stderr: output.stderr })
    }

    pub fn run(&self, args: &GitArgs) -> GitResult<ExecResult> {
        self.run_with_input(args, None)
    }

    /// Runs and fails with [`GitError::Failed`] on a non-zero exit code.
    pub fn run_checked(&self, args: &GitArgs) -> GitResult<ExecResult> {
        let r = self.run(args)?;
        if !r.success() {
            return Err(GitError::Failed { args: args.to_string(), exit_code: r.exit_code, stderr: r.all_output() });
        }
        Ok(r)
    }

    /// Runs and returns stdout as (lossy) UTF-8, failing on non-zero exit.
    pub fn output(&self, args: &GitArgs) -> GitResult<String> {
        Ok(self.run_checked(args)?.stdout_str())
    }

    /// Starts the process with piped stdout (stderr discarded) for incremental parsing.
    pub fn spawn_raw(&self, args: &GitArgs) -> GitResult<RawProcess> {
        log_start(&self.file_name, &args.to_string(), &self.working_dir);
        let mut cmd = self.command(args.as_slice());
        cmd.stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null());
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().unwrap();
        Ok(RawProcess { stdout, child: Mutex::new(child) })
    }

    /// Starts the process and streams its output line by line.
    pub fn spawn_streaming(&self, args: &GitArgs) -> GitResult<RunningProcess> {
        let display = args.to_string();
        let log_index = log_start(&self.file_name, &display, &self.working_dir);
        let started = Instant::now();
        let mut cmd = self.command(args.as_slice());
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (tx, rx) = mpsc::channel();
        let child = Arc::new(Mutex::new(Some(child)));
        let cancelled = Arc::new(AtomicBool::new(false));

        let tx_out = tx.clone();
        let out_thread = std::thread::spawn(move || stream_lines(stdout, |line, progress| {
            let _ = tx_out.send(if progress { OutputEvent::Progress(line) } else { OutputEvent::Stdout(line) });
        }));
        let tx_err = tx.clone();
        let err_thread = std::thread::spawn(move || stream_lines(stderr, |line, progress| {
            let _ = tx_err.send(if progress { OutputEvent::Progress(line) } else { OutputEvent::Stderr(line) });
        }));
        let child_wait = Arc::clone(&child);
        std::thread::spawn(move || {
            let _ = out_thread.join();
            let _ = err_thread.join();
            let code = {
                let mut guard = child_wait.lock().unwrap();
                match guard.as_mut() {
                    Some(c) => c.wait().ok().and_then(|s| s.code()).unwrap_or(-1),
                    None => -1,
                }
            };
            log_end(log_index, &display, started, code);
            let _ = tx.send(OutputEvent::Exited(code));
        });
        Ok(RunningProcess { events: rx, child, cancelled })
    }
}

/// Splits a byte stream into lines on `\n` and progress updates on `\r`.
fn stream_lines(mut reader: impl Read, mut emit: impl FnMut(String, bool)) {
    let mut buf = [0u8; 4096];
    let mut line: Vec<u8> = Vec::new();
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        for &b in &buf[..n] {
            match b {
                b'\n' => {
                    emit(String::from_utf8_lossy(&line).into_owned(), false);
                    line.clear();
                }
                b'\r' => {
                    if !line.is_empty() {
                        emit(String::from_utf8_lossy(&line).into_owned(), true);
                    }
                    line.clear();
                }
                _ => line.push(b),
            }
        }
    }
    if !line.is_empty() {
        emit(String::from_utf8_lossy(&line).into_owned(), false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_git_version_and_logs_command() {
        let exe = Executable::git(std::env::temp_dir());
        let out = exe.output(&GitArgs::new("version")).unwrap();
        assert!(out.starts_with("git version"));
        assert!(command_log_entries().iter().any(|e| e.arguments == "version" && e.exit_code == Some(0)));
    }

    #[test]
    fn failed_command_returns_error() {
        let exe = Executable::git(std::env::temp_dir());
        let err = exe.run_checked(&GitArgs::new("no-such-command")).unwrap_err();
        assert!(matches!(err, GitError::Failed { .. }));
    }

    #[test]
    fn stream_lines_splits_progress() {
        let mut events = Vec::new();
        stream_lines(&b"a\nprog 1%\rprog 2%\rdone\n"[..], |l, p| events.push((l, p)));
        assert_eq!(
            events,
            vec![("a".into(), false), ("prog 1%".into(), true), ("prog 2%".into(), true), ("done".into(), false)]
        );
    }

    #[test]
    fn streaming_reports_exit_code() {
        let exe = Executable::git(std::env::temp_dir());
        let p = exe.spawn_streaming(&GitArgs::new("version")).unwrap();
        let events: Vec<_> = p.events.iter().collect();
        assert!(events.iter().any(|e| matches!(e, OutputEvent::Stdout(s) if s.starts_with("git version"))));
        assert_eq!(events.last(), Some(&OutputEvent::Exited(0)));
    }
}
