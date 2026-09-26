use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ly_plc::now_ms;
use serde::Serialize;
use tokio::sync::mpsc::UnboundedSender;

use crate::cycle::Input;

/// 一帧图像的元数据。计数器对应相机 Chunk 中的帧计数与 Line0 触发计数。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub frame_counter: u64,
    pub trigger_counter: u64,
    pub ts: i64,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraStatus {
    pub source: &'static str,
    pub ready: bool,
    pub triggers: u64,
    pub frames: u64,
}

/// 模拟相机：收到触发后经过传输延时交付一帧。海康 MVS 接入（P2）后作为开发与回归用的图像源保留。
pub struct SimCamera {
    tx: UnboundedSender<Input>,
    triggers: Arc<AtomicU64>,
    frames: Arc<AtomicU64>,
    transfer: Duration,
}

impl SimCamera {
    pub fn new(tx: UnboundedSender<Input>) -> Self {
        Self {
            tx,
            triggers: Arc::default(),
            frames: Arc::default(),
            transfer: Duration::from_millis(180),
        }
    }

    pub fn status(&self) -> CameraStatus {
        CameraStatus {
            source: "模拟相机",
            ready: true,
            triggers: self.triggers.load(Ordering::SeqCst),
            frames: self.frames.load(Ordering::SeqCst),
        }
    }

    /// Line0 上升沿。`lose_in_transfer` 模拟相机已曝光但帧在 GigE 传输中丢失，主机侧表现为帧计数跳号。
    pub fn trigger(&self, lose_in_transfer: bool) {
        let trigger_counter = self.triggers.fetch_add(1, Ordering::SeqCst) + 1;
        let frame_counter = self.frames.fetch_add(1, Ordering::SeqCst) + 1;
        if lose_in_transfer {
            return;
        }
        let tx = self.tx.clone();
        let delay = self.transfer;
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = tx.send(Input::Frame(Frame { frame_counter, trigger_counter, ts: now_ms() }));
        });
    }
}
