use std::collections::HashMap;
use std::sync::RwLock;

use ly_plc::{Access, ConnectionConfig, DataType, EdgeMode, HeartbeatConfig, PlcConfig, PlcEngine, PlcPoint, PlcValue};
use serde_json::Value;

pub mod tag {
    pub const PART_START: &str = "partStart";
    pub const PART_END: &str = "partEnd";
    pub const RESULT_ACK: &str = "resultAck";
    pub const FAULT_RESET: &str = "faultReset";
    pub const PART_SN: &str = "partSn";
    pub const PRODUCT_CODE: &str = "productCode";
    pub const SHOT_COUNT: &str = "shotCount";
    pub const VISION_READY: &str = "visionReady";
    pub const ARMED: &str = "armed";
    pub const BUSY: &str = "busy";
    pub const DONE: &str = "done";
    pub const RESULT_CODE: &str = "resultCode";
    pub const FAULT_CODE: &str = "faultCode";
    pub const RESULT_SN: &str = "resultSn";
    /// 随动：机器人已走过的胶路弧长（按配方里的换算系数转成 mm）
    pub const PATH_PROGRESS: &str = "pathProgress";
}

/// 标签 → 点位 id。随动每帧都要读进度，不能每次都把整个地址表克隆一遍；地址表保存后清空重建。
static TAG_IDS: RwLock<Option<HashMap<String, String>>> = RwLock::new(None);

pub fn invalidate_tags() {
    *TAG_IDS.write().unwrap() = None;
}

fn point_id(engine: &PlcEngine, tag: &str) -> Option<String> {
    if let Some(map) = TAG_IDS.read().unwrap().as_ref() {
        return map.get(tag).cloned();
    }
    let mut map = HashMap::new();
    for p in engine.config().points {
        for t in p.tags {
            // 同一标签挂在多个点位上时取第一个，与原来的查找顺序一致
            map.entry(t).or_insert_with(|| p.id.clone());
        }
    }
    let id = map.get(tag).cloned();
    *TAG_IDS.write().unwrap() = Some(map);
    id
}

pub fn read_tag(engine: &PlcEngine, tag: &str) -> Option<PlcValue> {
    let id = point_id(engine, tag)?;
    engine.values().get(&id).and_then(|v| v.value.clone())
}

pub fn tag_is_on(engine: &PlcEngine, tag: &str) -> bool {
    read_tag(engine, tag).is_some_and(|v| v.is_truthy())
}

/// 标签的数值与读到它的轮询时刻（ms）。
pub fn read_tag_f32_ts(engine: &PlcEngine, tag: &str) -> Option<(f32, i64)> {
    let id = point_id(engine, tag)?;
    let v = engine.values().get(&id)?.clone();
    let x = match v.value? {
        PlcValue::Int(i) => i as f32,
        PlcValue::Float(f) => f as f32,
        PlcValue::Bool(b) => b as u8 as f32,
    };
    Some((x, v.ts))
}

pub fn read_tag_u32(engine: &PlcEngine, tag: &str) -> Option<u32> {
    match read_tag(engine, tag)? {
        PlcValue::Int(i) => u32::try_from(i).ok(),
        PlcValue::Float(f) => Some(f as u32),
        PlcValue::Bool(b) => Some(b as u32),
    }
}

pub async fn write_tag(engine: &PlcEngine, tag: &str, value: Value) -> Result<(), String> {
    let id = point_id(engine, tag).ok_or_else(|| format!("地址表中没有标签为 {tag} 的点位"))?;
    engine.write_point(&id, &value).await
}

/// PLC→PC 信号默认放在线圈和保持寄存器上，模拟器协议下软件可以代替 PLC 写入它们来跑模拟节拍。
pub fn default_plc_config() -> PlcConfig {
    let point = |id: &str, name: &str, address: &str, data_type, edge, tag: &str| PlcPoint {
        id: id.into(),
        name: name.into(),
        address: address.into(),
        data_type,
        access: Access::ReadWrite,
        edge,
        log_changes: true,
        tags: if tag.is_empty() { Vec::new() } else { vec![tag.to_string()] },
        ..PlcPoint::default()
    };
    use DataType::{Bool, U16, U32};
    use EdgeMode::{None as NoEdge, Rising};
    PlcConfig {
        connection: ConnectionConfig { poll_interval_ms: 50, ..ConnectionConfig::default() },
        points: vec![
            point("p_part_start", "工件开始", "C10", Bool, Rising, tag::PART_START),
            point("p_part_end", "运动结束", "C11", Bool, Rising, tag::PART_END),
            point("p_result_ack", "结果确认", "C12", Bool, Rising, tag::RESULT_ACK),
            point("p_fault_reset", "故障复位", "C13", Bool, Rising, tag::FAULT_RESET),
            point("p_part_sn", "工件序列号", "HR100", U32, NoEdge, tag::PART_SN),
            point("p_product_code", "产品代码", "HR102", U16, NoEdge, tag::PRODUCT_CODE),
            point("p_shot_count", "计划拍照点数", "HR103", U16, NoEdge, tag::SHOT_COUNT),
            PlcPoint { log_changes: false, ..point("p_path_progress", "随动进度（0.1 mm）", "HR104", U32, NoEdge, tag::PATH_PROGRESS) },
            point("p_vision_ready", "视觉就绪", "C20", Bool, NoEdge, tag::VISION_READY),
            point("p_armed", "已布防", "C21", Bool, NoEdge, tag::ARMED),
            point("p_busy", "检测中", "C22", Bool, NoEdge, tag::BUSY),
            point("p_done", "结果有效", "C23", Bool, NoEdge, tag::DONE),
            point("p_result_code", "结果码", "HR110", U16, NoEdge, tag::RESULT_CODE),
            point("p_fault_code", "异常码", "HR111", U16, NoEdge, tag::FAULT_CODE),
            point("p_result_sn", "结果 SN", "HR112", U32, NoEdge, tag::RESULT_SN),
            PlcPoint { log_changes: false, ..point("p_heartbeat", "上位机心跳", "C0", Bool, NoEdge, "") },
        ],
        heartbeat: HeartbeatConfig { point_id: Some("p_heartbeat".into()), interval_ms: 1000 },
        auto_connect: true,
        ..PlcConfig::default()
    }
}
