use crate::types::{AppError, CreatedKey, PortalForm};

#[tauri::command]
pub async fn portal_start_login() -> Result<(), AppError> {
    Err(AppError::unimplemented("portal_start_login"))
}

#[tauri::command]
pub async fn portal_get_form() -> Result<PortalForm, AppError> {
    Err(AppError::unimplemented("portal_get_form"))
}

#[tauri::command]
pub async fn portal_create_key(name: String, unit: Option<String>) -> Result<CreatedKey, AppError> {
    let _ = (name, unit);
    Err(AppError::unimplemented("portal_create_key"))
}

#[tauri::command]
pub async fn portal_close() -> Result<(), AppError> {
    Err(AppError::unimplemented("portal_close"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stubs_report_unimplemented() {
        let login = tauri::async_runtime::block_on(portal_start_login()).unwrap_err();
        assert_eq!(login.kind, "unimplemented");

        let form = tauri::async_runtime::block_on(portal_get_form()).unwrap_err();
        assert_eq!(form.kind, "unimplemented");

        let key =
            tauri::async_runtime::block_on(portal_create_key("Alice".into(), None)).unwrap_err();
        assert_eq!(key.kind, "unimplemented");

        let close = tauri::async_runtime::block_on(portal_close()).unwrap_err();
        assert_eq!(close.kind, "unimplemented");
    }
}
