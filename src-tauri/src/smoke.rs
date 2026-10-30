use crate::types::{AppError, SmokeResult};

#[tauri::command]
pub async fn smoke_test() -> Result<SmokeResult, AppError> {
    Err(AppError::unimplemented("smoke_test"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_reports_unimplemented() {
        let error = tauri::async_runtime::block_on(smoke_test()).unwrap_err();
        assert_eq!(error.kind, "unimplemented");
    }
}
