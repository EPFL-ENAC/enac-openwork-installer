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
    let outcome = install_into(&client, &release, asset, platform, &dir, &log).await;
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
    platform: &str,
    dir: &Path,
    log: &Logger,
) -> Result<(), AppError> {
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
    let metadata = match platform {
        "macos" => "latest-mac.yml",
        _ => "latest.yml",
    };
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
        other => Err(AppError::new(
            "unsupported",
            format!("OpenWork installs on macOS and Windows only, this host runs {other}"),
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

#[cfg(any(windows, test))]
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
        _ => return None,
    };
    assets
        .iter()
        .find(|asset| asset.name.starts_with(prefix) && asset.name.ends_with(extension))
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
        assert_eq!(pick("linux", "x86_64"), None);
    }

    #[test]
    fn never_selects_the_npm_package_or_a_blockmap() {
        let release = release();
        for platform in ["macos", "windows"] {
            for arch in ["aarch64", "x86_64"] {
                if let Some(asset) = select_asset(&release.assets, platform, arch) {
                    assert!(!asset.name.contains("blockmap"));
                    assert!(!asset.name.contains("cloud"));
                    assert!(!asset.name.contains("enterprise"));
                    assert!(asset.name.ends_with(".dmg") || asset.name.ends_with(".exe"));
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
}
