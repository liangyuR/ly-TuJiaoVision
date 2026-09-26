use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ly_plc::{now_ms, EdgeEvent, LinkState, PlcEngine};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::camera::{CameraStatus, Frame, SimCamera};
use crate::inspection::{read_tag_u32, tag, tag_is_on, write_tag};
use crate::judge::{self, fault, Judgement, PointState, Verdict};
use crate::measure::{self, Job, Measured};
use crate::plc::PlcHost;
use crate::recipe::{self, Recipe, TriggerMode};
use crate::settings::{CycleSettings, ProductSource};
use crate::sim::{Scenario, SimCtl};

pub enum Input {
    Edge(EdgeEvent),
    Frame(Frame),
    Measured(Measured),
    Reset,
    Refresh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Phase {
    Idle,
    Validate,
    Acquire,
    Drain,
    Judge,
    Report,
    Release,
    Fault,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FrameStatus {
    Waiting,
    Measuring,
    Done,
    LocateFailed,
    Missing,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameView {
    pub status: FrameStatus,
    pub arrived_ms: Option<u64>,
    pub frame_counter: Option<u64>,
    pub trigger_counter: Option<u64>,
    pub counter_jump: bool,
    pub score: Option<f32>,
    pub points: usize,
    pub gap_points: usize,
    pub ms: Option<u32>,
}

impl FrameView {
    fn waiting() -> Self {
        Self {
            status: FrameStatus::Waiting,
            arrived_ms: None,
            frame_counter: None,
            trigger_counter: None,
            counter_jump: false,
            score: None,
            points: 0,
            gap_points: 0,
            ms: None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartView {
    pub sn: u32,
    pub recipe_id: String,
    pub n: usize,
    pub received: usize,
    pub triggers: u64,
    pub queue: usize,
    pub filled: usize,
    pub total: usize,
    pub frames: Vec<FrameView>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultView {
    pub sn: u32,
    pub recipe_id: Option<String>,
    pub ts: i64,
    pub drain_ms: Option<u64>,
    #[serde(flatten)]
    pub judgement: Judgement,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Stats {
    pub total: u64,
    pub ok: u64,
    pub ng: u64,
    pub err: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub phase: Phase,
    pub since: i64,
    pub fault: Option<String>,
    pub product_source: ProductSource,
    pub active_recipe_id: Option<String>,
    pub trigger_mode: Option<TriggerMode>,
    pub part: Option<PartView>,
    pub result: Option<ResultView>,
    pub stats: Stats,
    pub stray_frames: u64,
    pub alarms: Vec<String>,
    pub camera: CameraStatus,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub ts: i64,
    pub level: &'static str,
    pub ev: String,
    pub msg: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeSummary {
    pub id: String,
    pub name: String,
    pub version: u32,
    pub hash: String,
    pub product_code: u16,
    pub shot_count: usize,
    pub trigger_mode: TriggerMode,
}

struct Shared {
    snapshot: Option<Snapshot>,
    logs: VecDeque<LogLine>,
    measured: Vec<Measured>,
    settings: CycleSettings,
}

pub struct CycleHost {
    pub tx: UnboundedSender<Input>,
    pub camera: SimCamera,
    pub sim: SimCtl,
    recipes: Vec<Arc<Recipe>>,
    settings_path: PathBuf,
    shared: Mutex<Shared>,
    rx: Mutex<Option<UnboundedReceiver<Input>>>,
}

impl CycleHost {
    pub fn init(app: &AppHandle) -> Result<Self, String> {
        let settings_path = app.path().app_config_dir().map_err(|e| e.to_string())?.join("cycle.json");
        let (tx, rx) = unbounded_channel();
        Ok(Self {
            camera: SimCamera::new(tx.clone()),
            tx,
            sim: SimCtl::default(),
            recipes: recipe::builtin(),
            shared: Mutex::new(Shared {
                snapshot: None,
                logs: VecDeque::new(),
                measured: Vec::new(),
                settings: CycleSettings::load(&settings_path),
            }),
            settings_path,
            rx: Mutex::new(Some(rx)),
        })
    }

    pub fn start(app: &AppHandle) {
        let host = app.state::<CycleHost>();
        let Some(mut rx) = host.rx.lock().unwrap().take() else { return };
        let mut machine = Machine::new(app.clone(), measure::spawn_worker(host.tx.clone()));
        tauri::async_runtime::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(20));
            machine.publish();
            loop {
                tokio::select! {
                    input = rx.recv() => match input {
                        Some(input) => machine.on_input(input).await,
                        None => break,
                    },
                    _ = tick.tick() => machine.on_tick().await,
                }
                if machine.dirty {
                    machine.publish();
                }
            }
        });
    }

    pub fn recipe(&self, id: &str) -> Option<Arc<Recipe>> {
        self.recipes.iter().find(|r| r.id == id).cloned()
    }

    fn settings(&self) -> CycleSettings {
        self.shared.lock().unwrap().settings.clone()
    }
}

fn plc(app: &AppHandle) -> &PlcEngine {
    app.state::<PlcHost>().inner().engine()
}

fn host(app: &AppHandle) -> &CycleHost {
    app.state::<CycleHost>().inner()
}

fn log(app: &AppHandle, level: &'static str, ev: impl Into<String>, msg: impl Into<String>) {
    let line = LogLine { ts: now_ms(), level, ev: ev.into(), msg: msg.into() };
    {
        let mut s = host(app).shared.lock().unwrap();
        s.logs.push_back(line.clone());
        if s.logs.len() > 300 {
            s.logs.pop_front();
        }
    }
    let _ = app.emit("cycle://log", line);
}

async fn put(app: &AppHandle, t: &str, v: Value) -> Result<(), String> {
    write_tag(plc(app), t, v).await
}

struct Part {
    sn: u32,
    recipe: Arc<Recipe>,
    scenario: Scenario,
    frames: Vec<FrameView>,
    measuring_since: Vec<Option<Instant>>,
    table: Vec<PointState>,
    received: usize,
    extra: usize,
    queue: usize,
    base_frame: u64,
    base_trigger: u64,
    last_frame: u64,
    last_trigger: u64,
    armed_at: Instant,
    end_at: Option<Instant>,
    fault: Option<(u16, String)>,
}

impl Part {
    fn n(&self) -> usize {
        self.recipe.shot_count()
    }

    fn view(&self) -> PartView {
        PartView {
            sn: self.sn,
            recipe_id: self.recipe.id.clone(),
            n: self.n(),
            received: self.received,
            triggers: self.last_trigger.saturating_sub(self.base_trigger),
            queue: self.queue,
            filled: self.table.iter().filter(|p| **p != PointState::Pending).count(),
            total: self.table.len(),
            frames: self.frames.clone(),
        }
    }
}

struct Machine {
    app: AppHandle,
    measure_tx: UnboundedSender<Job>,
    phase: Phase,
    since: i64,
    part: Option<Part>,
    active_recipe: Option<Arc<Recipe>>,
    fault: Option<String>,
    fault_needs_reset: bool,
    done_at: Option<Instant>,
    ack_alarmed: bool,
    result: Option<ResultView>,
    stats: Stats,
    stray: u64,
    stray_times: VecDeque<Instant>,
    alarms: Vec<String>,
    dirty: bool,
}

impl Machine {
    fn new(app: AppHandle, measure_tx: UnboundedSender<Job>) -> Self {
        Self {
            app,
            measure_tx,
            phase: Phase::Fault,
            since: now_ms(),
            part: None,
            active_recipe: None,
            fault: Some("PLC 未连接".into()),
            fault_needs_reset: false,
            done_at: None,
            ack_alarmed: false,
            result: None,
            stats: Stats::default(),
            stray: 0,
            stray_times: VecDeque::new(),
            alarms: Vec::new(),
            dirty: true,
        }
    }

    fn set_phase(&mut self, phase: Phase) {
        self.phase = phase;
        self.since = now_ms();
        self.dirty = true;
    }

    fn alarm(&mut self, msg: String) {
        if !self.alarms.contains(&msg) {
            log(&self.app, "err", "报警", msg.clone());
            self.alarms.push(msg);
            self.dirty = true;
        }
    }

    fn publish(&mut self) {
        self.dirty = false;
        let host = host(&self.app);
        let settings = host.settings();
        let active = match settings.product_source {
            ProductSource::Manual => settings.manual_recipe_id.as_deref().and_then(|id| host.recipe(id)),
            ProductSource::Plc => self.active_recipe.clone(),
        };
        let snapshot = Snapshot {
            phase: self.phase,
            since: self.since,
            fault: self.fault.clone(),
            product_source: settings.product_source,
            active_recipe_id: active.as_ref().map(|r| r.id.clone()),
            trigger_mode: active.as_ref().map(|r| r.trigger_mode),
            part: self.part.as_ref().map(Part::view),
            result: self.result.clone(),
            stats: self.stats.clone(),
            stray_frames: self.stray,
            alarms: self.alarms.clone(),
            camera: host.camera.status(),
        };
        host.shared.lock().unwrap().snapshot = Some(snapshot.clone());
        let _ = self.app.emit("cycle://snapshot", snapshot);
    }

    async fn on_input(&mut self, input: Input) {
        self.dirty = true;
        match input {
            Input::Edge(e) if e.rising => self.on_edge(e).await,
            Input::Edge(_) => {}
            Input::Frame(f) => self.on_frame(f),
            Input::Measured(m) => self.on_measured(m),
            Input::Reset => self.reset().await,
            Input::Refresh => {}
        }
    }

    async fn on_edge(&mut self, e: EdgeEvent) {
        let has = |t: &str| e.tags.iter().any(|x| x == t);
        if has(tag::PART_START) {
            if self.phase == Phase::Idle {
                self.start_part().await;
            } else {
                log(&self.app, "warn", "partStart↑", format!("当前状态 {:?}，忽略", self.phase));
            }
        } else if has(tag::PART_END) {
            if self.phase == Phase::Acquire {
                if let Some(p) = self.part.as_mut() {
                    p.end_at = Some(Instant::now());
                }
                self.set_phase(Phase::Drain);
                log(&self.app, "info", "partEnd↑", "运动结束，等待剩余帧");
            }
        } else if has(tag::RESULT_ACK) {
            if self.phase == Phase::Report {
                let a = self.app.clone();
                let r = async { put(&a, tag::DONE, json!(false)).await?; put(&a, tag::BUSY, json!(false)).await }.await;
                if let Err(e) = r {
                    self.alarm(format!("写 PLC 失败：{e}"));
                }
                self.alarms.retain(|m| !m.starts_with("PLC 未确认"));
                self.set_phase(Phase::Release);
                log(&self.app, "info", "resultAck↑", "PLC 已确认 · done↓ busy↓");
            }
        } else if has(tag::FAULT_RESET) {
            self.reset().await;
        }
    }

    async fn start_part(&mut self) {
        self.set_phase(Phase::Validate);
        self.part = None;
        self.result = None;
        self.alarms.clear();
        host(&self.app).shared.lock().unwrap().measured.clear();
        let t0 = Instant::now();
        let app = self.app.clone();
        let engine = plc(&app);
        let sn = read_tag_u32(engine, tag::PART_SN).unwrap_or(0);
        let code = read_tag_u32(engine, tag::PRODUCT_CODE).unwrap_or(0);
        let count = read_tag_u32(engine, tag::SHOT_COUNT).unwrap_or(0) as usize;
        log(&app, "info", "partStart↑", format!("SN={sn} 产品代码={code} N={count}"));

        let host = host(&app);
        let settings = host.settings();
        let recipe = match settings.product_source {
            ProductSource::Plc => host.recipes.iter().find(|r| r.product_code as u32 == code).cloned(),
            ProductSource::Manual => settings.manual_recipe_id.as_deref().and_then(|id| host.recipe(id)),
        };
        let Some(recipe) = recipe else {
            let reason = match settings.product_source {
                ProductSource::Plc => format!("产品代码 {code} 没有对应的配方"),
                ProductSource::Manual => "未选择配方".to_string(),
            };
            log(&app, "err", "校验失败", format!("{reason}，不布防"));
            return self.report(sn, None, Judgement::error(fault::NO_RECIPE, reason)).await;
        };
        self.active_recipe = Some(recipe.clone());
        let n = recipe.shot_count();
        if count != n {
            let reason = format!("PLC 下发拍照点数 {count}，配方 {} 为 {n}", recipe.id);
            log(&app, "err", "校验失败", format!("{reason}，不布防"));
            return self.report(sn, Some(recipe.id.clone()), Judgement::error(fault::SHOT_COUNT_MISMATCH, reason)).await;
        }

        let cam = host.camera.status();
        self.part = Some(Part {
            sn,
            scenario: host.sim.part_scenario(),
            frames: vec![FrameView::waiting(); n],
            measuring_since: vec![None; n],
            table: vec![PointState::Pending; recipe.point_count()],
            received: 0,
            extra: 0,
            queue: 0,
            base_frame: cam.frames,
            base_trigger: cam.triggers,
            last_frame: cam.frames,
            last_trigger: cam.triggers,
            armed_at: Instant::now(),
            end_at: None,
            fault: None,
            recipe: recipe.clone(),
        });
        let r = async { put(&app, tag::ARMED, json!(true)).await?; put(&app, tag::BUSY, json!(true)).await }.await;
        if let Err(e) = r {
            self.alarm(format!("写 PLC 失败：{e}"));
        }
        self.set_phase(Phase::Acquire);
        let elapsed = t0.elapsed();
        log(&app, "info", "armed↑ busy↑", format!("{} · N={n} · 布防耗时 {} ms", recipe.id, elapsed.as_millis()));
        if elapsed > host.settings().timeouts.arm() {
            log(&app, "warn", "布防慢", format!("超过 T_arm {} ms", host.settings().timeouts.arm_ms));
        }
    }

    fn on_frame(&mut self, f: Frame) {
        let accepting = matches!(self.phase, Phase::Acquire | Phase::Drain);
        let Some(part) = self.part.as_mut().filter(|_| accepting) else {
            self.stray += 1;
            let now = Instant::now();
            self.stray_times.push_back(now);
            self.stray_times.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
            log(&self.app, "warn", "游离帧", format!("空闲时收到帧（帧计数 {}），已丢弃", f.frame_counter));
            if self.stray_times.len() >= 3 {
                self.alarm("1 分钟内游离帧 ≥ 3，检查 Line0 接线与输入滤波".into());
            }
            return;
        };
        part.received += 1;
        let jump = f.frame_counter != part.last_frame + 1;
        part.last_frame = f.frame_counter;
        part.last_trigger = f.trigger_counter;
        // 有 Chunk 帧计数时按它定位 k：中途丢一帧后，后续帧仍能落到正确的拍照点上（整件仍判漏帧）。
        let k = (f.frame_counter.saturating_sub(part.base_frame + 1)) as usize;
        if k >= part.n() || part.frames[k].status != FrameStatus::Waiting {
            part.extra += 1;
            log(&self.app, "err", "多帧", format!("帧计数 {} 超出计划 N={}", f.frame_counter - part.base_frame, part.n()));
            return;
        }
        part.frames[k] = FrameView {
            status: FrameStatus::Measuring,
            arrived_ms: Some(part.armed_at.elapsed().as_millis() as u64),
            frame_counter: Some(f.frame_counter - part.base_frame),
            trigger_counter: Some(f.trigger_counter - part.base_trigger),
            counter_jump: jump,
            ..FrameView::waiting()
        };
        part.measuring_since[k] = Some(Instant::now());
        part.queue += 1;
        let _ = self.measure_tx.send(Job { sn: part.sn, k, recipe: part.recipe.clone(), scenario: part.scenario });
        let msg = format!(
            "k={k} · Chunk 帧 {} 触发 {}{}",
            f.frame_counter - part.base_frame,
            f.trigger_counter - part.base_trigger,
            if jump { "（帧计数跳号）" } else { "" }
        );
        log(&self.app, if jump { "err" } else { "info" }, "帧到达", msg);
    }

    fn on_measured(&mut self, m: Measured) {
        let Some(part) = self.part.as_mut().filter(|p| p.sn == m.sn) else { return };
        let frame = &mut part.frames[m.k];
        if frame.status != FrameStatus::Measuring {
            return;
        }
        part.queue -= 1;
        part.measuring_since[m.k] = None;
        for (i, &j) in m.idx.iter().enumerate() {
            part.table[j as usize] = m.point_state(i);
        }
        let gaps = m.st.iter().filter(|&&s| s == measure::ST_GAP).count();
        frame.status = if m.located { FrameStatus::Done } else { FrameStatus::LocateFailed };
        frame.score = Some(m.score);
        frame.points = m.idx.len();
        frame.gap_points = gaps;
        frame.ms = Some(m.ms);
        let (level, ev, msg) = if m.located {
            let extra = if gaps > 0 { format!(" · 缺胶 {gaps} 点") } else { String::new() };
            (if gaps > 0 { "ng" } else { "info" }, "测量完成", format!("k={} 分数 {:.2} · {} 点 · {} ms{extra}", m.k, m.score, m.idx.len(), m.ms))
        } else {
            ("err", "定位失败", format!("k={} 匹配分数 {:.2} < 0.60", m.k, m.score))
        };
        log(&self.app, level, ev, msg);
        let _ = self.app.emit("cycle://frame", &m);
        host(&self.app).shared.lock().unwrap().measured.push(m);
    }

    async fn on_tick(&mut self) {
        let connected = plc(&self.app).status().state == LinkState::Connected;
        if !connected && self.phase != Phase::Fault {
            return self.enter_fault("PLC 未连接".into());
        }
        let timeouts = host(&self.app).settings().timeouts;
        match self.phase {
            Phase::Fault if connected && !self.fault_needs_reset => self.recover().await,
            Phase::Acquire => {
                let Some(part) = self.part.as_mut() else { return };
                if part.armed_at.elapsed() > timeouts.motion() {
                    part.fault = Some((fault::MOTION_TIMEOUT, format!("布防后 {} s 内未收到 partEnd", timeouts.motion_ms / 1000)));
                    part.end_at = Some(Instant::now());
                    log(&self.app, "err", "运动超时", format!("T_motion {} ms", timeouts.motion_ms));
                    self.set_phase(Phase::Drain);
                }
            }
            Phase::Drain => {
                let Some(part) = self.part.as_mut() else { return };
                let frames_done = part.received >= part.n() || part.end_at.is_some_and(|t| t.elapsed() > timeouts.drain());
                let stuck = part.measuring_since.iter().flatten().any(|t| t.elapsed() > timeouts.proc());
                if stuck && part.fault.is_none() {
                    part.fault = Some((fault::PROCESS_TIMEOUT, format!("单帧测量超过 T_proc {} ms", timeouts.proc_ms)));
                }
                if frames_done && (part.queue == 0 || stuck) {
                    if part.received < part.n() {
                        let msg = format!("T_drain {} ms：帧 {}/{}", timeouts.drain_ms, part.received, part.n());
                        log(&self.app, "err", "收尾超时", msg);
                    } else {
                        log(&self.app, "info", "帧已齐", format!("{}/{}，队列空", part.received, part.n()));
                    }
                    self.judge_part().await;
                }
            }
            Phase::Report => {
                if !self.ack_alarmed && self.done_at.is_some_and(|t| t.elapsed() > timeouts.ack()) {
                    self.ack_alarmed = true;
                    self.alarm(format!("PLC 未确认结果（超过 T_ack {} ms），保持 done", timeouts.ack_ms));
                }
            }
            Phase::Release => {
                if !tag_is_on(plc(&self.app), tag::PART_START) {
                    self.set_phase(Phase::Idle);
                    log(&self.app, "info", "partStart↓", "回到空闲");
                }
            }
            _ => {}
        }
    }

    async fn judge_part(&mut self) {
        self.set_phase(Phase::Judge);
        let Some(part) = self.part.as_mut() else { return };
        let n = part.n();
        let mut missing = Vec::new();
        for (k, f) in part.frames.iter_mut().enumerate() {
            if matches!(f.status, FrameStatus::Waiting | FrameStatus::Measuring) {
                if f.status == FrameStatus::Waiting {
                    missing.push(k);
                }
                f.status = FrameStatus::Missing;
            }
        }
        let triggers = part.last_trigger - part.base_trigger;
        let judgement = if let Some((code, reason)) = part.fault.clone() {
            Judgement::error(code, reason)
        } else if !missing.is_empty() {
            let ks = missing.iter().map(|k| format!("k={k}")).collect::<Vec<_>>().join("、");
            let cause = if triggers as usize >= n { "触发已到，传输丢帧" } else { "可能触发丢失" };
            Judgement::error(fault::MISSING_FRAME, format!("帧 {ks} 未收到：触发计数 {triggers}，收到 {}/{n}，{cause}", part.received))
        } else if part.extra > 0 {
            Judgement::error(fault::EXTRA_FRAME, format!("多收到 {} 帧，无法确定对应关系", part.extra))
        } else if let Some(k) = part.frames.iter().position(|f| f.status == FrameStatus::LocateFailed) {
            let score = part.frames[k].score.unwrap_or(0.0);
            Judgement::error(fault::LOCATE_FAILED, format!("帧 k={k} 定位失败：匹配分数 {score:.2} < 0.60"))
        } else {
            judge::judge(&part.recipe, &part.table)
        };
        let (sn, id) = (part.sn, part.recipe.id.clone());
        self.report(sn, Some(id), judgement).await;
    }

    async fn report(&mut self, sn: u32, recipe_id: Option<String>, judgement: Judgement) {
        let app = self.app.clone();
        let r = async {
            put(&app, tag::RESULT_CODE, json!(judgement.plc_code)).await?;
            put(&app, tag::FAULT_CODE, json!(judgement.fault_code)).await?;
            put(&app, tag::RESULT_SN, json!(sn)).await?;
            put(&app, tag::DONE, json!(true)).await?;
            put(&app, tag::ARMED, json!(false)).await
        }
        .await;
        if let Err(e) = r {
            self.alarm(format!("回写 PLC 失败：{e}"));
        }
        self.set_phase(Phase::Report);
        self.done_at = Some(Instant::now());
        self.ack_alarmed = false;
        self.count(judgement.verdict);
        let level = match judgement.verdict {
            Verdict::Ok | Verdict::OkWithExcursion => "ok",
            Verdict::ErrInspect => "err",
            _ => "ng",
        };
        let fault = if judgement.fault_code > 0 { format!(" faultCode={}", judgement.fault_code) } else { String::new() };
        log(&app, level, "回写", format!("resultCode={}{fault} resultSn={sn} done↑ · {}", judgement.plc_code, judgement.reason));
        let drain_ms = self.part.as_ref().and_then(|p| p.end_at).map(|t| t.elapsed().as_millis() as u64);
        self.result = Some(ResultView { sn, recipe_id, ts: now_ms(), drain_ms, judgement });
    }

    fn count(&mut self, v: Verdict) {
        self.stats.total += 1;
        match v {
            Verdict::Ok | Verdict::OkWithExcursion => self.stats.ok += 1,
            Verdict::ErrInspect => self.stats.err += 1,
            _ => self.stats.ng += 1,
        }
    }

    fn enter_fault(&mut self, reason: String) {
        let in_flight = matches!(self.phase, Phase::Validate | Phase::Acquire | Phase::Drain | Phase::Judge);
        if in_flight {
            let sn = self.part.as_ref().map_or(0, |p| p.sn);
            let judgement = Judgement::error(fault::DEVICE_LOST, format!("{reason}，结果未回写"));
            self.count(judgement.verdict);
            log(&self.app, "err", "在途件中断", format!("SN {sn} 记 ERR 98，需人工处理该件"));
            self.result = Some(ResultView {
                sn,
                recipe_id: self.part.as_ref().map(|p| p.recipe.id.clone()),
                ts: now_ms(),
                drain_ms: None,
                judgement,
            });
        }
        self.fault_needs_reset = in_flight;
        log(&self.app, "err", "故障", reason.clone());
        self.fault = Some(reason);
        self.set_phase(Phase::Fault);
    }

    async fn recover(&mut self) {
        let app = self.app.clone();
        let r = async {
            put(&app, tag::ARMED, json!(false)).await?;
            put(&app, tag::BUSY, json!(false)).await?;
            put(&app, tag::DONE, json!(false)).await?;
            put(&app, tag::VISION_READY, json!(true)).await
        }
        .await;
        match r {
            Ok(()) => {
                self.fault = None;
                self.alarms.clear();
                self.set_phase(Phase::Idle);
                log(&app, "ok", "visionReady↑", "视觉就绪，等待工件");
            }
            Err(e) => {
                self.fault_needs_reset = true;
                self.fault = Some(format!("无法写入视觉就绪信号：{e}"));
                self.dirty = true;
            }
        }
    }

    async fn reset(&mut self) {
        if self.phase != Phase::Fault {
            return;
        }
        self.fault_needs_reset = false;
        log(&self.app, "info", "故障复位", "");
        if plc(&self.app).status().state == LinkState::Connected {
            self.recover().await;
        }
    }
}

#[tauri::command]
pub fn cycle_snapshot(cycle: State<'_, CycleHost>) -> Option<Snapshot> {
    cycle.shared.lock().unwrap().snapshot.clone()
}

#[tauri::command]
pub fn cycle_logs(cycle: State<'_, CycleHost>) -> Vec<LogLine> {
    cycle.shared.lock().unwrap().logs.iter().cloned().collect()
}

#[tauri::command]
pub fn cycle_part_data(cycle: State<'_, CycleHost>) -> Vec<Measured> {
    cycle.shared.lock().unwrap().measured.clone()
}

#[tauri::command]
pub fn cycle_recipes(cycle: State<'_, CycleHost>) -> Vec<RecipeSummary> {
    cycle
        .recipes
        .iter()
        .map(|r| RecipeSummary {
            id: r.id.clone(),
            name: r.name.clone(),
            version: r.version,
            hash: r.hash.clone(),
            product_code: r.product_code,
            shot_count: r.shot_count(),
            trigger_mode: r.trigger_mode,
        })
        .collect()
}

#[tauri::command]
pub fn cycle_layout(cycle: State<'_, CycleHost>, recipe_id: String) -> Result<Arc<Recipe>, String> {
    cycle.recipe(&recipe_id).ok_or_else(|| format!("配方不存在：{recipe_id}"))
}

#[tauri::command]
pub fn cycle_get_settings(cycle: State<'_, CycleHost>) -> CycleSettings {
    cycle.settings()
}

#[tauri::command]
pub fn cycle_save_settings(cycle: State<'_, CycleHost>, settings: CycleSettings) -> Result<(), String> {
    settings.validate()?;
    if let Some(id) = &settings.manual_recipe_id {
        cycle.recipe(id).ok_or("配方不存在")?;
    }
    settings.save(&cycle.settings_path)?;
    cycle.shared.lock().unwrap().settings = settings;
    let _ = cycle.tx.send(Input::Refresh);
    Ok(())
}

/// 人工选择配方，只能在空闲或故障时切换，新配方从下一个工件起生效。
#[tauri::command]
pub fn cycle_select_recipe(cycle: State<'_, CycleHost>, recipe_id: String) -> Result<(), String> {
    cycle.recipe(&recipe_id).ok_or("配方不存在")?;
    let mut shared = cycle.shared.lock().unwrap();
    let phase = shared.snapshot.as_ref().map_or(Phase::Idle, |s| s.phase);
    if !matches!(phase, Phase::Idle | Phase::Fault) {
        return Err("检测进行中，工件结束后再切换".into());
    }
    let mut settings = shared.settings.clone();
    settings.manual_recipe_id = Some(recipe_id);
    settings.save(&cycle.settings_path)?;
    shared.settings = settings;
    let _ = cycle.tx.send(Input::Refresh);
    Ok(())
}

#[tauri::command]
pub fn cycle_reset(cycle: State<'_, CycleHost>) {
    let _ = cycle.tx.send(Input::Reset);
}
