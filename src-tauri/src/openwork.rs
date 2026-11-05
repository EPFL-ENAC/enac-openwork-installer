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
/// Install directory names of the electron-builder NSIS per-user build, most
/// current first. Older releases used the bare product name.
#[cfg(any(windows, test))]
const OPENWORK_PROGRAMS_DIRS: [&str; 2] = ["@openworkdesktop", "openwork"];

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
                None => log.log(format!(
                    "{} lists no sha512 for this asset, skipped",
                    metadata
                )),
            }
        }
        None => log.log(format!("the release holds no {metadata}, checksum skipped")),
    }

    match platform {
        "macos" => install_dmg(&installer, log).await,
        "windows" => install_nsis(&installer, log).await,
        #[cfg(target_os = "linux")]
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
    let _ = util::run(
        Path::new("hdiutil"),
        &["detach", &mount_path, "-quiet"],
        SHORT_TIMEOUT,
    )
    .await;
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
    let entry = entries
        .iter()
        .find(|entry| is_openwork_uninstall_entry(entry));
    if let Some(programs) = programs_dir {
        for name in OPENWORK_PROGRAMS_DIRS {
            let exe = programs.join(name).join(OPENWORK_EXE);
            if exe.is_file() {
                return Some(installed_at(
                    &exe,
                    entry.and_then(|entry| entry.version.clone()),
                ));
            }
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
    let Ok(uninstall) = winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
        KEY_READ,
    ) else {
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
/// ("OpenWork Installer.desktop" or the older "EPFL OpenWork Installer.desktop"),
/// so it never counts.
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

// ===== OpenWork runtime configuration =====
//
// The bundled v2 engine mirrors providers from OpenWork's own runtime config
// (runtime.sqlite row "__openwork_engine_global__") and resolves "{env:NAME}"
// templates of machine-owned providers against the env store (env.json); the
// machine-ownership row approves exactly that resolution. The workspace
// default model lives in workspace_default_models. The desktop keeps its
// workspace list in <Electron user data>/openwork-workspaces.json and adopts
// ~/OpenWork Chat at the first launch, so a fresh folder already has the id
// the desktop will compute.
//
// The desktop's model picker reads no store: the default it shows comes from
// its renderer localStorage (openwork.workspaceDefaultModel.v1...), so the
// workspace_default_models row only feeds automations, remote sessions and
// headless callers, which look the model up as models[modelID] of its provider.
// What the picker preselects on a fresh profile is the "Auto" model of the
// built-in "OpenWork Models" provider, so the runtime config turns that
// provider off with disabled_providers, the desktop's own Auto switch.

pub const OPENWORK_PROVIDER_ID: &str = "epfl-aiaas";
/// The built-in "OpenWork Models" provider the desktop ships with and turns on
/// for its free "Auto" model.
pub const OPENWORK_FREE_PROVIDER_ID: &str = "openwork-free";
pub const OPENWORK_ENV_KEY: &str = "EPFL_AIAAS_API_KEY";
// The RCP gateway names its models with the vendor prefix, so the config key is
// also the model id the engine sends upstream.
pub const OPENWORK_MODEL_LARGE: &str = "epfl-aiaas/large";
pub const OPENWORK_MODEL_SMALL: &str = "epfl-aiaas/small";
const RCP_BASE_URL: &str = "https://inference-rcp.epfl.ch/v1";
const ENGINE_GLOBAL_ID: &str = "__openwork_engine_global__";
const MACHINE_OWNERSHIP_ID: &str = "__machine_provider_template_ownership__";
const OPENWORK_USER_DATA_DIR: &str = "com.differentai.openwork";
const OPENWORK_CHAT_DIR: &str = "OpenWork Chat";
const ENV_STORE_FILE: &str = "env.json";
const RUNTIME_DB_FILE: &str = "runtime.sqlite";
const WORKSPACE_STATE_FILE: &str = "openwork-workspaces.json";
const WORKSPACE_CONFIG_FILE: &str = "opencode.jsonc";
const ANALYTICS_STORE_FILE: &str = "free-auto-analytics";
const ANALYTICS_JOURNEY_FILE: &str = "free-auto-journey";

/// What one configure run changed, for the trace and the step result.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenworkConfigureOutcome {
    pub config_dir: String,
    pub provider_written: bool,
    pub env_written: bool,
    pub analytics_written: bool,
    pub workspace_config_written: bool,
    pub default_model_workspace_id: Option<String>,
    pub default_model_written: bool,
}

/// The RCP provider in the V1 shape OpenWork documents for custom providers.
fn provider_definition() -> serde_json::Value {
    serde_json::json!({
        "npm": "@ai-sdk/openai-compatible",
        "name": "RCP AI Inference as a Service",
        "options": {
            "apiKey": format!("{{env:{OPENWORK_ENV_KEY}}}"),
            "baseURL": RCP_BASE_URL,
        },
        "models": {
            (OPENWORK_MODEL_LARGE): {
                "name": "RCP AIaaS Large",
                "attachment": true,
                "modalities": {"input": ["text", "image"], "output": ["text"]},
            },
            (OPENWORK_MODEL_SMALL): {
                "name": "RCP AIaaS Small",
                "attachment": true,
                "modalities": {"input": ["text", "image"], "output": ["text"]},
                "variants": {
                    "none": {"reasoningEffort": "none"},
                    "low": {"reasoningEffort": "low"},
                    "high": {"reasoningEffort": "high"},
                    "xhigh": {"reasoningEffort": "xhigh"},
                    "max": {"reasoningEffort": "max"},
                },
            },
        },
    })
}

/// The machine-ownership approval binds the template privilege to this exact
/// definition, so the hash must match OpenWork's providerConfigFingerprint:
/// stable JSON with keys sorted by localeCompare, sha256 hex. serde_json maps
/// sort by byte order, which agrees with localeCompare for every key in the
/// definition above; the frozen-value test pins the contract.
fn provider_fingerprint(value: &serde_json::Value) -> String {
    use sha2::Digest as _;
    let canonical = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
    let digest = sha2::Sha256::digest(canonical.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn default_model_value() -> serde_json::Value {
    serde_json::json!({"providerID": OPENWORK_PROVIDER_ID, "modelID": OPENWORK_MODEL_LARGE})
}

/// The key rules OpenWork's env store applies: a name token that is not one of
/// its own reserved prefixes.
fn valid_env_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    if upper.starts_with("OPENWORK_") || upper.starts_with("OPENCODE_") {
        return false;
    }
    let bytes = key.as_bytes();
    match bytes.first() {
        Some(first) if first.is_ascii_alphabetic() || *first == b'_' => {}
        _ => return false,
    }
    bytes[1..]
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

/// Merges one variable into the env store, preserving everything else.
/// A store that parses but holds unexpected shapes is an error, not a reset:
/// the file holds secrets and silently dropping them would lose them.
fn merge_env_store(
    existing: Option<serde_json::Value>,
    key: &str,
    value: &str,
    now: i64,
) -> Result<(serde_json::Value, bool), AppError> {
    if !valid_env_key(key) {
        return Err(AppError::new(
            "invalid-key",
            format!("{key} is not a valid environment variable name"),
        ));
    }
    let mut store = existing.unwrap_or_else(
        || serde_json::json!({"schemaVersion": 1, "updatedAt": now, "variables": []}),
    );
    let Some(object) = store.as_object_mut() else {
        return Err(AppError::parse(
            "the OpenWork environment store is not a JSON object",
        ));
    };
    object
        .entry("schemaVersion")
        .or_insert(serde_json::json!(1));
    let variables = object.entry("variables").or_insert(serde_json::json!([]));
    let Some(list) = variables.as_array_mut() else {
        return Err(AppError::parse(
            "the OpenWork environment store holds no variable list",
        ));
    };
    for entry in list.iter_mut() {
        let Some(entry_object) = entry.as_object_mut() else {
            return Err(AppError::parse(
                "the OpenWork environment store holds a variable that is not an object",
            ));
        };
        if entry_object.get("key").and_then(serde_json::Value::as_str) != Some(key) {
            continue;
        }
        if entry_object
            .get("value")
            .and_then(serde_json::Value::as_str)
            == Some(value)
        {
            return Ok((store, false));
        }
        entry_object.insert("value".into(), serde_json::json!(value));
        entry_object.insert("updatedAt".into(), serde_json::json!(now));
        object.insert("updatedAt".into(), serde_json::json!(now));
        return Ok((store, true));
    }
    list.push(serde_json::json!({"key": key, "value": value, "updatedAt": now}));
    sort_env_variables(list);
    object.insert("updatedAt".into(), serde_json::json!(now));
    Ok((store, true))
}

fn sort_env_variables(list: &mut [serde_json::Value]) {
    list.sort_by_key(|entry| {
        entry
            .get("key")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string()
    });
}

/// Adds the RCP provider to the engine-global runtime config and turns its
/// built-in "OpenWork Models" provider off, keeping every other provider and
/// setting in place.
fn merge_runtime_provider(
    existing: serde_json::Value,
) -> Result<(serde_json::Value, bool), AppError> {
    let definition = provider_definition();
    let mut next = existing;
    let Some(object) = next.as_object_mut() else {
        return Err(AppError::parse(
            "the OpenWork runtime config row is not a JSON object",
        ));
    };
    let provider_changed = {
        let providers = object.entry("provider").or_insert(serde_json::json!({}));
        let Some(providers) = providers.as_object_mut() else {
            return Err(AppError::parse(
                "the OpenWork runtime config holds no provider map",
            ));
        };
        if providers.get(OPENWORK_PROVIDER_ID) == Some(&definition) {
            false
        } else {
            providers.insert(OPENWORK_PROVIDER_ID.to_string(), definition);
            true
        }
    };
    let entry = object
        .entry("disabled_providers")
        .or_insert_with(|| serde_json::json!([]));
    let mut disabled = match entry.as_array() {
        Some(list) => list.clone(),
        None => Vec::new(),
    };
    let disabled_changed = !disabled
        .iter()
        .any(|id| id.as_str() == Some(OPENWORK_FREE_PROVIDER_ID));
    if disabled_changed {
        disabled.push(serde_json::json!(OPENWORK_FREE_PROVIDER_ID));
        *entry = serde_json::Value::Array(disabled);
    }
    Ok((next, provider_changed || disabled_changed))
}

/// Approves the template resolution of our provider for this exact definition.
fn merge_machine_ownership(
    existing: serde_json::Value,
    fingerprint: &str,
) -> Result<(serde_json::Value, bool), AppError> {
    let mut next = existing;
    let Some(object) = next.as_object_mut() else {
        return Err(AppError::parse(
            "the OpenWork machine ownership row is not a JSON object",
        ));
    };
    if object
        .get(OPENWORK_PROVIDER_ID)
        .and_then(serde_json::Value::as_str)
        == Some(fingerprint)
    {
        return Ok((next, false));
    }
    object.insert(
        OPENWORK_PROVIDER_ID.to_string(),
        serde_json::json!(fingerprint),
    );
    Ok((next, true))
}

/// The workspace the desktop selects, from the state file it maintains.
fn pick_workspace_id(state: &serde_json::Value) -> Option<String> {
    for key in [
        "selectedId",
        "selectedWorkspaceId",
        "activeId",
        "activeWorkspaceId",
        "watchedId",
        "watchedWorkspaceId",
    ] {
        if let Some(id) = state.get(key).and_then(serde_json::Value::as_str) {
            let id = id.trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }
    let workspaces = state
        .get("workspaces")
        .and_then(serde_json::Value::as_array)?;
    workspaces.iter().find_map(|entry| {
        let kind = entry
            .get("workspaceType")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("local");
        let id = entry
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)?;
        (kind == "local" && !id.is_empty()).then(|| id.to_string())
    })
}

/// `ws_` plus the first 12 hex digits of the sha256 of the path, exactly the
/// derivation of the desktop's stableWorkspaceId and the server's
/// workspaceIdForPath.
fn workspace_id_for_path(path: &str) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(path.as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("ws_{}", &hex[..12])
}

/// The path string the desktop hashes: resolved on Unix (it realpaths the
/// folder), lexical on Windows where the desktop resolution keeps backslashes.
fn workspace_path_string(path: &Path) -> String {
    if std::env::consts::OS == "windows" {
        path.to_string_lossy().into_owned()
    } else {
        std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .into_owned()
    }
}

fn default_workspace_dir(home: &Path) -> PathBuf {
    home.join(OPENWORK_CHAT_DIR)
}

fn unix_openwork_config_dir(xdg_config_home: Option<&std::ffi::OsStr>, home: &Path) -> PathBuf {
    let root = xdg_config_home
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    root.join("openwork")
}

fn windows_openwork_config_dir(appdata: Option<&std::ffi::OsStr>, home: &Path) -> PathBuf {
    let root = appdata
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData/Roaming"));
    root.join("openwork")
}

/// Electron's app.getPath("appData") plus the packaged app identifier, which
/// is where the desktop keeps openwork-workspaces.json.
fn electron_user_data_dir(os: &str, config_base: Option<&std::ffi::OsStr>, home: &Path) -> PathBuf {
    let root = match os {
        "windows" => config_base
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Roaming")),
        "macos" => home.join("Library/Application Support"),
        _ => config_base
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config")),
    };
    root.join(OPENWORK_USER_DATA_DIR)
}

pub(crate) fn openwork_config_dir() -> Result<PathBuf, AppError> {
    let home = util::home_dir()
        .ok_or_else(|| AppError::io("cannot find the home directory of the user"))?;
    let os = std::env::consts::OS;
    let base = if os == "windows" {
        std::env::var_os("APPDATA")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
    };
    Ok(match os {
        "windows" => windows_openwork_config_dir(base.as_deref(), &home),
        _ => unix_openwork_config_dir(base.as_deref(), &home),
    })
}

/// Reads one config_json row. A missing database, table or row is "absent",
/// so a first run on an existing install works as well as a fresh one.
fn read_config_row(db: &Path, workspace_id: &str) -> Result<Option<String>, AppError> {
    read_runtime_row(db, "runtime_opencode_configs", "config_json", workspace_id)
}

fn read_model_row(db: &Path, workspace_id: &str) -> Result<Option<String>, AppError> {
    read_runtime_row(db, "workspace_default_models", "model_json", workspace_id)
}

fn read_runtime_row(
    db: &Path,
    table: &str,
    column: &str,
    workspace_id: &str,
) -> Result<Option<String>, AppError> {
    use rusqlite::params;
    if !db.is_file() {
        return Ok(None);
    }
    let connection =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| AppError::io(format!("cannot open {}: {e}", db.display())))?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| AppError::io(format!("cannot wait for {}: {e}", db.display())))?;
    let present: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![table],
            |row| row.get(0),
        )
        .map_err(|e| AppError::io(format!("cannot read {}: {e}", db.display())))?;
    if present == 0 {
        return Ok(None);
    }
    connection
        .query_row(
            &format!("SELECT {column} FROM {table} WHERE workspace_id = ?1"),
            params![workspace_id],
            |row| row.get::<_, String>(0),
        )
        .map(Some)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(AppError::io(format!(
                "cannot read {table} in {}: {other}",
                db.display()
            ))),
        })
}

/// A row value that parses or is absent. Corrupt rows stop the step instead of
/// being overwritten: the row drives OpenWork's provider configuration and a
/// blind reset would drop the user's own providers.
fn parse_row_value(row: Option<String>, label: &str) -> Result<serde_json::Value, AppError> {
    match row {
        None => Ok(serde_json::json!({})),
        Some(text) => {
            let value: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| AppError::parse(format!("{label} is not valid JSON: {e}")))?;
            if value.is_object() {
                Ok(value)
            } else {
                Err(AppError::parse(format!("{label} is not a JSON object")))
            }
        }
    }
}

struct RuntimeRowWrite {
    table: &'static str,
    column: &'static str,
    workspace_id: String,
    json: String,
}

fn write_runtime_rows(db: &Path, writes: &[RuntimeRowWrite], now: i64) -> Result<(), AppError> {
    use rusqlite::params;
    if let Some(parent) = db.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::io(format!("cannot create {}: {e}", parent.display())))?;
    }
    let mut connection = rusqlite::Connection::open(db)
        .map_err(|e| AppError::io(format!("cannot create {}: {e}", db.display())))?;
    // The rows carry the literal RCP key, so the database must be owner-only
    // whether we just created it or OpenWork left it group-readable.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(db, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| AppError::io(format!("cannot restrict {}: {e}", db.display())))?;
    }
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| AppError::io(format!("cannot wait for {}: {e}", db.display())))?;
    let transaction = connection
        .transaction()
        .map_err(|e| AppError::io(format!("cannot start writing {}: {e}", db.display())))?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS runtime_opencode_configs (workspace_id TEXT PRIMARY KEY NOT NULL, config_json TEXT NOT NULL, updated_at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS openwork_workspace_configs (workspace_id TEXT PRIMARY KEY NOT NULL, config_json TEXT NOT NULL, updated_at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS workspace_default_models (workspace_id TEXT PRIMARY KEY NOT NULL, model_json TEXT NOT NULL, updated_at INTEGER NOT NULL);",
        )
        .map_err(|e| AppError::io(format!("cannot prepare {}: {e}", db.display())))?;
    for write in writes {
        let sql = format!(
            "INSERT INTO {t} (workspace_id, {c}, updated_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(workspace_id) DO UPDATE SET {c} = excluded.{c}, updated_at = excluded.updated_at",
            t = write.table,
            c = write.column,
        );
        transaction
            .execute(&sql, params![write.workspace_id, write.json, now])
            .map_err(|e| {
                AppError::io(format!(
                    "cannot write {} in {}: {e}",
                    write.table,
                    db.display()
                ))
            })?;
    }
    transaction
        .commit()
        .map_err(|e| AppError::io(format!("cannot finish writing {}: {e}", db.display())))
}

/// Writes the env store atomically, owner-only. The config dir may not exist
/// yet on a machine where OpenWork never ran.
fn write_env_store(path: &Path, store: &serde_json::Value) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::io(format!("cannot create {}: {e}", parent.display())))?;
    }
    let temporary = path.with_extension("json.tmp");
    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(store)
            .map_err(|e| AppError::parse(format!("cannot serialize the env store: {e}")))?
    );
    std::fs::write(&temporary, &text)
        .map_err(|e| AppError::io(format!("cannot write {}: {e}", temporary.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| AppError::io(format!("cannot restrict {}: {e}", temporary.display())))?;
    }
    std::fs::rename(&temporary, path)
        .map_err(|e| AppError::io(format!("cannot update {}: {e}", path.display())))
}

/// The workspace the default model belongs to: the desktop's selected one when
/// it has run, otherwise the folder the desktop adopts at its first launch.
struct WorkspaceTarget {
    id: String,
    path: Option<PathBuf>,
}

fn resolve_default_workspace(
    user_data_dir: &Path,
    home: &Path,
) -> Result<Option<WorkspaceTarget>, AppError> {
    let state_path = user_data_dir.join(WORKSPACE_STATE_FILE);
    if !state_path.is_file() {
        let folder = default_workspace_dir(home);
        // Creation failures stay silent: the desktop creates the same folder on
        // its first launch, and the lexical fallback yields the same id.
        let _ = std::fs::create_dir_all(&folder);
        return Ok(Some(WorkspaceTarget {
            id: workspace_id_for_path(&workspace_path_string(&folder)),
            path: Some(folder),
        }));
    }
    let text = std::fs::read_to_string(&state_path)
        .map_err(|e| AppError::io(format!("cannot read {}: {e}", state_path.display())))?;
    let Ok(state) = serde_json::from_str::<serde_json::Value>(&text) else {
        // A state file the desktop cannot read either: it rebuilds its list on
        // the next launch, so the default model has no workspace yet.
        return Ok(None);
    };
    let Some(id) = pick_workspace_id(&state) else {
        return Ok(None);
    };
    // The folder the desktop recorded for that workspace, when it is local.
    let path = state
        .get("workspaces")
        .and_then(serde_json::Value::as_array)
        .and_then(|list| {
            list.iter().find_map(|entry| {
                let same = entry.get("id").and_then(serde_json::Value::as_str) == Some(id.as_str());
                let kind = entry
                    .get("workspaceType")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("local");
                let folder = entry
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|path| !path.is_empty())?;
                (same && kind == "local").then(|| PathBuf::from(folder))
            })
        });
    Ok(Some(WorkspaceTarget { id, path }))
}

/// Earlier installs wrote the provider into the workspace opencode.jsonc. The
/// engine reads that file as a project layer and resolves the `{env:}` apiKey
/// itself, but OpenWork spawns the engine with a whitelisted environment that
/// never carries the RCP key, so the empty key shadowed the provider OpenWork
/// injects globally and every model answered a credentials error. A fresh
/// workspace carries no such file, so this removes the entry and drops the file
/// when nothing else remains. A file with JSONC comments is left alone.
fn heal_workspace_config(workspace_dir: &Path) -> Result<bool, AppError> {
    let path = workspace_dir.join(WORKSPACE_CONFIG_FILE);
    let existing = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<serde_json::Value>(&text)
            .unwrap_or(serde_json::Value::String(text)), // sentinel: not plain JSON
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(AppError::io(format!(
                "cannot read {}: {error}",
                path.display()
            )))
        }
    };
    let Some(mut next) = existing.as_object().cloned() else {
        return Ok(false);
    };
    let removed = match next
        .get_mut("provider")
        .and_then(serde_json::Value::as_object_mut)
    {
        Some(providers) => {
            let removed = providers.remove(OPENWORK_PROVIDER_ID).is_some();
            if removed && providers.is_empty() {
                next.remove("provider");
            }
            removed
        }
        None => false,
    };
    if !removed {
        return Ok(false);
    }
    if next.keys().all(|key| key == "$schema") {
        std::fs::remove_file(&path)
            .map_err(|e| AppError::io(format!("cannot remove {}: {e}", path.display())))?;
        return Ok(true);
    }
    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(&next)
            .map_err(|e| AppError::parse(format!("cannot serialize the workspace config: {e}")))?
    );
    std::fs::write(&path, &text)
        .map_err(|e| AppError::io(format!("cannot write {}: {e}", path.display())))?;
    Ok(true)
}

/// One configure run: env store, stale workspace config, provider rows,
/// default model. Every write is idempotent, so a retry only rewrites what
/// changed.
pub fn configure_openwork_in(
    config_dir: &Path,
    user_data_dir: &Path,
    home: &Path,
    token: &str,
    now: i64,
) -> Result<OpenworkConfigureOutcome, AppError> {
    let mut outcome = OpenworkConfigureOutcome {
        config_dir: config_dir.display().to_string(),
        provider_written: false,
        env_written: false,
        analytics_written: false,
        workspace_config_written: false,
        default_model_workspace_id: None,
        default_model_written: false,
    };
    let token = token.trim();
    if token.is_empty() {
        return Err(AppError::new(
            "missing-key",
            "no RCP key is available for the OpenWork configuration",
        ));
    }

    let env_path = config_dir.join(ENV_STORE_FILE);
    let existing = match std::fs::read_to_string(&env_path) {
        Ok(text) => Some(parse_json_object(
            &text,
            &format!("the OpenWork environment store {}", env_path.display()),
        )?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(AppError::io(format!(
                "cannot read {}: {error}",
                env_path.display()
            )))
        }
    };
    let (store, env_changed) = merge_env_store(existing, OPENWORK_ENV_KEY, token, now)?;
    if env_changed {
        write_env_store(&env_path, &store)?;
    }
    outcome.env_written = env_changed;
    outcome.analytics_written = disable_telemetry(config_dir)?;

    let db_path = config_dir.join(RUNTIME_DB_FILE);
    let fingerprint = provider_fingerprint(&provider_definition());
    let runtime = parse_row_value(
        read_config_row(&db_path, ENGINE_GLOBAL_ID)?,
        "the OpenWork runtime config row",
    )?;
    let (runtime_next, runtime_changed) = merge_runtime_provider(runtime)?;
    let ownership = parse_row_value(
        read_runtime_row(
            &db_path,
            "openwork_workspace_configs",
            "config_json",
            MACHINE_OWNERSHIP_ID,
        )?,
        "the OpenWork machine ownership row",
    )?;
    let (ownership_next, ownership_changed) = merge_machine_ownership(ownership, &fingerprint)?;

    let (workspace_id, workspace_path, model_changed) =
        match resolve_default_workspace(user_data_dir, home)? {
            Some(target) => {
                let stored = read_model_row(&db_path, &target.id)?;
                let changed = match stored {
                    Some(text) => serde_json::from_str::<serde_json::Value>(&text)
                        .map(|stored| stored != default_model_value())
                        .unwrap_or(true),
                    None => true,
                };
                (Some(target.id), target.path, changed)
            }
            None => (None, None, false),
        };
    outcome.default_model_workspace_id = workspace_id.clone();
    if let Some(workspace_dir) = &workspace_path {
        outcome.workspace_config_written = heal_workspace_config(workspace_dir)?;
    }

    let mut writes = Vec::new();
    if runtime_changed {
        writes.push(RuntimeRowWrite {
            table: "runtime_opencode_configs",
            column: "config_json",
            workspace_id: ENGINE_GLOBAL_ID.to_string(),
            json: runtime_next.to_string(),
        });
    }
    if ownership_changed {
        writes.push(RuntimeRowWrite {
            table: "openwork_workspace_configs",
            column: "config_json",
            workspace_id: MACHINE_OWNERSHIP_ID.to_string(),
            json: ownership_next.to_string(),
        });
    }
    if model_changed {
        if let Some(workspace_id) = &outcome.default_model_workspace_id {
            writes.push(RuntimeRowWrite {
                table: "workspace_default_models",
                column: "model_json",
                workspace_id: workspace_id.clone(),
                json: default_model_value().to_string(),
            });
        }
    }
    if !writes.is_empty() {
        write_runtime_rows(&db_path, &writes, now)?;
    }
    outcome.provider_written = runtime_changed || ownership_changed;
    outcome.default_model_written = model_changed;
    Ok(outcome)
}

/// OpenWork reads this file when its server starts. The journey id makes the
/// anonymous events linkable, so turning the switch off also drops it.
fn disable_telemetry(config_dir: &Path) -> Result<bool, AppError> {
    std::fs::create_dir_all(config_dir)
        .map_err(|e| AppError::io(format!("cannot create {}: {e}", config_dir.display())))?;
    let store = config_dir.join(ANALYTICS_STORE_FILE);
    let current = match std::fs::read_to_string(&store) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(AppError::io(format!(
                "cannot read {}: {error}",
                store.display()
            )))
        }
    };
    let mut changed = false;
    if current.trim() != "disabled" {
        std::fs::write(&store, "disabled")
            .map_err(|e| AppError::io(format!("cannot write {}: {e}", store.display())))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| AppError::io(format!("cannot restrict {}: {e}", store.display())))?;
        }
        changed = true;
    }
    let journey = config_dir.join(ANALYTICS_JOURNEY_FILE);
    match std::fs::remove_file(&journey) {
        Ok(()) => changed = true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(AppError::io(format!(
                "cannot remove {}: {error}",
                journey.display()
            )))
        }
    }
    Ok(changed)
}

fn parse_json_object(text: &str, label: &str) -> Result<serde_json::Value, AppError> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| AppError::parse(format!("{label} is not valid JSON: {e}")))?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(AppError::parse(format!("{label} is not a JSON object")))
    }
}

fn configure_openwork_blocking(token: &str) -> Result<OpenworkConfigureOutcome, AppError> {
    let home = util::home_dir()
        .ok_or_else(|| AppError::io("cannot find the home directory of the user"))?;
    let config_dir = openwork_config_dir()?;
    let os = std::env::consts::OS;
    let base = if os == "windows" {
        std::env::var_os("APPDATA")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
    };
    configure_openwork_in(
        &config_dir,
        &electron_user_data_dir(os, base.as_deref(), &home),
        &home,
        token,
        now_ms(),
    )
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or_default()
}

#[tauri::command]
pub async fn configure_openwork(
    token: String,
    app: tauri::AppHandle,
) -> Result<OpenworkConfigureOutcome, AppError> {
    let log = Logger::new(&app, "openwork");
    let trimmed = token.trim().to_string();
    let outcome = util::spawn_blocking(move || configure_openwork_blocking(&trimmed)).await??;
    log.log(format!(
        "the OpenWork configuration lives in {}",
        outcome.config_dir
    ));
    log.log(if outcome.provider_written {
        format!("the {OPENWORK_PROVIDER_ID} provider is written into the OpenWork runtime config")
    } else {
        format!("the {OPENWORK_PROVIDER_ID} provider is already configured in OpenWork")
    });
    log.log(format!(
        "the built-in {OPENWORK_FREE_PROVIDER_ID} provider is turned off, so its Auto model cannot be the default"
    ));
    log.log(if outcome.env_written {
        format!("the RCP key is stored in the OpenWork environment store as {OPENWORK_ENV_KEY}")
    } else {
        "the RCP key is already stored in the OpenWork environment store".to_string()
    });
    log.log(if outcome.workspace_config_written {
        format!(
            "the stale {OPENWORK_PROVIDER_ID} provider is removed from the workspace opencode.jsonc"
        )
    } else {
        format!("the workspace opencode.jsonc carries no {OPENWORK_PROVIDER_ID} provider override")
    });
    match (&outcome.default_model_workspace_id, outcome.default_model_written) {
        (Some(workspace_id), true) => log.log(format!(
            "the default model is set to {OPENWORK_MODEL_LARGE} for the OpenWork workspace {workspace_id}"
        )),
        (Some(_), false) => log.log(format!(
            "the default model {OPENWORK_MODEL_LARGE} is already set for this OpenWork workspace"
        )),
        (None, _) => log.log(
            "OpenWork has no workspace yet, its default model is set when OpenWork creates one",
        ),
    }
    Ok(outcome)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name)
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
        let dir =
            std::env::temp_dir().join(format!("openwork-detect-{}-{tag}", std::process::id()));
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
        let exe = write_exe(&programs.join("@openworkdesktop"), "OpenWork.exe");
        let status = windows_status_from(Some(&programs), &[]).unwrap();
        assert!(status.installed);
        assert_eq!(status.path, Some(exe.display().to_string()));
    }

    #[test]
    fn detects_openwork_in_a_legacy_programs_directory() {
        let programs = scratch("legacy-path");
        let exe = write_exe(&programs.join("openwork"), "OpenWork.exe");
        let status = windows_status_from(Some(&programs), &[]).unwrap();
        assert_eq!(status.path, Some(exe.display().to_string()));
    }

    #[test]
    fn prefers_the_current_programs_directory_over_the_legacy_one() {
        let programs = scratch("both-paths");
        let current = write_exe(&programs.join("@openworkdesktop"), "OpenWork.exe");
        write_exe(&programs.join("openwork"), "OpenWork.exe");
        let status = windows_status_from(Some(&programs), &[]).unwrap();
        assert_eq!(status.path, Some(current.display().to_string()));
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
        let cases: Vec<Case> = serde_json::from_str(
            &std::fs::read_to_string(fixture("openwork-uninstall-entries.json")).unwrap(),
        )
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
        for name in ["OpenWork Installer", "EPFL OpenWork Installer"] {
            let apps = scratch("our-app-bundle");
            std::fs::create_dir_all(apps.join(format!("{name}.app")).join("Contents")).unwrap();
            assert_eq!(macos_status_from(&apps), None, "{name}.app must not count");
        }
    }

    #[test]
    fn selects_asset_per_platform_and_arch() {
        let release = release();
        let pick = |platform, arch| {
            select_asset(&release.assets, platform, arch).map(|asset| asset.name.as_str())
        };
        assert_eq!(
            pick("macos", "aarch64"),
            Some("openwork-mac-arm64-0.18.63.dmg")
        );
        assert_eq!(
            pick("macos", "x86_64"),
            Some("openwork-mac-x64-0.18.63.dmg")
        );
        assert_eq!(
            pick("windows", "x86_64"),
            Some("openwork-win-x64-0.18.63.exe")
        );
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
        let win =
            parse_latest_yml(&std::fs::read_to_string(fixture("openwork-latest.yml")).unwrap());
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

        let mac =
            parse_latest_yml(&std::fs::read_to_string(fixture("openwork-latest-mac.yml")).unwrap());
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
        for name in ["OpenWork Installer", "EPFL OpenWork Installer"] {
            let home = scratch("our-own-linux");
            let appimage = home.join("opt/openwork.AppImage");
            write_exe(appimage.parent().unwrap(), "openwork.AppImage");
            appimage_entry(&home, &format!("{name}.desktop"), &appimage);
            assert_eq!(
                linux_status_from(&home),
                None,
                "{name}.desktop must not count"
            );

            let own = write_exe(&home.join("opt"), name);
            appimage_entry(&home, OPENWORK_DESKTOP_NAME, &own);
            assert_eq!(linux_status_from(&home), None, "{name} must not count");
        }
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

    // --- OpenWork configuration: env store, runtime rows, default model ---

    const STATE_FILE: &str = r#"{
  "selectedId": "ws_05771fd9f545",
  "watchedId": "ws_05771fd9f545",
  "activeId": "ws_05771fd9f545",
  "workspaces": [
    {
      "id": "ws_05771fd9f545",
      "name": "openwork-test",
      "path": "/home/tester/Downloads/openwork-test",
      "preset": "starter",
      "workspaceType": "local",
      "remoteType": null,
      "baseUrl": null,
      "directory": null,
      "displayName": null,
      "openworkHostUrl": null,
      "openworkToken": null,
      "openworkClientToken": null,
      "openworkHostToken": null,
      "openworkWorkspaceId": null,
      "openworkWorkspaceName": null,
      "sandboxBackend": null,
      "sandboxRunId": null,
      "sandboxContainerName": null
    }
  ],
  "selectedWorkspaceId": "ws_05771fd9f545",
  "watchedWorkspaceId": "ws_05771fd9f545"
}"#;

    fn env_store(now: i64) -> serde_json::Value {
        serde_json::json!({
            "schemaVersion": 1,
            "updatedAt": now,
            "variables": [
                {"key": "MY_VAR", "value": "one", "updatedAt": now},
                {"key": "OTHER", "value": "two", "updatedAt": now}
            ]
        })
    }

    fn configure_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("openwork-configure-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The exact provider block an earlier install wrote into the default
    /// workspace. The engine resolves the {env:} template against a whitelisted
    /// environment that never carries the RCP key, so the empty key failed every
    /// model request with a credentials error.
    fn stale_workspace_config() -> String {
        r#"{"provider":{"epfl-aiaas":{"models":{"epfl-aiaas/large":{"name":"RCP AIaaS Large"},"epfl-aiaas/small":{"name":"RCP AIaaS Small"}},"name":"RCP AI Inference as a Service","npm":"@ai-sdk/openai-compatible","options":{"apiKey":"{env:EPFL_AIAAS_API_KEY}","baseURL":"https://inference-rcp.epfl.ch/v1"}}}}"#
            .to_string()
    }

    /// A state file the desktop would write for one local workspace under a
    /// temporary folder, so the workspace config write stays inside the test
    /// sandbox.
    fn state_with_workspace(id: &str, workspace_dir: &Path) -> String {
        serde_json::json!({
            "selectedId": id,
            "workspaces": [
                {"id": id, "name": "openwork-test", "path": workspace_dir.display().to_string(), "workspaceType": "local"}
            ]
        })
        .to_string()
    }

    fn read_row(db: &Path, sql: &str, id: &str) -> Option<String> {
        let conn =
            rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        conn.query_row(sql, [id], |row| row.get::<_, String>(0))
            .ok()
    }

    fn runtime_config_json(db: &Path, id: &str) -> Option<String> {
        read_row(
            db,
            "SELECT config_json FROM runtime_opencode_configs WHERE workspace_id = ?1",
            id,
        )
    }

    fn workspace_config_json(db: &Path, id: &str) -> Option<String> {
        read_row(
            db,
            "SELECT config_json FROM openwork_workspace_configs WHERE workspace_id = ?1",
            id,
        )
    }

    fn default_model_json(db: &Path, id: &str) -> Option<String> {
        read_row(
            db,
            "SELECT model_json FROM workspace_default_models WHERE workspace_id = ?1",
            id,
        )
    }

    /// OpenWork looks a stored default model up as `models[modelID]` inside the
    /// provider map, so a row selects a model only when that key is there.
    fn resolves_in(providers: &serde_json::Value, model: &serde_json::Value) -> bool {
        let provider_id = model["providerID"].as_str().unwrap_or_default();
        let model_id = model["modelID"].as_str().unwrap_or_default();
        providers[provider_id]["models"]
            .as_object()
            .is_some_and(|models| models.contains_key(model_id))
    }

    fn seeded_db(db: &Path) {
        let conn = rusqlite::Connection::open(db).unwrap();
        conn.execute_batch(
            "CREATE TABLE runtime_opencode_configs (workspace_id TEXT PRIMARY KEY NOT NULL, config_json TEXT NOT NULL, updated_at INTEGER NOT NULL);
             CREATE TABLE workspace_default_models (workspace_id TEXT PRIMARY KEY NOT NULL, model_json TEXT NOT NULL, updated_at INTEGER NOT NULL);
             INSERT INTO runtime_opencode_configs VALUES ('__openwork_engine_global__', '{\"disabled_providers\":[\"opencode\"],\"provider\":{\"openwork-free\":{\"name\":\"OpenWork Models (Free)\",\"npm\":\"@ai-sdk/openai\"}}}', 1);
             INSERT INTO workspace_default_models VALUES ('ws_other', '{\"providerID\":\"other\",\"modelID\":\"model\"}', 1);",
        )
        .unwrap();
    }

    #[test]
    fn builds_the_v1_shape_provider_definition() {
        let definition = provider_definition();
        assert_eq!(definition["npm"], "@ai-sdk/openai-compatible");
        assert_eq!(definition["name"], "RCP AI Inference as a Service");
        assert_eq!(
            definition["options"]["baseURL"],
            "https://inference-rcp.epfl.ch/v1"
        );
        assert_eq!(definition["options"]["apiKey"], "{env:EPFL_AIAAS_API_KEY}");
        let models = definition["models"].as_object().unwrap();
        assert_eq!(
            models.keys().collect::<Vec<_>>(),
            [OPENWORK_MODEL_LARGE, OPENWORK_MODEL_SMALL]
        );
        assert_eq!(
            definition["models"][OPENWORK_MODEL_LARGE]["name"],
            "RCP AIaaS Large"
        );
        assert_eq!(
            definition["models"][OPENWORK_MODEL_SMALL]["name"],
            "RCP AIaaS Small"
        );
        assert_eq!(
            definition["models"][OPENWORK_MODEL_SMALL]["modalities"]["input"],
            serde_json::json!(["text", "image"])
        );
    }

    #[test]
    fn fingerprints_the_provider_like_the_opencode_canonical_form() {
        // Frozen from OpenWork v0.18.63 providerConfigFingerprint: stable JSON
        // with keys sorted by localeCompare, sha256 hex. Computed with node.
        assert_eq!(
            provider_fingerprint(&provider_definition()),
            "a41eac2a78f1a132c6a3d3c61fa8fa9f7c33b708512a288324df4b5bda224029"
        );
    }

    #[test]
    fn derives_workspace_ids_like_opencode() {
        assert_eq!(
            workspace_id_for_path("/home/tester/Downloads/openwork-test"),
            "ws_e4d0e7ca2c80"
        );
        assert_eq!(workspace_id_for_path(""), {
            use sha2::Digest as _;
            let digest = sha2::Sha256::digest(b"");
            format!(
                "ws_{}",
                &digest
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()[..12]
            )
        });
    }

    #[test]
    fn builds_the_openwork_config_dir_per_platform() {
        use std::ffi::OsStr;
        let home = Path::new("/home/tester");
        assert_eq!(
            unix_openwork_config_dir(None, home),
            home.join(".config/openwork")
        );
        assert_eq!(
            unix_openwork_config_dir(Some(OsStr::new("/xdg")), home),
            PathBuf::from("/xdg/openwork")
        );
        let windows_home = Path::new(r"C:\Users\tester");
        let fallback = windows_openwork_config_dir(None, windows_home);
        assert_eq!(fallback.file_name().unwrap(), "openwork");
        assert!(fallback.starts_with(windows_home.join("AppData/Roaming")));
        let explicit = windows_openwork_config_dir(
            Some(OsStr::new(r"C:\Users\tester\AppData\Roaming")),
            windows_home,
        );
        assert_eq!(explicit.file_name().unwrap(), "openwork");
        assert!(explicit.starts_with(r"C:\Users\tester\AppData\Roaming"));
    }

    #[test]
    fn builds_the_electron_user_data_dir_per_platform() {
        use std::ffi::OsStr;
        let home = Path::new("/home/tester");
        assert_eq!(
            electron_user_data_dir("linux", None, home),
            home.join(".config/com.differentai.openwork")
        );
        assert_eq!(
            electron_user_data_dir("linux", Some(OsStr::new("/xdg")), home),
            PathBuf::from("/xdg/com.differentai.openwork")
        );
        assert_eq!(
            electron_user_data_dir("macos", None, home),
            home.join("Library/Application Support/com.differentai.openwork")
        );
        let windows_home = Path::new(r"C:\Users\tester");
        assert_eq!(
            electron_user_data_dir("windows", None, windows_home),
            windows_home.join("AppData/Roaming/com.differentai.openwork")
        );
    }

    #[test]
    fn merges_the_env_store_and_preserves_existing_variables() {
        let (merged, changed) =
            merge_env_store(Some(env_store(1)), OPENWORK_ENV_KEY, "tok", 99).unwrap();
        assert!(changed);
        assert_eq!(merged["schemaVersion"], 1);
        assert_eq!(merged["updatedAt"], 99);
        let variables = merged["variables"].as_array().unwrap();
        assert_eq!(
            variables
                .iter()
                .map(|entry| entry["key"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["EPFL_AIAAS_API_KEY", "MY_VAR", "OTHER"]
        );
        assert!(variables[0]["value"] == "tok" && variables[1]["value"] == "one");
    }

    #[test]
    fn env_store_merges_are_idempotent_for_the_same_value() {
        let (merged, changed) =
            merge_env_store(Some(env_store(1)), OPENWORK_ENV_KEY, "tok", 99).unwrap();
        assert!(changed);
        let (again, changed) = merge_env_store(Some(merged), OPENWORK_ENV_KEY, "tok", 100).unwrap();
        assert!(!changed);
        assert_eq!(again["updatedAt"], 99);
    }

    #[test]
    fn env_store_rejects_reserved_or_malformed_keys() {
        for key in [
            "OPENWORK_SERVER",
            "OPENCODE_CONFIG",
            "openwork_home",
            "BAD-NAME",
            "1ABC",
            "",
            "A B",
        ] {
            assert!(
                merge_env_store(Some(env_store(1)), key, "tok", 1).is_err(),
                "{key} must be rejected"
            );
        }
    }

    #[test]
    fn env_store_rejects_unexpected_shapes() {
        let not_object = serde_json::json!([1, 2]);
        assert!(merge_env_store(Some(not_object), OPENWORK_ENV_KEY, "tok", 1).is_err());
        let bad_variables = serde_json::json!({"variables": "nope"});
        assert!(merge_env_store(Some(bad_variables), OPENWORK_ENV_KEY, "tok", 1).is_err());
        let bad_entry = serde_json::json!({"variables": ["nope"]});
        assert!(merge_env_store(Some(bad_entry), OPENWORK_ENV_KEY, "tok", 1).is_err());
    }

    #[test]
    fn merges_the_provider_into_an_existing_runtime_config() {
        let existing = serde_json::json!({
            "disabled_providers": ["opencode"],
            "provider": {"openwork-free": {"name": "OpenWork Models (Free)"}}
        });
        let (merged, changed) = merge_runtime_provider(existing).unwrap();
        assert!(changed);
        assert_eq!(
            merged["disabled_providers"],
            serde_json::json!(["opencode", OPENWORK_FREE_PROVIDER_ID])
        );
        assert_eq!(
            merged["provider"]["openwork-free"]["name"],
            "OpenWork Models (Free)"
        );
        assert_eq!(
            merged["provider"]["epfl-aiaas"]["npm"],
            "@ai-sdk/openai-compatible"
        );

        let (again, changed) = merge_runtime_provider(merged).unwrap();
        assert!(!changed);
        assert!(again["provider"]["openwork-free"].is_object());
    }

    #[test]
    fn masks_the_built_in_provider_on_a_runtime_config_without_one() {
        let (merged, changed) = merge_runtime_provider(serde_json::json!({})).unwrap();
        assert!(changed);
        assert_eq!(
            merged["disabled_providers"],
            serde_json::json!([OPENWORK_FREE_PROVIDER_ID])
        );
        assert_eq!(
            merged["provider"]["epfl-aiaas"]["name"],
            "RCP AI Inference as a Service"
        );
    }

    #[test]
    fn rewrites_the_mask_when_a_user_turns_auto_back_on() {
        let (merged, _) = merge_runtime_provider(serde_json::json!({})).unwrap();
        let mut reenabled = merged.clone();
        reenabled["disabled_providers"] = serde_json::json!(["opencode"]);

        let (healed, changed) = merge_runtime_provider(reenabled).unwrap();
        assert!(changed);
        assert_eq!(
            healed["disabled_providers"],
            serde_json::json!(["opencode", OPENWORK_FREE_PROVIDER_ID])
        );
    }

    #[test]
    fn replaces_an_unexpected_disabled_provider_list() {
        let (merged, changed) = merge_runtime_provider(serde_json::json!({
            "disabled_providers": "opencode"
        }))
        .unwrap();
        assert!(changed);
        assert_eq!(
            merged["disabled_providers"],
            serde_json::json!([OPENWORK_FREE_PROVIDER_ID])
        );
    }

    #[test]
    fn rejects_a_runtime_config_with_an_unexpected_shape() {
        assert!(merge_runtime_provider(serde_json::json!([1])).is_err());
        assert!(merge_runtime_provider(serde_json::json!({"provider": []})).is_err());
    }

    #[test]
    fn merges_the_machine_ownership_row() {
        let fingerprint = provider_fingerprint(&provider_definition());
        let existing = serde_json::json!({"openwork-free": "a".repeat(64)});
        let (merged, changed) = merge_machine_ownership(existing, &fingerprint).unwrap();
        assert!(changed);
        assert_eq!(merged["epfl-aiaas"], fingerprint.as_str());
        assert_eq!(merged["openwork-free"], "a".repeat(64));

        let (again, changed) = merge_machine_ownership(merged, &fingerprint).unwrap();
        assert!(!changed);
        assert!(again["openwork-free"].is_string());
    }

    #[test]
    fn picks_the_selected_workspace_from_the_state_file() {
        let state: serde_json::Value = serde_json::from_str(STATE_FILE).unwrap();
        assert_eq!(
            pick_workspace_id(&state).as_deref(),
            Some("ws_05771fd9f545")
        );

        let bare: serde_json::Value = serde_json::from_str(
            r#"{"workspaces": [{"id": "ws_first", "workspaceType": "local"}, {"id": "ws_second"}]}"#,
        )
        .unwrap();
        assert_eq!(pick_workspace_id(&bare).as_deref(), Some("ws_first"));

        let remote_only: serde_json::Value =
            serde_json::from_str(r#"{"workspaces": [{"id": "ws_r", "workspaceType": "remote"}]}"#)
                .unwrap();
        assert_eq!(pick_workspace_id(&remote_only), None);
        assert_eq!(pick_workspace_id(&serde_json::json!({})), None);
    }

    #[test]
    fn configures_a_fresh_home_end_to_end() {
        let config_dir = configure_dir("fresh-config");
        let user_data = configure_dir("fresh-user-data");
        let home = configure_dir("fresh-home");

        let outcome = configure_openwork_in(
            &config_dir,
            &user_data,
            &home,
            "test-token",
            1_700_000_000_000,
        )
        .unwrap();
        assert!(outcome.provider_written);
        assert!(outcome.env_written);
        assert!(outcome.analytics_written);
        assert!(!outcome.workspace_config_written);
        assert!(outcome.default_model_written);
        assert_eq!(
            std::fs::read_to_string(config_dir.join(ANALYTICS_STORE_FILE)).unwrap(),
            "disabled"
        );

        let folder = default_workspace_dir(&home);
        assert!(folder.is_dir());
        let expected_id = workspace_id_for_path(
            &std::fs::canonicalize(&folder)
                .unwrap()
                .display()
                .to_string(),
        );
        assert_eq!(
            outcome.default_model_workspace_id.as_deref(),
            Some(expected_id.as_str())
        );

        let env: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(config_dir.join("env.json")).unwrap())
                .unwrap();
        assert_eq!(env["variables"][0]["key"], OPENWORK_ENV_KEY);
        assert_eq!(env["variables"][0]["value"], "test-token");
        let db = config_dir.join("runtime.sqlite");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let env_mode = std::fs::metadata(config_dir.join("env.json"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(env_mode & 0o777, 0o600);
            let db_mode = std::fs::metadata(&db).unwrap().permissions().mode();
            assert_eq!(db_mode & 0o777, 0o600);
        }

        let runtime: serde_json::Value =
            serde_json::from_str(&runtime_config_json(&db, "__openwork_engine_global__").unwrap())
                .unwrap();
        assert_eq!(
            runtime["provider"]["epfl-aiaas"]["options"]["apiKey"],
            "{env:EPFL_AIAAS_API_KEY}"
        );
        let runtime_models = runtime["provider"]["epfl-aiaas"]["models"]
            .as_object()
            .unwrap();
        assert_eq!(
            runtime_models.keys().collect::<Vec<_>>(),
            [OPENWORK_MODEL_LARGE, OPENWORK_MODEL_SMALL]
        );
        assert_eq!(
            runtime_models[OPENWORK_MODEL_LARGE]["name"],
            "RCP AIaaS Large"
        );
        assert_eq!(
            runtime_models[OPENWORK_MODEL_SMALL]["name"],
            "RCP AIaaS Small"
        );
        let ownership: serde_json::Value = serde_json::from_str(
            &workspace_config_json(&db, "__machine_provider_template_ownership__").unwrap(),
        )
        .unwrap();
        assert!(ownership["epfl-aiaas"].is_string());
        let model: serde_json::Value =
            serde_json::from_str(&default_model_json(&db, &expected_id).unwrap()).unwrap();
        assert_eq!(model["providerID"], "epfl-aiaas");
        assert_eq!(model["modelID"], OPENWORK_MODEL_LARGE);
        assert!(resolves_in(&runtime["provider"], &model));
        // The desktop's own Auto off switch is this entry, and Auto is the
        // model a fresh profile preselects otherwise.
        assert_eq!(
            runtime["disabled_providers"],
            serde_json::json!([OPENWORK_FREE_PROVIDER_ID])
        );

        // The engine injects the configured provider globally, and a workspace
        // file that repeats it breaks the auth, so a fresh home gets no file.
        assert!(!folder.join("opencode.jsonc").exists());

        let again = configure_openwork_in(
            &config_dir,
            &user_data,
            &home,
            "test-token",
            1_700_000_000_001,
        )
        .unwrap();
        assert!(!again.provider_written);
        assert!(!again.env_written);
        assert!(!again.analytics_written);
        assert!(!again.workspace_config_written);
        assert!(!again.default_model_written);
        assert_eq!(
            again.default_model_workspace_id.as_deref(),
            Some(expected_id.as_str())
        );
    }

    #[test]
    fn replaces_an_enabled_analytics_store_and_drops_the_journey() {
        let config_dir = configure_dir("telemetry-config");
        let user_data = configure_dir("telemetry-user-data");
        let home = configure_dir("telemetry-home");
        std::fs::write(config_dir.join(ANALYTICS_STORE_FILE), "enabled\n").unwrap();
        std::fs::write(config_dir.join(ANALYTICS_JOURNEY_FILE), "journey-123").unwrap();

        let outcome =
            configure_openwork_in(&config_dir, &user_data, &home, "test-token", 1).unwrap();
        assert!(outcome.analytics_written);
        assert_eq!(
            std::fs::read_to_string(config_dir.join(ANALYTICS_STORE_FILE)).unwrap(),
            "disabled"
        );
        assert!(!config_dir.join(ANALYTICS_JOURNEY_FILE).exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(config_dir.join(ANALYTICS_STORE_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        let again = configure_openwork_in(&config_dir, &user_data, &home, "test-token", 2).unwrap();
        assert!(!again.analytics_written);
    }

    #[test]
    fn configures_the_workspace_from_an_existing_state_file() {
        let config_dir = configure_dir("state-config");
        let user_data = configure_dir("state-user-data");
        let home = configure_dir("state-home");
        let workspace_dir = configure_dir("state-workspace");
        std::fs::create_dir_all(&user_data).unwrap();
        std::fs::write(
            user_data.join("openwork-workspaces.json"),
            state_with_workspace("ws_05771fd9f545", &workspace_dir),
        )
        .unwrap();

        let outcome =
            configure_openwork_in(&config_dir, &user_data, &home, "test-token", 1).unwrap();
        assert!(outcome.default_model_written);
        assert!(!outcome.workspace_config_written);
        assert_eq!(
            outcome.default_model_workspace_id.as_deref(),
            Some("ws_05771fd9f545")
        );
        assert!(!default_workspace_dir(&home).exists());
        assert!(
            default_model_json(&config_dir.join("runtime.sqlite"), "ws_05771fd9f545").is_some()
        );
        assert!(!workspace_dir.join("opencode.jsonc").exists());
    }

    #[test]
    fn upserts_without_clobbering_existing_rows() {
        let config_dir = configure_dir("upsert-config");
        let user_data = configure_dir("upsert-user-data");
        let home = configure_dir("upsert-home");
        let workspace_dir = configure_dir("upsert-workspace");
        std::fs::create_dir_all(&user_data).unwrap();
        std::fs::write(
            user_data.join("openwork-workspaces.json"),
            state_with_workspace("ws_05771fd9f545", &workspace_dir),
        )
        .unwrap();
        std::fs::create_dir_all(&config_dir).unwrap();
        seeded_db(&config_dir.join("runtime.sqlite"));
        std::fs::write(
            config_dir.join("env.json"),
            r#"{"schemaVersion": 1, "updatedAt": 1, "variables": [{"key": "MINE", "value": "keep", "updatedAt": 1}]}"#,
        )
        .unwrap();

        let outcome =
            configure_openwork_in(&config_dir, &user_data, &home, "test-token", 2).unwrap();
        assert!(outcome.provider_written);
        assert!(outcome.env_written);
        assert!(!outcome.workspace_config_written);
        assert!(outcome.default_model_written);

        let db = config_dir.join("runtime.sqlite");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let db_mode = std::fs::metadata(&db).unwrap().permissions().mode();
            assert_eq!(db_mode & 0o777, 0o600);
        }
        let runtime: serde_json::Value =
            serde_json::from_str(&runtime_config_json(&db, "__openwork_engine_global__").unwrap())
                .unwrap();
        assert_eq!(
            runtime["provider"]["openwork-free"]["name"],
            "OpenWork Models (Free)"
        );
        assert_eq!(
            runtime["disabled_providers"],
            serde_json::json!(["opencode", OPENWORK_FREE_PROVIDER_ID])
        );
        assert!(runtime["provider"]["epfl-aiaas"].is_object());
        let other = default_model_json(&db, "ws_other").unwrap();
        assert_eq!(other, r#"{"providerID":"other","modelID":"model"}"#);
        let env: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(config_dir.join("env.json")).unwrap())
                .unwrap();
        let variables = env["variables"].as_array().unwrap();
        assert!(variables
            .iter()
            .any(|entry| entry["key"] == "MINE" && entry["value"] == "keep"));
        assert!(variables
            .iter()
            .any(|entry| entry["key"] == OPENWORK_ENV_KEY));

        let again = configure_openwork_in(&config_dir, &user_data, &home, "test-token", 3).unwrap();
        assert!(!again.provider_written);
        assert!(!again.env_written);
        assert!(!again.workspace_config_written);
        assert!(!again.default_model_written);
    }

    #[test]
    fn leaves_a_jsonc_workspace_config_with_comments_alone() {
        let config_dir = configure_dir("jsonc-config");
        let user_data = configure_dir("jsonc-user-data");
        let home = configure_dir("jsonc-home");
        let workspace_dir = configure_dir("jsonc-workspace");
        std::fs::create_dir_all(&user_data).unwrap();
        std::fs::write(
            user_data.join("openwork-workspaces.json"),
            state_with_workspace("ws_05771fd9f545", &workspace_dir),
        )
        .unwrap();
        let original =
            "// hand written\n{\n  \"provider\": {\"openwork-free\": {\"name\": \"kept\"}}\n}\n";
        std::fs::write(workspace_dir.join("opencode.jsonc"), original).unwrap();

        let outcome =
            configure_openwork_in(&config_dir, &user_data, &home, "test-token", 1).unwrap();
        assert!(outcome.provider_written);
        assert!(!outcome.workspace_config_written);
        assert_eq!(
            std::fs::read_to_string(workspace_dir.join("opencode.jsonc")).unwrap(),
            original
        );
    }

    #[test]
    fn heals_the_workspace_config_written_by_an_earlier_version() {
        let config_dir = configure_dir("heal-config");
        let user_data = configure_dir("heal-user-data");
        let home = configure_dir("heal-home");
        let workspace_dir = configure_dir("heal-workspace");
        std::fs::create_dir_all(&user_data).unwrap();
        std::fs::write(
            user_data.join("openwork-workspaces.json"),
            state_with_workspace("ws_05771fd9f545", &workspace_dir),
        )
        .unwrap();
        std::fs::write(
            workspace_dir.join("opencode.jsonc"),
            stale_workspace_config(),
        )
        .unwrap();

        let outcome =
            configure_openwork_in(&config_dir, &user_data, &home, "test-token", 1).unwrap();
        assert!(outcome.workspace_config_written);
        assert!(!workspace_dir.join("opencode.jsonc").exists());

        let again = configure_openwork_in(&config_dir, &user_data, &home, "test-token", 2).unwrap();
        assert!(!again.workspace_config_written);
        assert!(!workspace_dir.join("opencode.jsonc").exists());
    }

    #[test]
    fn keeps_foreign_keys_when_healing_the_workspace_config() {
        let config_dir = configure_dir("heal-foreign-config");
        let user_data = configure_dir("heal-foreign-user-data");
        let home = configure_dir("heal-foreign-home");
        let workspace_dir = configure_dir("heal-foreign-workspace");
        std::fs::create_dir_all(&user_data).unwrap();
        std::fs::write(
            user_data.join("openwork-workspaces.json"),
            state_with_workspace("ws_05771fd9f545", &workspace_dir),
        )
        .unwrap();
        let mut file: serde_json::Value = serde_json::from_str(&stale_workspace_config()).unwrap();
        file["$schema"] = serde_json::json!("https://opencode.ai/config.json");
        file["model"] = serde_json::json!("other/model");
        file["provider"]["other"] = serde_json::json!({"name": "kept"});
        std::fs::write(workspace_dir.join("opencode.jsonc"), file.to_string()).unwrap();

        let outcome =
            configure_openwork_in(&config_dir, &user_data, &home, "test-token", 1).unwrap();
        assert!(outcome.workspace_config_written);
        let healed: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(workspace_dir.join("opencode.jsonc")).unwrap(),
        )
        .unwrap();
        assert!(healed["provider"][OPENWORK_PROVIDER_ID].is_null());
        assert_eq!(healed["provider"]["other"]["name"], "kept");
        assert_eq!(healed["model"], "other/model");
        assert_eq!(healed["$schema"], "https://opencode.ai/config.json");
    }

    #[test]
    fn skips_the_workspace_config_for_a_remote_workspace() {
        let config_dir = configure_dir("remote-config");
        let user_data = configure_dir("remote-user-data");
        let home = configure_dir("remote-home");
        std::fs::create_dir_all(&user_data).unwrap();
        std::fs::write(
            user_data.join("openwork-workspaces.json"),
            r#"{"selectedId": "ws_remote", "workspaces": [{"id": "ws_remote", "path": "/home/tester/remote", "workspaceType": "remote"}]}"#,
        )
        .unwrap();

        let outcome =
            configure_openwork_in(&config_dir, &user_data, &home, "test-token", 1).unwrap();
        assert!(outcome.default_model_written);
        assert_eq!(
            outcome.default_model_workspace_id.as_deref(),
            Some("ws_remote")
        );
        assert!(!outcome.workspace_config_written);
        assert!(!default_workspace_dir(&home).exists());
        assert!(!Path::new("/home/tester/remote").exists());
    }

    #[test]
    fn skips_the_default_model_when_no_workspace_is_listed() {
        let config_dir = configure_dir("empty-state-config");
        let user_data = configure_dir("empty-state-user-data");
        let home = configure_dir("empty-state-home");
        std::fs::create_dir_all(&user_data).unwrap();
        std::fs::write(
            user_data.join("openwork-workspaces.json"),
            r#"{"selectedId": null, "workspaces": []}"#,
        )
        .unwrap();

        let outcome =
            configure_openwork_in(&config_dir, &user_data, &home, "test-token", 1).unwrap();
        assert!(outcome.provider_written);
        assert!(outcome.env_written);
        assert_eq!(outcome.default_model_workspace_id, None);
        assert!(!outcome.default_model_written);
        assert!(!default_workspace_dir(&home).exists());
    }

    #[test]
    fn skips_the_default_model_when_the_state_file_is_corrupt() {
        let config_dir = configure_dir("corrupt-state-config");
        let user_data = configure_dir("corrupt-state-user-data");
        let home = configure_dir("corrupt-state-home");
        std::fs::create_dir_all(&user_data).unwrap();
        std::fs::write(user_data.join("openwork-workspaces.json"), "not json").unwrap();

        let outcome =
            configure_openwork_in(&config_dir, &user_data, &home, "test-token", 1).unwrap();
        assert!(outcome.provider_written);
        assert_eq!(outcome.default_model_workspace_id, None);
    }

    #[test]
    fn fails_when_the_env_store_is_corrupt() {
        let config_dir = configure_dir("corrupt-env-config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("env.json"), "{oops").unwrap();

        let error = configure_openwork_in(
            &config_dir,
            &configure_dir("corrupt-env-user-data"),
            &configure_dir("corrupt-env-home"),
            "test-token",
            1,
        )
        .unwrap_err();
        assert_eq!(error.kind, "parse");
        assert!(error.message.contains("env.json"));
    }

    #[test]
    fn fails_when_the_runtime_row_is_corrupt() {
        let config_dir = configure_dir("corrupt-row-config");
        std::fs::create_dir_all(&config_dir).unwrap();
        seeded_db(&config_dir.join("runtime.sqlite"));
        {
            let conn = rusqlite::Connection::open(config_dir.join("runtime.sqlite")).unwrap();
            conn.execute(
                "UPDATE runtime_opencode_configs SET config_json = 'not json' WHERE workspace_id = '__openwork_engine_global__'",
                [],
            )
            .unwrap();
        }

        let error = configure_openwork_in(
            &config_dir,
            &configure_dir("corrupt-row-user-data"),
            &configure_dir("corrupt-row-home"),
            "test-token",
            1,
        )
        .unwrap_err();
        assert_eq!(error.kind, "parse");
    }

    #[test]
    #[ignore = "sandbox probe for the real OpenWork code: OW_VERIFY_SANDBOX=<home> cargo test -- --ignored"]
    fn configures_a_sandbox_home_for_real_app_verification() {
        let home = PathBuf::from(
            std::env::var_os("OW_VERIFY_SANDBOX")
                .expect("set OW_VERIFY_SANDBOX to the sandbox home")
                .to_string_lossy()
                .into_owned(),
        );
        let config_dir = unix_openwork_config_dir(None, &home);
        let user_data = home.join(".config").join(OPENWORK_USER_DATA_DIR);
        let folder = default_workspace_dir(&home);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(WORKSPACE_CONFIG_FILE), stale_workspace_config()).unwrap();

        let outcome =
            configure_openwork_in(&config_dir, &user_data, &home, "rcp-verify-token", now_ms())
                .unwrap();
        println!("{outcome:?}");
        // Acceptance: the default workspace ends up with the shape a fresh
        // workspace has, no provider override, while the runtime row keeps it.
        assert!(outcome.workspace_config_written);
        assert!(!folder.join(WORKSPACE_CONFIG_FILE).exists());
        let runtime = runtime_config_json(
            &config_dir.join("runtime.sqlite"),
            "__openwork_engine_global__",
        )
        .unwrap();
        assert!(runtime.contains(OPENWORK_PROVIDER_ID));
        let runtime: serde_json::Value = serde_json::from_str(&runtime).unwrap();
        let db = config_dir.join("runtime.sqlite");
        let workspace_id = workspace_id_for_path(&workspace_path_string(&folder));
        let model: serde_json::Value =
            serde_json::from_str(&default_model_json(&db, &workspace_id).unwrap()).unwrap();
        assert!(resolves_in(&runtime["provider"], &model));
        assert_eq!(model["modelID"], OPENWORK_MODEL_LARGE);
        let disabled: Vec<&str> = runtime["disabled_providers"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|id| id.as_str())
            .collect();
        assert!(disabled.contains(&OPENWORK_FREE_PROVIDER_ID));

        let again =
            configure_openwork_in(&config_dir, &user_data, &home, "rcp-verify-token", now_ms())
                .unwrap();
        assert!(!again.workspace_config_written);
        assert!(!folder.join(WORKSPACE_CONFIG_FILE).exists());
    }

    #[test]
    fn configures_a_home_without_any_openwork_state() {
        let home = configure_dir("virgin-home");
        let config_dir = unix_openwork_config_dir(None, &home);
        let user_data = home.join(".config").join(OPENWORK_USER_DATA_DIR);

        let outcome =
            configure_openwork_in(&config_dir, &user_data, &home, "test-token", 1).unwrap();
        assert!(outcome.env_written);
        assert!(outcome.provider_written);
        assert!(!outcome.workspace_config_written);
        assert!(outcome.default_model_written);
        assert!(config_dir.join("env.json").is_file());
        assert!(config_dir.join("runtime.sqlite").is_file());
    }

    #[test]
    fn fails_without_a_key() {
        let error = configure_openwork_in(
            &configure_dir("no-key-config"),
            &configure_dir("no-key-user-data"),
            &configure_dir("no-key-home"),
            "  ",
            1,
        )
        .unwrap_err();
        assert_eq!(error.kind, "missing-key");
    }
}
