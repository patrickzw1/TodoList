use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const FORMAT: &str = "todolist-log-v1";
const MAX_RECORD_BYTES: usize = 1024;
const QUEUE_CAPACITY: usize = 256;
const ERROR_WINDOW: Duration = Duration::from_secs(30);
const DETAIL_DURATION: Duration = Duration::from_secs(15 * 60);
const DETAIL_REFRESH: Duration = Duration::from_secs(30);
const DETAIL_CONTROL_FILE: &str = ".todolist-detailed-until";

#[derive(Clone, Copy, Debug)]
pub enum Component {
    Gui,
    Mcp,
}

impl Component {
    fn label(self) -> &'static str {
        match self {
            Self::Gui => "gui",
            Self::Mcp => "mcp",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathIdentity {
    pub display_path: String,
    pub fingerprint: String,
    pub selection: &'static str,
}

impl PathIdentity {
    pub fn database(path: &Path, selected_by_override: bool) -> Self {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(path)
        };
        let normalized = absolute.to_string_lossy().replace('/', "\\");
        let digest = Sha256::digest(normalized.to_lowercase().as_bytes());
        let fingerprint = digest[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let profile = std::env::var("USERPROFILE").ok();
        let display_path = redact_database_path(
            &normalized,
            if selected_by_override {
                None
            } else {
                profile.as_deref()
            },
        );
        Self {
            display_path,
            fingerprint,
            selection: if selected_by_override {
                "override"
            } else {
                "default"
            },
        }
    }
}

fn redact_database_path(path: &str, profile: Option<&str>) -> String {
    let normalized_profile = profile.map(|value| value.replace('/', "\\"));
    let masked = normalized_profile.as_deref().and_then(|profile| {
        if path
            .get(..profile.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(profile))
        {
            let suffix = &path[profile.len()..];
            if suffix.is_empty() || suffix.starts_with('\\') {
                return Some(format!("%USERPROFILE%{suffix}"));
            }
        }
        None
    });
    let fallback = || {
        let drive = path
            .chars()
            .next()
            .filter(|_| path.as_bytes().get(1) == Some(&b':'));
        match drive {
            Some(letter) => format!("{}:\\…\\<database>", letter.to_ascii_uppercase()),
            None => "<custom database path>".to_string(),
        }
    };
    let value = masked.unwrap_or_else(fallback);
    if value.chars().count() <= 160 {
        value
    } else {
        format!("{}…", value.chars().take(159).collect::<String>())
    }
}

pub fn classify_error(error: &str) -> &'static str {
    let text = error
        .chars()
        .take(512)
        .collect::<String>()
        .to_ascii_lowercase();
    if text.contains("permission") || text.contains("access is denied") || text.contains("拒绝访问")
    {
        "permission"
    } else if text.contains("locked") || text.contains("busy") {
        "busy"
    } else if text.contains("timeout") || text.contains("超时") {
        "timeout"
    } else if text.contains("version conflict") || text.contains("version_conflict") {
        "version_conflict"
    } else if text.contains("no such file") || text.contains("not found") {
        "missing"
    } else {
        "other"
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub event: &'static str,
    pub operation: &'static str,
    pub outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_version: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_version: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projects: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_class: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<PathIdentity>,
}

impl Record {
    pub fn new(event: &'static str, operation: &'static str, outcome: &'static str) -> Self {
        Self {
            event,
            operation,
            outcome,
            build_version: None,
            channel: None,
            duration_ms: None,
            workspace_version: None,
            projects: None,
            tasks: None,
            error_class: None,
            source: None,
        }
    }

    pub fn build(mut self, version: &'static str, channel: &'static str) -> Self {
        self.build_version = Some(version);
        self.channel = Some(channel);
        self
    }

    pub fn workspace(mut self, version: u64, projects: usize, tasks: usize) -> Self {
        self.workspace_version = Some(version);
        self.projects = Some(projects);
        self.tasks = Some(tasks);
        self
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration_ms = Some(duration.as_millis().min(u128::from(u64::MAX)) as u64);
        self
    }

    pub fn error(mut self, error: &str) -> Self {
        self.error_class = Some(classify_error(error));
        self
    }

    pub fn source(mut self, source: PathIdentity) -> Self {
        self.source = Some(source);
        self
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogStatus {
    pub available: bool,
    pub directory: Option<String>,
    pub reason: Option<&'static str>,
    pub detailed_seconds_remaining: u64,
}

#[derive(Clone, Copy)]
struct Limits {
    file_bytes: u64,
    total_bytes: u64,
    age: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            file_bytes: 2 * 1024 * 1024,
            total_bytes: 10 * 1024 * 1024,
            age: Duration::from_secs(7 * 24 * 60 * 60),
        }
    }
}

enum Message {
    Record(Record),
    Flush(mpsc::Sender<()>),
}

struct Inner {
    sender: Option<mpsc::SyncSender<Message>>,
    status: Arc<Mutex<LogStatus>>,
    detail_file: Option<PathBuf>,
    detail_state: Mutex<DetailState>,
    dropped: Arc<AtomicU64>,
}

struct DetailState {
    checked_at: Instant,
    until_ms: u64,
}

impl Default for DetailState {
    fn default() -> Self {
        Self {
            checked_at: Instant::now() - DETAIL_REFRESH,
            until_ms: 0,
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(sender) = &self.sender {
            let (done, received) = mpsc::channel();
            if sender.try_send(Message::Flush(done)).is_ok() {
                let _ = received.recv_timeout(Duration::from_millis(250));
            }
        }
    }
}

#[derive(Clone)]
pub struct DiagnosticLog {
    inner: Arc<Inner>,
}

impl DiagnosticLog {
    pub fn for_current_exe(component: Component) -> Self {
        match std::env::current_exe() {
            Ok(executable) => Self::at_executable(component, &executable),
            Err(_) => Self::disabled("executable_unavailable", None),
        }
    }

    pub fn at_executable(component: Component, executable: &Path) -> Self {
        Self::at_executable_with_limits(component, executable, Limits::default())
    }

    fn at_executable_with_limits(component: Component, executable: &Path, limits: Limits) -> Self {
        let Some(parent) = executable.parent() else {
            return Self::disabled("executable_unavailable", None);
        };
        let directory = parent.join("logs");
        if fs::create_dir_all(&directory).is_err() {
            return Self::disabled("directory_unwritable", Some(directory));
        }
        let (file, path, session) = match create_file(&directory, component, 0) {
            Ok(value) => value,
            Err(_) => return Self::disabled("file_unwritable", Some(directory)),
        };
        let status = Arc::new(Mutex::new(LogStatus {
            available: true,
            directory: Some(directory.display().to_string()),
            reason: None,
            detailed_seconds_remaining: 0,
        }));
        let dropped = Arc::new(AtomicU64::new(0));
        let detail_file = directory.join(DETAIL_CONTROL_FILE);
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let writer_status = status.clone();
        let writer_dropped = dropped.clone();
        let spawn = thread::Builder::new()
            .name(format!("todolist-{}-log", component.label()))
            .spawn(move || {
                let mut writer = Writer {
                    directory,
                    component,
                    session,
                    segment: 0,
                    file,
                    path,
                    size: 0,
                    limits,
                    status: writer_status,
                    dropped: writer_dropped,
                    repeated: HashMap::new(),
                };
                writer.size = writer.file.metadata().map(|value| value.len()).unwrap_or(0);
                writer.cleanup();
                writer.run(receiver);
            });
        if spawn.is_err() {
            return Self::disabled("writer_unavailable", Some(parent.join("logs")));
        }
        Self {
            inner: Arc::new(Inner {
                sender: Some(sender),
                status,
                detail_file: Some(detail_file),
                detail_state: Mutex::new(DetailState::default()),
                dropped,
            }),
        }
    }

    fn disabled(reason: &'static str, directory: Option<PathBuf>) -> Self {
        Self {
            inner: Arc::new(Inner {
                sender: None,
                status: Arc::new(Mutex::new(LogStatus {
                    available: false,
                    directory: directory.map(|value| value.display().to_string()),
                    reason: Some(reason),
                    detailed_seconds_remaining: 0,
                })),
                detail_file: None,
                detail_state: Mutex::new(DetailState::default()),
                dropped: Arc::new(AtomicU64::new(0)),
            }),
        }
    }

    pub fn status(&self) -> LogStatus {
        let mut status = self.inner.status.lock().unwrap().clone();
        if let Some(path) = &self.inner.detail_file {
            let mut detail = self.inner.detail_state.lock().unwrap();
            if detail.checked_at.elapsed() >= DETAIL_REFRESH {
                detail.until_ms = fs::metadata(path)
                    .ok()
                    .filter(|meta| meta.len() <= 32)
                    .and_then(|_| fs::read_to_string(path).ok())
                    .and_then(|value| value.trim().parse::<u64>().ok())
                    .unwrap_or(0);
                detail.checked_at = Instant::now();
            }
            let remaining_ms = u128::from(detail.until_ms)
                .saturating_sub(now_ms())
                .min(DETAIL_DURATION.as_millis());
            status.detailed_seconds_remaining = (remaining_ms as u64).div_ceil(1000);
        }
        status
    }

    pub fn directory(&self) -> Option<PathBuf> {
        self.inner
            .status
            .lock()
            .unwrap()
            .directory
            .as_ref()
            .map(PathBuf::from)
    }

    pub fn detailed(&self) -> bool {
        self.status().detailed_seconds_remaining > 0
    }

    pub fn set_detailed(&self, enabled: bool) -> Result<LogStatus, &'static str> {
        if !self.status().available {
            return Err("logs_unavailable");
        }
        let path = self.inner.detail_file.as_ref().ok_or("logs_unavailable")?;
        let until_ms = if enabled {
            (now_ms() + DETAIL_DURATION.as_millis()) as u64
        } else {
            0
        };
        fs::write(path, format!("{until_ms}\n")).map_err(|_| "detail_control_unwritable")?;
        let mut detail = self.inner.detail_state.lock().unwrap();
        detail.until_ms = until_ms;
        detail.checked_at = Instant::now();
        drop(detail);
        self.record(Record::new(
            "detail_mode",
            "diagnostics",
            if enabled { "enabled" } else { "disabled" },
        ));
        Ok(self.status())
    }

    pub fn record(&self, record: Record) {
        if !self.inner.status.lock().unwrap().available {
            return;
        }
        if let Some(sender) = &self.inner.sender {
            match sender.try_send(Message::Record(record)) {
                Ok(()) => {}
                Err(mpsc::TrySendError::Full(_)) => {
                    self.inner.dropped.fetch_add(1, Ordering::Relaxed);
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    let mut status = self.inner.status.lock().unwrap();
                    status.available = false;
                    status.reason = Some("writer_unavailable");
                }
            }
        }
    }

    pub fn flush(&self, timeout: Duration) -> bool {
        let Some(sender) = &self.inner.sender else {
            return false;
        };
        let (done_tx, done_rx) = mpsc::channel();
        let mut message = Message::Flush(done_tx);
        let deadline = Instant::now() + timeout;
        loop {
            match sender.try_send(message) {
                Ok(()) => return done_rx.recv_timeout(timeout).is_ok(),
                Err(mpsc::TrySendError::Full(returned)) if Instant::now() < deadline => {
                    message = returned;
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => return false,
            }
        }
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn create_file(
    directory: &Path,
    component: Component,
    segment: u32,
) -> std::io::Result<(File, PathBuf, String)> {
    let session = format!("{}-{}", std::process::id(), now_ms());
    create_file_for_session(directory, component, &session, segment)
        .map(|(file, path)| (file, path, session))
}

fn create_file_for_session(
    directory: &Path,
    component: Component,
    session: &str,
    segment: u32,
) -> std::io::Result<(File, PathBuf)> {
    let path = directory.join(format!(
        "todolist-{}-{session}-{segment}.log",
        component.label()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0x0000_0001 | 0x0000_0002);
    }
    let mut file = options.open(&path)?;
    file.write_all(
        format!(
            "{{\"format\":\"{FORMAT}\",\"component\":\"{}\",\"event\":\"file_opened\"}}\n",
            component.label()
        )
        .as_bytes(),
    )?;
    Ok((file, path))
}

struct Writer {
    directory: PathBuf,
    component: Component,
    session: String,
    segment: u32,
    file: File,
    path: PathBuf,
    size: u64,
    limits: Limits,
    status: Arc<Mutex<LogStatus>>,
    dropped: Arc<AtomicU64>,
    repeated: HashMap<(&'static str, &'static str), (Instant, u64)>,
}

impl Writer {
    fn run(&mut self, receiver: mpsc::Receiver<Message>) {
        loop {
            if !self.status.lock().unwrap().available {
                return;
            }
            match receiver.recv_timeout(ERROR_WINDOW) {
                Ok(Message::Record(record)) => {
                    self.write_dropped();
                    self.write_record(record);
                }
                Ok(Message::Flush(done)) => {
                    self.write_dropped();
                    self.write_repeats(true);
                    let _ = self.file.flush();
                    let _ = done.send(());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => self.write_repeats(false),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.write_dropped();
                    self.write_repeats(true);
                    let _ = self.file.flush();
                    return;
                }
            }
        }
    }

    fn write_dropped(&mut self) {
        let count = self.dropped.swap(0, Ordering::Relaxed);
        if count > 0 {
            self.append(&Record::new("queue_dropped", "logger", "warning"), count);
        }
    }

    fn write_record(&mut self, record: Record) {
        if record.outcome == "error" {
            let key = (record.event, record.operation);
            if let Some((last, suppressed)) = self.repeated.get_mut(&key) {
                if last.elapsed() < ERROR_WINDOW {
                    *suppressed += 1;
                    return;
                }
                let count = *suppressed;
                *last = Instant::now();
                *suppressed = 0;
                self.append(&record, count);
                return;
            }
            self.repeated.insert(key, (Instant::now(), 0));
        }
        self.append(&record, 0);
    }

    fn write_repeats(&mut self, all: bool) {
        let mut summaries = Vec::new();
        for ((event, operation), (last, suppressed)) in &mut self.repeated {
            if *suppressed > 0 && (all || last.elapsed() >= ERROR_WINDOW) {
                summaries.push((
                    Record::new("error_repeats", operation, "warning"),
                    *suppressed,
                    *event,
                ));
                *suppressed = 0;
                *last = Instant::now();
            }
        }
        for (record, count, _original_event) in summaries {
            self.append(&record, count);
        }
    }

    fn append(&mut self, record: &Record, suppressed: u64) {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Entry<'a> {
            format: &'static str,
            time_ms: u128,
            component: &'static str,
            #[serde(flatten)]
            record: &'a Record,
            #[serde(skip_serializing_if = "is_zero")]
            suppressed: u64,
        }
        fn is_zero(value: &u64) -> bool {
            *value == 0
        }
        let entry = Entry {
            format: FORMAT,
            time_ms: now_ms(),
            component: self.component.label(),
            record,
            suppressed,
        };
        let mut bytes = serde_json::to_vec(&entry).unwrap_or_default();
        if bytes.len() + 1 > MAX_RECORD_BYTES {
            bytes =
                format!("{{\"format\":\"{FORMAT}\",\"event\":\"record_truncated\"}}").into_bytes();
        }
        bytes.push(b'\n');
        let mut rotated = false;
        if self.size + bytes.len() as u64 > self.limits.file_bytes {
            if self.rotate().is_err() {
                self.fail("rotation_failed");
                return;
            }
            rotated = true;
        }
        if self.file.write_all(&bytes).is_err() {
            self.fail("write_failed");
            return;
        }
        self.size += bytes.len() as u64;
        if rotated {
            self.cleanup();
        }
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        self.file.flush()?;
        self.segment += 1;
        let (file, path) =
            create_file_for_session(&self.directory, self.component, &self.session, self.segment)?;
        self.size = file.metadata()?.len();
        self.file = file;
        self.path = path;
        Ok(())
    }

    fn fail(&self, reason: &'static str) {
        let mut status = self.status.lock().unwrap();
        status.available = false;
        status.reason = Some(reason);
    }

    fn cleanup(&self) {
        cleanup_owned_logs(&self.directory, &self.path, self.limits);
    }
}

fn cleanup_owned_logs(directory: &Path, active: &Path, limits: Limits) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path == active || !is_own_log(&path) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        if SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default()
            > limits.age
        {
            if fs::remove_file(&path).is_ok() {
                continue;
            }
        }
        files.push((path, modified, metadata.len()));
    }
    files.sort_by_key(|(_, modified, _)| *modified);
    let active_size = fs::metadata(active).map(|value| value.len()).unwrap_or(0);
    let mut total = active_size + files.iter().map(|(_, _, len)| len).sum::<u64>();
    for (path, _, len) in files {
        if total <= limits.total_bytes {
            break;
        }
        if fs::remove_file(path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

fn is_own_log(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let Some(rest) = name
        .strip_prefix("todolist-gui-")
        .or_else(|| name.strip_prefix("todolist-mcp-"))
    else {
        return false;
    };
    let Some(stem) = rest.strip_suffix(".log") else {
        return false;
    };
    if stem.is_empty()
        || !stem
            .bytes()
            .all(|value| value.is_ascii_digit() || value == b'-')
    {
        return false;
    }
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut prefix = [0_u8; 96];
    let Ok(read) = file.read(&mut prefix) else {
        return false;
    };
    prefix[..read]
        .windows(FORMAT.len())
        .any(|window| window == FORMAT.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn rolls_files_and_keeps_only_owned_logs_under_total_limit() {
        let root = tempdir().unwrap();
        let executable = root.path().join("todolist-desktop.exe");
        let logs = root.path().join("logs");
        fs::create_dir_all(&logs).unwrap();
        let unrelated = logs.join("notes.log");
        fs::write(&unrelated, b"keep this user file").unwrap();
        let logger = DiagnosticLog::at_executable_with_limits(
            Component::Gui,
            &executable,
            Limits {
                file_bytes: 500,
                total_bytes: 1300,
                age: Duration::from_secs(1),
            },
        );
        for _ in 0..35 {
            logger.record(Record::new("read_completed", "load_workspace", "ok"));
        }
        assert!(logger.flush(Duration::from_secs(2)));
        let owned: Vec<_> = fs::read_dir(&logs)
            .unwrap()
            .flatten()
            .filter(|entry| is_own_log(&entry.path()))
            .collect();
        assert!(owned.len() >= 2);
        assert!(owned
            .iter()
            .all(|entry| entry.metadata().unwrap().len() <= 500));
        assert!(
            owned
                .iter()
                .map(|entry| entry.metadata().unwrap().len())
                .sum::<u64>()
                <= 1300
        );
        assert_eq!(fs::read(unrelated).unwrap(), b"keep this user file");
    }

    #[test]
    fn removes_expired_owned_log_but_preserves_unmarked_files() {
        let root = tempdir().unwrap();
        let logs = root.path().join("logs");
        fs::create_dir_all(&logs).unwrap();
        let owned = logs.join("todolist-mcp-123-456-0.log");
        let unmarked = logs.join("todolist-mcp-123-457-0.log");
        fs::write(&owned, b"{\"format\":\"todolist-log-v1\"}\n").unwrap();
        fs::write(&unmarked, b"user data\n").unwrap();
        let old = fs::FileTimes::new()
            .set_modified(SystemTime::now() - Duration::from_secs(8 * 24 * 60 * 60));
        OpenOptions::new()
            .write(true)
            .open(&owned)
            .unwrap()
            .set_times(old)
            .unwrap();
        OpenOptions::new()
            .write(true)
            .open(&unmarked)
            .unwrap()
            .set_times(old)
            .unwrap();
        let logger =
            DiagnosticLog::at_executable(Component::Gui, &root.path().join("todolist-desktop.exe"));
        assert!(logger.flush(Duration::from_secs(2)));
        assert!(!owned.exists());
        assert_eq!(fs::read(unmarked).unwrap(), b"user data\n");
    }

    #[test]
    fn unavailable_install_directory_never_falls_back_elsewhere() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("logs"), b"not a directory").unwrap();
        let logger =
            DiagnosticLog::at_executable(Component::Mcp, &root.path().join("todolist-mcp.exe"));
        let status = logger.status();
        assert!(!status.available);
        assert_eq!(status.reason, Some("directory_unwritable"));
        assert!(!logger.flush(Duration::from_millis(50)));
        assert_eq!(
            fs::read(root.path().join("logs")).unwrap(),
            b"not a directory"
        );
    }

    #[test]
    fn path_and_error_privacy_are_bounded() {
        let path = redact_database_path(
            "C:\\Users\\Patrick\\AppData\\Roaming\\app.todolist.desktop\\todolist.sqlite",
            Some("C:\\Users\\Patrick"),
        );
        assert_eq!(
            path,
            "%USERPROFILE%\\AppData\\Roaming\\app.todolist.desktop\\todolist.sqlite"
        );
        assert!(!path.contains("Patrick"));
        assert_eq!(
            redact_database_path("C:\\Users\\Patrick\\private\\todolist.sqlite", None),
            "C:\\…\\<database>"
        );
        assert_eq!(
            classify_error("could not open C:\\private\\secret-key.txt: Access is denied"),
            "permission"
        );
        let root = tempdir().unwrap();
        let logger =
            DiagnosticLog::at_executable(Component::Mcp, &root.path().join("todolist-mcp.exe"));
        logger.record(
            Record::new("read_failed", "load_workspace", "error")
                .error("could not open C:\\private\\secret-key.txt: Access is denied"),
        );
        assert!(logger.flush(Duration::from_secs(2)));
        let content = fs::read_to_string(
            fs::read_dir(root.path().join("logs"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path(),
        )
        .unwrap();
        assert!(content.contains("permission"));
        assert!(!content.contains("secret-key"));
        assert!(!content.contains("private"));
    }

    #[test]
    fn repeated_errors_are_limited_and_summarized() {
        let root = tempdir().unwrap();
        let logger =
            DiagnosticLog::at_executable(Component::Gui, &root.path().join("todolist-desktop.exe"));
        for _ in 0..20 {
            logger.record(Record::new("read_failed", "load_workspace", "error").error("locked"));
        }
        assert!(logger.flush(Duration::from_secs(2)));
        let content = fs::read_to_string(
            fs::read_dir(root.path().join("logs"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path(),
        )
        .unwrap();
        assert_eq!(content.matches("\"event\":\"read_failed\"").count(), 1);
        assert!(content.contains("\"event\":\"error_repeats\""));
        assert!(content.contains("\"suppressed\":19"));
    }

    #[test]
    fn detail_mode_is_shared_across_processes_and_expires() {
        let root = tempdir().unwrap();
        let logger =
            DiagnosticLog::at_executable(Component::Gui, &root.path().join("todolist-desktop.exe"));
        assert!(
            logger
                .set_detailed(true)
                .unwrap()
                .detailed_seconds_remaining
                > 0
        );
        let mcp =
            DiagnosticLog::at_executable(Component::Mcp, &root.path().join("todolist-mcp.exe"));
        assert!(mcp.detailed());
        assert_eq!(mcp.status().directory, logger.status().directory);
        fs::write(
            root.path().join("logs").join(DETAIL_CONTROL_FILE),
            format!("{}\n", now_ms() - 1),
        )
        .unwrap();
        let mut state = mcp.inner.detail_state.lock().unwrap();
        state.checked_at = Instant::now() - DETAIL_REFRESH;
        drop(state);
        assert!(!mcp.detailed());
        assert_eq!(
            logger
                .set_detailed(false)
                .unwrap()
                .detailed_seconds_remaining,
            0
        );
        assert!(!logger.detailed());
    }
}
