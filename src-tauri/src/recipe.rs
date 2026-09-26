use std::f32::consts::PI;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SegmentKind {
    Line,
    Corner,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TriggerMode {
    Fly,
    Stop,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JudgeParams {
    pub nominal: f32,
    pub tol_upper: f32,
    pub tol_lower: f32,
    pub abs_min: f32,
    pub abs_max: f32,
    pub max_excursion_len: f32,
}

impl JudgeParams {
    pub fn lower(&self) -> f32 {
        self.nominal - self.tol_lower
    }
    pub fn upper(&self) -> f32 {
        self.nominal + self.tol_upper
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub name: String,
    pub kind: SegmentKind,
    pub s0: f32,
    pub s1: f32,
    pub params: JudgeParams,
}

/// 名义胶路上的测量点，按弧长等间距排列，第 j 个点的弧长为 j * spacing。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PathPoints {
    pub x: Vec<f32>,
    pub y: Vec<f32>,
    pub seg: Vec<u8>,
    pub k: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    pub id: String,
    pub name: String,
    pub version: u32,
    pub hash: String,
    pub product_code: u16,
    pub trigger_mode: TriggerMode,
    /// 工件外形：宽、高、圆角半径（mm）
    pub part: [f32; 3],
    pub fov: [f32; 2],
    pub shots: Vec<[f32; 2]>,
    pub spacing: f32,
    pub filter_window: usize,
    pub max_gap_len: f32,
    pub segments: Vec<Segment>,
    pub points: PathPoints,
}

impl Recipe {
    pub fn shot_count(&self) -> usize {
        self.shots.len()
    }

    pub fn point_count(&self) -> usize {
        self.points.k.len()
    }

    pub fn owned_points(&self, k: usize) -> impl Iterator<Item = usize> + '_ {
        self.points.k.iter().enumerate().filter(move |(_, &o)| o as usize == k).map(|(j, _)| j)
    }
}

const FOV: [f32; 2] = [216.0, 145.0];

fn line_params() -> JudgeParams {
    JudgeParams { nominal: 0.75, tol_upper: 0.75, tol_lower: 0.75, abs_min: 0.0, abs_max: 2.0, max_excursion_len: 2.0 }
}

fn corner_params() -> JudgeParams {
    JudgeParams { nominal: 0.75, tol_upper: 1.0, tol_lower: 1.0, abs_min: 0.0, abs_max: 2.2, max_excursion_len: 3.0 }
}

fn rounded_rect(id: &str, name: &str, product_code: u16, w: f32, h: f32, r: f32, shots: Vec<[f32; 2]>) -> Recipe {
    type PathFn = Box<dyn Fn(f32) -> (f32, f32)>;
    let arc = |cx: f32, cy: f32, a0: f32| -> PathFn { Box::new(move |t| (cx + r * (a0 + t / r).cos(), cy + r * (a0 + t / r).sin())) };
    let quarter = PI * r / 2.0;
    let pieces: Vec<(&str, SegmentKind, f32, PathFn)> = vec![
        ("长边 A", SegmentKind::Line, w - 2.0 * r, Box::new(move |t| (r + t, 0.0))),
        ("R 角 1", SegmentKind::Corner, quarter, arc(w - r, r, -PI / 2.0)),
        ("短边 B", SegmentKind::Line, h - 2.0 * r, Box::new(move |t| (w, r + t))),
        ("R 角 2", SegmentKind::Corner, quarter, arc(w - r, h - r, 0.0)),
        ("长边 C", SegmentKind::Line, w - 2.0 * r, Box::new(move |t| (w - r - t, h))),
        ("R 角 3", SegmentKind::Corner, quarter, arc(r, h - r, PI / 2.0)),
        ("短边 D", SegmentKind::Line, h - 2.0 * r, Box::new(move |t| (0.0, h - r - t))),
        ("R 角 4", SegmentKind::Corner, quarter, arc(r, r, PI)),
    ];

    let mut segments = Vec::new();
    let mut s = 0.0;
    for (name, kind, len, _) in &pieces {
        let params = if *kind == SegmentKind::Line { line_params() } else { corner_params() };
        segments.push(Segment { name: name.to_string(), kind: *kind, s0: s, s1: s + len, params });
        s += len;
    }
    let total = s;

    let spacing = 0.5;
    let mut points = PathPoints::default();
    let mut j = 0;
    while (j as f32) * spacing < total {
        let s = j as f32 * spacing;
        let gi = segments.iter().position(|g| s < g.s1).unwrap_or(segments.len() - 1);
        let (x, y) = (pieces[gi].3)(s - segments[gi].s0);
        let mut owner = 0;
        let mut best = f32::MAX;
        for (k, [cx, cy]) in shots.iter().enumerate() {
            let d = (x - cx).powi(2) + (y - cy).powi(2);
            if d <= best {
                best = d;
                owner = k;
            }
        }
        points.x.push(x);
        points.y.push(y);
        points.seg.push(gi as u8);
        points.k.push(owner as u8);
        j += 1;
    }

    let mut recipe = Recipe {
        id: id.into(),
        name: name.into(),
        version: 1,
        hash: String::new(),
        product_code,
        trigger_mode: TriggerMode::Fly,
        part: [w, h, r],
        fov: FOV,
        shots,
        spacing,
        filter_window: 5,
        max_gap_len: 0.5,
        segments,
        points,
    };
    recipe.hash = content_hash(&recipe);
    recipe
}

fn content_hash(recipe: &Recipe) -> String {
    let bytes = serde_json::to_vec(recipe).unwrap_or_default();
    let h = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3));
    format!("{:016x}", h)
}

/// P1 阶段内置的演示配方；配方管理与示教在 P3 接入后替换。
pub fn builtin() -> Vec<Arc<Recipe>> {
    let mut b = rounded_rect(
        "MTR-HSG-B",
        "电机壳体 B",
        13,
        380.0,
        200.0,
        24.0,
        vec![[95.0, 50.0], [285.0, 50.0], [285.0, 150.0], [95.0, 150.0]],
    );
    b.trigger_mode = TriggerMode::Stop;
    b.hash = content_hash(&b);
    vec![
        Arc::new(rounded_rect(
            "MTR-HSG-A",
            "电机壳体 A",
            12,
            520.0,
            230.0,
            28.0,
            vec![[95.0, 57.5], [260.0, 57.5], [425.0, 57.5], [425.0, 172.5], [260.0, 172.5], [95.0, 172.5]],
        )),
        Arc::new(b),
    ]
}
