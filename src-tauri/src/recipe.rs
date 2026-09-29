//! 配方：磁盘上存可编辑的 `RecipeDoc`，加载时生成运行用的 `Recipe`（分段、测量点、哈希）。
//! `Recipe` 同时是检测记录里的配方快照，新增字段都要带默认值，旧快照才能读回来。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

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

/// 检测工况：飞拍（涂完后机器人带相机逐点拍）或随动（相机装在胶枪上边涂边测）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InspectMode {
    #[default]
    FlyShot,
    Follow,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

    fn validate(&self, what: &str) -> Result<(), String> {
        let ok = [self.nominal, self.tol_upper, self.tol_lower, self.abs_min, self.abs_max, self.max_excursion_len].iter().all(|v| v.is_finite());
        if !ok || self.tol_upper < 0.0 || self.tol_lower < 0.0 || self.max_excursion_len < 0.0 {
            return Err(format!("{what}：限值必须是有限数，公差与允许超差长度不能为负"));
        }
        if !(self.abs_min <= self.nominal && self.nominal <= self.abs_max) {
            return Err(format!("{what}：名义值 {:.2} 要在绝对限 [{:.2}, {:.2}] 之内", self.nominal, self.abs_min, self.abs_max));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub name: String,
    pub kind: SegmentKind,
    pub s0: f32,
    pub s1: f32,
    /// 位置：飞拍是内边→胶中线距离，随动是胶中线相对名义胶路的横向偏移
    pub params: JudgeParams,
    /// 胶宽；为空时不判胶宽
    #[serde(default)]
    pub width: Option<JudgeParams>,
}

/// 名义胶路上的测量点，按弧长等间距排列，第 j 个点的弧长为 j * spacing。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PathPoints {
    pub x: Vec<f32>,
    pub y: Vec<f32>,
    pub seg: Vec<u8>,
    /// 飞拍：负责该点的拍照点；随动恒为 0（运行时按帧分配）
    pub k: Vec<u8>,
}

/// 名义胶路的几何。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PathSpec {
    RoundedRect { width: f32, height: f32, radius: f32 },
    /// 折线，拐角按 radius 倒圆（0 为尖角）
    Polyline { points: Vec<[f32; 2]>, closed: bool, radius: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Polarity {
    /// 胶条比背景暗
    Dark,
    Light,
    Any,
}

/// 随动时胶嘴沿胶路走到哪里。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum FollowTiming {
    /// 布防后等 delay_ms 开始，按名义速度匀速走
    Timed { speed_mm_s: f32, delay_ms: f32 },
    /// PLC 寄存器 pathProgress 给出已走弧长，值 × scale = mm
    Plc { scale: f32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowSpec {
    /// 参与检测的相机（相机组序号）
    pub cameras: Vec<u8>,
    pub timing: FollowTiming,
    /// 胶嘴后方可测窗口：离胶嘴 near..far mm 的胶条
    pub near_mm: f32,
    pub far_mm: f32,
    /// 新进入窗口的胶条累计到这么长才测一帧
    pub step_mm: f32,
    /// 走完胶路后多走的距离（关胶后），让最后一段也进窗口
    pub overrun_mm: f32,
    /// 卡尺沿法向的搜索半宽
    pub search_mm: f32,
    pub bead_width: f32,
    pub polarity: Polarity,
    /// 胶条边缘的最小灰度差，低于它记为缺胶
    pub min_contrast: f32,
    /// 从图像同步胶嘴位置：起点处找胶条起点、拐角处用横向偏移反推沿程误差
    #[serde(default = "yes")]
    pub auto_sync: bool,
    /// 起点区（开放胶路还有终点区）内不判断胶与测不成：起胶、收胶处胶条本来就不规整
    #[serde(default = "default_zone")]
    pub start_zone_mm: f32,
}

fn default_zone() -> f32 {
    4.0
}

impl FollowSpec {
    fn validate(&self) -> Result<(), String> {
        if self.cameras.is_empty() {
            return Err("随动配方至少要有 1 台相机".into());
        }
        if !(self.near_mm >= 0.0 && self.far_mm > self.near_mm + 1.0) {
            return Err("可测窗口需满足 0 ≤ 近端 < 远端 − 1 mm".into());
        }
        if !(self.step_mm > 0.0 && self.step_mm <= self.far_mm - self.near_mm) {
            return Err("测量步长需在 0 到窗口长度之间".into());
        }
        if !(self.search_mm > 0.0 && self.bead_width > 0.0 && self.overrun_mm >= 0.0 && self.min_contrast >= 0.0 && self.start_zone_mm >= 0.0) {
            return Err("搜索半宽、名义胶宽需为正，超行程、最小灰度差与起点区不能为负".into());
        }
        match self.timing {
            FollowTiming::Timed { speed_mm_s, delay_ms } if !(speed_mm_s > 0.0 && delay_ms >= 0.0) => Err("名义速度需为正、起步延时不能为负".into()),
            FollowTiming::Plc { scale } if !(scale > 0.0) => Err("进度换算系数需为正".into()),
            _ => Ok(()),
        }
    }
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    pub id: String,
    pub name: String,
    pub version: u32,
    pub hash: String,
    pub product_code: u16,
    #[serde(default)]
    pub mode: InspectMode,
    pub trigger_mode: TriggerMode,
    /// 工件外形：宽、高、圆角半径（mm）；折线胶路为包围盒宽高、半径 0
    pub part: [f32; 3],
    #[serde(default)]
    pub path: Option<PathSpec>,
    #[serde(default = "yes")]
    pub closed: bool,
    pub fov: [f32; 2],
    pub shots: Vec<[f32; 2]>,
    /// 飞拍用的相机（相机组序号）
    #[serde(default)]
    pub camera: u8,
    pub spacing: f32,
    pub filter_window: usize,
    pub max_gap_len: f32,
    pub segments: Vec<Segment>,
    pub points: PathPoints,
    #[serde(default)]
    pub follow: Option<FollowSpec>,
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

    /// 胶路全长。闭合胶路的最后一个点到第一个点还有一个间距。
    pub fn length(&self) -> f32 {
        let n = self.point_count() as f32;
        if self.closed { n * self.spacing } else { (n - 1.0).max(0.0) * self.spacing }
    }

    /// 只看胶路几何与拍照点的哈希：示教资料跟它走，改判定限值不用重新示教。
    pub fn geometry_hash(&self) -> String {
        let key = serde_json::json!([self.path, self.part, self.spacing, self.shots, self.fov, self.closed]);
        let bytes = serde_json::to_vec(&key).unwrap_or_default();
        let h = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3));
        format!("{:016x}", h)
    }

    /// 本配方要用到的相机。
    pub fn cameras(&self) -> Vec<u8> {
        match (&self.mode, &self.follow) {
            (InspectMode::Follow, Some(f)) => f.cameras.clone(),
            _ => vec![self.camera],
        }
    }

    /// 弧长 s 处的名义位置。闭合胶路按周长取模；开放胶路超出两端时沿端点切向外推。
    pub fn pos(&self, s: f32) -> [f32; 2] {
        let n = self.point_count();
        if n == 0 {
            return [0.0, 0.0];
        }
        if n == 1 {
            return [self.points.x[0], self.points.y[0]];
        }
        let sp = self.spacing;
        let p = |j: usize| [self.points.x[j], self.points.y[j]];
        if self.closed {
            let s = s.rem_euclid(self.length());
            let j = ((s / sp) as usize).min(n - 1);
            let t = s / sp - j as f32;
            let (a, b) = (p(j), p((j + 1) % n));
            return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        }
        let j = ((s / sp).floor().max(0.0) as usize).min(n - 2);
        let t = s / sp - j as f32;
        let (a, b) = (p(j), p(j + 1));
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
    }

    /// 弧长 s 处的单位切向（沿涂胶方向）。
    pub fn tangent(&self, s: f32) -> [f32; 2] {
        let h = self.spacing * 0.5;
        let (a, b) = (self.pos(s - h), self.pos(s + h));
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = (dx * dx + dy * dy).sqrt().max(1e-6);
        [dx / l, dy / l]
    }

    /// 横向偏移的正方向：切向顺时针转 90°（y 向下的坐标里是涂胶方向的左手侧）。
    pub fn normal(&self, s: f32) -> [f32; 2] {
        let [tx, ty] = self.tangent(s);
        [ty, -tx]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentLimits {
    pub position: JudgeParams,
    #[serde(default)]
    pub width: Option<JudgeParams>,
}

/// 配方文件的内容，也是配方页编辑的对象。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeDoc {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: u32,
    pub product_code: u16,
    #[serde(default)]
    pub mode: InspectMode,
    pub trigger_mode: TriggerMode,
    #[serde(default)]
    pub camera: u8,
    pub path: PathSpec,
    pub spacing: f32,
    pub filter_window: usize,
    pub max_gap_len: f32,
    pub line: SegmentLimits,
    pub corner: SegmentLimits,
    /// 按段名覆盖的限值
    #[serde(default)]
    pub segment_overrides: BTreeMap<String, SegmentLimits>,
    #[serde(default)]
    pub fov: [f32; 2],
    #[serde(default)]
    pub shots: Vec<[f32; 2]>,
    #[serde(default)]
    pub follow: Option<FollowSpec>,
}

#[derive(Clone, Copy, Debug)]
enum Piece {
    Line { a: [f32; 2], b: [f32; 2] },
    Arc { c: [f32; 2], r: f32, a0: f32, sweep: f32 },
}

impl Piece {
    fn len(&self) -> f32 {
        match *self {
            Piece::Line { a, b } => ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt(),
            Piece::Arc { r, sweep, .. } => r * sweep.abs(),
        }
    }

    fn at(&self, t: f32) -> [f32; 2] {
        match *self {
            Piece::Line { a, b } => {
                let l = self.len().max(1e-9);
                [a[0] + (b[0] - a[0]) * t / l, a[1] + (b[1] - a[1]) * t / l]
            }
            Piece::Arc { c, r, a0, sweep } => {
                let a = a0 + sweep.signum() * t / r;
                [c[0] + r * a.cos(), c[1] + r * a.sin()]
            }
        }
    }
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn unit(v: [f32; 2]) -> [f32; 2] {
    let l = (v[0] * v[0] + v[1] * v[1]).sqrt().max(1e-9);
    [v[0] / l, v[1] / l]
}

/// 折线按半径倒圆，得到依次首尾相接的直线段与圆弧。
fn fillet(points: &[[f32; 2]], closed: bool, radius: f32, names: Option<&[&str]>) -> Result<Vec<(String, SegmentKind, Piece)>, String> {
    let n = points.len();
    if n < 2 || (closed && n < 3) {
        return Err("胶路至少要有 2 个点（闭合至少 3 个）".into());
    }
    let edges = if closed { n } else { n - 1 };
    let dir: Vec<[f32; 2]> = (0..edges).map(|i| unit(sub(points[(i + 1) % n], points[i]))).collect();
    let elen: Vec<f32> = (0..edges).map(|i| Piece::Line { a: points[i], b: points[(i + 1) % n] }.len()).collect();
    if elen.iter().any(|&l| l < 1e-3) {
        return Err("胶路有重合的相邻点".into());
    }
    // 每个顶点：进、出切点与圆弧（没有拐角或半径为 0 时切点就是顶点）
    let corner = |i: usize| -> ([f32; 2], [f32; 2], Option<Piece>) {
        let v = points[i];
        let interior = closed || (i > 0 && i < n - 1);
        if !interior || radius <= 0.0 {
            return (v, v, None);
        }
        let (din, dout) = (dir[(i + edges - 1) % edges], dir[i % edges]);
        let phi = (din[0] * dout[1] - din[1] * dout[0]).atan2(din[0] * dout[0] + din[1] * dout[1]);
        if phi.abs() < 1e-4 {
            return (v, v, None);
        }
        let half = (phi.abs() / 2.0).tan();
        let t = (radius * half).min(elen[(i + edges - 1) % edges] / 2.0).min(elen[i % edges] / 2.0);
        let r = t / half;
        let t1 = [v[0] - din[0] * t, v[1] - din[1] * t];
        let t2 = [v[0] + dout[0] * t, v[1] + dout[1] * t];
        let s = phi.signum();
        let c = [t1[0] - din[1] * r * s, t1[1] + din[0] * r * s];
        let a0 = (t1[1] - c[1]).atan2(t1[0] - c[0]);
        (t1, t2, Some(Piece::Arc { c, r, a0, sweep: phi }))
    };
    let corners: Vec<_> = (0..n).map(corner).collect();
    let mut pieces = Vec::new();
    // 第 e 条边与它末端的拐角；给了名字表时按"边、拐角、边、拐角…"的顺序取
    let name = |kind: SegmentKind, e: usize| -> String {
        let i = if kind == SegmentKind::Line { e * 2 } else { e * 2 + 1 };
        match (names.and_then(|n| n.get(i)), kind) {
            (Some(nm), _) => nm.to_string(),
            (None, SegmentKind::Line) => format!("边 {}", e + 1),
            (None, SegmentKind::Corner) => format!("拐角 {}", e + 1),
        }
    };
    for e in 0..edges {
        let (a, b) = (corners[e].1, corners[(e + 1) % n].0);
        let line = Piece::Line { a, b };
        if line.len() > 1e-4 {
            pieces.push((name(SegmentKind::Line, e), SegmentKind::Line, line));
        }
        if let Some(arc) = corners[(e + 1) % n].2 {
            if closed || e + 1 < n - 1 {
                pieces.push((name(SegmentKind::Corner, e), SegmentKind::Corner, arc));
            }
        }
    }
    Ok(pieces)
}

const RECT_NAMES: [&str; 8] = ["长边 A", "R 角 1", "短边 B", "R 角 2", "长边 C", "R 角 3", "短边 D", "R 角 4"];

impl RecipeDoc {
    pub fn validate(&self) -> Result<(), String> {
        let id_ok = !self.id.is_empty() && self.id.len() <= 32 && self.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if !id_ok {
            return Err("配方编号只能用字母、数字、- 和 _，最长 32 个字符".into());
        }
        if self.name.trim().is_empty() {
            return Err("配方名称不能为空".into());
        }
        if !(0.1..=5.0).contains(&self.spacing) {
            return Err("测量点间距需在 0.1–5 mm 之间".into());
        }
        if self.filter_window == 0 || self.filter_window > 31 || self.filter_window % 2 == 0 {
            return Err("中值滤波窗口需为 1–31 的奇数".into());
        }
        if !(self.max_gap_len >= 0.0) {
            return Err("允许断胶长度不能为负".into());
        }
        if self.camera >= 8 {
            return Err("相机序号超出范围".into());
        }
        for (what, l) in [("直线段", &self.line), ("拐角", &self.corner)].into_iter().chain(self.segment_overrides.iter().map(|(k, v)| (k.as_str(), v))) {
            l.position.validate(&format!("{what} 位置"))?;
            if let Some(w) = &l.width {
                w.validate(&format!("{what} 胶宽"))?;
            }
        }
        match self.path {
            PathSpec::RoundedRect { width, height, radius } => {
                if !(width > 0.0 && height > 0.0 && radius >= 0.0 && 2.0 * radius <= width.min(height)) {
                    return Err("圆角矩形的宽高需为正，圆角半径不超过短边一半".into());
                }
            }
            PathSpec::Polyline { ref points, radius, .. } => {
                if points.iter().flatten().any(|v| !v.is_finite()) || radius < 0.0 {
                    return Err("胶路点坐标必须是有限数，倒圆半径不能为负".into());
                }
            }
        }
        match self.mode {
            InspectMode::FlyShot => {
                if self.shots.is_empty() || self.shots.len() > 64 {
                    return Err("飞拍配方需要 1–64 个拍照点".into());
                }
                if !(self.fov[0] > 0.0 && self.fov[1] > 0.0) {
                    return Err("视野宽高需为正".into());
                }
            }
            InspectMode::Follow => self.follow.as_ref().ok_or("随动配方缺少随动参数")?.validate()?,
        }
        Ok(())
    }

    pub fn build(&self) -> Result<Recipe, String> {
        self.validate()?;
        let (pieces, closed, part) = match self.path {
            PathSpec::RoundedRect { width: w, height: h, radius: r } => {
                let pts = [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]];
                (fillet(&pts, true, r, Some(&RECT_NAMES))?, true, [w, h, r])
            }
            PathSpec::Polyline { ref points, closed, radius } => {
                let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for p in points {
                    (x0, y0, x1, y1) = (x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1]));
                }
                (fillet(points, closed, radius, None)?, closed, [x1 - x0, y1 - y0, 0.0])
            }
        };
        let limits = |name: &str, kind: SegmentKind| {
            self.segment_overrides.get(name).cloned().unwrap_or_else(|| if kind == SegmentKind::Line { self.line.clone() } else { self.corner.clone() })
        };
        let mut segments = Vec::new();
        let mut s = 0.0;
        for (name, kind, piece) in &pieces {
            let l = limits(name, *kind);
            let len = piece.len();
            segments.push(Segment { name: name.clone(), kind: *kind, s0: s, s1: s + len, params: l.position, width: l.width });
            s += len;
        }
        let total = s;
        if total < 2.0 * self.spacing {
            return Err("胶路太短".into());
        }
        if total / self.spacing > 200_000.0 {
            return Err("测量点超过 20 万个，加大间距".into());
        }
        let follow = self.mode == InspectMode::Follow;
        let mut points = PathPoints::default();
        let mut j = 0usize;
        loop {
            let s = j as f32 * self.spacing;
            if s >= total - if closed { 1e-4 } else { -1e-4 } {
                break;
            }
            let gi = segments.iter().position(|g| s < g.s1).unwrap_or(segments.len() - 1);
            let [x, y] = pieces[gi].2.at((s - segments[gi].s0).min(pieces[gi].2.len()));
            let owner = if follow {
                0
            } else {
                self.shots
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| ((x - a[0]).powi(2) + (y - a[1]).powi(2)).total_cmp(&((x - b[0]).powi(2) + (y - b[1]).powi(2))))
                    .map_or(0, |(k, _)| k)
            };
            points.x.push(x);
            points.y.push(y);
            points.seg.push(gi.min(255) as u8);
            points.k.push(owner as u8);
            j += 1;
        }
        if segments.len() > 255 {
            return Err("胶路分段超过 255 段".into());
        }
        let mut recipe = Recipe {
            id: self.id.clone(),
            name: self.name.trim().to_string(),
            version: 0,
            hash: String::new(),
            product_code: self.product_code,
            mode: self.mode,
            trigger_mode: self.trigger_mode,
            part,
            path: Some(self.path.clone()),
            closed,
            fov: self.fov,
            shots: if follow { Vec::new() } else { self.shots.clone() },
            camera: self.camera,
            spacing: self.spacing,
            filter_window: self.filter_window,
            max_gap_len: self.max_gap_len,
            segments,
            points,
            follow: if follow { self.follow.clone() } else { None },
        };
        recipe.hash = content_hash(&recipe);
        recipe.version = self.version.max(1);
        Ok(recipe)
    }
}

/// 只看内容的哈希（不含版本号），检测记录按它存配方快照。
fn content_hash(recipe: &Recipe) -> String {
    let bytes = serde_json::to_vec(recipe).unwrap_or_default();
    let h = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3));
    format!("{:016x}", h)
}

fn line_limits() -> SegmentLimits {
    SegmentLimits {
        position: JudgeParams { nominal: 0.75, tol_upper: 0.75, tol_lower: 0.75, abs_min: 0.0, abs_max: 2.0, max_excursion_len: 2.0 },
        width: None,
    }
}

fn corner_limits() -> SegmentLimits {
    SegmentLimits {
        position: JudgeParams { nominal: 0.75, tol_upper: 1.0, tol_lower: 1.0, abs_min: 0.0, abs_max: 2.2, max_excursion_len: 3.0 },
        width: None,
    }
}

fn follow_limits(bead: f32, corner: bool) -> SegmentLimits {
    let k = if corner { 1.3 } else { 1.0 };
    SegmentLimits {
        position: JudgeParams { nominal: 0.0, tol_upper: 0.8 * k, tol_lower: 0.8 * k, abs_min: -2.0 * k, abs_max: 2.0 * k, max_excursion_len: 3.0 },
        width: Some(JudgeParams {
            nominal: bead,
            tol_upper: 0.35 * bead,
            tol_lower: 0.3 * bead,
            abs_min: 0.4 * bead,
            abs_max: 1.9 * bead,
            max_excursion_len: 3.0,
        }),
    }
}

pub fn default_follow_spec(cameras: Vec<u8>, speed: f32) -> FollowSpec {
    FollowSpec {
        cameras,
        timing: FollowTiming::Timed { speed_mm_s: speed, delay_ms: 300.0 },
        near_mm: 3.0,
        far_mm: 18.0,
        step_mm: 6.0,
        overrun_mm: 20.0,
        search_mm: 4.0,
        bead_width: 2.0,
        polarity: Polarity::Dark,
        min_contrast: 18.0,
        auto_sync: true,
        start_zone_mm: 4.0,
    }
}

/// 首次启动写入的样例配方。
pub fn samples() -> Vec<RecipeDoc> {
    let fly = |id: &str, name: &str, code: u16, w: f32, h: f32, r: f32, shots: Vec<[f32; 2]>, trigger_mode| RecipeDoc {
        id: id.into(),
        name: name.into(),
        version: 1,
        product_code: code,
        mode: InspectMode::FlyShot,
        trigger_mode,
        camera: 0,
        path: PathSpec::RoundedRect { width: w, height: h, radius: r },
        spacing: 0.5,
        filter_window: 5,
        max_gap_len: 0.5,
        line: line_limits(),
        corner: corner_limits(),
        segment_overrides: BTreeMap::new(),
        fov: [216.0, 145.0],
        shots,
        follow: None,
    };
    let follow = RecipeDoc {
        id: "FLW-RECT".into(),
        name: "随动演示 · 圆角矩形".into(),
        version: 1,
        product_code: 21,
        mode: InspectMode::Follow,
        trigger_mode: TriggerMode::Fly,
        camera: 0,
        path: PathSpec::RoundedRect { width: 240.0, height: 140.0, radius: 20.0 },
        spacing: 0.5,
        filter_window: 5,
        max_gap_len: 1.0,
        line: follow_limits(2.0, false),
        corner: follow_limits(2.0, true),
        segment_overrides: BTreeMap::new(),
        fov: [0.0, 0.0],
        shots: Vec::new(),
        follow: Some(default_follow_spec(vec![0, 1, 2], 80.0)),
    };
    vec![
        fly(
            "MTR-HSG-A",
            "电机壳体 A",
            12,
            520.0,
            230.0,
            28.0,
            vec![[95.0, 57.5], [260.0, 57.5], [425.0, 57.5], [425.0, 172.5], [260.0, 172.5], [95.0, 172.5]],
            TriggerMode::Fly,
        ),
        fly("MTR-HSG-B", "电机壳体 B", 13, 380.0, 200.0, 24.0, vec![[95.0, 50.0], [285.0, 50.0], [285.0, 150.0], [95.0, 150.0]], TriggerMode::Stop),
        follow,
    ]
}

/// 测试与重判里需要一份现成配方时用。
#[cfg(test)]
pub fn builtin() -> Vec<Arc<Recipe>> {
    samples().iter().map(|d| Arc::new(d.build().unwrap())).collect()
}

/// 从 CSV（每行 x,y，可有表头）或 DXF（LWPOLYLINE / POLYLINE 顶点、LINE 端点）的文本读出折线点。
pub fn parse_points(text: &str, ext: &str) -> Result<Vec<[f32; 2]>, String> {
    let points = if ext.eq_ignore_ascii_case("dxf") { dxf_points(text) } else { csv_points(text) };
    let mut out: Vec<[f32; 2]> = Vec::new();
    for p in points {
        if out.last().is_none_or(|q| (q[0] - p[0]).abs() > 1e-4 || (q[1] - p[1]).abs() > 1e-4) {
            out.push(p);
        }
    }
    if out.len() < 2 {
        return Err("文件里没读到至少 2 个点".into());
    }
    Ok(out)
}

fn csv_points(text: &str) -> Vec<[f32; 2]> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split([',', ';', '\t', ' ']).map(str::trim).filter(|s| !s.is_empty());
            let x = it.next()?.parse::<f32>().ok()?;
            let y = it.next()?.parse::<f32>().ok()?;
            (x.is_finite() && y.is_finite()).then_some([x, y])
        })
        .collect()
}

fn dxf_points(text: &str) -> Vec<[f32; 2]> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out = Vec::new();
    let mut entity = "";
    let mut x: Option<f32> = None;
    let mut i = 0;
    while i + 1 < lines.len() {
        let (code, value) = (lines[i], lines[i + 1]);
        match code {
            "0" => {
                entity = value;
                x = None;
            }
            "10" | "11" if matches!(entity, "LWPOLYLINE" | "VERTEX" | "LINE") => x = value.parse().ok(),
            "20" | "21" if matches!(entity, "LWPOLYLINE" | "VERTEX" | "LINE") => {
                if let (Some(px), Ok(py)) = (x.take(), value.parse::<f32>()) {
                    out.push([px, py]);
                }
            }
            _ => {}
        }
        i += 2;
    }
    out
}

/// 磁盘上的配方库：每个配方一个 `<id>.json`。
pub struct RecipeStore {
    dir: PathBuf,
    inner: RwLock<Vec<(RecipeDoc, Arc<Recipe>)>>,
    errors: RwLock<Vec<String>>,
}

impl RecipeStore {
    pub fn open(dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建配方目录失败：{e}"))?;
        let store = Self { dir, inner: RwLock::new(Vec::new()), errors: RwLock::new(Vec::new()) };
        let empty = std::fs::read_dir(&store.dir).map_err(|e| e.to_string())?.flatten().all(|e| e.path().extension().is_none_or(|x| x != "json"));
        if empty {
            for doc in samples() {
                store.write(&doc)?;
            }
        }
        store.reload();
        Ok(store)
    }

    fn file(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    fn write(&self, doc: &RecipeDoc) -> Result<(), String> {
        let tmp = self.dir.join(format!("{}.json.tmp", doc.id));
        std::fs::write(&tmp, serde_json::to_string_pretty(doc).map_err(|e| e.to_string())?).map_err(|e| format!("写配方失败：{e}"))?;
        std::fs::rename(&tmp, self.file(&doc.id)).map_err(|e| format!("写配方失败：{e}"))
    }

    /// 重读目录。坏文件跳过并记下原因，不影响其他配方。
    pub fn reload(&self) {
        let mut list = Vec::new();
        let mut errors = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            let mut paths: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
            paths.sort();
            for p in paths {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
                let parsed = std::fs::read_to_string(&p)
                    .map_err(|e| e.to_string())
                    .and_then(|s| serde_json::from_str::<RecipeDoc>(&s).map_err(|e| e.to_string()))
                    .and_then(|d| d.build().map(|r| (d, Arc::new(r))));
                match parsed {
                    Ok(pair) => list.push(pair),
                    Err(e) => errors.push(format!("{name}：{e}")),
                }
            }
        }
        *self.inner.write().unwrap() = list;
        *self.errors.write().unwrap() = errors;
    }

    pub fn list(&self) -> Vec<Arc<Recipe>> {
        self.inner.read().unwrap().iter().map(|(_, r)| r.clone()).collect()
    }

    pub fn errors(&self) -> Vec<String> {
        self.errors.read().unwrap().clone()
    }

    pub fn get(&self, id: &str) -> Option<Arc<Recipe>> {
        self.inner.read().unwrap().iter().find(|(d, _)| d.id == id).map(|(_, r)| r.clone())
    }

    pub fn doc(&self, id: &str) -> Option<RecipeDoc> {
        self.inner.read().unwrap().iter().find(|(d, _)| d.id == id).map(|(d, _)| d.clone())
    }

    /// 保存配方。内容变了版本号 +1；产品代码不能和别的配方重复。
    pub fn save(&self, mut doc: RecipeDoc, original_id: Option<&str>) -> Result<Arc<Recipe>, String> {
        doc.name = doc.name.trim().to_string();
        let built = doc.build()?;
        let mut inner = self.inner.write().unwrap();
        let replacing = original_id.unwrap_or(&doc.id).to_string();
        if let Some((d, _)) = inner.iter().find(|(d, _)| d.id != replacing && (d.id == doc.id || d.product_code == doc.product_code)) {
            return Err(if d.id == doc.id { format!("配方编号 {} 已存在", doc.id) } else { format!("产品代码 {} 已被配方 {} 使用", doc.product_code, d.id) });
        }
        let old = inner.iter().find(|(d, _)| d.id == replacing).map(|(d, r)| (d.version, r.hash.clone()));
        doc.version = match old {
            Some((v, h)) if h == built.hash => v,
            Some((v, _)) => v + 1,
            None => doc.version.max(1),
        };
        let recipe = Arc::new(Recipe { version: doc.version, ..built });
        self.write(&doc)?;
        if replacing != doc.id {
            let _ = std::fs::remove_file(self.file(&replacing));
        }
        inner.retain(|(d, _)| d.id != replacing && d.id != doc.id);
        inner.push((doc, recipe.clone()));
        inner.sort_by(|a, b| a.0.id.cmp(&b.0.id));
        Ok(recipe)
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        let mut inner = self.inner.write().unwrap();
        if !inner.iter().any(|(d, _)| d.id == id) {
            return Err("配方不存在".into());
        }
        std::fs::remove_file(self.file(id)).map_err(|e| format!("删除配方文件失败：{e}"))?;
        inner.retain(|(d, _)| d.id != id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn rounded_rect_matches_perimeter() {
        let r = samples()[0].build().unwrap();
        let [w, h, rad] = r.part;
        let perimeter = 2.0 * (w + h - 4.0 * rad) + 2.0 * PI * rad;
        assert!((r.segments.last().unwrap().s1 - perimeter).abs() < 1e-2);
        assert_eq!(r.segments[1].name, "R 角 1");
        let p = r.pos(r.segments[1].s1);
        assert!((p[0] - w).abs() < 1e-2 && (p[1] - rad).abs() < 1e-2);
    }

    #[test]
    fn open_polyline_fillet() {
        let doc = RecipeDoc {
            path: PathSpec::Polyline { points: vec![[0.0, 0.0], [100.0, 0.0], [100.0, 50.0]], closed: false, radius: 10.0 },
            ..samples()[2].clone()
        };
        let r = doc.build().unwrap();
        assert!(!r.closed);
        assert_eq!(r.segments.len(), 3);
        let total = r.segments[2].s1;
        assert!((total - (90.0 + 40.0 + PI * 5.0)).abs() < 1e-2);
    }
}
