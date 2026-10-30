mod opencode;
mod openwork;
mod portal;
mod smoke;
mod types;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            opencode::opencode_status,
            opencode::install_opencode,
            openwork::openwork_status,
            openwork::install_openwork,
            smoke::smoke_test,
            portal::portal_start_login,
            portal::portal_get_form,
            portal::portal_create_key,
            portal::portal_close,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
