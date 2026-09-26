use std::collections::VecDeque;
use std::ffi::{c_uint, c_void};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ly_plc::now_ms;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc::UnboundedSender;

use crate::cycle::{self, CycleHost, Input, Phase};
use crate::mvs::{self, DeviceSummary, FrameInfo};

/// 一帧图像的元数据。计数器取自相机 Chunk（帧计数、Line0 触发计数），未开启 Chunk 时退化为 SDK 帧号。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub frame_counter: u64,
    pub trigger_counter: u64,
    pub lost_packets: u32,
    pub ts: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CameraSource {
    Sim,
    Mvs,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CameraConfig {
    pub source: CameraSource,
    pub serial: String,
    pub trigger_source: String,
    pub trigger_activation: String,
    pub trigger_delay_us: f32,
    pub debouncer_us: i64,
    pub exposure_us: f32,
    pub gain_db: f32,
    pub strobe: bool,
    pub chunk: bool,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            source: CameraSource::Sim,
            serial: String::new(),
            trigger_source: "Line0".into(),
            trigger_activation: "RisingEdge".into(),
            trigger_delay_us: 0.0,
            debouncer_us: 5,
            exposure_us: 60.0,
            gain_db: 6.0,
            strobe: true,
            chunk: true,
        }
    }
}

impl CameraConfig {
    fn load(path: &PathBuf) -> Self {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    fn save(&self, path: &PathBuf) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self).map_err(|e| e.to_string())?).map_err(|e| format!("写入配置失败: {e}"))
    }

    fn validate(&self) -> Result<(), String> {
        if !(1.0..=1_000_000.0).contains(&self.exposure_us) {
            return Err("曝光时间需在 1–1000000 µs 之间".into());
        }
        if !["Line0", "Software"].contains(&self.trigger_source.as_str()) {
            return Err("触发源只能是 Line0 或 Software".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraStatus {
    pub source: CameraSource,
    pub ready: bool,
    pub message: String,
    pub device: Option<DeviceSummary>,
    pub sdk_version: Option<String>,
    pub frames: u64,
    pub fps: f32,
    pub max_fps: Option<f32>,
    pub lost_packets: u64,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DryFrame {
    pub t_ms: f64,
    pub frame_counter: u64,
    pub trigger_counter: u64,
    pub lost_packets: u32,
}

struct Preview {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

/// 取图回调与模拟相机共用的交付通道。
struct Shared {
    app: AppHandle,
    tx: UnboundedSender<Input>,
    frames: AtomicU64,
    lost_packets: AtomicU64,
    recent: Mutex<VecDeque<Instant>>,
    preview: Mutex<Option<Preview>>,
    dry_run: Mutex<Option<(Instant, Vec<DryFrame>)>>,
    disconnected: AtomicBool,
}

impl Shared {
    fn deliver(&self, frame: Frame) {
        self.frames.fetch_add(1, Ordering::Relaxed);
        self.lost_packets.fetch_add(frame.lost_packets as u64, Ordering::Relaxed);
        {
            let now = Instant::now();
            let mut recent = self.recent.lock().unwrap();
            recent.push_back(now);
            while recent.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(3)) {
                recent.pop_front();
            }
        }
        let _ = self.app.emit("camera://frame", &frame);
        if let Some((started, frames)) = self.dry_run.lock().unwrap().as_mut() {
            frames.push(DryFrame {
                t_ms: started.elapsed().as_secs_f64() * 1000.0,
                frame_counter: frame.frame_counter,
                trigger_counter: frame.trigger_counter,
                lost_packets: frame.lost_packets,
            });
            return;
        }
        let _ = self.tx.send(Input::Frame(frame));
    }

    fn fps(&self) -> f32 {
        let recent = self.recent.lock().unwrap();
        match (recent.front(), recent.back()) {
            (Some(a), Some(b)) if recent.len() > 1 && Instant::now().duration_since(*b) < Duration::from_secs(3) => {
                (recent.len() - 1) as f32 / b.duration_since(*a).as_secs_f32().max(0.001)
            }
            _ => 0.0,
        }
    }
}

extern "system" fn on_image(data: *mut u8, info: *mut FrameInfo, user: *mut c_void) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if info.is_null() || user.is_null() {
            return;
        }
        let shared = unsafe { &*(user as *const Shared) };
        let info = unsafe { &*info };
        let frame_counter = if info.frame_counter != 0 { info.frame_counter } else { info.frame_num } as u64;
        let trigger_counter = if info.trigger_index != 0 { info.trigger_index as u64 } else { frame_counter };
        if info.pixel_type == mvs::PIXEL_MONO8 && !data.is_null() {
            let w = if info.extend_width != 0 { info.extend_width } else { info.width as u32 } as usize;
            let h = if info.extend_height != 0 { info.extend_height } else { info.height as u32 } as usize;
            let src = unsafe { std::slice::from_raw_parts(data, w * h) };
            let step = w.div_ceil(960).max(1);
            let (pw, ph) = (w / step, h / step);
            let mut out = Vec::with_capacity(pw * ph);
            for y in 0..ph {
                let row = &src[y * step * w..];
                out.extend((0..pw).map(|x| row[x * step]));
            }
            *shared.preview.lock().unwrap() = Some(Preview { width: pw as u32, height: ph as u32, data: out });
        }
        shared.deliver(Frame { frame_counter, trigger_counter, lost_packets: info.lost_packet, ts: now_ms() });
    }));
}

extern "system" fn on_exception(msg: c_uint, user: *mut c_void) {
    if msg == mvs::MV_EXCEPTION_DEV_DISCONNECT && !user.is_null() {
        let shared = unsafe { &*(user as *const Shared) };
        shared.disconnected.store(true, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct DeviceState {
    message: String,
    warnings: Vec<String>,
    max_fps: Option<f32>,
}

pub struct CameraHost {
    shared: Arc<Shared>,
    config: Mutex<CameraConfig>,
    config_path: PathBuf,
    device: Mutex<Option<mvs::Device>>,
    state: Mutex<DeviceState>,
    sim_frames: Arc<AtomicU64>,
    sim_triggers: AtomicU64,
}

impl CameraHost {
    pub fn new(app: &AppHandle, tx: UnboundedSender<Input>) -> Result<Self, String> {
        let config_path = app.path().app_config_dir().map_err(|e| e.to_string())?.join("camera.json");
        Ok(Self {
            shared: Arc::new(Shared {
                app: app.clone(),
                tx,
                frames: AtomicU64::new(0),
                lost_packets: AtomicU64::new(0),
                recent: Mutex::new(VecDeque::new()),
                preview: Mutex::new(None),
                dry_run: Mutex::new(None),
                disconnected: AtomicBool::new(false),
            }),
            config: Mutex::new(CameraConfig::load(&config_path)),
            config_path,
            device: Mutex::new(None),
            state: Mutex::new(DeviceState::default()),
            sim_frames: Arc::default(),
            sim_triggers: AtomicU64::new(0),
        })
    }

    pub fn config(&self) -> CameraConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn status(&self) -> CameraStatus {
        let config = self.config();
        let state = self.state.lock().unwrap();
        let device = self.device.lock().unwrap();
        let (ready, device_summary) = match config.source {
            CameraSource::Sim => (true, None),
            CameraSource::Mvs => (
                device.is_some() && !self.shared.disconnected.load(Ordering::SeqCst),
                device.as_ref().map(|d| d.summary.clone()),
            ),
        };
        CameraStatus {
            source: config.source,
            ready,
            message: if config.source == CameraSource::Sim { "模拟相机：收到触发后约 180 ms 交付一帧".into() } else { state.message.clone() },
            device: device_summary,
            sdk_version: mvs::api().ok().map(|a| a.version.clone()),
            frames: self.shared.frames.load(Ordering::Relaxed),
            fps: self.shared.fps(),
            max_fps: state.max_fps,
            lost_packets: self.shared.lost_packets.load(Ordering::Relaxed),
            warnings: state.warnings.clone(),
        }
    }

    /// Line0 上升沿（模拟相机）或软触发（MVS 且触发源为 Software）。返回是否实际发出了触发。
    /// `lose_in_transfer` 仅模拟相机使用：相机已曝光但帧在传输中丢失，主机侧表现为帧计数跳号。
    pub fn trigger(&self, lose_in_transfer: bool) -> bool {
        let config = self.config();
        match config.source {
            CameraSource::Sim => {
                let trigger_counter = self.sim_triggers.fetch_add(1, Ordering::SeqCst) + 1;
                let frame_counter = self.sim_frames.fetch_add(1, Ordering::SeqCst) + 1;
                if !lose_in_transfer {
                    let shared = self.shared.clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(180)).await;
                        shared.deliver(Frame { frame_counter, trigger_counter, lost_packets: 0, ts: now_ms() });
                    });
                }
                true
            }
            CameraSource::Mvs => {
                config.trigger_source == "Software"
                    && self.device.lock().unwrap().as_ref().is_some_and(|d| d.command("TriggerSoftware").is_ok())
            }
        }
    }

    fn close(&self) {
        self.device.lock().unwrap().take();
        self.shared.disconnected.store(false, Ordering::SeqCst);
    }

    fn open(&self) -> Result<String, String> {
        self.close();
        let result = self.try_open();
        if let Err(e) = &result {
            self.state.lock().unwrap().message = e.clone();
        }
        result
    }

    fn try_open(&self) -> Result<String, String> {
        let config = self.config();
        let device = mvs::Device::open(&config.serial)?;
        let (warnings, max_fps) = apply(&device, &config);
        device.start(on_image, on_exception, Arc::as_ptr(&self.shared) as *mut c_void)?;
        let msg = format!("已连接 {} · {}", device.summary.model, device.summary.serial);
        *self.device.lock().unwrap() = Some(device);
        let mut state = self.state.lock().unwrap();
        state.warnings = warnings;
        state.max_fps = max_fps;
        state.message = msg.clone();
        Ok(msg)
    }

    /// 相机源为 MVS 时保持连接：未打开或掉线后每 2 s 重试一次。
    pub fn start(app: &AppHandle) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut last_error: Option<String> = None;
            loop {
                let host = &app.state::<CycleHost>().camera;
                let lost = host.shared.disconnected.load(Ordering::SeqCst);
                let need = host.config().source == CameraSource::Mvs && (lost || host.device.lock().unwrap().is_none());
                if need {
                    if lost {
                        cycle::log(&app, "err", "相机断线", "尝试重新连接");
                    }
                    let a = app.clone();
                    let result = tauri::async_runtime::spawn_blocking(move || a.state::<CycleHost>().camera.open()).await;
                    let host = &app.state::<CycleHost>().camera;
                    match result {
                        Ok(Ok(msg)) => {
                            last_error = None;
                            cycle::log(&app, "ok", "相机", msg);
                        }
                        Ok(Err(e)) => {
                            if last_error.as_ref() != Some(&e) {
                                cycle::log(&app, "warn", "相机", e.clone());
                                last_error = Some(e);
                            }
                        }
                        Err(e) => host.state.lock().unwrap().message = e.to_string(),
                    }
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    }
}

/// 写入飞拍所需的相机参数。个别型号不支持的节点记为警告，不阻止取流。
fn apply(d: &mvs::Device, c: &CameraConfig) -> (Vec<String>, Option<f32>) {
    let mut warnings = Vec::new();
    let mut must = |r: Result<(), String>| {
        if let Err(e) = r {
            warnings.push(e);
        }
    };
    must(d.set_enum("AcquisitionMode", "Continuous"));
    must(d.set_enum("PixelFormat", "Mono8"));
    let _ = d.set_enum("TriggerSelector", "FrameBurstStart");
    must(d.set_enum("TriggerMode", "On"));
    must(d.set_enum("TriggerSource", &c.trigger_source));
    if c.trigger_source == "Line0" {
        must(d.set_enum("TriggerActivation", &c.trigger_activation));
        must(d.set_float("TriggerDelay", c.trigger_delay_us));
        must(d.set_enum("LineSelector", "Line0"));
        must(d.set_int("LineDebouncerTime", c.debouncer_us));
    }
    must(d.set_enum("ExposureAuto", "Off"));
    must(d.set_float("ExposureTime", c.exposure_us));
    must(d.set_enum("GainAuto", "Off"));
    must(d.set_float("Gain", c.gain_db));
    if d.set_enum("LineSelector", "Line1").is_ok() {
        if c.strobe {
            must(d.set_enum("LineMode", "Strobe"));
            must(d.set_enum("LineSource", "ExposureStartActive"));
            must(d.set_bool("StrobeEnable", true));
        } else {
            let _ = d.set_bool("StrobeEnable", false);
        }
    }
    if c.chunk {
        must(d.set_bool("ChunkModeActive", true));
        match d.enum_entries("ChunkSelector") {
            Ok(entries) => {
                let wanted: Vec<_> = entries
                    .iter()
                    .filter(|e| {
                        let l = e.to_lowercase();
                        (l.contains("frame") && l.contains("count")) || l.contains("trigger") || l.contains("timestamp")
                    })
                    .collect();
                if wanted.is_empty() {
                    must(Err(format!("相机不支持帧计数 / 触发计数 Chunk（可选项：{}）", entries.join("、"))));
                }
                for e in wanted {
                    must(d.set_enum("ChunkSelector", e));
                    must(d.set_bool("ChunkEnable", true));
                }
            }
            Err(e) => must(Err(e)),
        }
    } else {
        let _ = d.set_bool("ChunkModeActive", false);
    }
    if d.summary.transport == "GigE" {
        if let Some(size) = d.optimal_packet_size() {
            must(d.set_int("GevSCPSPacketSize", size));
        }
    }
    let max_fps = d.get_float("ResultingFrameRate").ok();
    (warnings, max_fps)
}

#[tauri::command]
pub fn camera_status(cycle: State<'_, CycleHost>) -> CameraStatus {
    cycle.camera.status()
}

#[tauri::command]
pub fn camera_get_config(cycle: State<'_, CycleHost>) -> CameraConfig {
    cycle.camera.config()
}

#[tauri::command]
pub async fn camera_save_config(app: AppHandle, config: CameraConfig) -> Result<Vec<String>, String> {
    config.validate()?;
    let host = &app.state::<CycleHost>().camera;
    config.save(&host.config_path)?;
    *host.config.lock().unwrap() = config.clone();
    host.state.lock().unwrap().warnings.clear();
    if config.source == CameraSource::Sim {
        host.close();
        return Ok(Vec::new());
    }
    let a = app.clone();
    tauri::async_runtime::spawn_blocking(move || a.state::<CycleHost>().camera.open()).await.map_err(|e| e.to_string())??;
    Ok(app.state::<CycleHost>().camera.state.lock().unwrap().warnings.clone())
}

#[tauri::command]
pub async fn camera_list_devices() -> Result<Vec<DeviceSummary>, String> {
    tauri::async_runtime::spawn_blocking(mvs::enumerate).await.map_err(|e| e.to_string())?
}

/// 最近一帧的缩略图：前 8 字节为宽、高（u32 小端），其后为 8 位灰度像素。
#[tauri::command]
pub fn camera_preview(cycle: State<'_, CycleHost>) -> tauri::ipc::Response {
    let preview = cycle.camera.shared.preview.lock().unwrap();
    let mut out = Vec::new();
    if let Some(p) = preview.as_ref() {
        out.extend_from_slice(&p.width.to_le_bytes());
        out.extend_from_slice(&p.height.to_le_bytes());
        out.extend_from_slice(&p.data);
    }
    tauri::ipc::Response::new(out)
}

#[tauri::command]
pub fn camera_soft_trigger(cycle: State<'_, CycleHost>) -> Result<(), String> {
    if cycle.camera.config().trigger_source != "Software" {
        return Err("触发源为 Line0，软触发前先把触发源改为 Software".into());
    }
    if cycle.camera.trigger(false) {
        Ok(())
    } else {
        Err("相机未连接".into())
    }
}

/// 空跑测试：机器人不带工件走一遍路径，期间的帧只计数不进入检测节拍。
#[tauri::command]
pub fn camera_dry_run_start(cycle: State<'_, CycleHost>) -> Result<(), String> {
    if cycle.phase() != Phase::Idle {
        return Err("检测节拍不在空闲状态，不能开始空跑".into());
    }
    *cycle.camera.shared.dry_run.lock().unwrap() = Some((Instant::now(), Vec::new()));
    Ok(())
}

#[tauri::command]
pub fn camera_dry_run_get(cycle: State<'_, CycleHost>) -> Option<Vec<DryFrame>> {
    cycle.camera.shared.dry_run.lock().unwrap().as_ref().map(|(_, f)| f.clone())
}

#[tauri::command]
pub fn camera_dry_run_stop(cycle: State<'_, CycleHost>) -> Vec<DryFrame> {
    cycle.camera.shared.dry_run.lock().unwrap().take().map(|(_, f)| f).unwrap_or_default()
}
