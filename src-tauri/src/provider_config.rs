use crate::types::AppError;
use crate::util;
use std::path::Path;

pub const CONFIG_PATH: &str = ".config/opencode/opencode.json";
pub const BACKUP_PATH: &str = ".config/opencode/opencode.json.bak";

/// The OpenCode config carries the RCP token, so the config and its backup must
/// stay readable by the owner only.
#[tauri::command]
pub fn restrict_provider_config() -> Result<(), AppError> {
    let Some(home) = util::home_dir() else {
        return Ok(());
    };
    for path in [CONFIG_PATH, BACKUP_PATH] {
        restrict(&home.join(path))?;
    }
    Ok(())
}

fn restrict(path: &Path) -> Result<(), AppError> {
    if !path.exists() {
        return Ok(());
    }
    set_owner_only(path)
        .map_err(|e| AppError::io(format!("cannot restrict {}: {e}", path.display())))
}

#[cfg(unix)]
fn set_owner_only(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

// Windows ignores POSIX modes, and %USERPROFILE% is already per-user.
#[cfg(not(unix))]
fn set_owner_only(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util;

    #[test]
    fn restrict_keeps_the_file_readable_only_by_the_owner() {
        let dir = util::temp_dir("provider-config").expect("temp dir");
        let path = dir.join("opencode.json");
        std::fs::write(&path, "{}").expect("write fixture");

        restrict(&path).expect("restrict succeeds");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }

        util::remove_dir(&dir);
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        let dir = util::temp_dir("provider-config-missing").expect("temp dir");

        assert!(restrict(&dir.join("absent.json")).is_ok());

        util::remove_dir(&dir);
    }
}
