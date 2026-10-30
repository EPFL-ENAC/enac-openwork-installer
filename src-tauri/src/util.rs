use crate::types::{AppError, BackendLog};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::Emitter;

pub struct Logger {
    app: Option<tauri::AppHandle>,
    task: &'static str,
}

impl Logger {
    pub fn new(app: &tauri::AppHandle, task: &'static str) -> Self {
        Self {
            app: Some(app.clone()),
            task,
        }
    }

    pub fn log(&self, message: impl Into<String>) {
        let Some(app) = &self.app else {
            return;
        };
        let payload = BackendLog {
            task: self.task.to_string(),
            message: message.into(),
        };
        let _ = app.emit("backend-log", payload);
    }
}

pub fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

pub fn arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86_64"
    }
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

pub fn temp_dir(tag: &str) -> Result<PathBuf, AppError> {
    let dir = std::env::temp_dir().join(format!("epfl-openwork-setup-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::io(format!("cannot create {}: {e}", dir.display())))?;
    Ok(dir)
}

pub fn remove_dir(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}

pub fn client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .user_agent(concat!("EPFL-OpenWork-Setup/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| AppError::network(format!("cannot create the HTTP client: {e}")))
}

pub async fn fetch_text(client: &reqwest::Client, url: &str) -> Result<String, AppError> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::network(format!("GET {url} failed: {e}")))?;
    if !response.status().is_success() {
        return Err(AppError::network(format!(
            "GET {url} returned HTTP {}",
            response.status()
        )));
    }
    response
        .text()
        .await
        .map_err(|e| AppError::network(format!("GET {url} failed to read the body: {e}")))
}

pub async fn download_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, AppError> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::network(format!("GET {url} failed: {e}")))?;
    if !response.status().is_success() {
        return Err(AppError::network(format!(
            "GET {url} returned HTTP {}",
            response.status()
        )));
    }
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|e| AppError::network(format!("GET {url} failed to read the body: {e}")))
}

pub async fn download_to(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    log: &Logger,
) -> Result<(), AppError> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::network(format!("GET {url} failed: {e}")))?;
    if !response.status().is_success() {
        return Err(AppError::network(format!(
            "GET {url} returned HTTP {}",
            response.status()
        )));
    }
    let total = response.content_length().unwrap_or(0);
    let mut file = std::fs::File::create(dest)
        .map_err(|e| AppError::io(format!("cannot create {}: {e}", dest.display())))?;
    let mut written = 0u64;
    let mut next_report = 0u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| AppError::network(format!("GET {url} was interrupted: {e}")))?
    {
        file.write_all(&chunk)
            .map_err(|e| AppError::io(format!("cannot write {}: {e}", dest.display())))?;
        written += chunk.len() as u64;
        if total > 0 && written >= next_report {
            log.log(format!(
                "downloaded {} of {} MB",
                written / 1_000_000,
                total / 1_000_000
            ));
            next_report = written + total / 5;
        }
    }
    file.flush()
        .map_err(|e| AppError::io(format!("cannot write {}: {e}", dest.display())))?;
    Ok(())
}

pub async fn spawn_blocking<T, F>(work: F) -> Result<T, AppError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| AppError::io(format!("a background task failed: {e}")))
}

pub async fn run(
    program: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<CommandOutput, AppError> {
    let program = program.to_path_buf();
    let owned: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
    let display = program.display().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let refs: Vec<&str> = owned.iter().map(String::as_str).collect();
        run_with_timeout(&program, &refs, timeout)
    })
    .await
    .map_err(|e| AppError::io(format!("cannot run {display}: {e}")))?
}

pub async fn run_checked(
    program: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<CommandOutput, AppError> {
    let output = run(program, args, timeout).await?;
    if output.success {
        return Ok(output);
    }
    let detail = output
        .stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .or_else(|| output.stdout.lines().map(str::trim).find(|line| !line.is_empty()))
        .unwrap_or("no output");
    Err(AppError::io(format!(
        "{} failed: {detail}",
        program.display()
    )))
}

pub struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub fn run_with_timeout(
    program: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<CommandOutput, AppError> {
    let dir = temp_dir("run")?;
    let out_path = dir.join("stdout");
    let err_path = dir.join("stderr");
    let out = std::fs::File::create(&out_path)
        .map_err(|e| AppError::io(format!("cannot create {}: {e}", out_path.display())))?;
    let err = std::fs::File::create(&err_path)
        .map_err(|e| AppError::io(format!("cannot create {}: {e}", err_path.display())))?;

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
        .map_err(|e| AppError::io(format!("cannot run {}: {e}", program.display())))?;

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                remove_dir(&dir);
                return Err(AppError::io(format!(
                    "{} did not finish within {} seconds",
                    program.display(),
                    timeout.as_secs()
                )));
            }
            Err(e) => {
                remove_dir(&dir);
                return Err(AppError::io(format!(
                    "cannot wait for {}: {e}",
                    program.display()
                )));
            }
        }
    };

    let stdout = std::fs::read_to_string(&out_path).unwrap_or_default();
    let stderr = std::fs::read_to_string(&err_path).unwrap_or_default();
    remove_dir(&dir);
    Ok(CommandOutput {
        success: status.success(),
        stdout,
        stderr,
    })
}
