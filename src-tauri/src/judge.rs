use serde::Serialize;

use crate::recipe::Recipe;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    Ok,
    OkWithExcursion,
    NgPosition,
    NgAbsolute,
    NgGap,
    ErrInspect,
}

impl Verdict {
    pub fn plc_code(self) -> u16 {
        match self {
            Verdict::Ok => 1,
            Verdict::OkWithExcursion => 2,
            Verdict::NgPosition => 11,
            Verdict::NgAbsolute => 12,
            Verdict::NgGap => 13,
            Verdict::ErrInspect => 90,
        }
    }
}

pub mod fault {
    pub const MISSING_FRAME: u16 = 91;
    pub const LOCATE_FAILED: u16 = 92;
    pub const INVALID_POINTS: u16 = 93;
    pub const SHOT_COUNT_MISMATCH: u16 = 94;
    pub const NO_RECIPE: u16 = 95;
    pub const EXTRA_FRAME: u16 = 96;
    pub const MOTION_TIMEOUT: u16 = 97;
    pub const DEVICE_LOST: u16 = 98;
    pub const PROCESS_TIMEOUT: u16 = 99;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointState {
    Pending,
    Measured(f32),
    /// 找到内边但未找到胶条
    Gap,
    /// 内边未找到，该点测不了
    Invalid,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentResult {
    pub verdict: Verdict,
    pub min: Option<f32>,
    pub max: Option<f32>,
    pub excursion_len: f32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GapRun {
    pub segment: usize,
    pub s0: f32,
    pub s1: f32,
    pub len: f32,
    pub frames: Vec<u8>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Judgement {
    pub verdict: Verdict,
    pub plc_code: u16,
    pub fault_code: u16,
    pub reason: String,
    pub segments: Vec<SegmentResult>,
    pub gaps: Vec<GapRun>,
}

impl Judgement {
    pub fn error(fault_code: u16, reason: impl Into<String>) -> Self {
        Self {
            verdict: Verdict::ErrInspect,
            plc_code: Verdict::ErrInspect.plc_code(),
            fault_code,
            reason: reason.into(),
            segments: Vec::new(),
            gaps: Vec::new(),
        }
    }
}

const MAX_INVALID_LEN: f32 = 2.0;

pub fn judge(recipe: &Recipe, table: &[PointState]) -> Judgement {
    let sp = recipe.spacing;
    let n = table.len();

    if let Some(j) = table.iter().position(|p| *p == PointState::Pending) {
        return Judgement::error(fault::INVALID_POINTS, format!("测量点 j={j} 未填写"));
    }
    for run in cyclic_runs(table, |p| *p == PointState::Invalid) {
        let len = run.len() as f32 * sp;
        if len > MAX_INVALID_LEN {
            let seg = &recipe.segments[recipe.points.seg[run[0]] as usize];
            return Judgement::error(fault::INVALID_POINTS, format!("{} 内边连续 {len:.1} mm 未找到", seg.name));
        }
    }

    let mut segments: Vec<SegmentResult> = recipe
        .segments
        .iter()
        .enumerate()
        .map(|(gi, seg)| {
            let p = &seg.params;
            let mut min: Option<f32> = None;
            let mut max: Option<f32> = None;
            let mut absolute = false;
            let mut max_run = 0usize;
            for run in linear_runs((0..n).filter(|&j| recipe.points.seg[j] as usize == gi), table) {
                let d = median_filter(&run, recipe.filter_window);
                let mut out = 0usize;
                for &v in &d {
                    min = Some(min.map_or(v, |m| m.min(v)));
                    max = Some(max.map_or(v, |m| m.max(v)));
                    absolute |= v < p.abs_min || v > p.abs_max;
                    if v < p.lower() || v > p.upper() {
                        out += 1;
                        max_run = max_run.max(out);
                    } else {
                        out = 0;
                    }
                }
            }
            let excursion_len = max_run as f32 * sp;
            let verdict = if absolute {
                Verdict::NgAbsolute
            } else if excursion_len > p.max_excursion_len {
                Verdict::NgPosition
            } else if excursion_len > 0.0 {
                Verdict::OkWithExcursion
            } else {
                Verdict::Ok
            };
            SegmentResult { verdict, min, max, excursion_len }
        })
        .collect();

    let mut gaps = Vec::new();
    for run in cyclic_runs(table, |p| *p == PointState::Gap) {
        let len = run.len() as f32 * sp;
        if len <= recipe.max_gap_len {
            continue;
        }
        let segment = recipe.points.seg[run[0]] as usize;
        let mut frames: Vec<u8> = run.iter().map(|&j| recipe.points.k[j]).collect();
        frames.dedup();
        segments[segment].verdict = segments[segment].verdict.max(Verdict::NgGap);
        gaps.push(GapRun { segment, s0: run[0] as f32 * sp, s1: (run[run.len() - 1] + 1) as f32 * sp, len, frames });
    }

    let verdict = segments.iter().map(|s| s.verdict).max().unwrap_or(Verdict::Ok);
    let reason = match verdict {
        Verdict::NgGap => {
            let g = &gaps[0];
            let frames = g.frames.iter().map(|k| format!("帧 {k}")).collect::<Vec<_>>().join(" + ");
            let merged = if g.frames.len() > 1 { "，跨帧合并" } else { "" };
            format!(
                "{} 断胶 {:.1} mm > {:.1} mm · s={:.1}–{:.1} · {frames}{merged}",
                recipe.segments[g.segment].name, g.len, recipe.max_gap_len, g.s0, g.s1
            )
        }
        Verdict::Ok => format!("{} 段全部合格 · {n} 点", segments.len()),
        _ => {
            let (gi, s) = segments.iter().enumerate().find(|(_, s)| s.verdict == verdict).unwrap();
            let seg = &recipe.segments[gi];
            match verdict {
                Verdict::NgAbsolute => format!(
                    "{} 超出绝对限 [{:.2}, {:.2}] · 实测 {:.2}–{:.2}",
                    seg.name,
                    seg.params.abs_min,
                    seg.params.abs_max,
                    s.min.unwrap_or(0.0),
                    s.max.unwrap_or(0.0)
                ),
                Verdict::NgPosition => format!(
                    "{} 连续超差 {:.1} mm > 允许 {:.1} mm",
                    seg.name, s.excursion_len, seg.params.max_excursion_len
                ),
                _ => format!(
                    "{} 局部超差 {:.1} mm ≤ 允许 {:.1} mm",
                    seg.name, s.excursion_len, seg.params.max_excursion_len
                ),
            }
        }
    };

    Judgement { verdict, plc_code: verdict.plc_code(), fault_code: 0, reason, segments, gaps }
}

/// 在闭合胶路上找满足条件的连续区间，首尾相接处合并为一段。
fn cyclic_runs(table: &[PointState], pred: impl Fn(&PointState) -> bool) -> Vec<Vec<usize>> {
    let n = table.len();
    let Some(start) = (0..n).find(|&j| !pred(&table[j])) else {
        return if n > 0 { vec![(0..n).collect()] } else { Vec::new() };
    };
    let mut runs = Vec::new();
    let mut cur = Vec::new();
    for i in 1..=n {
        let j = (start + i) % n;
        if pred(&table[j]) {
            cur.push(j);
        } else if !cur.is_empty() {
            runs.push(std::mem::take(&mut cur));
        }
    }
    runs
}

/// 段内连续的已测点，缺胶或无效点处断开，供滤波与超差长度统计。
fn linear_runs(indices: impl Iterator<Item = usize>, table: &[PointState]) -> Vec<Vec<f32>> {
    let mut runs = Vec::new();
    let mut cur = Vec::new();
    for j in indices {
        if let PointState::Measured(d) = table[j] {
            cur.push(d);
        } else if !cur.is_empty() {
            runs.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    runs
}

fn median_filter(values: &[f32], window: usize) -> Vec<f32> {
    let half = window / 2;
    if half == 0 || values.len() < window {
        return values.to_vec();
    }
    (0..values.len())
        .map(|i| {
            let lo = i.saturating_sub(half);
            let hi = (i + half + 1).min(values.len());
            let mut w = values[lo..hi].to_vec();
            w.sort_by(|a, b| a.total_cmp(b));
            w[w.len() / 2]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::builtin;

    fn base(recipe: &Recipe) -> Vec<PointState> {
        vec![PointState::Measured(0.75); recipe.point_count()]
    }

    #[test]
    fn gap_split_across_frames_is_merged() {
        let recipe = builtin().remove(0);
        let mut table = base(&recipe);
        let j = (0..recipe.point_count() - 1).find(|&j| recipe.points.k[j] == 1 && recipe.points.k[j + 1] == 2).unwrap();
        table[j] = PointState::Gap;
        table[j + 1] = PointState::Gap;
        let r = judge(&recipe, &table);
        assert_eq!(r.verdict, Verdict::NgGap);
        assert_eq!(r.gaps[0].frames, vec![1, 2]);
    }

    #[test]
    fn gap_wrapping_path_start_is_one_run() {
        let recipe = builtin().remove(0);
        let mut table = base(&recipe);
        let n = table.len();
        table[0] = PointState::Gap;
        table[n - 1] = PointState::Gap;
        let r = judge(&recipe, &table);
        assert_eq!(r.gaps.len(), 1);
        assert_eq!(r.gaps[0].len, 1.0);
    }

    #[test]
    fn short_excursion_is_allowed_long_one_is_ng() {
        let recipe = builtin().remove(0);
        let mut table = base(&recipe);
        (100..104).for_each(|j| table[j] = PointState::Measured(1.6));
        assert_eq!(judge(&recipe, &table).verdict, Verdict::OkWithExcursion);
        (100..110).for_each(|j| table[j] = PointState::Measured(1.6));
        assert_eq!(judge(&recipe, &table).verdict, Verdict::NgPosition);
    }
}
