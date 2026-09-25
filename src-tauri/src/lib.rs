mod commands;
mod detection;

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::engine_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
