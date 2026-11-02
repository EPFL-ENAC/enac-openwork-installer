mod opencode;
mod openwork;
mod openwork_state;
mod portal;
mod prereq;
mod provider_config;
mod smoke;
mod types;
mod util;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(portal::PortalState::default())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            opencode::opencode_status,
            opencode::install_opencode,
            prereq::linux_prereq_status,
            openwork::openwork_status,
            openwork::install_openwork,
            openwork::launch_openwork,
            openwork::configure_openwork,
            openwork_state::openwork_state_status,
            openwork_state::openwork_repair_state,
            smoke::smoke_test,
            provider_config::restrict_provider_config,
            portal::portal_start_login,
            portal::portal_get_form,
            portal::portal_list_keys,
            portal::portal_use_key,
            portal::portal_create_key,
            portal::portal_close,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
