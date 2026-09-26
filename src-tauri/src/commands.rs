use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::cycle::CycleHost;
use crate::vision::VisionHost;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    name: String,
    version: String,
}

#[tauri::command]
pub fn app_info(app: AppHandle) -> AppInfo {
    let pkg = app.package_info();
    AppInfo {
        name: pkg.name.clone(),
        version: pkg.version.to_string(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    backend: &'static str,
    ready: bool,
    message: String,
    version: Option<String>,
    path: Option<String>,
    measuring: bool,
}

#[tauri::command]
pub async fn engine_status(app: AppHandle) -> EngineStatus {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = app.state::<CycleHost>().settings();
        let s = app.state::<VisionHost>().status(settings.lyflow_core.as_deref());
        let measuring = settings.vision && s.loaded;
        EngineStatus {
            backend: "LyFlow",
            ready: s.loaded,
            message: if s.loaded && !settings.vision { "已加载，未启用（使用模拟测量）".into() } else { s.message },
            version: s.version,
            path: s.path,
            measuring,
        }
    })
    .await
    .unwrap_or(EngineStatus { backend: "LyFlow", ready: false, message: "查询失败".into(), version: None, path: None, measuring: false })
}
