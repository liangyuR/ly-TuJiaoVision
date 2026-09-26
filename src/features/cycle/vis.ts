import type { Judgement, Measured, PartView, PointVis, Recipe } from "./types";

export const visColor: Record<PointVis, string> = {
  none: "#3b4a66",
  ok: "#22c55e",
  exc: "#f59e0b",
  ng: "#ef4444",
  gap: "#ef4444",
  inv: "#a78bfa",
  miss: "#a78bfa",
};

export function computeVis(layout: Recipe, part: PartView | null, measured: Measured[], result: Judgement | null): PointVis[] {
  const n = layout.points.k.length;
  const vis: PointVis[] = new Array(n).fill("none");
  if (!part || part.recipeId !== layout.id) return vis;
  for (const m of measured) {
    m.idx.forEach((j, i) => {
      const st = m.st[i];
      if (st === 1) vis[j] = "gap";
      else if (st === 2) vis[j] = "inv";
      else {
        const p = layout.segments[layout.points.seg[j]].params;
        const d = m.d[i];
        vis[j] = d < p.nominal - p.tolLower || d > p.nominal + p.tolUpper ? "exc" : "ok";
      }
    });
  }
  part.frames.forEach((f, k) => {
    if (f.status !== "missing") return;
    layout.points.k.forEach((owner, j) => {
      if (owner === k && vis[j] === "none") vis[j] = "miss";
    });
  });
  result?.segments.forEach((s, gi) => {
    if (s.verdict !== "NG_POSITION" && s.verdict !== "NG_ABSOLUTE") return;
    layout.points.seg.forEach((seg, j) => {
      if (seg === gi && vis[j] === "exc") vis[j] = "ng";
    });
  });
  return vis;
}

export interface Run {
  state: PointVis;
  from: number;
  to: number;
}

/** 按显示状态把闭合胶路切成连续段，末段与首段相连。 */
export function runs(vis: PointVis[]): Run[] {
  const out: Run[] = [];
  vis.forEach((v, j) => {
    const last = out[out.length - 1];
    if (last && last.state === v) last.to = j;
    else out.push({ state: v, from: j, to: j });
  });
  return out;
}

export function polyline(layout: Recipe, from: number, to: number, joinPrev: boolean, close: boolean) {
  const { x, y } = layout.points;
  const pts: string[] = [];
  for (let j = joinPrev ? Math.max(0, from - 1) : from; j <= to; j++) pts.push(`${x[j].toFixed(1)},${y[j].toFixed(1)}`);
  if (close) pts.push(`${x[0].toFixed(1)},${y[0].toFixed(1)}`);
  return pts.join(" ");
}

export function currentFrame(part: PartView | null) {
  if (!part) return -1;
  let k = -1;
  part.frames.forEach((f, i) => {
    if (f.status !== "waiting" && f.status !== "missing") k = i;
  });
  return k;
}
