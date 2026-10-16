use crate::core;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Argument {
    pub name: String,
    #[serde(default)]
    pub placeholder: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Command,
    Search,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_icon")]
    pub icon: String,
    pub command: Vec<String>,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub args: Vec<Argument>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(skip)]
    pub directory: PathBuf,
}
fn default_icon() -> String {
    "utilities-terminal-symbolic".into()
}
fn default_timeout() -> u64 {
    5000
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Copy {
        text: String,
    },
    Open {
        url: String,
    },
    Run {
        #[serde(default)]
        args: Vec<String>,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResultItem {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub action: Option<Action>,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Message {
    Result {
        #[serde(flatten)]
        item: ResultItem,
    },
    Results {
        items: Vec<ResultItem>,
    },
    Status {
        text: String,
    },
    Done,
}
#[derive(Debug)]
pub enum Event {
    Item(ResultItem),
    Replace(Vec<ResultItem>),
    Status(String),
    Done(Result<(), String>),
}
pub fn query(text: &str) -> Option<(&str, &str)> {
    let (head, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    if head != "ext" {
        return None;
    }
    Some(
        rest.trim_start()
            .split_once(char::is_whitespace)
            .map(|(id, args)| (id, args.trim()))
            .unwrap_or((rest.trim(), "")),
    )
}
pub fn discover() -> (Vec<Manifest>, Vec<String>) {
    discover_at(&core::data_dir().join("scripts"))
}
pub fn discover_at(root: &Path) -> (Vec<Manifest>, Vec<String>) {
    let mut files = Vec::new();
    for e in fs::read_dir(root).into_iter().flatten().flatten().take(256) {
        let path = e.path();
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            let path = path.join("manifest.json");
            if path.is_file() {
                files.push(path);
            }
        } else if path.extension().is_some_and(|s| s == "json") {
            files.push(path);
        }
    }
    files.sort();
    let mut manifests = Vec::new();
    let mut errors = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for path in files.into_iter().take(64) {
        let result: Result<Manifest, String> = (|| {
            if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 32768 {
                return Err("Manifest exceeds 32 KiB".into());
            }
            let mut manifest: Manifest =
                serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            manifest.directory = path
                .parent()
                .unwrap()
                .canonicalize()
                .map_err(|e| e.to_string())?;
            manifest.validate()?;
            Ok(manifest)
        })();
        match result {
            Ok(m) if seen.insert(m.id.clone()) => manifests.push(m),
            Ok(m) => errors.push(format!("Duplicate extension id: {}", m.id)),
            Err(e) => errors.push(format!("{}: {e}", path.display())),
        }
    }
    (manifests, errors)
}
impl Manifest {
    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("Only schema_version 1 is supported".into());
        }
        if self.id.is_empty()
            || self.id.len() > 64
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        {
            return Err(
                "id must contain 1–64 ASCII letters, digits, hyphens or underscores".into(),
            );
        }
        if self.name.is_empty()
            || self.name.len() > 160
            || self.description.len() > 1024
            || self.command.is_empty()
            || self.command.first().is_some_and(|s| s.is_empty())
            || self.command.len() > 32
            || self
                .command
                .iter()
                .any(|s| s.len() > 4096 || s.contains('\0'))
        {
            return Err("Invalid name, description, or command argument vector".into());
        }
        if !(100..=30000).contains(&self.timeout_ms) || self.args.len() > 16 {
            return Err("timeout_ms must be 100–30000; at most 16 arguments are supported".into());
        }
        if self.args.iter().any(|a| {
            a.name.is_empty()
                || a.name.len() > 80
                || a.default.len() > 4096
                || a.placeholder.len() > 160
        }) {
            return Err("Invalid argument definition".into());
        }
        Ok(())
    }
    pub fn arguments(&self, text: &str) -> Result<Vec<String>, String> {
        if self.mode == Mode::Search {
            return Ok(vec![text.into()]);
        }
        let args = split_args(text)?;
        self.validate_args(&args)?;
        Ok(args)
    }
    pub fn validate_args(&self, args: &[String]) -> Result<(), String> {
        if args.len() > 16 || args.iter().any(|s| s.len() > 4096 || s.contains('\0')) {
            return Err("At most 16 arguments of 4 KiB are supported".into());
        }
        if self.mode == Mode::Command
            && (args.len() > self.args.len()
                || self
                    .args
                    .iter()
                    .enumerate()
                    .any(|(i, a)| a.required && args.get(i).is_none_or(|s| s.is_empty())))
        {
            return Err(format!(
                "Arguments: {}",
                self.args
                    .iter()
                    .map(|a| format!("{}{}", a.name, if a.required { " (required)" } else { "" }))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        Ok(())
    }
    pub fn icon_path(&self, icon: &str) -> Option<PathBuf> {
        let path = self.directory.join(icon);
        let real = path.canonicalize().ok()?;
        (real.starts_with(&self.directory)
            && fs::metadata(&real).is_ok_and(|m| m.is_file() && m.len() <= 262144))
        .then_some(real)
    }
}
pub fn split_args(text: &str) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut started = false;
    for c in text.chars() {
        if escaped {
            word.push(c);
            escaped = false;
            started = true;
            continue;
        }
        if c == '\\' && quote != Some('\'') {
            escaped = true;
            started = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                word.push(c);
            }
            started = true;
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
            started = true;
        } else if c.is_whitespace() {
            if started {
                result.push(std::mem::take(&mut word));
                started = false;
            }
        } else {
            word.push(c);
            started = true;
        }
    }
    if escaped || quote.is_some() {
        return Err("Finish the quoted argument or escape".into());
    }
    if started {
        result.push(word);
    }
    Ok(result)
}
fn valid_result(item: &ResultItem) -> Result<(), String> {
    if item.id.is_empty()
        || item.id.len() > 128
        || item.title.is_empty()
        || item.title.len() > 1024
        || item.subtitle.len() > 8192
    {
        return Err("Invalid result id, title, or subtitle".into());
    }
    match &item.action {
        Some(Action::Copy { text }) if text.len() > 65536 || text.contains('\0') => {
            return Err("Copy result exceeds 64 KiB or contains NUL".into());
        }
        Some(Action::Open { url }) => {
            let uri = gtk::glib::Uri::parse(url, gtk::glib::UriFlags::NONE)
                .map_err(|_| "Invalid result URL")?;
            if url.len() > 8192 || !["https", "http", "file"].contains(&uri.scheme().as_str()) {
                return Err("Open results support only HTTP, HTTPS, and file URLs".into());
            }
        }
        Some(Action::Run { args })
            if args.len() > 16 || args.iter().any(|a| a.len() > 4096 || a.contains('\0')) =>
        {
            return Err("Invalid run arguments".into());
        }
        _ => {}
    }
    Ok(())
}
pub fn run(
    manifest: Manifest,
    args: Vec<String>,
    cancel: Arc<AtomicBool>,
    events: mpsc::Sender<Event>,
) {
    let result = run_inner(&manifest, &args, &cancel, &events);
    let _ = events.send(Event::Done(result));
}
fn run_inner(
    manifest: &Manifest,
    args: &[String],
    cancel: &AtomicBool,
    events: &mpsc::Sender<Event>,
) -> Result<(), String> {
    manifest.validate()?;
    manifest.validate_args(args)?;
    let exe = &manifest.command[0];
    let program = if !Path::new(exe).is_absolute() && exe.contains('/') {
        manifest.directory.join(exe)
    } else {
        PathBuf::from(exe)
    };
    let mut child = Command::new(program)
        .args(&manifest.command[1..])
        .args(args)
        .current_dir(&manifest.directory)
        .env("SPOTLIGHT_EXTENSION_API", "1")
        .env(
            "SPOTLIGHT_QUERY",
            if manifest.mode == Mode::Search {
                args.first().map(String::as_str).unwrap_or("")
            } else {
                ""
            },
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("Could not start {}: {e}", manifest.name))?;
    let group = child.id() as i32;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = mpsc::sync_channel(8);
    let (errtx, errrx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = Vec::new();
            match reader.by_ref().take(65537).read_until(b'\n', &mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut reader = BufReader::new(stderr);
        let mut buf = [0; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            let keep = n.min(8192usize.saturating_sub(bytes.len()));
            bytes.extend_from_slice(&buf[..keep]);
        }
        let _ = errtx.send(bytes);
    });
    let started = Instant::now();
    let mut total = 0;
    let mut count = 0;
    let mut done = false;
    let result = (|| {
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("Extension cancelled".into());
            }
            if started.elapsed() > Duration::from_millis(manifest.timeout_ms) {
                return Err("Extension timed out".into());
            }
            match rx.recv_timeout(Duration::from_millis(25)) {
                Ok(line) => {
                    total += line.len();
                    if total > 1_048_576 || line.len() > 65536 {
                        return Err("Extension output exceeded its limit".into());
                    }
                    if line.iter().all(u8::is_ascii_whitespace) {
                        continue;
                    }
                    let message: Message = serde_json::from_slice(&line)
                        .map_err(|e| format!("Invalid JSON result: {e}"))?;
                    match message {
                        Message::Result { item } => {
                            valid_result(&item)?;
                            count += 1;
                            if count > 100 {
                                return Err("Extension emitted more than 100 result updates".into());
                            }
                            events
                                .send(Event::Item(item))
                                .map_err(|_| "Extension view closed")?;
                        }
                        Message::Results { items } => {
                            if items.len() > 50 {
                                return Err("At most 50 visible results are supported".into());
                            }
                            for item in &items {
                                valid_result(item)?;
                            }
                            events
                                .send(Event::Replace(items))
                                .map_err(|_| "Extension view closed")?;
                        }
                        Message::Status { text } => {
                            if text.len() > 1024 {
                                return Err("Status text exceeds 1 KiB".into());
                            }
                            let _ = events.send(Event::Status(text));
                        }
                        Message::Done => {
                            done = true;
                            break;
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        Ok(())
    })();
    drop(rx);
    if result.is_err() || done {
        unsafe {
            kill(-group, 9);
        }
        let _ = child.wait();
        return result;
    }
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    let bytes = errrx
                        .recv_timeout(Duration::from_millis(100))
                        .unwrap_or_default();
                    return Err(format!(
                        "Extension exited with {status}: {}",
                        String::from_utf8_lossy(&bytes)
                    ));
                }
                break;
            }
            Ok(None)
                if started.elapsed() < Duration::from_millis(manifest.timeout_ms)
                    && !cancel.load(Ordering::Relaxed) =>
            {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                unsafe {
                    kill(-group, 9);
                }
                let _ = child.wait();
                return Err("Extension timed out or was cancelled".into());
            }
        }
    }
    Ok(())
}
unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_arguments_and_manifest_validation() {
        assert_eq!(
            split_args("one \"two words\" '$(no shell)' \"\"").unwrap(),
            vec!["one", "two words", "$(no shell)", ""]
        );
        assert!(split_args("'unfinished").is_err());
        let mut manifest: Manifest = serde_json::from_str(
            r#"{"schema_version":1,"id":"test","name":"Test","command":["./test.sh"]}"#,
        )
        .unwrap();
        assert!(manifest.validate().is_ok());
        manifest.schema_version = 2;
        assert!(manifest.validate().is_err());
    }
    #[test]
    fn streaming_and_cancellation() {
        let root = std::env::temp_dir().join(format!("spotlight-extension-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let script = root.join("fixture.sh");
        fs::write(&script,"printf '%s\\n' '{\"type\":\"result\",\"id\":\"one\",\"title\":\"First\",\"action\":{\"type\":\"copy\",\"text\":\"hello\"}}'\nsleep 2\n").unwrap();
        let manifest = Manifest {
            schema_version: 1,
            id: "test".into(),
            name: "Test".into(),
            description: String::new(),
            icon: default_icon(),
            command: vec!["sh".into(), "fixture.sh".into()],
            mode: Mode::Search,
            args: vec![],
            timeout_ms: 5000,
            directory: root.clone(),
        };
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker =
            std::thread::spawn(move || run(manifest, vec!["literal".into()], worker_cancel, tx));
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            Event::Item(_)
        ));
        cancel.store(true, Ordering::Relaxed);
        worker.join().unwrap();
        assert!(matches!(rx.recv().unwrap(), Event::Done(Err(_))));
        fs::remove_dir_all(root).unwrap();
    }
}
