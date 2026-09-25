use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use ly_plc::{
    describe_address, ConnectionConfig, DataType, EventSink, HistorySample, LogPage, LogQuery, Logbook, PlcConfig,
    PlcEngine, PlcEvent, PlcStatus, PointValue,
};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::inspection;

pub struct PlcHost {
    engine: PlcEngine,
    config_path: PathBuf,
}

impl PlcHost {
    pub fn init(app: &AppHandle) -> Result<Self, String> {
        let config_path = app.path().app_config_dir().map_err(|e| e.to_string())?.join("plc.json");
        let log_path = app.path().app_data_dir().map_err(|e| e.to_string())?.join("logs").join("plc.db");
        let config = PlcConfig::load(&config_path).unwrap_or_else(inspection::default_plc_config);
        let handle = app.clone();
        let sink: EventSink = Arc::new(move |event| match event {
            PlcEvent::Status(s) => {
                let _ = handle.emit("plc://status", s);
            }
            PlcEvent::Values(v) => {
                let _ = handle.emit("plc://values", v);
            }
            PlcEvent::Logs(l) => {
                let _ = handle.emit("plc://log", l);
            }
            PlcEvent::Edge(e) => {
                let _ = handle.emit("plc://edge", &e);
                inspection::on_plc_edge(&handle, &e);
            }
        });
        let logbook = Arc::new(Logbook::open(&log_path, sink.clone(), config.log_retention_days)?);
        Ok(Self { engine: PlcEngine::new(config, logbook, sink), config_path })
    }

    pub fn start_if_configured(app: &AppHandle) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let host = app.state::<PlcHost>();
            if host.engine.config().auto_connect {
                host.engine.connect().await;
            }
        });
    }
}

#[tauri::command]
pub fn plc_get_config(plc: State<'_, PlcHost>) -> PlcConfig {
    plc.engine.config()
}

#[tauri::command]
pub async fn plc_save_config(plc: State<'_, PlcHost>, config: PlcConfig) -> Result<(), String> {
    config.validate()?;
    config.save(&plc.config_path)?;
    plc.engine.apply_config(config).await
}

#[tauri::command]
pub async fn plc_connect(plc: State<'_, PlcHost>) -> Result<(), String> {
    plc.engine.connect().await;
    Ok(())
}

#[tauri::command]
pub async fn plc_disconnect(plc: State<'_, PlcHost>) -> Result<(), String> {
    plc.engine.disconnect().await;
    Ok(())
}

#[tauri::command]
pub fn plc_get_status(plc: State<'_, PlcHost>) -> PlcStatus {
    plc.engine.status()
}

#[tauri::command]
pub fn plc_get_values(plc: State<'_, PlcHost>) -> HashMap<String, PointValue> {
    plc.engine.values()
}

#[tauri::command]
pub async fn plc_write_point(plc: State<'_, PlcHost>, id: String, value: Value) -> Result<(), String> {
    plc.engine.write_point(&id, &value).await
}

#[tauri::command]
pub async fn plc_query_logs(plc: State<'_, PlcHost>, query: LogQuery) -> Result<LogPage, String> {
    plc.engine.logbook().query(&query)
}

#[tauri::command]
pub async fn plc_point_history(
    plc: State<'_, PlcHost>,
    point_id: String,
    start: i64,
    end: i64,
) -> Result<Vec<HistorySample>, String> {
    plc.engine.logbook().point_history(&point_id, start, end)
}

#[tauri::command]
pub fn plc_check_address(connection: ConnectionConfig, address: String, data_type: DataType) -> Result<String, String> {
    describe_address(&connection, &address, data_type)
}
