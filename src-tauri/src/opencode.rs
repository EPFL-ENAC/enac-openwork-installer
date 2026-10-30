use crate::types::{AppError, InstallResult, InstallStatus};
use crate::util::{self, Logger};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const INSTALL_SCRIPT_URL: &str = "https://opencode.ai/v2/install";
const ZIP_BASE_URL: &str = "https://opencode.ai/update/api/latest/cli/opencode";
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(600);
const STATUS_TIMEOUT: Duration = Duration::from_secs(60);

#[tauri::command]
pub async fn opencode_status() -> Result<InstallStatus, AppError> {
    util::spawn_blocking(status_blocking).await
}

#[tauri::command]
pub async fn install_opencode(app: tauri::AppHandle) -> Result<InstallResult, AppError> {
    let log = Logger::new(&app, "opencode");

    log.log("checking for an existing OpenCode install");
    if let Some(path) = binary_path() {
        let version = version_at(&path).await;
        log.log(format!(
            "OpenCode is already installed at {}, nothing was changed",
            path.display()
        ));
        return Ok(InstallResult {
            version: version.unwrap_or_else(|| "unknown".to_string()),
            path: path.display().to_string(),
        });
    }

    let platform = util::platform();
    let installed = match platform {
        "macos" => install_macos(&log).await?,
        "windows" => install_windows(&log).await?,
        other => {
            return Err(AppError::new(
                "unsupported",
                format!("OpenCode V2 installs on macOS and Windows only, this host runs {other}"),
            ))
        }
    };

    let version = version_at(&installed).await.unwrap_or_else(|| "unknown".to_string());
    log.log(format!(
        "OpenCode {version} is installed at {}",
        installed.display()
    ));
    Ok(InstallResult {
        version,
        path: installed.display().to_string(),
    })
}

async fn install_macos(log: &Logger) -> Result<PathBuf, AppError> {
    let client = util::client()?;
    log.log("downloading the OpenCode install script");
    let script = util::fetch_text(&client, INSTALL_SCRIPT_URL).await?;
    let dir = util::temp_dir("opencode-script")?;
    let script_path = dir.join("install.sh");
    std::fs::write(&script_path, script)
        .map_err(|e| AppError::io(format!("cannot write {}: {e}", script_path.display())))?;
    log.log("running the OpenCode install script");
    let script_arg = script_path.display().to_string();
    let result = util::run_checked(Path::new("bash"), &[&script_arg], SCRIPT_TIMEOUT).await;
    util::remove_dir(&dir);
    match result {
        Ok(_) => match binary_path() {
            Some(path) => Ok(path),
            None => {
                log.log("the install script left no binary, falling back to the release zip");
                install_zip(&client, log).await
            }
        },
        Err(error) => {
            log.log(format!("{}; falling back to the release zip", error.message));
            install_zip(&client, log).await
        }
    }
}

async fn install_zip(client: &reqwest::Client, log: &Logger) -> Result<PathBuf, AppError> {
    let asset = zip_asset_name(util::platform(), util::arch()).ok_or_else(|| {
        AppError::new(
            "unsupported",
            "no OpenCode release zip exists for this platform and architecture",
        )
    })?;
    log.log(format!("downloading {asset}"));
    let bytes = util::download_bytes(client, &zip_url(&asset)).await?;
    let dir = home_install_dir()
        .ok_or_else(|| AppError::io("cannot find the user home directory"))?;
    log.log(format!("extracting the binary to {}", dir.display()));
    extract_binary(&bytes, &dir)
}

async fn install_windows(log: &Logger) -> Result<PathBuf, AppError> {
    let client = util::client()?;
    let binary = install_zip(&client, log).await?;
    let dir = home_install_dir()
        .ok_or_else(|| AppError::io("cannot find the user home directory"))?;
    log.log("adding the OpenCode directory to the user PATH");
    if set_user_path(&dir.display().to_string())? {
        log.log("user PATH updated, new terminals find opencode");
    } else {
        log.log("user PATH already contains the OpenCode directory");
    }
    Ok(binary)
}

fn status_blocking() -> InstallStatus {
    let Some(path) = binary_path() else {
        return InstallStatus {
            installed: false,
            version: None,
            path: None,
        };
    };
    let version = util::run_with_timeout(&path, &["--version"], STATUS_TIMEOUT)
        .ok()
        .and_then(|output| {
            parse_version(&output.stdout).or_else(|| parse_version(&output.stderr))
        });
    InstallStatus {
        installed: true,
        version,
        path: Some(path.display().to_string()),
    }
}

pub async fn version_at(binary: &Path) -> Option<String> {
    let output = util::run(binary, &["--version"], STATUS_TIMEOUT).await.ok()?;
    parse_version(&output.stdout).or_else(|| parse_version(&output.stderr))
}

pub fn parse_version(output: &str) -> Option<String> {
    output.split_whitespace().find_map(|token| {
        let candidate = token
            .trim_start_matches(['v', 'V'])
            .trim_end_matches(['.', ',', ')', ']', ';']);
        let first_digit = candidate.starts_with(|c: char| c.is_ascii_digit());
        let version_like = candidate.contains('.')
            && candidate
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '~'));
        (first_digit && version_like).then(|| candidate.to_string())
    })
}

pub fn bin_name() -> &'static str {
    if cfg!(windows) {
        "opencode.exe"
    } else {
        "opencode"
    }
}

pub fn home_install_dir() -> Option<PathBuf> {
    util::home_dir().map(|home| home.join(".opencode").join("bin"))
}

pub fn binary_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            let candidate = dir.join(bin_name());
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let fallback = home_install_dir()?.join(bin_name());
    fallback.is_file().then_some(fallback)
}

pub fn zip_asset_name(platform: &str, arch: &str) -> Option<String> {
    let name = match (platform, arch) {
        ("macos", "aarch64") => "opencode-darwin-arm64.zip",
        ("macos", "x86_64") => "opencode-darwin-x64.zip",
        ("windows", "x86_64") => "opencode-windows-x64.zip",
        _ => return None,
    };
    Some(name.to_string())
}

pub fn zip_url(asset: &str) -> String {
    format!("{ZIP_BASE_URL}/{asset}")
}

pub fn extract_binary(zip_bytes: &[u8], dest_dir: &Path) -> Result<PathBuf, AppError> {
    let dest = dest_dir.join(bin_name());
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))
        .map_err(|e| AppError::parse(format!("cannot read the OpenCode archive: {e}")))?;
    let mut payload: Option<Vec<u8>> = None;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| AppError::parse(format!("cannot read an entry of the OpenCode archive: {e}")))?;
        let is_binary = {
            let name = entry
                .name()
                .map_err(|e| AppError::parse(format!("cannot read an entry name: {e}")))?;
            name.rsplit('/')
                .next()
                .is_some_and(|name| name == "opencode" || name == "opencode.exe")
        };
        if !is_binary || !entry.is_file() {
            continue;
        }
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes)
            .map_err(|e| AppError::parse(format!("cannot unpack the OpenCode binary: {e}")))?;
        payload = Some(bytes);
    }

    let payload = payload
        .ok_or_else(|| AppError::parse("the OpenCode archive holds no opencode binary"))?;
    if dest.exists() {
        return Err(AppError::io(format!(
            "{} already exists, it was not overwritten",
            dest.display()
        )));
    }
    std::fs::create_dir_all(dest_dir)
        .map_err(|e| AppError::io(format!("cannot create {}: {e}", dest_dir.display())))?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&dest)
        .map_err(|e| AppError::io(format!("cannot create {}: {e}", dest.display())))?;
    std::io::Write::write_all(&mut file, &payload)
        .map_err(|e| AppError::io(format!("cannot write {}: {e}", dest.display())))?;
    drop(file);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| AppError::io(format!("cannot make {} executable: {e}", dest.display())))?;
    }

    Ok(dest)
}

#[cfg(any(windows, test))]
pub fn append_to_user_path(current: Option<&str>, dir: &str) -> Option<String> {
    let wanted = normalize_path_entry(dir);
    let existing = current.unwrap_or("");
    let present = existing
        .split(';')
        .any(|entry| !entry.trim().is_empty() && normalize_path_entry(entry) == wanted);
    if present {
        return None;
    }
    let base = existing.trim_end_matches([';', ' ']);
    if base.is_empty() {
        Some(dir.to_string())
    } else {
        Some(format!("{base};{dir}"))
    }
}

#[cfg(any(windows, test))]
fn normalize_path_entry(entry: &str) -> String {
    entry
        .trim()
        .trim_end_matches(['\\', '/'])
        .to_lowercase()
}

#[cfg(windows)]
pub fn set_user_path(dir: &str) -> Result<bool, AppError> {
    use winreg::enums::{RegType, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::{RegKey, RegValue};

    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
        .map_err(|e| AppError::io(format!("cannot open HKCU\\Environment: {e}")))?;
    let raw = key.get_raw_value("Path").ok();
    let (current, vtype) = match &raw {
        Some(value) => (Some(decode_reg_string(value)?), value.vtype.clone()),
        None => (None, RegType::REG_EXPAND_SZ),
    };
    let Some(updated) = append_to_user_path(current.as_deref(), dir) else {
        return Ok(false);
    };
    let mut bytes: Vec<u8> = updated
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    bytes.extend([0, 0]);
    key.set_raw_value("Path", &RegValue { bytes: bytes.into(), vtype })
        .map_err(|e| AppError::io(format!("cannot write HKCU\\Environment\\Path: {e}")))?;
    broadcast_environment_change();
    Ok(true)
}

#[cfg(windows)]
fn decode_reg_string(value: &winreg::RegValue) -> Result<String, AppError> {
    use winreg::enums::RegType;
    if !matches!(value.vtype, RegType::REG_SZ | RegType::REG_EXPAND_SZ) {
        return Err(AppError::parse(
            "HKCU\\Environment\\Path is not a string value",
        ));
    }
    let units: Vec<u16> = value
        .bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16(&units)
        .map_err(|e| AppError::parse(format!("HKCU\\Environment\\Path is not valid UTF-16: {e}")))
}

#[cfg(windows)]
fn broadcast_environment_change() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    let payload: Vec<u16> = "Environment\0".encode_utf16().collect();
    let mut result = 0usize;
    // SAFETY: the payload outlives the call and the other arguments are plain values.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            payload.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            5_000,
            &mut result,
        );
    }
}

#[cfg(not(windows))]
pub fn set_user_path(_dir: &str) -> Result<bool, AppError> {
    Err(AppError::new(
        "unsupported",
        "the user PATH registry value exists on Windows only",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name)
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("opencode-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            for (name, body) in entries {
                writer
                    .start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(body).unwrap();
            }
            writer.finish().unwrap();
        }
        buf
    }

    #[test]
    fn parses_version_output() {
        let recorded = std::fs::read_to_string(fixture("opencode-version.txt")).unwrap();
        assert_eq!(parse_version(&recorded).as_deref(), Some("2.2.3"));
        assert_eq!(parse_version("opencode v2.2.3\n").as_deref(), Some("2.2.3"));
        assert_eq!(parse_version("1.19.5\n").as_deref(), Some("1.19.5"));
        assert_eq!(parse_version("0.18.3-beta.4").as_deref(), Some("0.18.3-beta.4"));
        assert_eq!(parse_version("build from source, no version"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn selects_zip_asset_per_platform_and_arch() {
        assert_eq!(
            zip_asset_name("macos", "aarch64").as_deref(),
            Some("opencode-darwin-arm64.zip")
        );
        assert_eq!(
            zip_asset_name("macos", "x86_64").as_deref(),
            Some("opencode-darwin-x64.zip")
        );
        assert_eq!(
            zip_asset_name("windows", "x86_64").as_deref(),
            Some("opencode-windows-x64.zip")
        );
        assert_eq!(zip_asset_name("windows", "aarch64"), None);
        assert_eq!(zip_asset_name("linux", "x86_64"), None);
        assert_eq!(
            zip_url("opencode-darwin-arm64.zip"),
            "https://opencode.ai/update/api/latest/cli/opencode/opencode-darwin-arm64.zip"
        );
    }

    #[test]
    fn extracts_binary_from_flat_and_nested_zip() {
        let flat = temp_dir("flat");
        let zip = zip_with(&[("opencode", b"#!/bin/sh\n")]);
        let out = extract_binary(&zip, &flat).unwrap();
        assert_eq!(out, flat.join(bin_name()));
        assert_eq!(std::fs::read(&out).unwrap(), b"#!/bin/sh\n");

        let nested = temp_dir("nested");
        let zip = zip_with(&[("package/README.md", b"docs"), ("package/bin/opencode", b"bin")]);
        let out = extract_binary(&zip, &nested).unwrap();
        assert_eq!(out, nested.join(bin_name()));
        assert_eq!(std::fs::read(&out).unwrap(), b"bin");
    }

    #[test]
    fn extract_refuses_to_overwrite_and_rejects_missing_binary() {
        let dir = temp_dir("existing");
        let target = dir.join(bin_name());
        std::fs::write(&target, b"keep").unwrap();
        let zip = zip_with(&[("opencode", b"new")]);
        let err = extract_binary(&zip, &dir).unwrap_err();
        assert_eq!(err.kind, "io");
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");

        let zip = zip_with(&[("package/README.md", b"docs")]);
        let err = extract_binary(&zip, &dir).unwrap_err();
        assert_eq!(err.kind, "parse");
    }

    #[test]
    fn builds_user_path_value() {
        assert_eq!(
            append_to_user_path(Some(r"C:\Users\a;D:\tools"), r"C:\Users\a\.opencode\bin").as_deref(),
            Some(r"C:\Users\a;D:\tools;C:\Users\a\.opencode\bin")
        );
        assert_eq!(
            append_to_user_path(None, r"C:\Users\a\.opencode\bin").as_deref(),
            Some(r"C:\Users\a\.opencode\bin")
        );
        assert_eq!(
            append_to_user_path(Some(r"C:\Users\a; d:\TOOLS\;;"), r"D:\tools"),
            None
        );
        assert_eq!(
            append_to_user_path(Some(r"C:\Users\a\.opencode\bin\"), r"C:\Users\a\.opencode\bin"),
            None
        );
    }
}
