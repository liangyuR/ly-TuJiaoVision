use std::sync::Arc;
use std::time::{Duration, Instant};

use ly_plc::now_ms;
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::sync::Semaphore;

use crate::cycle::{CycleHost, Input};
use crate::judge::PointState;
use crate::recipe::Recipe;
use crate::sim::Scenario;
use crate::vision::{self, FrameImage, Pose, StationMeasure, VisionHost, FLYSHOT_GRAPH};

pub struct Job {
    pub sn: u32,
    pub k: usize,
    pub recipe: Arc<Recipe>,
    pub scenario: Scenario,
    pub image: Option<Arc<FrameImage>>,
}

pub const ST_OK: u8 = 0;
pub const ST_GAP: u8 = 1;
pub const ST_INVALID: u8 = 2;

/// 单帧测量结果，只含该帧负责的测量点。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Measured {
    pub sn: u32,
    pub k: usize,
    pub located: bool,
    pub score: f32,
    pub ms: u32,
    /// 测量本身没做成（lyFlow 运行失败、没有示教资料等）
    pub error: Option<String>,
    pub idx: Vec<u32>,
    pub d: Vec<f32>,
    pub st: Vec<u8>,
}

impl Measured {
    pub fn point_state(&self, i: usize) -> PointState {
        match self.st[i] {
            ST_OK => PointState::Measured(self.d[i]),
            ST_GAP => PointState::Gap,
            _ => PointState::Invalid,
        }
    }

    fn empty(job: &Job) -> Self {
        Self { sn: job.sn, k: job.k, located: false, score: 0.0, ms: 0, error: None, idx: Vec::new(), d: Vec::new(), st: Vec::new() }
    }
}

/// 逐帧测量。带图像的帧走 lyFlow（定位 + 逐点卡尺，最多两帧并行），不带图像的帧用模拟测量。
pub fn spawn_worker(app: AppHandle, out: UnboundedSender<Input>) -> UnboundedSender<Job> {
    let (tx, mut rx) = unbounded_channel::<Job>();
    let permits = Arc::new(Semaphore::new(2));
    tauri::async_runtime::spawn(async move {
        while let Some(job) = rx.recv().await {
            let Ok(permit) = permits.clone().acquire_owned().await else { break };
            let (app, out) = (app.clone(), out.clone());
            tauri::async_runtime::spawn(async move {
                let started = Instant::now();
                let mut m = match job.image.clone() {
                    Some(image) => {
                        let fallback = Measured::empty(&job);
                        tauri::async_runtime::spawn_blocking(move || match measure_lyflow(&app, &job, &image) {
                            Ok(m) => m,
                            Err(e) => Measured { error: Some(e), ..Measured::empty(&job) },
                        })
                        .await
                        .unwrap_or(Measured { error: Some("测量线程异常退出".into()), ..fallback })
                    }
                    None => {
                        tokio::time::sleep(Duration::from_millis(170 + (job.k as u64 * 13) % 60)).await;
                        simulate(&job)
                    }
                };
                m.ms = started.elapsed().as_millis() as u32;
                let _ = out.send(Input::Measured(m));
                drop(permit);
            });
        }
    });
    tx
}

fn measure_lyflow(app: &AppHandle, job: &Job, image: &FrameImage) -> Result<Measured, String> {
    let settings = app.state::<CycleHost>().settings();
    let engine = app.state::<VisionHost>().engine(settings.lyflow_core.as_deref()).ok_or("lyFlow 核心库未加载")?;
    let assets = vision::assets_for(app, &job.recipe)?;
    if !assets.calib.exists() {
        return Err("工位未标定：先在图像源页用标定板标定".into());
    }
    let params = assets.params(job.k).ok_or_else(|| format!("拍照点 k={} 没有示教资料", job.k))?;
    let base = assets.calib.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let run_id = format!("{}-k{}-{}", job.sn, job.k, now_ms());
    let r = engine.run(FLYSHOT_GRAPH, &run_id, &base, image, &params)?;
    if r.status() == "failed" {
        return Err(r.failure());
    }
    let pose: Pose = r.record("pose").and_then(|v| serde_json::from_value(v.clone()).ok()).ok_or("图没有输出 pose")?;
    let sm: StationMeasure = r.record("measure").and_then(|v| serde_json::from_value(v.clone()).ok()).ok_or("图没有输出 measure")?;
    if sm.unit != "mm" {
        return Err(format!("测量单位是 {}，标定文件不是毫米", sm.unit));
    }
    let mut m = Measured { located: pose.ok, score: pose.score as f32, ..Measured::empty(job) };
    for (i, id) in sm.ids.iter().enumerate() {
        let Some(j) = id.as_u64() else { continue };
        let (d, st) = match sm.status[i].as_str() {
            "ok" => match sm.inner_center[i] {
                Some(d) => (d as f32, ST_OK),
                None => (0.0, ST_INVALID),
            },
            "no_bead" => (0.0, ST_GAP),
            _ => (0.0, ST_INVALID),
        };
        m.idx.push(j as u32);
        m.d.push(d);
        m.st.push(st);
    }
    Ok(m)
}

fn noise(s: f32) -> f32 {
    let v = (s * 12.9898).sin() * 43758.547;
    (v - v.floor()) * 2.0 - 1.0
}

fn simulate(job: &Job) -> Measured {
    let r = &job.recipe;
    let located = job.scenario.locate_fail_frame(r.shot_count()) != Some(job.k);
    let gap = job.scenario.gap_points(r);
    let bump_at = (job.scenario == Scenario::Excursion).then(|| {
        let seg = &r.segments[4.min(r.segments.len() - 1)];
        seg.s0 + (seg.s1 - seg.s0) * 0.3
    });

    let mut m = Measured {
        located,
        score: if located { 0.91 + ((job.k * 7) % 5) as f32 / 100.0 } else { 0.38 },
        ..Measured::empty(job)
    };
    for j in r.owned_points(job.k) {
        let s = j as f32 * r.spacing;
        let seg = &r.segments[r.points.seg[j] as usize];
        let mut d = 0.74 + 0.09 * (s / 43.0).sin() + 0.035 * (s / 5.7 + 1.3).sin() + 0.03 * noise(s);
        if seg.kind == crate::recipe::SegmentKind::Corner {
            d += 0.12 * ((s - seg.s0) / (seg.s1 - seg.s0) * std::f32::consts::PI).sin();
        }
        if let Some(c) = bump_at {
            d += (-((s - c) / 1.5).powi(2)).exp();
        }
        let st = if !located {
            ST_INVALID
        } else if gap.contains(&j) {
            ST_GAP
        } else {
            ST_OK
        };
        m.idx.push(j as u32);
        m.d.push(d);
        m.st.push(st);
    }
    m
}
