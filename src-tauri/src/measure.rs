use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

use crate::cycle::Input;
use crate::judge::PointState;
use crate::recipe::Recipe;
use crate::sim::Scenario;

pub struct Job {
    pub sn: u32,
    pub k: usize,
    pub recipe: Arc<Recipe>,
    pub scenario: Scenario,
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
}

/// 逐帧测量工作线程。P1 为模拟测量，P3 起替换为 lyFlow 流程（locate + caliper）。
pub fn spawn_worker(out: UnboundedSender<Input>) -> UnboundedSender<Job> {
    let (tx, mut rx) = unbounded_channel::<Job>();
    tauri::async_runtime::spawn(async move {
        while let Some(job) = rx.recv().await {
            let started = Instant::now();
            tokio::time::sleep(Duration::from_millis(170 + (job.k as u64 * 13) % 60)).await;
            let mut m = simulate(&job);
            m.ms = started.elapsed().as_millis() as u32;
            if out.send(Input::Measured(m)).is_err() {
                break;
            }
        }
    });
    tx
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
        sn: job.sn,
        k: job.k,
        located,
        score: if located { 0.91 + ((job.k * 7) % 5) as f32 / 100.0 } else { 0.38 },
        ms: 0,
        idx: Vec::new(),
        d: Vec::new(),
        st: Vec::new(),
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
