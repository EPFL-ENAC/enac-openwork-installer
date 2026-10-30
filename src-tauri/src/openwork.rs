use crate::types::{AppError, InstallResult, InstallStatus};
use crate::util::{self, Logger};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub const RELEASE_URL: &str = "https://api.github.com/repos/different-ai/openwork/releases/latest";
const INSTALL_TIMEOUT: Duration = Duration::from_secs(900);
const SHORT_TIMEOUT: Duration = Duration::from_secs(120);
#[cfg(any(target_os = "macos", test))]
const OPENWORK_APP: &str = "OpenWork.app";
const OPENWORK_EXE: &str = "OpenWork.exe";
#[cfg(any(target_os = "linux", test))]
const OPENWORK_APPIMAGE: &str = "OpenWork.AppImage";
#[cfg(any(target_os = "linux", test))]
pub const OPENWORK_DESKTOP_NAME: &str = "openwork.desktop";
#[cfg(any(windows, test))]
const OPENWORK_PRODUCT_NAME: &str = "openwork";
/// Install directory name of the electron-builder NSIS per-user build.
#[cfg(any(windows, test))]
const OPENWORK_PROGRAMS_DIR: &str = "openwork";

/// One entry of the HKCU uninstall registry key, as far as detection needs it.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg(any(windows, test))]
pub struct UninstallEntry {
    pub display_name: String,
    pub publisher: Option<String>,
    pub install_location: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub version: String,
    pub assets: Vec<Asset>,
}

#[derive(serde::Deserialize)]
struct RawRelease {
    tag_name: String,
    assets: Vec<RawAsset>,
}

#[derive(serde::Deserialize)]
struct RawAsset {
    name: String,
    size: u64,
    browser_download_url: String,
}

#[tauri::command]
pub async fn openwork_status() -> Result<InstallStatus, AppError> {
    util::spawn_blocking(status_blocking).await
}

#[tauri::command]
pub async fn install_openwork(app: tauri::AppHandle) -> Result<InstallResult, AppError> {
    let log = Logger::new(&app, "openwork");

    log.log("checking for an existing OpenWork install");
    let existing = util::spawn_blocking(status_blocking).await?;
    if existing.installed {
        let version = existing.version.unwrap_or_else(|| "unknown".to_string());
        let path = existing.path.unwrap_or_else(|| "unknown".to_string());
        log.log(format!(
            "OpenWork {version} is already installed at {path}, nothing was changed"
        ));
        return Ok(InstallResult { version, path });
    }

    let platform = util::platform();
    let arch = util::arch();
    log.log("resolving the latest OpenWork release");
    let client = util::client()?;
    let release = parse_release(&util::fetch_text(&client, RELEASE_URL).await?)?;
    log.log(format!("latest OpenWork release is {}", release.version));
    let asset = select_asset(&release.assets, platform, arch).ok_or_else(|| {
        AppError::new(
            "unsupported",
            format!("OpenWork publishes no installer for {platform} {arch}"),
        )
    })?;

    let dir = util::temp_dir("openwork")?;
    let outcome = install_into(&client, &release, asset, &dir, &log).await;
    util::remove_dir(&dir);
    outcome?;

    let installed = util::spawn_blocking(status_blocking).await?;
    match (installed.installed, installed.path) {
        (true, Some(path)) => {
            let version = installed.version.unwrap_or_else(|| release.version.clone());
            log.log(format!("OpenWork {version} is installed at {path}"));
            Ok(InstallResult { version, path })
        }
        _ => Err(AppError::io(
            "the OpenWork installer finished but no installation was found",
        )),
    }
}

#[tauri::command]
pub async fn launch_openwork() -> Result<(), AppError> {
    util::spawn_blocking(launch_blocking).await?
}

fn launch_blocking() -> Result<(), AppError> {
    let status = status_blocking();
    if !status.installed {
        return Err(AppError::new("unsupported", "OpenWork is not installed"));
    }
    match util::platform() {
        "macos" => {
            let target = status
                .path
                .unwrap_or_else(|| "/Applications/OpenWork.app".to_string());
            Command::new("open")
                .arg(target)
                .spawn()
                .map_err(|e| AppError::io(format!("cannot launch OpenWork: {e}")))?;
            Ok(())
        }
        "windows" => {
            let path = status
                .path
                .ok_or_else(|| AppError::new("unsupported", "OpenWork is not installed"))?;
            Command::new(windows_launch_target(&path))
                .spawn()
                .map_err(|e| AppError::io(format!("cannot launch OpenWork: {e}")))?;
            Ok(())
        }
        "linux" => {
            let path = status
                .path
                .ok_or_else(|| AppError::new("unsupported", "OpenWork is not installed"))?;
            Command::new(path)
                .spawn()
                .map_err(|e| AppError::io(format!("cannot launch OpenWork: {e}")))?;
            Ok(())
        }
        other => Err(AppError::new(
            "unsupported",
            format!("cannot launch OpenWork on {other}"),
        )),
    }
}

fn windows_launch_target(path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("exe") => path,
        _ => path.join(OPENWORK_EXE),
    }
}

async fn install_into(
    client: &reqwest::Client,
    release: &Release,
    asset: &Asset,
    dir: &Path,
    log: &Logger,
) -> Result<(), AppError> {
    let platform = util::platform();
    let installer = dir.join(
        Path::new(&asset.name)
            .file_name()
            .unwrap_or(asset.name.as_ref()),
    );
    log.log(format!(
        "downloading {} ({} MB)",
        asset.name,
        asset.size / 1_000_000
    ));
    util::download_to(client, &asset.url, &installer, log).await?;

    log.log("verifying the download");
    let metadata = update_metadata_name(platform, util::arch());
    match release.assets.iter().find(|item| item.name == metadata) {
        Some(published) => {
            let entries = parse_latest_yml(&util::fetch_text(client, &published.url).await?);
            match expected_sha512(&entries, &asset.name) {
                Some(expected) => verify_file_sha512(&installer, expected)?,
                None => log.log(format!("{} lists no sha512 for this asset, skipped", metadata)),
            }
        }
        None => log.log(format!("the release holds no {metadata}, checksum skipped")),
    }

    match platform {
        "macos" => install_dmg(&installer, log).await,
        "windows" => install_nsis(&installer, log).await,
        "linux" => install_appimage(&installer, &release.version, log).await,
        other => Err(AppError::new(
            "unsupported",
            format!("OpenWork installs on macOS, Windows and Linux only, this host runs {other}"),
        )),
    }
}

async fn install_dmg(dmg: &Path, log: &Logger) -> Result<(), AppError> {
    let mount = util::temp_dir("openwork-mount")?;
    let mount_path = mount.display().to_string();
    let dmg_path = dmg.display().to_string();
    log.log("mounting the OpenWork disk image");
    util::run_checked(
        Path::new("hdiutil"),
        &[
            "attach",
            &dmg_path,
            "-nobrowse",
            "-noverify",
            "-noautoopen",
            "-quiet",
            "-mountpoint",
            &mount_path,
        ],
        SHORT_TIMEOUT,
    )
    .await?;

    let target = Path::new("/Applications/OpenWork.app");
    let copied = if target.exists() {
        Err(AppError::new(
            "conflict",
            "/Applications/OpenWork.app already exists, it was not overwritten",
        ))
    } else {
        log.log("copying OpenWork.app to /Applications");
        let source = mount.join("OpenWork.app");
        util::run_checked(
            Path::new("ditto"),
            &[&source.display().to_string(), "/Applications/OpenWork.app"],
            INSTALL_TIMEOUT,
        )
        .await
        .map(|_| ())
    };

    log.log("detaching the disk image");
    let _ = util::run(Path::new("hdiutil"), &["detach", &mount_path, "-quiet"], SHORT_TIMEOUT).await;
    util::remove_dir(&mount);
    copied
}

async fn install_nsis(installer: &Path, log: &Logger) -> Result<(), AppError> {
    log.log("running the OpenWork installer, this can take a few minutes");
    util::run_checked(installer, &["/S"], INSTALL_TIMEOUT)
        .await
        .map(|_| ())
}

fn status_blocking() -> InstallStatus {
    #[cfg(target_os = "macos")]
    if let Some(status) = macos_status_from(Path::new("/Applications")) {
        return status;
    }
    #[cfg(windows)]
    if let Some(status) = windows_status() {
        return status;
    }
    #[cfg(target_os = "linux")]
    if let Some(status) = util::home_dir().and_then(|home| linux_status_from(&home)) {
        return status;
    }
    InstallStatus {
        installed: false,
        version: None,
        path: None,
    }
}

#[cfg(any(target_os = "macos", test))]
fn macos_status_from(apps: &Path) -> Option<InstallStatus> {
    let app = apps.join(OPENWORK_APP);
    if !app.is_dir() {
        return None;
    }
    let version = std::fs::read_to_string(app.join("Contents/Info.plist"))
        .ok()
        .and_then(|plist| plist_string_value(&plist, "CFBundleShortVersionString"));
    Some(InstallStatus {
        installed: true,
        version,
        path: Some(app.display().to_string()),
    })
}

#[cfg(any(windows, test))]
fn normalized(value: &str) -> String {
    value.trim().to_lowercase()
}

/// Real OpenWork only: the exact product name from its own publisher. Our own
/// entries ("OpenWork Installer", "EPFL OpenWork Installer") and any name that
/// merely contains "openwork" fail the exact match, so they never count.
#[cfg(any(windows, test))]
pub fn is_openwork_uninstall_entry(entry: &UninstallEntry) -> bool {
    if normalized(&entry.display_name) != OPENWORK_PRODUCT_NAME {
        return false;
    }
    entry
        .publisher
        .as_deref()
        .is_some_and(|publisher| normalized(publisher).contains("different-ai"))
}

/// Detection shared by the tests and the Windows status check: the default
/// electron-builder install dir first, then the install location of a real
/// OpenWork uninstall entry. A path only counts when the exe is there.
#[cfg(any(windows, test))]
pub fn windows_status_from(
    programs_dir: Option<&Path>,
    entries: &[UninstallEntry],
) -> Option<InstallStatus> {
    let entry = entries.iter().find(|entry| is_openwork_uninstall_entry(entry));
    if let Some(exe) = programs_dir.map(|dir| dir.join(OPENWORK_PROGRAMS_DIR).join(OPENWORK_EXE)) {
        if exe.is_file() {
            return Some(installed_at(&exe, entry.and_then(|entry| entry.version.clone())));
        }
    }
    let dir = entry?.install_location.as_deref().map(str::trim)?;
    if dir.is_empty() {
        return None;
    }
    let exe = windows_launch_target(dir);
    exe.is_file()
        .then(|| installed_at(&exe, entry.and_then(|entry| entry.version.clone())))
}

#[cfg(any(windows, target_os = "linux", test))]
fn installed_at(exe: &Path, version: Option<String>) -> InstallStatus {
    InstallStatus {
        installed: true,
        version,
        path: Some(exe.display().to_string()),
    }
}

#[cfg(windows)]
fn windows_status() -> Option<InstallStatus> {
    let programs = std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("Programs"));
    windows_status_from(programs.as_deref(), &registry_entries())
}

#[cfg(windows)]
fn registry_entries() -> Vec<UninstallEntry> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    let Ok(uninstall) = winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Uninstall", KEY_READ)
    else {
        return Vec::new();
    };
    uninstall
        .enum_keys()
        .flatten()
        .filter_map(|name| uninstall.open_subkey_with_flags(&name, KEY_READ).ok())
        .map(|key| UninstallEntry {
            display_name: key.get_value("DisplayName").unwrap_or_default(),
            publisher: key.get_value("Publisher").ok(),
            install_location: key.get_value("InstallLocation").ok(),
            version: key.get_value("DisplayVersion").ok(),
        })
        .collect()
}

/// Managed Linux install: one AppImage plus a version sidecar in the user data
/// directory, so a re-run replaces the file instead of adding a second one.
#[cfg(any(target_os = "linux", test))]
pub fn linux_install_dir(home: &Path) -> PathBuf {
    home.join(".local/share/openwork")
}

#[cfg(any(target_os = "linux", test))]
pub fn linux_appimage_path(home: &Path) -> PathBuf {
    linux_install_dir(home).join(OPENWORK_APPIMAGE)
}

/// Real OpenWork only: the managed AppImage, or an openwork.desktop entry that
/// points at an existing AppImage. Our own entry carries a different file name
/// ("EPFL OpenWork Installer.desktop"), so it never counts.
#[cfg(any(target_os = "linux", test))]
pub fn linux_status_from(home: &Path) -> Option<InstallStatus> {
    let appimage = linux_appimage_path(home);
    if appimage.is_file() {
        return Some(installed_at(&appimage, read_version(&appimage)));
    }
    let entry = home
        .join(".local/share/applications")
        .join(OPENWORK_DESKTOP_NAME);
    let target = PathBuf::from(desktop_exec(&std::fs::read_to_string(entry).ok()?)?);
    (is_appimage(&target) && target.is_file()).then(|| installed_at(&target, read_version(&target)))
}

#[cfg(any(target_os = "linux", test))]
fn is_appimage(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("appimage"))
}

/// The AppImage carries no cheaply readable version, so install writes one in a
/// sidecar file next to it.
#[cfg(any(target_os = "linux", test))]
fn read_version(appimage: &Path) -> Option<String> {
    let text = std::fs::read_to_string(appimage.with_extension("version")).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The executable of a desktop entry, unquoted. A quoted value wins because a
/// home directory can hold spaces.
#[cfg(any(target_os = "linux", test))]
pub fn desktop_exec(text: &str) -> Option<String> {
    let value = text
        .lines()
        .find_map(|line| line.strip_prefix("Exec="))?
        .trim();
    let path = match value.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or(rest),
        None => value.split_whitespace().next().unwrap_or(value),
    };
    (!path.is_empty()).then(|| path.to_string())
}

#[cfg(any(target_os = "linux", test))]
pub fn desktop_entry(exec: &Path, icon: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=OpenWork\n\
         Exec=\"{}\"\n\
         Icon={icon}\n\
         Terminal=false\n\
         Categories=Development;\n",
        exec.display()
    )
}

#[cfg(any(target_os = "linux", test))]
pub fn pick_icon(bundled: Option<PathBuf>, extracted: Option<PathBuf>) -> String {
    bundled
        .or(extracted)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "application-x-executable".to_string())
}

#[cfg(target_os = "linux")]
async fn install_appimage(appimage: &Path, version: &str, log: &Logger) -> Result<(), AppError> {
    let home = util::home_dir()
        .ok_or_else(|| AppError::io("cannot find the home directory of the user"))?;
    let dir = linux_install_dir(&home);
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::io(format!("cannot create {}: {e}", dir.display())))?;

    let target = linux_appimage_path(&home);
    log.log(format!(
        "installing the OpenWork AppImage to {}",
        target.display()
    ));
    // Copy, not rename: the download lives in /tmp, usually another filesystem.
    let _ = std::fs::remove_file(&target);
    std::fs::copy(appimage, &target)
        .map_err(|e| AppError::io(format!("cannot write {}: {e}", target.display())))?;
    set_executable(&target)?;
    write_version(&target, version)?;

    // No OpenWork icon ships as a bundled resource yet, so the AppImage is the
    // only source. Pass the resource path as `bundled` when the bundler copies
    // one next to the binary.
    let icon_source = target.clone();
    let icon_dir = dir.clone();
    let extracted = util::spawn_blocking(move || extract_icon(&icon_source, &icon_dir))
        .await
        .ok()
        .flatten();
    let icon = pick_icon(None, extracted);

    let applications = home.join(".local/share/applications");
    std::fs::create_dir_all(&applications)
        .map_err(|e| AppError::io(format!("cannot create {}: {e}", applications.display())))?;
    let entry = applications.join(OPENWORK_DESKTOP_NAME);
    std::fs::write(&entry, desktop_entry(&target, &icon))
        .map_err(|e| AppError::io(format!("cannot write {}: {e}", entry.display())))?;
    log.log(format!(
        "added the OpenWork menu entry at {}",
        entry.display()
    ));
    Ok(())
}

#[cfg(target_os = "linux")]
fn set_executable(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| AppError::io(format!("cannot make {} executable: {e}", path.display())))
}

#[cfg(target_os = "linux")]
fn write_version(appimage: &Path, version: &str) -> Result<(), AppError> {
    let path = appimage.with_extension("version");
    std::fs::write(&path, format!("{version}\n"))
        .map_err(|e| AppError::io(format!("cannot write {}: {e}", path.display())))
}

/// Electron AppImages embed their icon. The largest png wins and is copied next
/// to the AppImage, so the desktop entry never points into a temp directory.
#[cfg(target_os = "linux")]
fn extract_icon(appimage: &Path, dir: &Path) -> Option<PathBuf> {
    use std::process::Stdio;
    use std::time::Instant;

    let scratch = util::temp_dir("openwork-icon").ok()?;
    let mut child = Command::new(appimage)
        .arg("--appimage-extract")
        .arg("*.png")
        .current_dir(&scratch)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + SHORT_TIMEOUT;
    while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();

    let mut found = Vec::new();
    png_files(&scratch.join("squashfs-root"), &mut found);
    found.sort();
    let source = found
        .into_iter()
        .max_by_key(|path| std::fs::metadata(path).map(|item| item.len()).unwrap_or(0));
    let icon = source.and_then(|source| {
        let dest = dir.join("openwork.png");
        std::fs::copy(source, &dest).ok().map(|_| dest)
    });
    util::remove_dir(&scratch);
    icon
}

#[cfg(target_os = "linux")]
fn png_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            png_files(&path, out);
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
        {
            out.push(path);
        }
    }
}

#[cfg(any(target_os = "macos", test))]
fn plist_string_value(xml: &str, key: &str) -> Option<String> {
    let marker = format!("<key>{key}</key>");
    let rest = &xml[xml.find(&marker)? + marker.len()..];
    let start = rest.find("<string>")? + "<string>".len();
    let end = rest[start..].find("</string>")? + start;
    Some(rest[start..end].trim().to_string())
}

pub fn parse_release(json: &str) -> Result<Release, AppError> {
    let raw: RawRelease = serde_json::from_str(json)
        .map_err(|e| AppError::parse(format!("cannot read the OpenWork release: {e}")))?;
    if raw.assets.is_empty() {
        return Err(AppError::parse("the OpenWork release holds no assets"));
    }
    Ok(Release {
        version: raw.tag_name.trim_start_matches('v').to_string(),
        assets: raw
            .assets
            .into_iter()
            .map(|asset| Asset {
                name: asset.name,
                url: asset.browser_download_url,
                size: asset.size,
            })
            .collect(),
    })
}

pub fn select_asset<'a>(assets: &'a [Asset], platform: &str, arch: &str) -> Option<&'a Asset> {
    let (prefix, extension) = match (platform, arch) {
        ("macos", "aarch64") => ("openwork-mac-arm64-", ".dmg"),
        ("macos", "x86_64") => ("openwork-mac-x64-", ".dmg"),
        ("windows", "x86_64") => ("openwork-win-x64-", ".exe"),
        ("linux", "aarch64") => ("openwork-linux-arm64-", ".AppImage"),
        ("linux", "x86_64") => ("openwork-linux-x86_64-", ".AppImage"),
        _ => return None,
    };
    assets
        .iter()
        .find(|asset| asset.name.starts_with(prefix) && asset.name.ends_with(extension))
}

/// electron-builder publishes one update metadata file per platform and, on
/// Linux, one per architecture.
pub fn update_metadata_name(platform: &str, arch: &str) -> &'static str {
    match (platform, arch) {
        ("macos", _) => "latest-mac.yml",
        ("linux", "aarch64") => "latest-linux-arm64.yml",
        ("linux", _) => "latest-linux.yml",
        _ => "latest.yml",
    }
}

pub fn parse_latest_yml(text: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    let mut url: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("- url:") {
            url = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("sha512:") {
            if let Some(name) = url.take() {
                entries.push((name, rest.trim().to_string()));
            }
        }
    }
    entries
}

pub fn expected_sha512<'a>(entries: &'a [(String, String)], asset_name: &str) -> Option<&'a str> {
    entries
        .iter()
        .find(|(name, _)| name == asset_name)
        .map(|(_, hash)| hash.as_str())
}

pub fn verify_file_sha512(path: &Path, expected_b64: &str) -> Result<(), AppError> {
    use base64::Engine as _;
    use sha2::Digest as _;

    let expected = base64::engine::general_purpose::STANDARD
        .decode(expected_b64.trim())
        .map_err(|_| AppError::parse("the published sha512 is not valid base64"))?;
    let bytes = std::fs::read(path)
        .map_err(|e| AppError::io(format!("cannot read {}: {e}", path.display())))?;
    if sha2::Sha512::digest(&bytes).to_vec() != expected {
        return Err(AppError::parse(format!(
            "the downloaded file {} does not match the published sha512",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name)
    }

    fn release() -> Release {
        parse_release(&std::fs::read_to_string(fixture("openwork-release.json")).unwrap()).unwrap()
    }

    #[test]
    fn parses_the_recorded_release() {
        let release = release();
        assert_eq!(release.version, "0.18.63");
        assert_eq!(release.assets.len(), 60);
        let win = release
            .assets
            .iter()
            .find(|a| a.name == "openwork-win-x64-0.18.63.exe")
            .unwrap();
        assert_eq!(win.size, 273_351_728);
        assert_eq!(
            win.url,
            "https://github.com/different-ai/openwork/releases/download/v0.18.63/openwork-win-x64-0.18.63.exe"
        );
    }

    #[test]
    fn rejects_a_release_without_assets() {
        assert_eq!(parse_release("{}").unwrap_err().kind, "parse");
        assert_eq!(parse_release("not json").unwrap_err().kind, "parse");
    }

    #[test]
    fn windows_launch_target_appends_the_exe_to_a_directory() {
        assert_eq!(
            windows_launch_target(r"C:\Programs\OpenWork"),
            PathBuf::from(r"C:\Programs\OpenWork").join("OpenWork.exe")
        );
        assert_eq!(
            windows_launch_target(r"C:\Programs\OpenWork\OpenWork.exe"),
            PathBuf::from(r"C:\Programs\OpenWork\OpenWork.exe")
        );
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "openwork-detect-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_exe(dir: &Path, name: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, b"").unwrap();
        path
    }

    fn uninstall(name: &str, publisher: &str, location: Option<&str>) -> UninstallEntry {
        UninstallEntry {
            display_name: name.to_string(),
            publisher: Some(publisher.to_string()),
            install_location: location.map(str::to_string),
            version: Some("0.18.63".to_string()),
        }
    }

    #[test]
    fn detects_openwork_at_the_electron_builder_default_path() {
        let programs = scratch("default-path");
        let exe = write_exe(&programs.join("openwork"), "OpenWork.exe");
        let status = windows_status_from(Some(&programs), &[]).unwrap();
        assert!(status.installed);
        assert_eq!(status.path, Some(exe.display().to_string()));
    }

    #[test]
    fn detects_openwork_from_a_different_ai_uninstall_entry() {
        let programs = scratch("registry-default");
        let custom = scratch("registry-custom");
        let exe = write_exe(&custom, "OpenWork.exe");
        let location = custom.display().to_string();
        let entries = [uninstall("OpenWork", "different-ai", Some(&location))];
        let status = windows_status_from(Some(&programs), &entries).unwrap();
        assert_eq!(status.path, Some(exe.display().to_string()));
        assert_eq!(status.version.as_deref(), Some("0.18.63"));
    }

    #[test]
    fn never_detects_our_own_installer_as_openwork() {
        let programs = scratch("our-own-app");
        let own = write_exe(
            &programs.join("EPFL OpenWork Installer"),
            "EPFL OpenWork Installer.exe",
        );
        let location = own.parent().unwrap().display().to_string();
        for name in ["OpenWork Installer", "EPFL OpenWork Installer"] {
            let entries = [uninstall(name, "EPFL", Some(&location))];
            assert_eq!(
                windows_status_from(Some(&programs), &entries),
                None,
                "{name} must not count as OpenWork"
            );
        }
    }

    #[test]
    fn matches_only_the_real_openwork_uninstall_entry() {
        #[derive(serde::Deserialize)]
        struct Case {
            #[serde(flatten)]
            entry: UninstallEntry,
            expected: bool,
        }
        let cases: Vec<Case> =
            serde_json::from_str(&std::fs::read_to_string(fixture("openwork-uninstall-entries.json")).unwrap())
                .unwrap();
        assert!(cases.iter().any(|case| case.expected));
        for case in &cases {
            assert_eq!(
                is_openwork_uninstall_entry(&case.entry),
                case.expected,
                "{} from {:?}",
                case.entry.display_name,
                case.entry.publisher
            );
        }
    }

    #[test]
    fn detects_openwork_without_localappdata() {
        let custom = scratch("no-localappdata");
        let exe = write_exe(&custom, "OpenWork.exe");
        let location = custom.display().to_string();
        let entries = [uninstall("OpenWork", "different-ai", Some(&location))];
        let status = windows_status_from(None, &entries).unwrap();
        assert_eq!(status.path, Some(exe.display().to_string()));
    }

    #[test]
    fn ignores_a_stale_openwork_uninstall_entry() {
        let programs = scratch("stale-entry");
        let gone = programs.join("gone").display().to_string();
        assert_eq!(
            windows_status_from(
                Some(&programs),
                &[uninstall("OpenWork", "different-ai", Some(&gone))]
            ),
            None
        );
    }

    #[test]
    fn finds_openwork_app_in_the_applications_directory() {
        let apps = scratch("applications");
        assert_eq!(macos_status_from(&apps), None);
        let contents = apps.join("OpenWork.app").join("Contents");
        std::fs::create_dir_all(&contents).unwrap();
        std::fs::write(
            contents.join("Info.plist"),
            "<key>CFBundleShortVersionString</key><string>0.18.63</string>",
        )
        .unwrap();
        let status = macos_status_from(&apps).unwrap();
        assert_eq!(status.version.as_deref(), Some("0.18.63"));
        assert_eq!(
            status.path,
            Some(apps.join("OpenWork.app").display().to_string())
        );
    }

    #[test]
    fn does_not_take_our_own_app_bundle_for_openwork() {
        let apps = scratch("our-app-bundle");
        std::fs::create_dir_all(apps.join("EPFL OpenWork Installer.app").join("Contents")).unwrap();
        assert_eq!(macos_status_from(&apps), None);
    }

    #[test]
    fn selects_asset_per_platform_and_arch() {
        let release = release();
        let pick = |platform, arch| {
            select_asset(&release.assets, platform, arch).map(|asset| asset.name.as_str())
        };
        assert_eq!(pick("macos", "aarch64"), Some("openwork-mac-arm64-0.18.63.dmg"));
        assert_eq!(pick("macos", "x86_64"), Some("openwork-mac-x64-0.18.63.dmg"));
        assert_eq!(pick("windows", "x86_64"), Some("openwork-win-x64-0.18.63.exe"));
        assert_eq!(pick("windows", "aarch64"), None);
        assert_eq!(
            pick("linux", "x86_64"),
            Some("openwork-linux-x86_64-0.18.63.AppImage")
        );
        assert_eq!(
            pick("linux", "aarch64"),
            Some("openwork-linux-arm64-0.18.63.AppImage")
        );
        assert_eq!(update_metadata_name("linux", "x86_64"), "latest-linux.yml");
        assert_eq!(
            update_metadata_name("linux", "aarch64"),
            "latest-linux-arm64.yml"
        );
        assert_eq!(update_metadata_name("macos", "x86_64"), "latest-mac.yml");
        assert_eq!(update_metadata_name("windows", "x86_64"), "latest.yml");
    }

    #[test]
    fn never_selects_the_npm_package_or_a_blockmap() {
        let release = release();
        for platform in ["macos", "windows", "linux"] {
            for arch in ["aarch64", "x86_64"] {
                if let Some(asset) = select_asset(&release.assets, platform, arch) {
                    assert!(!asset.name.contains("blockmap"));
                    assert!(!asset.name.contains("cloud"));
                    assert!(!asset.name.contains("enterprise"));
                    assert!(
                        asset.name.ends_with(".dmg")
                            || asset.name.ends_with(".exe")
                            || asset.name.ends_with(".AppImage")
                    );
                }
            }
        }
    }

    #[test]
    fn parses_the_electron_builder_update_metadata() {
        let win = parse_latest_yml(
            &std::fs::read_to_string(fixture("openwork-latest.yml")).unwrap(),
        );
        assert_eq!(
            win,
            vec![
                (
                    "openwork-win-arm64-0.18.63.exe".to_string(),
                    "2qp0KGyCdmqOzP+tWanpDuxsa2kBYb/9VsE3QJOlttF7uqz5fIzbXZvlwLnrB791rVxURN7ip9OaWnBzyTBD/g==".to_string(),
                ),
                (
                    "openwork-win-x64-0.18.63.exe".to_string(),
                    "H6Iuv0zxiF9G6TugH/RHsIGR9ePW77pusG6rrC0f0UghLRoj9pRPhTlkYNWdoPCP6rpRf08kc0IGH9p4NsfYIw==".to_string(),
                ),
            ]
        );

        let mac = parse_latest_yml(
            &std::fs::read_to_string(fixture("openwork-latest-mac.yml")).unwrap(),
        );
        assert_eq!(mac.len(), 4);
        assert_eq!(
            expected_sha512(&mac, "openwork-mac-x64-0.18.63.dmg"),
            Some("a/PGNtuau37PbpWj1lK7WA1Ml9Q0yCDlS94icEWyyMS0oFSDKY8hjLnH5aR6GOzjQ/su7reZN84Xtx/60fP84w==")
        );
        assert_eq!(expected_sha512(&mac, "openwork-mac-x64-9.9.9.dmg"), None);
        assert_eq!(parse_latest_yml(""), Vec::new());
    }

    #[test]
    fn verifies_the_sha512_of_a_small_fixture() {
        let bytes = fixture("small-asset.bin");
        let expected = std::fs::read_to_string(fixture("small-asset.sha512")).unwrap();
        assert!(verify_file_sha512(&bytes, expected.trim()).is_ok());

        let err = verify_file_sha512(&bytes, "AAAA").unwrap_err();
        assert_eq!(err.kind, "parse");
        assert!(!err.message.contains(expected.trim()));

        let err = verify_file_sha512(Path::new("/nonexistent/asset"), "AAAA").unwrap_err();
        assert_eq!(err.kind, "io");
    }

    #[test]
    fn reads_the_version_from_an_info_plist() {
        let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<dict>
  <key>CFBundleName</key><string>OpenWork</string>
  <key>CFBundleShortVersionString</key>
  <string>0.18.63</string>
</dict>"#;
        assert_eq!(
            plist_string_value(plist, "CFBundleShortVersionString").as_deref(),
            Some("0.18.63")
        );
        assert_eq!(plist_string_value(plist, "CFBundleVersion"), None);
    }

    fn appimage_entry(home: &Path, file_name: &str, exec: &Path) {
        let dir = home.join(".local/share/applications");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(file_name), desktop_entry(exec, "openwork")).unwrap();
    }

    #[test]
    fn installs_the_appimage_under_the_data_home() {
        let home = scratch("appimage-path");
        assert_eq!(
            linux_appimage_path(&home),
            home.join(".local/share/openwork/OpenWork.AppImage")
        );
    }

    #[test]
    fn builds_the_desktop_entry() {
        let exec = Path::new("/home/tester/.local/share/openwork/OpenWork.AppImage");
        assert_eq!(
            desktop_entry(exec, "/usr/share/icons/openwork.png"),
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=OpenWork\n\
             Exec=\"/home/tester/.local/share/openwork/OpenWork.AppImage\"\n\
             Icon=/usr/share/icons/openwork.png\n\
             Terminal=false\n\
             Categories=Development;\n"
        );
    }

    #[test]
    fn reads_the_exec_line_of_a_desktop_entry() {
        assert_eq!(
            desktop_exec("[Desktop Entry]\nExec=\"/opt/My Apps/openwork.AppImage\" %U\n")
                .as_deref(),
            Some("/opt/My Apps/openwork.AppImage")
        );
        assert_eq!(
            desktop_exec("Exec=/opt/openwork.AppImage\n").as_deref(),
            Some("/opt/openwork.AppImage")
        );
        assert_eq!(desktop_exec("[Desktop Entry]\nName=OpenWork\n"), None);
        assert_eq!(desktop_exec("Exec=\n"), None);
    }

    #[test]
    fn detects_openwork_at_the_managed_appimage_path() {
        let home = scratch("managed-appimage");
        assert_eq!(linux_status_from(&home), None);

        let target = linux_appimage_path(&home);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"\x7fELF").unwrap();
        let status = linux_status_from(&home).unwrap();
        assert!(status.installed);
        assert_eq!(status.path, Some(target.display().to_string()));
        assert_eq!(status.version, None);

        std::fs::write(target.with_extension("version"), "0.18.63\n").unwrap();
        assert_eq!(
            linux_status_from(&home).unwrap().version.as_deref(),
            Some("0.18.63")
        );
    }

    #[test]
    fn detects_openwork_from_a_desktop_entry() {
        let home = scratch("desktop-entry");
        let installed = home.join("opt/openwork.AppImage");
        write_exe(installed.parent().unwrap(), "openwork.AppImage");
        appimage_entry(&home, OPENWORK_DESKTOP_NAME, &installed);
        let status = linux_status_from(&home).unwrap();
        assert!(status.installed);
        assert_eq!(status.path, Some(installed.display().to_string()));
    }

    #[test]
    fn ignores_a_desktop_entry_that_points_nowhere() {
        let home = scratch("dead-desktop-entry");
        appimage_entry(
            &home,
            OPENWORK_DESKTOP_NAME,
            &home.join("opt/gone.AppImage"),
        );
        assert_eq!(linux_status_from(&home), None);
    }

    #[test]
    fn never_takes_our_own_installer_for_openwork_on_linux() {
        let home = scratch("our-own-linux");
        let appimage = home.join("opt/openwork.AppImage");
        write_exe(appimage.parent().unwrap(), "openwork.AppImage");
        appimage_entry(&home, "EPFL OpenWork Installer.desktop", &appimage);
        assert_eq!(linux_status_from(&home), None);

        let own = write_exe(&home.join("opt"), "EPFL OpenWork Installer");
        appimage_entry(&home, OPENWORK_DESKTOP_NAME, &own);
        assert_eq!(linux_status_from(&home), None);
    }

    #[test]
    fn prefers_a_bundled_icon_over_the_extracted_one() {
        let bundled = PathBuf::from("/usr/lib/openwork/openwork.png");
        let extracted = PathBuf::from("/home/tester/.local/share/openwork/openwork.png");
        assert_eq!(
            pick_icon(Some(bundled.clone()), Some(extracted.clone())),
            bundled.display().to_string()
        );
        assert_eq!(
            pick_icon(None, Some(extracted.clone())),
            extracted.display().to_string()
        );
        assert_eq!(pick_icon(None, None), "application-x-executable");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn extracts_the_largest_icon_and_falls_back_without_one() {
        use std::os::unix::fs::PermissionsExt as _;

        let home = scratch("extract-icon");
        let dir = home.join("install");
        std::fs::create_dir_all(&dir).unwrap();

        let fake = home.join("fake.AppImage");
        std::fs::write(
            &fake,
            "#!/bin/sh\n\
             mkdir -p squashfs-root/usr/share/icons/hicolor/16x16/apps\n\
             mkdir -p squashfs-root/usr/share/icons/hicolor/512x512/apps\n\
             printf small > squashfs-root/usr/share/icons/hicolor/16x16/apps/openwork.png\n\
             head -c 4096 /dev/zero > squashfs-root/usr/share/icons/hicolor/512x512/apps/openwork.png\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

        let icon = extract_icon(&fake, &dir).unwrap();
        assert_eq!(icon, dir.join("openwork.png"));
        assert_eq!(std::fs::metadata(&icon).unwrap().len(), 4096);

        let silent = home.join("silent.AppImage");
        std::fs::write(&silent, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&silent, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(extract_icon(&silent, &dir), None);
    }

    #[test]
    fn parses_the_linux_update_metadata() {
        let x64 = parse_latest_yml(
            &std::fs::read_to_string(fixture("openwork-latest-linux.yml")).unwrap(),
        );
        assert_eq!(
            expected_sha512(&x64, "openwork-linux-x86_64-0.18.63.AppImage"),
            Some("feIvvRUesroJKflwfQDR5LlWGERm4kUlNIvLO83oJhmW3WG5vYkrUow4tHER+xY8HVBGtOLmFxcayadGr76BBA==")
        );
        assert_eq!(
            expected_sha512(&x64, "openwork-linux-x64-0.18.63.AppImage"),
            None
        );

        let arm = parse_latest_yml(
            &std::fs::read_to_string(fixture("openwork-latest-linux-arm64.yml")).unwrap(),
        );
        assert_eq!(
            expected_sha512(&arm, "openwork-linux-arm64-0.18.63.AppImage"),
            Some("HZSBO5/lXWj88YxIJOMcItMNerNVQjDK46bqrdSeEa5cwD7JC/0VKacDBz6b4MTNhCtZ/xvAh9jfLoHw4jRUJw==")
        );
    }

    #[test]
    fn selects_the_plain_appimage_not_cloud_enterprise_or_a_tarball() {
        let release = release();
        let asset = select_asset(&release.assets, "linux", "x86_64").unwrap();
        assert_eq!(asset.name, "openwork-linux-x86_64-0.18.63.AppImage");
        assert!(!asset.name.contains("cloud"));
        assert!(!asset.name.contains("enterprise"));
    }
}
