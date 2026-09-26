mod camera;
mod commands;
mod cycle;
mod detection;
mod inspection;
mod judge;
mod measure;
mod mvs;
mod plc;
mod recipe;
mod settings;
mod sim;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            app.manage(cycle::CycleHost::init(app.handle())?);
            app.manage(plc::PlcHost::init(app.handle())?);
            plc::PlcHost::start_if_configured(app.handle());
            cycle::CycleHost::start(app.handle());
            camera::CameraHost::start(app.handle());
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
            cycle::cycle_snapshot,
            cycle::cycle_logs,
            cycle::cycle_part_data,
            cycle::cycle_recipes,
            cycle::cycle_layout,
            cycle::cycle_get_settings,
            cycle::cycle_save_settings,
            cycle::cycle_select_recipe,
            cycle::cycle_reset,
            camera::camera_status,
            camera::camera_get_config,
            camera::camera_save_config,
            camera::camera_list_devices,
            camera::camera_preview,
            camera::camera_soft_trigger,
            camera::camera_dry_run_start,
            camera::camera_dry_run_get,
            camera::camera_dry_run_stop,
            sim::sim_status,
            sim::sim_start,
            sim::sim_stop,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
