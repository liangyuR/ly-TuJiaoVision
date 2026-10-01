import { useEffect, useMemo, useRef, useState } from "react";
import type { JudgeParams, Measured, PointVis, Recipe } from "../types";
import { CAM_COLORS } from "../vis";

const PAD = { l: 38, r: 8, t: 8, b: 30 };

interface Props {
  layout: Recipe;
  measured: Measured[];
  vis: PointVis[];
  /** d：位置（飞拍为距内边距离，随动为横向偏移）；w：胶宽 */
  quantity?: "d" | "w";
}

function niceTicks(lo: number, hi: number): number[] {
  const span = hi - lo;
  const raw = span / 4;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw) ?? raw;
  const out: number[] = [];
  for (let v = Math.ceil(lo / step) * step; v <= hi + 1e-9; v += step) out.push(Number(v.toFixed(6)));
  return out;
}

export default function UnrolledCurve({ layout, measured, vis, quantity = "d" }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 800, h: 150 });

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setSize({ w: e.contentRect.width, h: e.contentRect.height }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const n = layout.points.k.length;
  const total = (layout.closed ? n : n - 1) * layout.spacing;
  const limitOf = (g: Recipe["segments"][number]): JudgeParams | null => (quantity === "w" ? g.width : g.params);

  const { values, cams } = useMemo(() => {
    const values = new Float32Array(n).fill(NaN);
    const cams = new Int8Array(n).fill(-1);
    measured.forEach((m) =>
      m.idx.forEach((j, i) => {
        if (m.st[i] !== 0) return;
        const v = quantity === "w" ? m.w?.[i] : m.d[i];
        if (v == null) return;
        values[j] = v;
        cams[j] = m.cam ?? 0;
      }),
    );
    return { values, cams };
  }, [measured, n, quantity]);

  const [lo, hi] = useMemo(() => {
    let lo = Infinity,
      hi = -Infinity;
    layout.segments.forEach((g) => {
      const p = limitOf(g);
      if (!p) return;
      lo = Math.min(lo, p.absMin, p.nominal - p.tolLower);
      hi = Math.max(hi, p.absMax, p.nominal + p.tolUpper);
    });
    values.forEach((v) => {
      if (!Number.isNaN(v)) {
        lo = Math.min(lo, v);
        hi = Math.max(hi, v);
      }
    });
    if (!Number.isFinite(lo)) return [0, 1];
    const pad = (hi - lo) * 0.08 || 0.1;
    return [lo - pad, hi + pad];
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [layout, values, quantity]);

  const { w, h } = size;
  const plotB = h - PAD.b;
  const X = (s: number) => PAD.l + (s / total) * (w - PAD.l - PAD.r);
  const Y = (v: number) => plotB - ((Math.max(lo, Math.min(hi, v)) - lo) / (hi - lo)) * (plotB - PAD.t);

  const lines = useMemo(() => {
    const out: string[] = [];
    let cur: string[] = [];
    for (let j = 0; j < n; j++) {
      if (Number.isNaN(values[j])) {
        if (cur.length > 1) out.push(cur.join(" "));
        cur = [];
      } else cur.push(`${X(j * layout.spacing).toFixed(1)},${Y(values[j]).toFixed(1)}`);
    }
    if (cur.length > 1) out.push(cur.join(" "));
    return out;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [values, w, h, n, total, lo, hi, layout.spacing]);

  // 底部色条：飞拍为各拍照点负责区间，随动为实际测到该点的相机
  const owners = useMemo(() => {
    const out: { k: number; s0: number; s1: number }[] = [];
    const key = (j: number) => (layout.mode === "flyShot" ? layout.points.k[j] : cams[j]);
    for (let j = 0; j < n; j++) {
      const k = key(j);
      const last = out[out.length - 1];
      if (last && last.k === k) last.s1 = (j + 1) * layout.spacing;
      else out.push({ k, s0: j * layout.spacing, s1: (j + 1) * layout.spacing });
    }
    return out.filter((o) => o.k >= 0);
  }, [layout, cams, n]);

  return (
    <div ref={ref} className="curve-box">
      <svg width={w} height={h}>
        {niceTicks(lo, hi).map((v) => (
          <g key={v}>
            <line x1={PAD.l} x2={w - PAD.r} y1={Y(v)} y2={Y(v)} stroke="#1c2944" />
            <text x={PAD.l - 5} y={Y(v) + 3} textAnchor="end" fontSize={10} fill="#6b7b97">
              {v}
            </text>
          </g>
        ))}
        {layout.segments.map((g) => {
          const p = limitOf(g);
          return (
            <g key={g.name}>
              {p && (
                <>
                  <rect x={X(g.s0)} width={X(g.s1) - X(g.s0)} y={Y(p.nominal + p.tolUpper)} height={Y(p.nominal - p.tolLower) - Y(p.nominal + p.tolUpper)} fill="rgba(34,197,94,0.09)" />
                  <line x1={X(g.s0)} x2={X(g.s1)} y1={Y(p.absMax)} y2={Y(p.absMax)} stroke="rgba(239,68,68,0.7)" strokeDasharray="4 3" />
                  <line x1={X(g.s0)} x2={X(g.s1)} y1={Y(p.absMin)} y2={Y(p.absMin)} stroke="rgba(239,68,68,0.7)" strokeDasharray="4 3" />
                </>
              )}
              <line x1={X(g.s0)} x2={X(g.s0)} y1={PAD.t} y2={plotB + 8} stroke="#2a3a5a" />
              {X(g.s1) - X(g.s0) > 36 && (
                <text x={(X(g.s0) + X(g.s1)) / 2} y={h - 6} textAnchor="middle" fontSize={10.5} fill="#8391ab">
                  {g.name}
                </text>
              )}
            </g>
          );
        })}
        {owners.map((o) => (
          <rect key={o.s0} x={X(o.s0)} width={Math.max(1, X(o.s1) - X(o.s0))} y={plotB + 3} height={4} fill={CAM_COLORS[o.k % CAM_COLORS.length]} opacity={0.7} />
        ))}
        {lines.map((pts, i) => (
          <polyline key={i} points={pts} fill="none" stroke="#cbd5e1" strokeWidth={1} />
        ))}
        {vis.map((v, j) =>
          (v === "exc" || v === "ng") && !Number.isNaN(values[j]) ? (
            <circle key={j} cx={X(j * layout.spacing)} cy={Y(values[j])} r={2.2} fill={v === "ng" ? "#ef4444" : "#f59e0b"} />
          ) : v === "gap" ? (
            <line key={j} x1={X(j * layout.spacing)} x2={X(j * layout.spacing)} y1={PAD.t} y2={plotB} stroke="#ef4444" strokeWidth={2} />
          ) : null,
        )}
      </svg>
    </div>
  );
}
