//! 本程序内置的卡尺测量（随动）：在每个测量点的投影位置沿胶路法向取灰度剖面，
//! 找一对极性相反的边缘作为胶条两侧，得出胶条中线的横向偏移与胶宽。

use crate::follow::{self, FollowCalib};
use crate::frame::FrameImage;
use crate::measure::{Measured, ST_GAP, ST_INVALID, ST_OK};
use crate::recipe::{FollowSpec, Polarity, Recipe};

/// 剖面采样步长（像素）。
const STEP_PX: f32 = 0.5;
/// 沿切向平均的半宽（像素），压噪声。
const BAND_PX: i32 = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bead {
    /// 胶条中线在剖面上的位置（样本序号，可带小数）
    pub center: f32,
    /// 胶宽（样本数）
    pub width: f32,
    /// 胶条内外的平均灰度差
    pub contrast: f32,
}

fn smooth(p: &[f32]) -> Vec<f32> {
    (0..p.len()).map(|i| (p[i.saturating_sub(1)] + p[i] + p[(i + 1).min(p.len() - 1)]) / 3.0).collect()
}

/// 梯度极值的抛物线亚像素位置。
fn refine(g: &[f32], i: usize) -> f32 {
    if i == 0 || i + 1 >= g.len() {
        return i as f32;
    }
    let (a, b, c) = (g[i - 1], g[i], g[i + 1]);
    let den = a - 2.0 * b + c;
    if den.abs() < 1e-6 {
        i as f32
    } else {
        i as f32 + (0.5 * (a - c) / den).clamp(-0.5, 0.5)
    }
}

fn mean(p: &[f32]) -> f32 {
    if p.is_empty() {
        0.0
    } else {
        p.iter().sum::<f32>() / p.len() as f32
    }
}

/// 暗胶条：先下降沿后上升沿。宽度限制与对比度都以样本为单位。
fn dark_bead(p: &[f32], w_min: f32, w_max: f32) -> Option<Bead> {
    let n = p.len();
    if n < 7 {
        return None;
    }
    let s = smooth(p);
    let g: Vec<f32> = (0..n).map(|i| (s[(i + 1).min(n - 1)] - s[i.saturating_sub(1)]) / 2.0).collect();
    let extrema = |sign: f32| -> Vec<usize> {
        (1..n - 1).filter(|&i| g[i] * sign > 0.0 && g[i] * sign >= g[i - 1] * sign && g[i] * sign >= g[i + 1] * sign).collect()
    };
    let (falls, rises) = (extrema(-1.0), extrema(1.0));
    let mut best: Option<(f32, Bead)> = None;
    for &f in &falls {
        for &r in rises.iter().filter(|&&r| r > f) {
            let (ef, er) = (refine(&g, f), refine(&g, r));
            let width = er - ef;
            if width < w_min || width > w_max {
                continue;
            }
            let side = ((width * 0.5).round() as usize).max(2);
            let inside = mean(&s[f + 1..r.max(f + 2).min(n)]);
            let outside = mean(&[&s[f.saturating_sub(side)..f], &s[(r + 1).min(n)..(r + 1 + side).min(n)]].concat());
            let contrast = outside - inside;
            let score = (-g[f]).min(g[r]) * contrast.max(0.0).sqrt();
            if best.is_none_or(|(b, _)| score > b) {
                best = Some((score, Bead { center: (ef + er) / 2.0, width, contrast }));
            }
        }
    }
    best.map(|(_, b)| b)
}

/// 在剖面里找胶条。w_min / w_max 与返回值都以样本为单位。
pub fn find_bead(profile: &[f32], polarity: Polarity, w_min: f32, w_max: f32) -> Option<Bead> {
    let inverted: Vec<f32> = profile.iter().map(|v| 255.0 - v).collect();
    match polarity {
        Polarity::Dark => dark_bead(profile, w_min, w_max),
        Polarity::Light => dark_bead(&inverted, w_min, w_max),
        Polarity::Any => match (dark_bead(profile, w_min, w_max), dark_bead(&inverted, w_min, w_max)) {
            (Some(a), Some(b)) => Some(if a.contrast >= b.contrast { a } else { b }),
            (a, b) => a.or(b),
        },
    }
}

/// 在某个位置沿法向找胶条的参数（毫米）。
#[derive(Clone, Copy, Debug)]
pub struct Search {
    pub search_mm: f32,
    pub bead_width: f32,
    pub polarity: Polarity,
    pub min_contrast: f32,
}

impl From<&FollowSpec> for Search {
    fn from(s: &FollowSpec) -> Self {
        Self { search_mm: s.search_mm, bead_width: s.bead_width, polarity: s.polarity, min_contrast: s.min_contrast }
    }
}

/// 一个位置上的卡尺读数。
#[derive(Clone, Copy, Debug)]
pub enum Reading {
    /// 找到胶条：中线相对 c 沿 n 的偏移、胶宽（mm）、灰度差、中线的像素位置
    Bead { offset_mm: f32, width_mm: f32, contrast: f32, px: [f32; 2] },
    /// 剖面在图里但没有够对比度的胶条
    NoBead,
    /// 剖面出了图像
    Outside,
}

/// 在像素 c 处沿单位向量 n 跑一次卡尺。测量、起点同步、标定试测共用这一个口径。
pub fn read_at(img: &FrameImage, mm_per_px: f32, c: [f32; 2], n: [f32; 2], q: &Search) -> Reading {
    let to_samples = |mm: f32| mm / mm_per_px / STEP_PX;
    let Some(prof) = profile(img, c, n, q.search_mm / mm_per_px) else { return Reading::Outside };
    let mid = (prof.len() / 2) as f32;
    match find_bead(&prof, q.polarity, to_samples(q.bead_width * 0.3), to_samples(q.bead_width * 2.2)) {
        Some(b) if b.contrast >= q.min_contrast => {
            let off = (b.center - mid) * STEP_PX;
            Reading::Bead { offset_mm: off * mm_per_px, width_mm: b.width * STEP_PX * mm_per_px, contrast: b.contrast, px: [c[0] + n[0] * off, c[1] + n[1] * off] }
        }
        _ => Reading::NoBead,
    }
}

/// 以像素 c 为中心、沿单位向量 n 取 ±half 像素的剖面，垂直方向平均 2·BAND_PX+1 条线。出了图像返回 None。
pub fn profile(img: &FrameImage, c: [f32; 2], n: [f32; 2], half: f32) -> Option<Vec<f32>> {
    let steps = (half / STEP_PX).ceil() as i32;
    let t = [-n[1], n[0]];
    let mut out = Vec::with_capacity((2 * steps + 1) as usize);
    for i in -steps..=steps {
        let a = i as f32 * STEP_PX;
        let mut sum = 0.0;
        for b in -BAND_PX..=BAND_PX {
            let (x, y) = (c[0] + n[0] * a + t[0] * b as f32, c[1] + n[1] * a + t[1] * b as f32);
            sum += img.sample(x, y)?;
        }
        out.push(sum / (2 * BAND_PX + 1) as f32);
    }
    Some(out)
}

/// 随动一帧：逐点在投影位置跑卡尺。
pub fn measure_follow(recipe: &Recipe, spec: &FollowSpec, calib: &FollowCalib, s: f32, points: &[u32], img: &FrameImage, out: &mut Measured) {
    if img.width != calib.image_size[0] || img.height != calib.image_size[1] {
        out.error = Some(format!("图像 {}×{} 与标定时的 {}×{} 不一致，重新标定该相机", img.width, img.height, calib.image_size[0], calib.image_size[1]));
        return;
    }
    let q = Search::from(spec);
    let mut contrast_sum = 0.0;
    let mut found = 0usize;
    for &j in points {
        let (c, n) = follow::project(recipe, calib, s, j as usize);
        let (d, w, st, p) = match read_at(img, calib.mm_per_px, c, n, &q) {
            Reading::Outside => (0.0, f32::NAN, ST_INVALID, c),
            Reading::NoBead => (0.0, f32::NAN, ST_GAP, c),
            Reading::Bead { offset_mm, width_mm, contrast, px } => {
                contrast_sum += contrast;
                found += 1;
                (offset_mm, width_mm, ST_OK, px)
            }
        };
        out.idx.push(j);
        out.d.push(d);
        out.w.push(w);
        out.st.push(st);
        out.px.push(p);
    }
    out.located = true;
    // 分数：找到胶条的点里平均灰度差相对最小要求的倍数，封顶 1
    out.score = if found == 0 { 0.0 } else { (contrast_sum / found as f32 / (spec.min_contrast.max(1.0) * 3.0)).min(1.0) };
    if spec.auto_sync {
        out.lateral_sync = lateral_sync(recipe, spec, s, out);
    }
}

/// 拐角里用横向偏移反推胶嘴沿胶路的位置误差 δ（推算 − 实际，mm），并从这一帧的偏移里扣掉。
/// 推算位置错了 δ，所有投影点就一起挪了 t·δ（t 为胶嘴处切向）；点 j 的横向读数因此多出 (n_j·t)·δ。
/// 直线段上 n_j·t ≈ 0，看不出也不需要修；拐角里 n_j·t 各不相同，最小二乘就能解出 δ。
fn lateral_sync(recipe: &Recipe, spec: &FollowSpec, s: f32, out: &mut Measured) -> Option<f32> {
    let ok: Vec<usize> = (0..out.idx.len()).filter(|&i| out.st[i] == ST_OK).collect();
    let coef = |t: [f32; 2], i: usize| {
        let n = recipe.normal(out.idx[i] as f32 * recipe.spacing);
        n[0] * t[0] + n[1] * t[1]
    };
    let solve = |t: [f32; 2], keep: &dyn Fn(usize) -> bool| {
        let (mut num, mut den) = (0.0f32, 0.0f32);
        for &i in ok.iter().filter(|&&i| keep(i)) {
            let c = coef(t, i);
            num += c * out.d[i];
            den += c * c;
        }
        (den >= 1.5).then(|| num / den)
    };
    let mut delta = solve(recipe.tangent(s), &|_| true)?;
    // 胶嘴误差的方向是 P(s) − P(s−δ) 的弦向，用中点切向再解一次，并剔除残差大的点
    let t = recipe.tangent(s - delta / 2.0);
    let d = out.d.clone();
    delta = solve(t, &|i| (d[i] - coef(t, i) * delta).abs() <= 0.4).unwrap_or(delta);
    let delta = delta.clamp(-spec.search_mm, spec.search_mm);
    for &i in &ok {
        out.d[i] -= coef(t, i) * delta;
    }
    Some(delta)
}

/// 起点同步：胶条起点在窗口里时，从胶嘴往回沿名义胶路逐 0.5 mm 看有没有胶，
/// 胶条断开处的弧长 u 就是推算误差 δ（推算 − 实际）：实际起点 s=0 在图上落在推算坐标的 u 处。
pub fn find_start(recipe: &Recipe, spec: &FollowSpec, calib: &FollowCalib, s: f32, img: &FrameImage) -> Option<f32> {
    let q = Search::from(spec);
    let half = spec.search_mm / calib.mm_per_px;
    let nozzle = recipe.pos(s);
    let mut last_present: Option<f32> = None;
    let mut absent = 0;
    let mut u = s - spec.near_mm;
    while u >= s - spec.far_mm - spec.step_mm - 12.0 {
        let here = u;
        u -= 0.5;
        let p = recipe.pos(here);
        let c = calib.to_px([p[0] - nozzle[0], p[1] - nozzle[1]]);
        if !calib.usable(c, half + 2.0) {
            // 靠近胶嘴的那段被挡住，跳过；已经走到图像外了就停
            if last_present.is_some() || !calib.inside(c, 0.0) {
                break;
            }
            continue;
        }
        let n = calib.dir_to_img(recipe.normal(here));
        if matches!(read_at(img, calib.mm_per_px, c, n, &q), Reading::Bead { .. }) {
            last_present = Some(here);
            absent = 0;
        } else if last_present.is_some() {
            absent += 1;
            if absent >= 4 {
                // 胶条起点是圆头，比起胶位置多出约半个胶宽
                return last_present.map(|u| u - 0.25 + spec.bead_width / 2.0);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_dark_bead_with_subsample_accuracy() {
        // 背景 180，样本 20.3–31.7 之间是 60 的暗胶条
        let p: Vec<f32> = (0..60)
            .map(|i| {
                let x = i as f32;
                let inside = ((x - 20.3).clamp(0.0, 1.0)) * ((31.7 - x).clamp(0.0, 1.0));
                180.0 - 120.0 * inside
            })
            .collect();
        let b = find_bead(&p, Polarity::Dark, 4.0, 30.0).unwrap();
        assert!((b.center - 26.0).abs() < 0.5, "{b:?}");
        assert!((b.width - 11.4).abs() < 1.0, "{b:?}");
        assert!(find_bead(&vec![180.0; 60], Polarity::Dark, 4.0, 30.0).is_none_or(|b| b.contrast < 5.0));
    }
}
