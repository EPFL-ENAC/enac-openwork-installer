use crate::types::{AppError, InstallResult, InstallStatus};

#[tauri::command]
pub async fn opencode_status() -> Result<InstallStatus, AppError> {
    Err(AppError::unimplemented("opencode_status"))
}

#[tauri::command]
pub async fn install_opencode() -> Result<InstallResult, AppError> {
    Err(AppError::unimplemented("install_opencode"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stubs_report_unimplemented() {
        let status = tauri::async_runtime::block_on(opencode_status()).unwrap_err();
        assert_eq!(status.kind, "unimplemented");

        let install = tauri::async_runtime::block_on(install_opencode()).unwrap_err();
        assert_eq!(install.kind, "unimplemented");
    }
}
