mod commands;
mod detection;
mod inspection;
mod plc;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            app.manage(plc::PlcHost::init(app.handle())?);
            plc::PlcHost::start_if_configured(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::engine_status,
            plc::plc_get_config,
            plc::plc_save_config,
            plc::plc_connect,
            plc::plc_disconnect,
            plc::plc_get_status,
            plc::plc_get_values,
            plc::plc_write_point,
            plc::plc_query_logs,
            plc::plc_point_history,
            plc::plc_check_address,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
