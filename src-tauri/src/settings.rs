use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// 帧录制：把一件工件的整帧图像与元数据落盘，供回放相机重放。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordMode {
    #[default]
    Off,
    /// 只留 NG / ERR 件
    Failed,
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProductSource {
    Plc,
    Manual,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Timeouts {
    pub arm_ms: u64,
    pub motion_ms: u64,
    pub drain_ms: u64,
    pub proc_ms: u64,
    pub ack_ms: u64,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self { arm_ms: 200, motion_ms: 30_000, drain_ms: 1000, proc_ms: 3000, ack_ms: 5000 }
    }
}

impl Timeouts {
    pub fn arm(&self) -> Duration {
        Duration::from_millis(self.arm_ms)
    }
    pub fn motion(&self) -> Duration {
        Duration::from_millis(self.motion_ms)
    }
    pub fn drain(&self) -> Duration {
        Duration::from_millis(self.drain_ms)
    }
    pub fn proc(&self) -> Duration {
        Duration::from_millis(self.proc_ms)
    }
    pub fn ack(&self) -> Duration {
        Duration::from_millis(self.ack_ms)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CycleSettings {
    pub product_source: ProductSource,
    pub manual_recipe_id: Option<String>,
    pub timeouts: Timeouts,
    pub history_days: u32,
    /// lyFlow 核心库（lyflow_core.dll）路径
    pub lyflow_core: Option<String>,
    /// 飞拍配方用 lyFlow 流程测量图像；关闭时用模拟测量
    pub vision: bool,
    /// 随动配方用本程序卡尺测量图像；关闭时用模拟测量
    #[serde(default = "yes")]
    pub follow_vision: bool,
    pub record: RecordMode,
    /// 帧录制最多保留多少件、总共多少 GB，超出删最旧的（随动一件三路约 0.5 GB）
    pub record_keep: u32,
    pub record_max_gb: f32,
}

impl Default for CycleSettings {
    fn default() -> Self {
        Self {
            product_source: ProductSource::Plc,
            manual_recipe_id: None,
            timeouts: Timeouts::default(),
            history_days: 180,
            lyflow_core: std::env::var("LYFLOW_CORE_DLL").ok(),
            vision: false,
            follow_vision: true,
            record: RecordMode::Off,
            record_keep: 100,
            record_max_gb: 20.0,
        }
    }
}

fn yes() -> bool {
    true
}

impl CycleSettings {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("写入配置失败: {e}"))
    }

    pub fn validate(&self) -> Result<(), String> {
        let t = &self.timeouts;
        if t.drain_ms < 200 {
            return Err("收尾等待不能小于 200 ms".into());
        }
        if !(1..=3650).contains(&self.history_days) {
            return Err("记录保留天数需在 1–3650 之间".into());
        }
        if t.motion_ms < 1000 || t.proc_ms < 200 || t.ack_ms < 500 {
            return Err("超时参数过小".into());
        }
        if !(1..=100_000).contains(&self.record_keep) {
            return Err("帧录制保留件数需在 1–100000 之间".into());
        }
        if !(self.record_max_gb >= 0.5 && self.record_max_gb <= 10_000.0) {
            return Err("帧录制总大小需在 0.5–10000 GB 之间".into());
        }
        Ok(())
    }
}
