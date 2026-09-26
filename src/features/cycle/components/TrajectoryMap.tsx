import { useMemo } from "react";
import type { PointVis, Recipe } from "../types";
import { polyline, runs, visColor } from "../vis";

interface Props {
  layout: Recipe;
  vis: PointVis[];
  current?: number;
  /** 放大到某个拍照点的视野 */
  focus?: number | null;
  className?: string;
  compact?: boolean;
}

export default function TrajectoryMap({ layout, vis, current = -1, focus = null, className, compact = false }: Props) {
  const [w, h, r] = layout.part;
  const [fw, fh] = layout.fov;

  const viewBox = useMemo(() => {
    if (focus !== null && focus >= 0) {
      const [cx, cy] = layout.shots[focus];
      return `${cx - fw / 2} ${cy - fh / 2} ${fw} ${fh}`;
    }
    const xs = layout.shots.flatMap(([x]) => [x - fw / 2, x + fw / 2]).concat([-30, w + 30]);
    const ys = layout.shots.flatMap(([, y]) => [y - fh / 2, y + fh / 2]).concat([-30, h + 30]);
    const x0 = Math.min(...xs) - 8, y0 = Math.min(...ys) - 8;
    return `${x0} ${y0} ${Math.max(...xs) + 8 - x0} ${Math.max(...ys) + 8 - y0}`;
  }, [layout, focus, w, h, fw, fh]);

  const segs = useMemo(() => {
    const rs = runs(vis);
    return rs.map((run, i) => ({
      ...run,
      points: polyline(layout, run.from, run.to, i > 0 && rs[i - 1].state !== "gap", i === rs.length - 1 && rs[0].state === run.state),
    }));
  }, [layout, vis]);

  const gaps = segs.filter((s) => s.state === "gap");
  const sw = compact ? 1.6 : 2.6;

  return (
    <svg className={className} viewBox={viewBox} preserveAspectRatio="xMidYMid meet">
      {!compact && <rect x={-28} y={-28} width={w + 56} height={h + 56} rx={r + 8} fill="#18233a" stroke="#2a3a5a" vectorEffect="non-scaling-stroke" />}
      <rect x={4} y={4} width={w - 8} height={h - 8} rx={Math.max(r - 4, 2)} fill="#0b1120" stroke="#475569" vectorEffect="non-scaling-stroke" />
      {!compact &&
        [[-12, -12], [w + 12, -12], [w + 12, h + 12], [-12, h + 12]].map(([cx, cy]) => (
          <circle key={`${cx},${cy}`} cx={cx} cy={cy} r={5} fill="#0b1120" stroke="#64748b" vectorEffect="non-scaling-stroke" />
        ))}
      {!compact &&
        layout.shots.map(([cx, cy], k) => {
          const on = k === current;
          return (
            <g key={k}>
              <rect
                x={cx - fw / 2}
                y={cy - fh / 2}
                width={fw}
                height={fh}
                fill={on ? "rgba(56,189,248,0.06)" : "none"}
                stroke={on ? "#38bdf8" : "#3a4a68"}
                strokeWidth={on ? 1.6 : 1}
                strokeDasharray={on ? undefined : "5 4"}
                vectorEffect="non-scaling-stroke"
              />
              <text x={cx - fw / 2 + 5} y={cy - fh / 2 + 13} fontSize={11} fill={on ? "#38bdf8" : "#64748b"} className="mono">
                k{k}
              </text>
            </g>
          );
        })}
      {segs.map(
        (s) =>
          s.state !== "gap" && (
            <polyline
              key={s.from}
              points={s.points}
              fill="none"
              stroke={visColor[s.state]}
              strokeWidth={sw}
              strokeLinecap="round"
              strokeDasharray={s.state === "none" ? "3 3" : s.state === "inv" || s.state === "miss" ? "4 3" : undefined}
              vectorEffect="non-scaling-stroke"
            />
          ),
      )}
      {!compact &&
        gaps.map((g) => {
          const j = Math.round((g.from + g.to) / 2);
          const x = layout.points.x[j], y = layout.points.y[j];
          return (
            <g key={`gap${g.from}`}>
              <circle cx={x} cy={y} r={9} fill="none" stroke="#ef4444" strokeWidth={1.8} vectorEffect="non-scaling-stroke" />
              <text x={x + 12} y={y + 22} fontSize={11} fill="#fca5a5">
                断胶 {((g.to - g.from + 1) * layout.spacing).toFixed(1)} mm
              </text>
            </g>
          );
        })}
    </svg>
  );
}
