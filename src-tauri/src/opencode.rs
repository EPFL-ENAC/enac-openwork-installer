use crate::types::{AppError, InstallResult, InstallStatus};
use crate::util::{self, Logger};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const INSTALL_SCRIPT_URL: &str = "https://opencode.ai/v2/install";
const RELEASE_BASE_URL: &str = "https://opencode.ai/update/api/latest/cli/opencode";
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(600);
const STATUS_TIMEOUT: Duration = Duration::from_secs(60);
const LIBC_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

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
        "linux" => install_linux(&log).await?,
        "windows" => install_windows(&log).await?,
        other => {
            return Err(AppError::new(
                "unsupported",
                format!("OpenCode V2 installs on macOS, Linux and Windows only, this host runs {other}"),
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

async fn run_install_script(log: &Logger) -> Result<Option<PathBuf>, AppError> {
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
            Some(path) => Ok(Some(path)),
            None => {
                log.log("the install script left no binary behind");
                Ok(None)
            }
        },
        Err(error) => {
            log.log(error.message);
            Ok(None)
        }
    }
}

async fn install_macos(log: &Logger) -> Result<PathBuf, AppError> {
    let installed = match run_install_script(log).await? {
        Some(path) => path,
        None => {
            log.log("falling back to the release zip");
            install_zip(&util::client()?, log).await?
        }
    };
    log_path_note(log, &installed);
    Ok(installed)
}

async fn install_linux(log: &Logger) -> Result<PathBuf, AppError> {
    let installed = match run_install_script(log).await? {
        Some(path) => path,
        None => {
            log.log("falling back to the Linux tarball");
            install_tar_gz(&util::client()?, log).await?
        }
    };
    log_path_note(log, &installed);
    Ok(installed)
}

async fn install_tar_gz(client: &reqwest::Client, log: &Logger) -> Result<PathBuf, AppError> {
    let arch = linux_arch(util::arch()).ok_or_else(|| {
        AppError::new(
            "unsupported",
            "no OpenCode release tarball exists for this architecture",
        )
    })?;
    let libc = detect_libc().await?;
    let asset = tar_asset_name(arch, libc);
    let dir = util::temp_dir("opencode-tarball")?;
    let archive = dir.join(&asset);
    log.log(format!("downloading {asset}"));
    let result = async {
        util::download_to(client, &tar_url(&asset), &archive, log).await?;
        let dest = home_install_dir()
            .ok_or_else(|| AppError::io("cannot find the user home directory"))?;
        log.log(format!("extracting the binary to {}", dest.display()));
        extract_tar_gz(&archive, &dest)
    }
    .await;
    util::remove_dir(&dir);
    result
}

async fn detect_libc() -> Result<&'static str, AppError> {
    util::spawn_blocking(|| {
        let alpine = Path::new("/etc/alpine-release").exists();
        let ldd = util::run_with_timeout(Path::new("ldd"), &["--version"], LIBC_PROBE_TIMEOUT)
            .ok()
            .map(|output| format!("{} {}", output.stdout, output.stderr));
        libc_flavor(alpine, ldd.as_deref())
    })
    .await
}

async fn install_zip(client: &reqwest::Client, log: &Logger) -> Result<PathBuf, AppError> {
    let asset = zip_asset_name(util::platform(), util::arch()).ok_or_else(|| {
        AppError::new(
            "unsupported",
            "no OpenCode release zip exists for this platform and architecture",
        )
    })?;
    log.log(format!("downloading {asset}"));
    let bytes = util::download_bytes(client, &release_url(&asset)).await?;
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

pub fn release_url(asset: &str) -> String {
    format!("{RELEASE_BASE_URL}/{asset}")
}

pub fn linux_arch(arch: &str) -> Option<&'static str> {
    match arch {
        "x86_64" => Some("x64"),
        "aarch64" => Some("arm64"),
        _ => None,
    }
}

pub fn libc_flavor(alpine_release: bool, ldd_version: Option<&str>) -> &'static str {
    let musl = alpine_release
        || ldd_version.is_some_and(|version| version.to_lowercase().contains("musl"));
    if musl {
        "musl"
    } else {
        "glibc"
    }
}

pub fn tar_asset_name(arch: &str, libc: &str) -> String {
    if libc == "musl" {
        format!("opencode-linux-{arch}-musl.tar.gz")
    } else {
        format!("opencode-linux-{arch}.tar.gz")
    }
}

pub fn tar_url(asset: &str) -> String {
    release_url(asset)
}

pub fn extract_tar_gz(archive_path: &Path, dest_dir: &Path) -> Result<PathBuf, AppError> {
    let dest = dest_dir.join(bin_name());
    if dest.exists() {
        return Err(AppError::io(format!(
            "{} already exists, it was not overwritten",
            dest.display()
        )));
    }
    let file = std::fs::File::open(archive_path)
        .map_err(|e| AppError::io(format!("cannot open {}: {e}", archive_path.display())))?;
    let reader = flate2::read::GzDecoder::new(std::io::BufReader::new(file));
    let mut tarball = tar::Archive::new(reader);
    let entries = tarball
        .entries()
        .map_err(|e| AppError::parse(format!("cannot read the OpenCode archive: {e}")))?;

    for entry in entries {
        let mut entry = entry
            .map_err(|e| AppError::parse(format!("cannot read an entry of the OpenCode archive: {e}")))?;
        let is_binary = entry
            .path()
            .map(|path| path.file_name().is_some_and(|name| name == bin_name()))
            .unwrap_or(false);
        if !is_binary || !entry.header().entry_type().is_file() {
            continue;
        }
        std::fs::create_dir_all(dest_dir)
            .map_err(|e| AppError::io(format!("cannot create {}: {e}", dest_dir.display())))?;
        return write_executable(&dest, &mut entry);
    }

    Err(AppError::parse(
        "the OpenCode archive holds no opencode binary",
    ))
}

pub fn path_note(binary: &Path, path_var: Option<&std::ffi::OsStr>) -> Option<String> {
    let dir = binary.parent()?;
    let on_path = path_var
        .is_some_and(|value| std::env::split_paths(value).any(|entry| entry == dir));
    (!on_path).then(|| {
        format!(
            "{} is not on PATH in this session, start it with the full path or open a new terminal",
            binary.display()
        )
    })
}

fn log_path_note(log: &Logger, binary: &Path) {
    if let Some(note) = path_note(binary, std::env::var_os("PATH").as_deref()) {
        log.log(note);
    }
}

fn write_executable(dest: &Path, payload: &mut impl std::io::Read) -> Result<PathBuf, AppError> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|e| AppError::io(format!("cannot create {}: {e}", dest.display())))?;
    std::io::copy(payload, &mut file)
        .map_err(|e| AppError::io(format!("cannot write {}: {e}", dest.display())))?;
    drop(file);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| AppError::io(format!("cannot make {} executable: {e}", dest.display())))?;
    }

    Ok(dest.to_path_buf())
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
    write_executable(&dest, &mut payload.as_slice())
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
            release_url("opencode-darwin-arm64.zip"),
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

    fn tar_gz_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        {
            let mut builder = tar::Builder::new(&mut gz);
            for (name, body) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_size(body.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                builder.append_data(&mut header, *name, *body).unwrap();
            }
            builder.finish().unwrap();
        }
        gz.finish().unwrap()
    }

    #[test]
    fn maps_linux_arch_to_tarball_asset_names() {
        assert_eq!(linux_arch("x86_64"), Some("x64"));
        assert_eq!(linux_arch("aarch64"), Some("arm64"));
        assert_eq!(linux_arch("armv7"), None);
        assert_eq!(tar_asset_name("x64", "glibc"), "opencode-linux-x64.tar.gz");
        assert_eq!(
            tar_asset_name("arm64", "musl"),
            "opencode-linux-arm64-musl.tar.gz"
        );
        assert_eq!(
            tar_url("opencode-linux-arm64-musl.tar.gz"),
            "https://opencode.ai/update/api/latest/cli/opencode/opencode-linux-arm64-musl.tar.gz"
        );
    }

    #[test]
    fn detects_musl_from_alpine_or_ldd() {
        let glibc = std::fs::read_to_string(fixture("ldd-version-glibc.txt")).unwrap();
        let musl = std::fs::read_to_string(fixture("ldd-version-musl.txt")).unwrap();
        assert_eq!(libc_flavor(false, Some(&glibc)), "glibc");
        assert_eq!(libc_flavor(false, Some(&musl)), "musl");
        assert_eq!(libc_flavor(true, Some(&glibc)), "musl");
        assert_eq!(libc_flavor(false, None), "glibc");
    }

    #[test]
    fn extracts_binary_from_nested_and_flat_tar_gz() {
        let dir = temp_dir("tar-fixture");
        let nested = dir.join("nested");
        let out = extract_tar_gz(&fixture("opencode-linux-x64.tar.gz"), &nested).unwrap();
        assert_eq!(out, nested.join(bin_name()));
        assert_eq!(std::fs::read(&out).unwrap(), b"#!/bin/sh\necho 2.2.3\n");

        let flat = dir.join("flat");
        let archive = dir.join("flat.tar.gz");
        std::fs::write(&archive, tar_gz_with(&[("opencode", b"root-bin")])).unwrap();
        let out = extract_tar_gz(&archive, &flat).unwrap();
        assert_eq!(out, flat.join(bin_name()));
        assert_eq!(std::fs::read(&out).unwrap(), b"root-bin");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&out).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111);
        }
    }

    #[test]
    fn tar_gz_extract_refuses_to_overwrite_and_rejects_missing_binary() {
        let dir = temp_dir("tar-existing");
        let target = dir.join(bin_name());
        std::fs::write(&target, b"keep").unwrap();
        let archive = dir.join("overwrite.tar.gz");
        std::fs::write(&archive, tar_gz_with(&[("opencode", b"new")])).unwrap();
        let err = extract_tar_gz(&archive, &dir).unwrap_err();
        assert_eq!(err.kind, "io");
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");

        let empty = dir.join("empty");
        let archive = dir.join("docs.tar.gz");
        std::fs::write(&archive, tar_gz_with(&[("package/README.md", b"docs")])).unwrap();
        let err = extract_tar_gz(&archive, &empty).unwrap_err();
        assert_eq!(err.kind, "parse");
        assert!(!empty.join(bin_name()).exists());
    }

    #[test]
    fn notes_a_binary_that_is_outside_the_current_path() {
        let binary = Path::new("/home/ada/.opencode/bin").join(bin_name());
        let on_path = std::ffi::OsStr::new("/usr/bin:/home/ada/.opencode/bin");
        assert_eq!(path_note(&binary, Some(on_path)), None);
        let off_path = std::ffi::OsStr::new("/usr/bin");
        let note = path_note(&binary, Some(off_path)).unwrap();
        assert!(note.contains(&binary.display().to_string()));
        assert!(path_note(&binary, None).is_some());
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
