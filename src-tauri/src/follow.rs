//! 随动检测的几何：相机装在胶枪上，胶嘴在图像里位置固定；工件坐标里胶嘴后方已涂好的那段胶条，
//! 经"相对胶嘴的位移 → 旋转（相机方位）→ 缩放（像素当量）"落到图像上。
//! 假设胶枪在涂胶过程中姿态不变（三目方案的前提：不管往哪个方向走，总有一台相机看得到身后的胶）。

use std::collections::VecDeque;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::recipe::{FollowSpec, FollowTiming, Recipe};

/// 一台随动相机相对胶嘴的标定（示教得到，属于相机工位，不随配方变）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowCalib {
    /// 胶嘴中心的像素坐标
    pub nozzle: [f32; 2],
    /// 工件坐标方向转到图像方向的角度（度，顺时针为正）
    pub angle_deg: f32,
    /// 工件坐标 x 轴在图像里是否镜像
    #[serde(default)]
    pub mirror: bool,
    pub mm_per_px: f32,
    /// 胶嘴遮挡半径（像素），圆内不测
    pub mask_px: f32,
    pub image_size: [u32; 2],
}

impl FollowCalib {
    pub fn validate(&self) -> Result<(), String> {
        if !(self.mm_per_px > 0.001 && self.mm_per_px < 5.0) {
            return Err("像素当量需在 0.001–5 mm/px 之间".into());
        }
        if !(self.mask_px >= 0.0 && self.image_size[0] > 16 && self.image_size[1] > 16) {
            return Err("胶嘴遮挡半径不能为负，图像尺寸需大于 16 px".into());
        }
        if !self.nozzle.iter().chain([self.angle_deg].iter()).all(|v| v.is_finite()) {
            return Err("胶嘴位置与方位必须是有限数".into());
        }
        Ok(())
    }

    /// 工件坐标里相对胶嘴的位移（mm）→ 像素。
    pub fn to_px(&self, d: [f32; 2]) -> [f32; 2] {
        let [x, y] = self.dir_to_img(d);
        [self.nozzle[0] + x / self.mm_per_px, self.nozzle[1] + y / self.mm_per_px]
    }


    /// 工件坐标里的方向 → 图像里的方向（不缩放）。
    pub fn dir_to_img(&self, d: [f32; 2]) -> [f32; 2] {
        let x = if self.mirror { -d[0] } else { d[0] };
        let (s, c) = self.angle_deg.to_radians().sin_cos();
        [x * c - d[1] * s, x * s + d[1] * c]
    }

    /// 像素点离图像边界至少 margin 像素。
    pub fn inside(&self, p: [f32; 2], margin: f32) -> bool {
        let [w, h] = self.image_size.map(|v| v as f32);
        p[0] >= margin && p[1] >= margin && p[0] <= w - 1.0 - margin && p[1] <= h - 1.0 - margin
    }

    /// 像素点离图像边界与胶嘴遮挡区都至少 margin 像素。
    pub fn usable(&self, p: [f32; 2], margin: f32) -> bool {
        let (dx, dy) = (p[0] - self.nozzle[0], p[1] - self.nozzle[1]);
        self.inside(p, margin) && (dx * dx + dy * dy).sqrt() > self.mask_px + margin
    }
}

/// 胶嘴在弧长 s 时，测量点 j 在它身后多远（mm）；还没涂到返回 None。闭合胶路绕圈后取最近一圈。
pub fn behind(recipe: &Recipe, s: f32, j: usize) -> Option<f32> {
    let sj = j as f32 * recipe.spacing;
    if s < sj {
        return None;
    }
    Some(if recipe.closed { (s - sj).rem_euclid(recipe.length()) } else { s - sj })
}

/// 一个测量点在某帧图像里的位置与法向（像素、单位向量）。
pub fn project(recipe: &Recipe, calib: &FollowCalib, s_nozzle: f32, j: usize) -> ([f32; 2], [f32; 2]) {
    let n = recipe.pos(s_nozzle);
    let s = j as f32 * recipe.spacing;
    let p = [recipe.points.x[j], recipe.points.y[j]];
    (calib.to_px([p[0] - n[0], p[1] - n[1]]), calib.dir_to_img(recipe.normal(s)))
}

/// 这台相机在胶嘴位于 s 时能测到的点：在可测窗口里、投影落在图像内且不被胶嘴挡住。
/// 远端多放一个测量步长：丢了一帧时，刚出窗口还没测的点下一帧还能补上。
pub fn visible(recipe: &Recipe, spec: &FollowSpec, calib: &FollowCalib, s: f32) -> Vec<usize> {
    let sp = recipe.spacing;
    let margin = spec.search_mm / calib.mm_per_px + 2.0;
    let n = recipe.point_count();
    let far = spec.far_mm + spec.step_mm;
    let mut out = Vec::new();
    // 只看窗口覆盖的那一小段弧长，不遍历整条胶路
    let first = ((s - far) / sp).floor() as i64;
    let last = ((s - spec.near_mm) / sp).ceil() as i64;
    for i in first..=last {
        let j = if recipe.closed { i.rem_euclid(n as i64) as usize } else if i < 0 || i >= n as i64 { continue } else { i as usize };
        let Some(b) = behind(recipe, s, j) else { continue };
        if b < spec.near_mm || b > far || out.contains(&j) {
            continue;
        }
        if calib.usable(project(recipe, calib, s, j).0, margin) {
            out.push(j);
        }
    }
    out
}

/// 一件工件在随动检测中的覆盖状态：哪些点已经分给某一帧去测。
pub struct Tracker {
    pub recipe: Arc<Recipe>,
    spec: FollowSpec,
    cams: Vec<(u8, FollowCalib)>,
    covered: Vec<bool>,
    /// 布防时刻（ms），按时间推算胶嘴位置的起点
    pub armed_ts: i64,
    /// 从图像里估出来的胶嘴位置修正（mm），加到推算值上
    pub offset: f32,
    start_synced: bool,
    start_probe_pending: bool,
    /// 每次起点同步改了位置就 +1；更早发出的帧里起点附近的读数作废
    pub generation: u32,
    /// 起点同步修正的量（mm）
    pub start_delta: f32,
    plc: Progress,
    /// 各相机上一帧的胶嘴位置，与相邻两帧间胶嘴走过的距离（平滑后）
    last_s: Vec<Option<f32>>,
    advance: f32,
}

/// PLC 进度寄存器只在轮询时读到，帧的时刻（拍下的时刻）与读到进度的时刻对不上：帧可能在好几次轮询之后才处理。
/// 所以记下最近几秒的变化（值与第一次读到它的轮询时刻），按帧的时刻取值：
/// - 帧落在两次变化之间：间隔正常就线性内插；间隔长（机器人停过，或轮询卡了）就先停在前一个值、到后一个值之前按速度赶上；
/// - 帧在最近一次变化之后：按估出的速度外推，最多推一个间隔，两个间隔还没变就是停了，停在读到的值上。
struct Progress {
    /// 零点：布防那一刻寄存器里的值（PLC 没清零时是上一件的终值）；读不到时用第一次读到的值。
    /// 之后读到比零点还小的值说明 PLC 清零了，零点回到 0
    zero: Option<f32>,
    /// 最近几秒的变化，旧的在前
    samples: VecDeque<(f32, i64)>,
    speed: f32,
    /// 机器人在走时相邻两次变化的间隔（ms），初值取轮询周期
    gap: f32,
}

/// 进度历史留这么久（ms）：处理得最晚的帧也在里面。
const HISTORY_MS: i64 = 3000;
/// 开走前进度比零点小这么多（mm）才算 PLC 清零了。
const CLEAR_DROP_MM: f32 = 1.0;

impl Progress {
    fn new(poll_ms: f32) -> Self {
        Self { zero: None, samples: VecDeque::new(), speed: 0.0, gap: poll_ms.max(10.0) }
    }

    fn update(&mut self, raw: f32, poll_ts: i64) {
        if !raw.is_finite() {
            return;
        }
        let zero = *self.zero.get_or_insert(raw);
        // 还没走就明显比零点小：PLC 布防后才清零，零点回到 0。差一两个计数是抖动；走起来以后变小是 PLC 收尾清零，都不理
        if raw < zero - CLEAR_DROP_MM && self.samples.len() <= 1 {
            self.zero = Some(0.0);
            self.samples.clear();
        }
        let value = raw - self.zero.unwrap();
        let Some(&(pv, pt)) = self.samples.back() else {
            self.samples.push_back((value, poll_ts));
            return;
        };
        if value <= pv + 1e-3 || poll_ts <= pt {
            return;
        }
        let dt = (poll_ts - pt) as f32;
        if dt > 3.0 * self.gap {
            // 停了一会儿又动了（起步延时、中途停顿），或者轮询卡了：起步时刻不知道，这一跳不估速度。
            // 间隔慢慢放大，轮询本来就慢时几次之后就按正常间隔估速度
            self.gap = (self.gap * 1.5).min(dt);
        } else {
            let inst = ((value - pv) / (dt / 1000.0)).clamp(0.0, 2000.0);
            self.speed = if self.speed == 0.0 { inst } else { 0.7 * self.speed + 0.3 * inst };
            self.gap = 0.8 * self.gap + 0.2 * dt;
        }
        self.samples.push_back((value, poll_ts));
        while self.samples.len() > 2 && poll_ts - self.samples[0].1 > HISTORY_MS {
            self.samples.pop_front();
        }
    }

    fn at(&self, frame_ts: i64) -> f32 {
        let Some(&(v, t)) = self.samples.back() else { return 0.0 };
        if frame_ts >= t {
            let dt = (frame_ts - t) as f32;
            return if dt > 2.0 * self.gap { v } else { v + self.speed * dt.min(self.gap) / 1000.0 };
        }
        let Some(i) = self.samples.iter().rposition(|&(_, ts)| ts <= frame_ts) else { return self.samples[0].0 };
        let ((v0, t0), (v1, t1)) = (self.samples[i], self.samples[i + 1]);
        let dt = (t1 - t0) as f32;
        if dt <= 3.0 * self.gap || self.speed <= 0.0 {
            v0 + (v1 - v0) * (frame_ts - t0) as f32 / dt
        } else {
            (v1 - self.speed * (t1 - frame_ts) as f32 / 1000.0).max(v0).min(v1)
        }
    }
}

/// 某帧要测的点；start_probe 为真时这一帧还要找胶条起点，做起点同步。calib 是本件开工时这台相机的标定。
pub struct Plan {
    pub points: Vec<u32>,
    pub start_probe: bool,
    pub calib: FollowCalib,
}

/// 单帧横向同步的修正量只采纳这么多，压住单帧噪声。
const LATERAL_GAIN: f32 = 0.7;

/// 开工时的 PLC 进度信息：寄存器此刻的原始值（按进度定位时当零点，读不到说明地址表里没有这个点位）与轮询周期。
pub struct PlcStart {
    pub zero: Option<f32>,
    pub poll_ms: f32,
}

impl Tracker {
    /// image_sync：这一件是图像测量（能从图里做起点同步）；模拟测量、或配方关了自动同步时不找起点。
    pub fn new(recipe: Arc<Recipe>, cams: Vec<(u8, FollowCalib)>, armed_ts: i64, image_sync: bool, plc: PlcStart) -> Result<Self, String> {
        let spec = recipe.follow.clone().ok_or("配方没有随动参数")?;
        if cams.is_empty() {
            return Err("随动相机都没有标定".into());
        }
        let mut progress = Progress::new(plc.poll_ms);
        if let (FollowTiming::Plc { scale }, Some(zero)) = (&spec.timing, plc.zero) {
            // 布防这一刻胶嘴在起点
            progress.update(zero * scale, armed_ts);
        }
        let n = recipe.point_count();
        Ok(Self {
            recipe,
            cams,
            covered: vec![false; n],
            armed_ts,
            offset: 0.0,
            start_synced: !(image_sync && spec.auto_sync),
            start_probe_pending: false,
            generation: 0,
            start_delta: 0.0,
            plc: progress,
            last_s: vec![None; 8],
            advance: 0.0,
            spec,
        })
    }

    pub fn uses_plc(&self) -> bool {
        matches!(self.spec.timing, FollowTiming::Plc { .. })
    }

    /// PLC 进度寄存器的一次变化（原始值与读到它的轮询时刻）。
    pub fn feed_progress(&mut self, raw: f32, poll_ts: i64) {
        if let FollowTiming::Plc { scale } = self.spec.timing {
            self.plc.update(raw * scale, poll_ts);
        }
    }

    /// 帧拍下时胶嘴的位置：按时间推算（或按 PLC 进度），再加上从图像里同步出来的修正。按进度定位却一次都没读到进度时返回 None。
    pub fn nozzle_s(&mut self, ts: i64) -> Option<f32> {
        let raw = match self.spec.timing {
            FollowTiming::Timed { speed_mm_s, delay_ms } => Some(speed_mm_s * ((ts - self.armed_ts) as f32 - delay_ms) / 1000.0),
            FollowTiming::Plc { .. } => (!self.plc.samples.is_empty()).then(|| self.plc.at(ts)),
        };
        raw.map(|s| s + self.offset)
    }

    /// 胶条起点可能在图上：可以沿胶路找它在哪里断开，一次校正起步误差。
    /// 胶嘴附近一段被遮住，推算又可能超前好几毫米，所以比可测窗口放宽到远端再加一个步长和 6 mm。
    fn start_in_window(&self, s: f32) -> bool {
        s - self.spec.near_mm - 2.0 > 0.0 && s < self.spec.far_mm + self.spec.step_mm + 6.0
    }

    /// 找起点的那一帧测完了（不管找没找到）。没找到时还在可找的范围里就等下一帧再找，出了范围由 start_given_up 收尾。
    pub fn probe_done(&mut self) {
        self.start_probe_pending = false;
    }

    /// 起点同步：δ = 推算位置 − 实际位置。之后发出的帧算新一代。
    pub fn apply_start(&mut self, delta: f32) {
        self.start_synced = true;
        self.offset -= delta;
        self.start_delta = delta;
        self.generation += 1;
    }

    /// 这些点已经测过（起点同步那一帧按新位置顺带重测的），后面的帧不用再分。
    pub fn cover(&mut self, points: &[u32]) {
        for &j in points {
            if let Some(c) = self.covered.get_mut(j as usize) {
                *c = true;
            }
        }
    }

    /// 拐角处的横向同步：δ = 推算位置 − 实际位置。
    pub fn apply_lateral(&mut self, delta: f32) {
        self.offset -= LATERAL_GAIN * delta;
    }

    /// 起点同步一直没成、胶条起点已经出了窗口时返回 true（只返回一次）。
    pub fn start_given_up(&mut self, s: f32) -> bool {
        if !self.start_synced && !self.start_probe_pending && s >= self.spec.far_mm + self.spec.step_mm + 6.0 {
            self.start_synced = true;
            return true;
        }
        false
    }

    /// 胶嘴位于 s 时由哪台相机测：能看到的点最多的那台（返回它在 cams 里的位置与能看到的点）。
    fn best_cam(&self, s: f32) -> Option<(usize, Vec<usize>)> {
        self.cams
            .iter()
            .enumerate()
            .map(|(i, (_, calib))| (i, visible(&self.recipe, &self.spec, calib, s)))
            .filter(|(_, v)| !v.is_empty())
            .max_by(|a, b| a.1.len().cmp(&b.1.len()).then(self.cams[b.0].0.cmp(&self.cams[a.0].0)))
    }

    /// 相机 cam 在胶嘴位于 s 时拍到的一帧：要不要测、测哪些点。分出去的点记为已覆盖。
    pub fn plan(&mut self, cam: u8, s: f32) -> Option<Plan> {
        if let Some(prev) = self.last_s.get(cam as usize).copied().flatten() {
            let d = s - prev;
            if d > 0.0 && d < self.spec.far_mm {
                self.advance = if self.advance == 0.0 { d } else { 0.8 * self.advance + 0.2 * d };
            }
        }
        if let Some(slot) = self.last_s.get_mut(cam as usize) {
            *slot = Some(s);
        }
        if s < self.spec.near_mm {
            return None;
        }
        let (best, visible) = self.best_cam(s)?;
        if self.cams[best].0 != cam {
            return None;
        }
        let fresh: Vec<usize> = visible.into_iter().filter(|&j| !self.covered[j]).collect();
        let start_probe = !self.start_synced && !self.start_probe_pending && self.start_in_window(s);
        let sp = self.recipe.spacing;
        let oldest = fresh.iter().filter_map(|&j| behind(&self.recipe, s, j)).fold(0.0f32, f32::max);
        let enough = fresh.len() as f32 * sp >= self.spec.step_mm;
        // 最老的那个点等不到下一帧就要出窗口了，不等攒够也测（胶嘴遮挡与搜索余量让实际窗口比配置的短）
        let leaving = oldest + (self.advance * 1.5).max(self.spec.step_mm) >= self.spec.far_mm;
        if !start_probe && (fresh.is_empty() || !(enough || leaving)) {
            return None;
        }
        for &j in &fresh {
            self.covered[j] = true;
        }
        self.start_probe_pending |= start_probe;
        Some(Plan { points: fresh.into_iter().map(|j| j as u32).collect(), start_probe, calib: self.cams[best].1.clone() })
    }


    /// 测不成的点放回去，之后的帧还在窗口里就再测一次。
    pub fn release(&mut self, points: &[u32]) {
        for &j in points {
            if let Some(c) = self.covered.get_mut(j as usize) {
                *c = false;
            }
        }
    }

    /// 胶嘴走完全程所需的名义弧长（含超行程）。
    pub fn end_s(&self) -> f32 {
        self.recipe.length() + self.spec.overrun_mm
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::builtin;

    /// 三台相机 120° 均布，胶嘴在图像下方正中，身后的胶条朝图像上方延伸。
    pub fn tri_calib() -> Vec<(u8, FollowCalib)> {
        (0..3u8)
            .map(|c| {
                (c, FollowCalib { nozzle: [640.0, 900.0], angle_deg: 120.0 * c as f32, mirror: false, mm_per_px: 0.05, mask_px: 60.0, image_size: [1280, 1024] })
            })
            .collect()
    }

    #[test]
    fn projection_rotates_then_scales() {
        let c = FollowCalib { nozzle: [100.0, 200.0], angle_deg: 90.0, mirror: true, mm_per_px: 0.5, mask_px: 0.0, image_size: [640, 480] };
        // 镜像后 (1,0) → (−1,0)，再顺时针转 90° → (0,−1)
        let p = c.to_px([1.0, 0.0]);
        assert!((p[0] - 100.0).abs() < 1e-4 && (p[1] - 198.0).abs() < 1e-4, "{p:?}");
    }

    /// 机器人 80 mm/s 走 1 s、停 0.5 s 再走；PLC 每 10 ms 刷新进度（0.1 mm 一个单位），视觉按不同周期轮询。
    /// 帧有的马上处理，有的晚 0.4 s 才处理（那时已经读到后面的进度了）。
    #[test]
    fn plc_progress_tracks_motion_and_holds_when_stopped() {
        let truth = |t: i64| {
            let t = t as f32 / 1000.0;
            80.0 * t.min(1.0) + 80.0 * (t - 1.5).max(0.0)
        };
        for (poll, moving_tol) in [(50i64, 1.5f32), (300, 3.0)] {
            let mut p = Progress::new(poll as f32);
            let mut read = (0.0f32, 0i64);
            let (mut moving, mut stopped) = (0.0f32, 0.0f32);
            let steady = |t: i64| (2 * poll..1000).contains(&t) || (poll == 50 && (1600..2500).contains(&t));
            let still = |t: i64| poll == 50 && (1150..1500).contains(&t);
            for t in 0..2500i64 {
                if t % poll == 0 {
                    let v = (truth(t - t % 10) * 10.0).round() / 10.0;
                    if v != read.0 {
                        read = (v, t);
                    }
                }
                if t % 10 != 0 {
                    continue;
                }
                p.update(read.0, read.1);
                for f in [t, t - 400] {
                    let e = (p.at(f) - truth(f)).abs();
                    if steady(f) && (f == t || poll == 50) {
                        moving = moving.max(e);
                    } else if still(f) {
                        stopped = stopped.max(e);
                    }
                }
            }
            assert!(moving < moving_tol, "轮询 {poll} ms：运动中误差 {moving:.2} mm");
            if poll == 50 {
                assert!(stopped < 0.2, "停下后还在往前推：误差 {stopped:.2} mm");
                // 收尾时 PLC 把进度清零，之前拍的帧还在处理：还按走过的进度取
                p.update(0.0, 2800);
                assert!((p.at(2490) - truth(2490)).abs() < moving_tol);
            }
        }
    }

    #[test]
    fn three_cameras_cover_whole_closed_path() {
        let recipe = builtin().into_iter().find(|r| r.follow.is_some()).unwrap();
        let mut t = Tracker::new(recipe.clone(), tri_calib(), 0, false, PlcStart { zero: None, poll_ms: 50.0 }).unwrap();
        let mut s = 0.0;
        while s < t.end_s() {
            for c in 0..3 {
                t.plan(c, s);
            }
            s += 80.0 / 25.0;
        }
        let missed = t.covered.iter().filter(|c| !**c).count();
        assert_eq!(missed, 0, "未覆盖 {missed} 点");
    }
}
