use crate::types::{AppError, InstallResult, InstallStatus};

#[tauri::command]
pub async fn openwork_status() -> Result<InstallStatus, AppError> {
    Err(AppError::unimplemented("openwork_status"))
}

#[tauri::command]
pub async fn install_openwork() -> Result<InstallResult, AppError> {
    Err(AppError::unimplemented("install_openwork"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stubs_report_unimplemented() {
        let status = tauri::async_runtime::block_on(openwork_status()).unwrap_err();
        assert_eq!(status.kind, "unimplemented");

        let install = tauri::async_runtime::block_on(install_openwork()).unwrap_err();
        assert_eq!(install.kind, "unimplemented");
    }
}
