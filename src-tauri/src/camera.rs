//! 相机组：N 台相机各自配置、状态与重连，共用一条有界帧通道交给检测节拍。
//! 飞拍用 1 台按触发取图；三目随动用 3 台连续采集，只在工件布防期间把帧送进节拍。

use std::collections::{BTreeMap, VecDeque};
use std::ffi::{c_uint, c_void};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};

use ly_plc::now_ms;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc::Sender;

use crate::cycle::{self, CycleHost, Phase};
use crate::follow::FollowCalib;
use crate::frame::{Frame, FrameImage, FramePool};
use crate::mvs::{self, DeviceSummary, FrameInfo};
use crate::recipe::Recipe;
use crate::replay;
use crate::sim::Scenario;
use crate::simimage::{self, PoseError};

/// 帧通道容量：检测节拍处理不过来时丢新帧并计数，不在内存里无限堆积。
pub const FRAME_QUEUE: usize = 64;

/// 模拟相机合成飞拍帧所需的信息：哪个配方的第几个拍照点、什么场景、机器人偏差。
pub struct SimRender {
    pub recipe: Arc<Recipe>,
    pub k: usize,
    pub scenario: Scenario,
    pub pose: PoseError,
    pub seed: u64,
}

/// 连续采集的模拟相机取图：给相机序号与时间戳，返回这一刻的画面。模拟随动节拍运行时设置。
pub type SimSource = Arc<dyn Fn(u8, i64) -> Option<FrameImage> + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CameraSource {
    Sim,
    Mvs,
    /// 从目录读图（帧录制或现场采的图）
    Replay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Acquisition {
    /// 每个触发出一帧（飞拍）
    Triggered,
    /// 按固定帧率连续采集，工件布防期间的帧进入节拍（随动）
    FreeRun,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CameraConfig {
    /// 相机编号，配方用它引用相机；建相机时分配，之后不变（增删别的相机也不变）
    pub id: String,
    pub name: String,
    pub source: CameraSource,
    pub serial: String,
    pub acquisition: Acquisition,
    pub fps: f32,
    pub trigger_source: String,
    pub trigger_activation: String,
    pub trigger_delay_us: f32,
    pub debouncer_us: i64,
    pub exposure_us: f32,
    pub gain_db: f32,
    pub strobe: bool,
    pub chunk: bool,
    pub replay_dir: String,
    /// 回放通道（从 1 开始），0 表示取目录里的第一个通道
    pub replay_channel: u32,
    /// 随动：胶嘴在图像里的位置、图像方位与像素当量（示教得到）
    pub follow: Option<FollowCalib>,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: "相机".into(),
            source: CameraSource::Sim,
            serial: String::new(),
            acquisition: Acquisition::Triggered,
            fps: 20.0,
            trigger_source: "Line0".into(),
            trigger_activation: "RisingEdge".into(),
            trigger_delay_us: 0.0,
            debouncer_us: 5,
            exposure_us: 60.0,
            gain_db: 6.0,
            strobe: true,
            chunk: true,
            replay_dir: String::new(),
            replay_channel: 0,
            follow: None,
        }
    }
}

impl CameraConfig {
    fn validate(&self) -> Result<(), String> {
        if !(1.0..=1_000_000.0).contains(&self.exposure_us) {
            return Err("曝光时间需在 1–1000000 µs 之间".into());
        }
        if !["Line0", "Software"].contains(&self.trigger_source.as_str()) {
            return Err("触发源只能是 Line0 或 Software".into());
        }
        if self.acquisition == Acquisition::FreeRun && !(1.0..=500.0).contains(&self.fps) {
            return Err("连续采集帧率需在 1–500 fps 之间".into());
        }
        if self.source == CameraSource::Replay && self.replay_dir.trim().is_empty() {
            return Err("回放相机需要填写图片目录".into());
        }
        if let Some(f) = &self.follow {
            f.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct RigFile {
    cameras: Vec<CameraConfig>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraStatus {
    pub cam: u8,
    pub id: String,
    pub name: String,
    pub source: CameraSource,
    pub acquisition: Acquisition,
    pub ready: bool,
    pub message: String,
    pub device: Option<DeviceSummary>,
    pub sdk_version: Option<String>,
    pub frames: u64,
    pub fps: f32,
    pub max_fps: Option<f32>,
    pub lost_packets: u64,
    /// 检测节拍来不及取走、被丢弃的帧
    pub dropped_frames: u64,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DryFrame {
    pub cam: u8,
    pub t_ms: f64,
    pub frame_counter: u64,
    pub trigger_counter: u64,
    pub lost_packets: u32,
}

struct Preview {
    width: u32,
    height: u32,
    full_width: u32,
    full_height: u32,
    data: Vec<u8>,
}

/// 降采样到宽度不超过 960 的缩略图。
fn make_preview(src: &[u8], w: usize, h: usize) -> Preview {
    let step = w.div_ceil(960).max(1);
    let (pw, ph) = (w / step, h / step);
    let mut data = Vec::with_capacity(pw * ph);
    for y in 0..ph {
        let row = &src[y * step * w..];
        data.extend((0..pw).map(|x| row[x * step]));
    }
    Preview { width: pw as u32, height: ph as u32, full_width: w as u32, full_height: h as u32, data }
}

/// 模拟帧并行合成、按帧计数顺序交付：节拍按帧计数的先后推 k，乱序会让后面的帧落错拍照点。
#[derive(Default)]
struct Reorder {
    next: u64,
    pending: BTreeMap<u64, Option<Frame>>,
}

struct ReplayState {
    files: Vec<PathBuf>,
    next: usize,
    /// 帧录制目录：各帧相对工件开始的时刻（ms）。有它时连续采集按原来的时刻出帧，放完为止
    times: Option<Vec<i64>>,
}

/// 连续采集的回放相机下一帧什么时候出。
enum Due {
    Now,
    Wait(Duration),
    Done,
}

/// 相机组共用的部分。
struct RigShared {
    app: AppHandle,
    tx: Sender<Frame>,
    pool: FramePool,
    /// 需要整帧图像：触发采集的相机（飞拍）与连续采集的相机（随动）分开，飞拍用模拟测量时不必拷整帧
    capture_triggered: AtomicBool,
    capture_free_run: AtomicBool,
    streaming: AtomicBool,
    dry_run: Mutex<Option<(Instant, Vec<DryFrame>)>>,
    sim_source: Mutex<Option<SimSource>>,
    stream_started: Mutex<Option<Instant>>,
}

/// 单台相机的交付通道，取图回调与模拟 / 回放共用。
struct Shared {
    cam: u8,
    rig: Arc<RigShared>,
    free_run: AtomicBool,
    frames: AtomicU64,
    lost_packets: AtomicU64,
    dropped: AtomicU64,
    recent: Mutex<VecDeque<Instant>>,
    preview: Mutex<Option<Preview>>,
    last_full: Mutex<Option<Arc<FrameImage>>>,
    last_emit: Mutex<Option<Instant>>,
    disconnected: AtomicBool,
    order: Mutex<Reorder>,
}

impl Shared {
    fn capture(&self) -> bool {
        if self.free_run.load(Ordering::Relaxed) { &self.rig.capture_free_run } else { &self.rig.capture_triggered }.load(Ordering::Relaxed)
    }

    /// 连续采集且不在布防期间的帧只更新缩略图，不送节拍。
    fn wanted(&self) -> bool {
        !self.free_run.load(Ordering::Relaxed) || self.rig.streaming.load(Ordering::Relaxed)
    }

    fn set_preview(&self, img: &FrameImage) {
        *self.preview.lock().unwrap() = Some(make_preview(&img.pixels, img.width as usize, img.height as usize));
    }

    fn deliver_in_order(&self, seq: u64, frame: Option<Frame>) {
        let mut order = self.order.lock().unwrap();
        // 每台相机的帧序号从 1 开始；不能拿先到的那帧当起点，否则先合成完的第 2 帧会把第 1 帧挤到后面
        if order.next == 0 {
            order.next = 1;
        }
        if seq < order.next {
            drop(order);
            if let Some(f) = frame {
                self.deliver(f);
            }
            return;
        }
        order.pending.insert(seq, frame);
        loop {
            let next = order.next;
            let Some(f) = order.pending.remove(&next) else { break };
            order.next += 1;
            if let Some(f) = f {
                self.deliver(f);
            }
        }
    }

    fn deliver(&self, frame: Frame) {
        if let Some(img) = &frame.image {
            *self.last_full.lock().unwrap() = Some(img.clone());
        }
        self.frames.fetch_add(1, Ordering::Relaxed);
        self.lost_packets.fetch_add(frame.lost_packets as u64, Ordering::Relaxed);
        let now = Instant::now();
        {
            let mut recent = self.recent.lock().unwrap();
            recent.push_back(now);
            while recent.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(3)) {
                recent.pop_front();
            }
        }
        {
            // 前端只要看到"有帧在来"，每台相机每秒最多通知 10 次
            let mut last = self.last_emit.lock().unwrap();
            if last.is_none_or(|t| now.duration_since(t) > Duration::from_millis(100)) {
                *last = Some(now);
                let _ = self.rig.app.emit("camera://frame", &frame);
            }
        }
        if let Some((started, frames)) = self.rig.dry_run.lock().unwrap().as_mut() {
            frames.push(DryFrame {
                cam: self.cam,
                t_ms: started.elapsed().as_secs_f64() * 1000.0,
                frame_counter: frame.frame_counter,
                trigger_counter: frame.trigger_counter,
                lost_packets: frame.lost_packets,
            });
            return;
        }
        if !self.wanted() {
            return;
        }
        if self.rig.tx.try_send(frame).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
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
        let mono = info.pixel_type == mvs::PIXEL_MONO8 && !data.is_null();
        let w = if info.extend_width != 0 { info.extend_width } else { info.width as u32 };
        let h = if info.extend_height != 0 { info.extend_height } else { info.height as u32 };
        let src = mono.then(|| unsafe { std::slice::from_raw_parts(data, (w * h) as usize) });
        if let Some(src) = src {
            *shared.preview.lock().unwrap() = Some(make_preview(src, w as usize, h as usize));
        }
        let image = src
            .filter(|_| shared.capture() && shared.wanted())
            .map(|src| Arc::new(shared.rig.pool.copy(w, h, src)));
        shared.deliver(Frame { cam: shared.cam, frame_counter, trigger_counter, lost_packets: info.lost_packet, ts: now_ms(), image });
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

pub struct CameraSlot {
    device: Mutex<Option<mvs::Device>>,
    shared: Arc<Shared>,
    config: Mutex<CameraConfig>,
    state: Mutex<DeviceState>,
    frame_seq: AtomicU64,
    trigger_seq: AtomicU64,
    replay: Mutex<Option<ReplayState>>,
}

impl Drop for CameraSlot {
    fn drop(&mut self) {
        // 先停取流，回调里用的 Shared 才能安全释放
        self.device.lock().unwrap().take();
    }
}

impl CameraSlot {
    fn new(rig: &Arc<RigShared>, cam: u8, config: CameraConfig) -> Self {
        let slot = Self {
            device: Mutex::new(None),
            shared: Arc::new(Shared {
                cam,
                rig: rig.clone(),
                free_run: AtomicBool::new(config.acquisition == Acquisition::FreeRun),
                frames: AtomicU64::new(0),
                lost_packets: AtomicU64::new(0),
                dropped: AtomicU64::new(0),
                recent: Mutex::new(VecDeque::new()),
                preview: Mutex::new(None),
                last_full: Mutex::new(None),
                last_emit: Mutex::new(None),
                disconnected: AtomicBool::new(false),
                order: Mutex::new(Reorder::default()),
            }),
            config: Mutex::new(config),
            state: Mutex::new(DeviceState::default()),
            frame_seq: AtomicU64::new(0),
            trigger_seq: AtomicU64::new(0),
            replay: Mutex::new(None),
        };
        if slot.config().source == CameraSource::Replay {
            let _ = slot.load_replay();
        }
        slot
    }

    pub fn config(&self) -> CameraConfig {
        self.config.lock().unwrap().clone()
    }

    fn set_config(&self, config: CameraConfig) {
        self.shared.free_run.store(config.acquisition == Acquisition::FreeRun, Ordering::Relaxed);
        *self.config.lock().unwrap() = config;
        self.state.lock().unwrap().warnings.clear();
    }

    pub fn status(&self) -> CameraStatus {
        let config = self.config();
        let state = self.state.lock().unwrap();
        let device = self.device.lock().unwrap();
        let replay = self.replay.lock().unwrap();
        let (ready, message) = match config.source {
            CameraSource::Sim => (
                true,
                match config.acquisition {
                    Acquisition::Triggered => "模拟相机：收到触发后约 180 ms 交付一帧".to_string(),
                    Acquisition::FreeRun => format!("模拟相机：布防期间按 {} fps 合成随动画面", config.fps),
                },
            ),
            CameraSource::Mvs => (device.is_some() && !self.shared.disconnected.load(Ordering::SeqCst), state.message.clone()),
            CameraSource::Replay => match replay.as_ref() {
                Some(r) => (true, format!("回放 {} 帧 · 下一帧第 {} 张", r.files.len(), r.next + 1)),
                None => (false, state.message.clone()),
            },
        };
        CameraStatus {
            cam: self.shared.cam,
            id: config.id.clone(),
            name: config.name.clone(),
            source: config.source,
            acquisition: config.acquisition,
            ready,
            message,
            device: device.as_ref().map(|d| d.summary.clone()),
            sdk_version: mvs::api().ok().map(|a| a.version.clone()),
            frames: self.shared.frames.load(Ordering::Relaxed),
            fps: self.shared.fps(),
            max_fps: state.max_fps,
            lost_packets: self.shared.lost_packets.load(Ordering::Relaxed),
            dropped_frames: self.shared.dropped.load(Ordering::Relaxed),
            warnings: state.warnings.clone(),
        }
    }


    pub fn last_full(&self) -> Option<Arc<FrameImage>> {
        self.shared.last_full.lock().unwrap().clone()
    }

    /// 只看就绪与否，不拼状态文字（节拍每 20 ms 查一次）。
    pub fn is_ready(&self) -> bool {
        match self.config.lock().unwrap().source {
            CameraSource::Sim => true,
            CameraSource::Mvs => self.device.lock().unwrap().is_some() && !self.shared.disconnected.load(Ordering::SeqCst),
            CameraSource::Replay => self.replay.lock().unwrap().is_some(),
        }
    }

    fn next_counters(&self) -> (u64, u64) {
        (self.frame_seq.fetch_add(1, Ordering::SeqCst) + 1, self.trigger_seq.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// 回放的下一张图片路径（到末尾后从头再来）。
    /// 下一张回放图。连续采集按录制时刻回放时放完为止（下次布防再从头）；其余情况到末尾后从头再来。
    fn next_replay(&self) -> Option<PathBuf> {
        let free_run = self.config().acquisition == Acquisition::FreeRun;
        let mut guard = self.replay.lock().unwrap();
        let r = guard.as_mut()?;
        let once = free_run && r.times.is_some();
        if once && r.next >= r.files.len() {
            return None;
        }
        let p = r.files[r.next % r.files.len()].clone();
        r.next = if once { r.next + 1 } else { (r.next + 1) % r.files.len() };
        Some(p)
    }

    /// 按录制时刻回放时，下一帧离现在还有多久；没有时间线返回 None（按帧率出帧）。
    fn replay_due(&self) -> Option<Due> {
        let guard = self.replay.lock().unwrap();
        let r = guard.as_ref()?;
        let times = r.times.as_ref()?;
        let Some(&t) = times.get(r.next) else { return Some(Due::Done) };
        let started = (*self.shared.rig.stream_started.lock().unwrap())?;
        let due = Duration::from_millis(t.max(0) as u64);
        let elapsed = started.elapsed();
        Some(if elapsed >= due { Due::Now } else { Due::Wait(due - elapsed) })
    }

    fn load_replay(&self) -> Result<String, String> {
        let config = self.config();
        let dir = config.replay_dir.trim().to_string();
        let mut guard = self.replay.lock().unwrap();
        match replay::scan(std::path::Path::new(&dir), config.replay_channel) {
            Ok(files) => {
                let times = replay::timeline(std::path::Path::new(&dir), &files);
                let msg = format!("回放目录 {dir}：{} 帧{}", files.len(), if times.is_some() { "（按录制时刻）" } else { "" });
                *guard = Some(ReplayState { files, next: 0, times });
                Ok(msg)
            }
            Err(e) => {
                *guard = None;
                self.state.lock().unwrap().message = e.clone();
                Err(e)
            }
        }
    }

    fn rewind_replay(&self) {
        if let Some(r) = self.replay.lock().unwrap().as_mut() {
            r.next = 0;
        }
    }

    /// 按触发出一帧。模拟相机合成（render 为空时只有元数据），回放读下一张，海康发软触发。
    /// `lose_in_transfer` 仅模拟相机使用：相机已曝光但帧在传输中丢失，主机侧表现为帧计数跳号。
    pub fn trigger(&self, lose_in_transfer: bool, render: Option<SimRender>) -> bool {
        let config = self.config();
        let cam = self.shared.cam;
        match config.source {
            CameraSource::Sim => {
                let (frame_counter, trigger_counter) = self.next_counters();
                let shared = self.shared.clone();
                if lose_in_transfer {
                    shared.deliver_in_order(frame_counter, None);
                    return true;
                }
                tauri::async_runtime::spawn(async move {
                    let started = Instant::now();
                    let image = match render {
                        Some(r) => tauri::async_runtime::spawn_blocking(move || simimage::render(&r.recipe, r.k, r.scenario, r.pose, r.seed)).await.ok(),
                        None => None,
                    };
                    let transfer = Duration::from_millis(180);
                    if started.elapsed() < transfer {
                        tokio::time::sleep(transfer - started.elapsed()).await;
                    }
                    let image = image.map(|img| {
                        shared.set_preview(&img);
                        Arc::new(shared.rig.pool.adopt(img))
                    });
                    let frame = Frame { cam, frame_counter, trigger_counter, lost_packets: 0, ts: now_ms(), image };
                    shared.deliver_in_order(frame_counter, Some(frame));
                });
                true
            }
            CameraSource::Mvs => {
                config.trigger_source == "Software" && self.device.lock().unwrap().as_ref().is_some_and(|d| d.command("TriggerSoftware").is_ok())
            }
            CameraSource::Replay => {
                let Some(path) = self.next_replay() else { return false };
                let (frame_counter, trigger_counter) = self.next_counters();
                let shared = self.shared.clone();
                tauri::async_runtime::spawn(async move {
                    let loaded = tauri::async_runtime::spawn_blocking(move || replay::load(&path)).await.map_err(|e| e.to_string());
                    match loaded.and_then(|r| r) {
                        Ok(img) => {
                            shared.set_preview(&img);
                            let image = Some(Arc::new(shared.rig.pool.adopt(img)));
                            shared.deliver_in_order(frame_counter, Some(Frame { cam, frame_counter, trigger_counter, lost_packets: 0, ts: now_ms(), image }));
                        }
                        Err(e) => {
                            cycle::log(&shared.rig.app, "err", "回放", e);
                            shared.deliver_in_order(frame_counter, None);
                        }
                    }
                });
                true
            }
        }
    }

    /// 连续采集的一帧（模拟 / 回放）。阻塞执行，由采集循环调用。
    fn produce_free_run(&self) -> Option<Frame> {
        let config = self.config();
        let ts = now_ms();
        let img = match config.source {
            CameraSource::Sim => {
                let source = self.shared.rig.sim_source.lock().unwrap().clone()?;
                source(self.shared.cam, ts)?
            }
            CameraSource::Replay => match replay::load(&self.next_replay()?) {
                Ok(img) => img,
                Err(e) => {
                    cycle::log(&self.shared.rig.app, "err", "回放", e);
                    return None;
                }
            },
            CameraSource::Mvs => return None,
        };
        self.shared.set_preview(&img);
        let (frame_counter, trigger_counter) = self.next_counters();
        self.shared.order.lock().unwrap().next = frame_counter + 1;
        let image = self.shared.capture().then(|| Arc::new(self.shared.rig.pool.adopt(img)));
        Some(Frame { cam: self.shared.cam, frame_counter, trigger_counter, lost_packets: 0, ts, image })
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

    /// 应用新配置：海康相机重新打开，回放重新扫描目录。返回相机未接受的参数。
    fn apply_config(&self) -> Result<Vec<String>, String> {
        match self.config().source {
            CameraSource::Sim => {
                self.close();
                Ok(Vec::new())
            }
            CameraSource::Replay => {
                self.close();
                self.load_replay()?;
                Ok(Vec::new())
            }
            CameraSource::Mvs => {
                self.open()?;
                Ok(self.state.lock().unwrap().warnings.clone())
            }
        }
    }
}

/// 写入相机参数。个别型号不支持的节点记为警告，不阻止取流。
fn apply(d: &mvs::Device, c: &CameraConfig) -> (Vec<String>, Option<f32>) {
    let mut warnings = Vec::new();
    let mut must = |r: Result<(), String>| {
        if let Err(e) = r {
            warnings.push(e);
        }
    };
    must(d.set_enum("AcquisitionMode", "Continuous"));
    must(d.set_enum("PixelFormat", "Mono8"));
    match c.acquisition {
        Acquisition::Triggered => {
            let _ = d.set_enum("TriggerSelector", "FrameBurstStart");
            must(d.set_enum("TriggerMode", "On"));
            must(d.set_enum("TriggerSource", &c.trigger_source));
            if c.trigger_source == "Line0" {
                must(d.set_enum("TriggerActivation", &c.trigger_activation));
                must(d.set_float("TriggerDelay", c.trigger_delay_us));
                must(d.set_enum("LineSelector", "Line0"));
                must(d.set_int("LineDebouncerTime", c.debouncer_us));
            }
        }
        Acquisition::FreeRun => {
            must(d.set_enum("TriggerMode", "Off"));
            must(d.set_bool("AcquisitionFrameRateEnable", true));
            must(d.set_float("AcquisitionFrameRate", c.fps));
        }
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

pub struct CameraRig {
    rig: Arc<RigShared>,
    slots: RwLock<Vec<Arc<CameraSlot>>>,
    path: PathBuf,
}

impl CameraRig {
    pub fn new(app: &AppHandle, tx: Sender<Frame>) -> Result<Self, String> {
        let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
        let path = dir.join("cameras.json");
        let mut file: RigFile = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        if file.cameras.is_empty() {
            // 单相机时代的 camera.json 迁移成相机组的第 1 台
            let old = std::fs::read_to_string(dir.join("camera.json")).ok().and_then(|s| serde_json::from_str::<CameraConfig>(&s).ok());
            file.cameras.push(CameraConfig { name: "相机 1".into(), ..old.unwrap_or_default() });
        }
        let assigned = assign_ids(&mut file.cameras);
        let rig = Arc::new(RigShared {
            app: app.clone(),
            tx,
            pool: FramePool::new(48),
            capture_triggered: AtomicBool::new(false),
            capture_free_run: AtomicBool::new(false),
            streaming: AtomicBool::new(false),
            dry_run: Mutex::new(None),
            sim_source: Mutex::new(None),
            stream_started: Mutex::new(None),
        });
        let slots = file.cameras.into_iter().enumerate().map(|(i, c)| Arc::new(CameraSlot::new(&rig, i as u8, c))).collect();
        let this = Self { rig, slots: RwLock::new(slots), path };
        if assigned {
            this.save()?;
        }
        Ok(this)
    }

    /// 编号为 id 的相机在相机组里的序号。
    pub fn index_of(&self, id: &str) -> Option<u8> {
        self.slots().iter().position(|s| s.config.lock().unwrap().id == id).map(|i| i as u8)
    }

    /// 把配方里的相机编号换成此刻的相机组序号；有不在相机组里的返回错误。
    pub fn resolve(&self, ids: &[String]) -> Result<Vec<u8>, String> {
        ids.iter().map(|id| self.index_of(id).ok_or_else(|| format!("相机组里没有编号为 {id} 的相机"))).collect()
    }

    /// 各相机累计被丢弃的帧。
    pub fn dropped_total(&self) -> u64 {
        self.slots().iter().map(|s| s.shared.dropped.load(Ordering::Relaxed)).sum()
    }

    fn save(&self) -> Result<(), String> {
        let file = RigFile { cameras: self.configs() };
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
        }
        std::fs::write(&self.path, serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?).map_err(|e| format!("写入配置失败: {e}"))
    }

    pub fn slots(&self) -> Vec<Arc<CameraSlot>> {
        self.slots.read().unwrap().clone()
    }

    pub fn slot(&self, cam: usize) -> Option<Arc<CameraSlot>> {
        self.slots.read().unwrap().get(cam).cloned()
    }

    pub fn configs(&self) -> Vec<CameraConfig> {
        self.slots().iter().map(|s| s.config()).collect()
    }

    pub fn statuses(&self) -> Vec<CameraStatus> {
        self.slots().iter().map(|s| s.status()).collect()
    }

    /// 给定编号的相机都在相机组里且就绪；否则返回第一台的原因。
    pub fn check_ready(&self, ids: &[String]) -> Result<(), String> {
        let slots = self.slots();
        for c in self.resolve(ids)? {
            let slot = &slots[c as usize];
            if !slot.is_ready() {
                let st = slot.status();
                return Err(format!("{}未就绪：{}", st.name, st.message));
            }
        }
        Ok(())
    }

    /// 取图回调是否拷贝整帧：触发采集（飞拍）与连续采集（随动）的相机分别设置。
    pub fn set_capture(&self, triggered: bool, free_run: bool) {
        self.rig.capture_triggered.store(triggered, Ordering::Relaxed);
        self.rig.capture_free_run.store(free_run, Ordering::Relaxed);
    }

    /// 布防期间打开：连续采集的相机开始把帧送进节拍。打开时回放从第一张开始。
    pub fn set_streaming(&self, on: bool) {
        if on && !self.rig.streaming.load(Ordering::SeqCst) {
            for s in self.slots() {
                if s.config().acquisition == Acquisition::FreeRun {
                    s.rewind_replay();
                }
            }
            *self.rig.stream_started.lock().unwrap() = Some(Instant::now());
        }
        self.rig.streaming.store(on, Ordering::SeqCst);
    }

    pub fn set_sim_source(&self, source: Option<SimSource>) {
        *self.rig.sim_source.lock().unwrap() = source;
    }

    pub fn trigger(&self, cam: u8, lose_in_transfer: bool, render: Option<SimRender>) -> bool {
        self.slot(cam as usize).is_some_and(|s| s.trigger(lose_in_transfer, render))
    }

    pub fn last_full(&self, cam: u8) -> Option<Arc<FrameImage>> {
        self.slot(cam as usize).and_then(|s| s.last_full())
    }

    fn rebuild(&self, app: &AppHandle, configs: Vec<CameraConfig>) {
        let slots: Vec<Arc<CameraSlot>> = configs.into_iter().enumerate().map(|(i, c)| Arc::new(CameraSlot::new(&self.rig, i as u8, c))).collect();
        *self.slots.write().unwrap() = slots.clone();
        for s in &slots {
            supervise(app, Arc::downgrade(s));
        }
    }

    pub fn start(app: &AppHandle) {
        for s in app.state::<CycleHost>().camera.slots() {
            supervise(app, Arc::downgrade(&s));
        }
    }
}

/// 每台相机一个后台循环：海康相机掉线或未打开时每 2 s 重连；模拟 / 回放相机连续采集时按帧率出帧。
/// 相机被移出相机组后循环自己结束。
fn supervise(app: &AppHandle, slot: Weak<CameraSlot>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut last_error: Option<String> = None;
        let mut next_retry = Instant::now();
        loop {
            let Some(s) = slot.upgrade() else { break };
            let config = s.config();
            let streaming = s.shared.rig.streaming.load(Ordering::SeqCst);
            if config.source == CameraSource::Mvs && Instant::now() >= next_retry {
                next_retry = Instant::now() + Duration::from_secs(2);
                let lost = s.shared.disconnected.load(Ordering::SeqCst);
                if lost || s.device.lock().unwrap().is_none() {
                    if lost {
                        cycle::log(&app, "err", "相机断线", format!("{}：尝试重新连接", config.name));
                    }
                    let s2 = s.clone();
                    match tauri::async_runtime::spawn_blocking(move || s2.open()).await {
                        Ok(Ok(msg)) => {
                            last_error = None;
                            cycle::log(&app, "ok", "相机", format!("{}：{msg}", config.name));
                        }
                        Ok(Err(e)) => {
                            if last_error.as_ref() != Some(&e) {
                                cycle::log(&app, "warn", "相机", format!("{}：{e}", config.name));
                                last_error = Some(e);
                            }
                        }
                        Err(e) => s.state.lock().unwrap().message = e.to_string(),
                    }
                }
            }
            let generate = streaming && config.acquisition == Acquisition::FreeRun && config.source != CameraSource::Mvs;
            if !generate {
                drop(s);
                tokio::time::sleep(Duration::from_millis(20)).await;
                continue;
            }
            let timeline = if config.source == CameraSource::Replay { s.replay_due() } else { None };
            match timeline {
                Some(Due::Done) => {
                    drop(s);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    continue;
                }
                Some(Due::Wait(d)) => {
                    drop(s);
                    tokio::time::sleep(d.min(Duration::from_millis(20))).await;
                    continue;
                }
                _ => {}
            }
            let started = Instant::now();
            let s2 = s.clone();
            if let Ok(Some(frame)) = tauri::async_runtime::spawn_blocking(move || s2.produce_free_run()).await {
                s.shared.deliver(frame);
            }
            drop(s);
            if timeline.is_none() {
                let period = Duration::from_secs_f32(1.0 / config.fps.max(1.0));
                tokio::time::sleep(period.saturating_sub(started.elapsed()).max(Duration::from_millis(1))).await;
            }
        }
    });
}

fn rig(app: &AppHandle) -> &CameraRig {
    &app.state::<CycleHost>().inner().camera
}

#[tauri::command]
pub fn camera_rig_status(cycle: State<'_, CycleHost>) -> Vec<CameraStatus> {
    cycle.camera.statuses()
}

#[tauri::command]
pub fn camera_rig_config(cycle: State<'_, CycleHost>) -> Vec<CameraConfig> {
    cycle.camera.configs()
}

/// 保存并应用一台相机的配置。返回相机未接受的参数。
#[tauri::command]
pub async fn camera_save_config(app: AppHandle, cam: usize, mut config: CameraConfig) -> Result<Vec<String>, String> {
    config.validate()?;
    let slot = rig(&app).slot(cam).ok_or("相机不存在")?;
    // 编号是配方引用相机的依据，不能改
    config.id = slot.config().id;
    slot.set_config(config);
    rig(&app).save()?;
    let _ = app.state::<CycleHost>().tx.send(cycle::Input::Refresh);
    tauri::async_runtime::spawn_blocking(move || slot.apply_config()).await.map_err(|e| e.to_string())?
}

/// 没有编号或编号重复的相机按位置补一个没用过的 "cam{n}"（旧配方里的相机序号 k 就对应 "cam{k+1}"）。返回是否改过。
fn assign_ids(configs: &mut [CameraConfig]) -> bool {
    let mut used: Vec<String> = Vec::new();
    let mut changed = false;
    for i in 0..configs.len() {
        let id = configs[i].id.clone();
        if !crate::recipe::valid_camera_id(&id) || used.contains(&id) {
            let mut n = i + 1;
            while used.contains(&format!("cam{n}")) || configs.iter().any(|c| c.id == format!("cam{n}")) {
                n += 1;
            }
            configs[i].id = format!("cam{n}");
            changed = true;
        }
        used.push(configs[i].id.clone());
    }
    changed
}

/// 相机组增删只能在空闲或故障时做：相机序号会重排（编号不变）。
fn check_idle(cycle: &CycleHost) -> Result<(), String> {
    if matches!(cycle.phase(), Phase::Idle | Phase::Fault) {
        Ok(())
    } else {
        Err("检测进行中，工件结束后再调整相机组".into())
    }
}

#[tauri::command]
pub fn camera_add(app: AppHandle, mut config: CameraConfig) -> Result<usize, String> {
    config.validate()?;
    let cycle = app.state::<CycleHost>();
    check_idle(&cycle)?;
    let mut configs = cycle.camera.configs();
    if configs.len() >= 8 {
        return Err("相机组最多 8 台".into());
    }
    config.id = String::new();
    configs.push(config);
    assign_ids(&mut configs);
    let n = configs.len();
    cycle.camera.rebuild(&app, configs);
    cycle.camera.save()?;
    let _ = cycle.tx.send(cycle::Input::Refresh);
    Ok(n - 1)
}

#[tauri::command]
pub fn camera_remove(app: AppHandle, cam: usize) -> Result<(), String> {
    let cycle = app.state::<CycleHost>();
    check_idle(&cycle)?;
    let mut configs = cycle.camera.configs();
    if cam >= configs.len() {
        return Err("相机不存在".into());
    }
    if configs.len() == 1 {
        return Err("相机组至少保留 1 台".into());
    }
    configs.remove(cam);
    cycle.camera.rebuild(&app, configs);
    cycle.camera.save()?;
    let _ = cycle.tx.send(cycle::Input::Refresh);
    Ok(())
}

#[tauri::command]
pub async fn camera_list_devices() -> Result<Vec<DeviceSummary>, String> {
    tauri::async_runtime::spawn_blocking(mvs::enumerate).await.map_err(|e| e.to_string())?
}

/// 最近一帧的缩略图：前 16 字节为缩略图宽、高与原图宽、高（u32 小端），其后为 8 位灰度像素。
#[tauri::command]
pub fn camera_preview(cycle: State<'_, CycleHost>, cam: usize) -> tauri::ipc::Response {
    let mut out = Vec::new();
    if let Some(slot) = cycle.camera.slot(cam) {
        if let Some(p) = slot.shared.preview.lock().unwrap().as_ref() {
            for v in [p.width, p.height, p.full_width, p.full_height] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            out.extend_from_slice(&p.data);
        }
    }
    tauri::ipc::Response::new(out)
}

#[tauri::command]
pub fn camera_soft_trigger(cycle: State<'_, CycleHost>, cam: usize) -> Result<(), String> {
    let slot = cycle.camera.slot(cam).ok_or("相机不存在")?;
    let config = slot.config();
    if config.source == CameraSource::Mvs && config.trigger_source != "Software" {
        return Err("触发源为 Line0，软触发前先把触发源改为 Software".into());
    }
    if slot.trigger(false, None) {
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
    *cycle.camera.rig.dry_run.lock().unwrap() = Some((Instant::now(), Vec::new()));
    Ok(())
}

#[tauri::command]
pub fn camera_dry_run_get(cycle: State<'_, CycleHost>) -> Option<Vec<DryFrame>> {
    cycle.camera.rig.dry_run.lock().unwrap().as_ref().map(|(_, f)| f.clone())
}

#[tauri::command]
pub fn camera_dry_run_stop(cycle: State<'_, CycleHost>) -> Vec<DryFrame> {
    cycle.camera.rig.dry_run.lock().unwrap().take().map(|(_, f)| f).unwrap_or_default()
}
