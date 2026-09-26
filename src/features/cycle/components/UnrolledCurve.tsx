import { useEffect, useMemo, useRef, useState } from "react";
import type { Measured, PointVis, Recipe } from "../types";

const OWNER_COLORS = ["#38bdf8", "#f472b6", "#facc15", "#34d399", "#fb923c", "#818cf8", "#2dd4bf", "#e879f9"];
const PAD = { l: 34, r: 8, t: 8, b: 30 };
const D_MAX = 2.3;

export default function UnrolledCurve({ layout, measured, vis }: { layout: Recipe; measured: Measured[]; vis: PointVis[] }) {
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
  const total = n * layout.spacing;
  const { w, h } = size;
  const plotB = h - PAD.b;
  const X = (s: number) => PAD.l + (s / total) * (w - PAD.l - PAD.r);
  const Y = (d: number) => plotB - (Math.max(-0.1, Math.min(D_MAX, d)) / D_MAX) * (plotB - PAD.t);

  const d = useMemo(() => {
    const arr = new Float32Array(n).fill(NaN);
    measured.forEach((m) => m.idx.forEach((j, i) => m.st[i] === 0 && (arr[j] = m.d[i])));
    return arr;
  }, [measured, n]);

  const lines = useMemo(() => {
    const out: string[] = [];
    let cur: string[] = [];
    for (let j = 0; j < n; j++) {
      if (Number.isNaN(d[j])) {
        if (cur.length > 1) out.push(cur.join(" "));
        cur = [];
      } else cur.push(`${X(j * layout.spacing).toFixed(1)},${Y(d[j]).toFixed(1)}`);
    }
    if (cur.length > 1) out.push(cur.join(" "));
    return out;
  }, [d, w, h, n, total, layout.spacing]);

  const owners = useMemo(() => {
    const out: { k: number; s0: number; s1: number }[] = [];
    layout.points.k.forEach((k, j) => {
      const last = out[out.length - 1];
      if (last && last.k === k) last.s1 = (j + 1) * layout.spacing;
      else out.push({ k, s0: j * layout.spacing, s1: (j + 1) * layout.spacing });
    });
    return out;
  }, [layout]);

  return (
    <div ref={ref} className="curve-box">
      <svg width={w} height={h}>
        {[0, 0.75, 1.5, 2.2].map((v) => (
          <g key={v}>
            <line x1={PAD.l} x2={w - PAD.r} y1={Y(v)} y2={Y(v)} stroke="#1c2944" />
            <text x={PAD.l - 5} y={Y(v) + 3} textAnchor="end" fontSize={10} fill="#6b7b97">
              {v}
            </text>
          </g>
        ))}
        {layout.segments.map((g) => (
          <g key={g.name}>
            <rect
              x={X(g.s0)}
              width={X(g.s1) - X(g.s0)}
              y={Y(g.params.nominal + g.params.tolUpper)}
              height={Y(Math.max(g.params.nominal - g.params.tolLower, 0)) - Y(g.params.nominal + g.params.tolUpper)}
              fill="rgba(34,197,94,0.09)"
            />
            <line x1={X(g.s0)} x2={X(g.s1)} y1={Y(g.params.absMax)} y2={Y(g.params.absMax)} stroke="rgba(239,68,68,0.7)" strokeDasharray="4 3" />
            <line x1={X(g.s0)} x2={X(g.s0)} y1={PAD.t} y2={plotB + 8} stroke="#2a3a5a" />
            <text x={(X(g.s0) + X(g.s1)) / 2} y={h - 6} textAnchor="middle" fontSize={10.5} fill="#8391ab">
              {g.name}
            </text>
          </g>
        ))}
        {owners.map((o) => (
          <rect key={o.s0} x={X(o.s0)} width={X(o.s1) - X(o.s0)} y={plotB + 3} height={4} fill={OWNER_COLORS[o.k % OWNER_COLORS.length]} opacity={0.7} />
        ))}
        {lines.map((pts, i) => (
          <polyline key={i} points={pts} fill="none" stroke="#cbd5e1" strokeWidth={1} />
        ))}
        {vis.map((v, j) =>
          v === "exc" || v === "ng" ? (
            <circle key={j} cx={X(j * layout.spacing)} cy={Y(d[j])} r={2.2} fill={v === "ng" ? "#ef4444" : "#f59e0b"} />
          ) : v === "gap" ? (
            <line key={j} x1={X(j * layout.spacing)} x2={X(j * layout.spacing)} y1={PAD.t} y2={plotB} stroke="#ef4444" strokeWidth={2} />
          ) : null,
        )}
      </svg>
    </div>
  );
}
