use ly_plc::{Access, DataType, EdgeEvent, EdgeMode, HeartbeatConfig, PlcConfig, PlcPoint};
use tauri::{AppHandle, Emitter};

pub const TAG_TRIGGER: &str = "trigger";
pub const TAG_RESULT_CODE: &str = "resultCode";
pub const TAG_GLUE_WIDTH: &str = "glueWidth";

pub fn on_plc_edge(app: &AppHandle, edge: &EdgeEvent) {
    if edge.rising && edge.tags.iter().any(|t| t == TAG_TRIGGER) {
        let _ = app.emit("inspection://trigger", edge);
    }
}

pub fn default_plc_config() -> PlcConfig {
    let point = |id: &str, name: &str, address: &str, data_type, access, edge, log_changes, tags: &[&str]| PlcPoint {
        id: id.into(),
        name: name.into(),
        address: address.into(),
        data_type,
        access,
        edge,
        log_changes,
        tags: tags.iter().map(|t| t.to_string()).collect(),
        ..PlcPoint::default()
    };
    PlcConfig {
        points: vec![
            point("p_trigger", "检测触发", "DI0", DataType::Bool, Access::Read, EdgeMode::Rising, true, &[TAG_TRIGGER]),
            point("p_counter", "运行计数", "IR0", DataType::U16, Access::Read, EdgeMode::None, false, &[]),
            point("p_heartbeat", "上位机心跳", "C0", DataType::Bool, Access::ReadWrite, EdgeMode::None, false, &[]),
            point("p_result", "检测结果", "HR10", DataType::U16, Access::ReadWrite, EdgeMode::None, true, &[TAG_RESULT_CODE]),
            point("p_width", "胶宽 (mm)", "HR20", DataType::F32, Access::ReadWrite, EdgeMode::None, true, &[TAG_GLUE_WIDTH]),
        ],
        heartbeat: HeartbeatConfig { point_id: Some("p_heartbeat".into()), interval_ms: 1000 },
        ..PlcConfig::default()
    }
}
