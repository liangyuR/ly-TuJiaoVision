import type { FrameView, PartView, PointVis, Recipe } from "../types";
import TrajectoryMap from "./TrajectoryMap";

const labels: Record<FrameView["status"], [string, string]> = {
  waiting: ["等待", "c-mut"],
  measuring: ["测量", "c-acc"],
  done: ["完成", "c-ok"],
  locateFailed: ["定位失败", "c-err"],
  missing: ["未收到", "c-err"],
};

function footer(f: FrameView) {
  switch (f.status) {
    case "waiting":
      return "—";
    case "measuring":
      return `到达 ${f.arrivedMs} ms · 测量中`;
    case "done":
      return `${f.score?.toFixed(2)} · ${f.points} 点 · ${f.ms} ms`;
    case "locateFailed":
      return `分数 ${f.score?.toFixed(2)} < 0.60`;
    case "missing":
      return f.counterJump ? "帧计数跳号" : "未到达";
  }
}

export default function ShotStrip({ layout, part, vis }: { layout: Recipe; part: PartView | null; vis: PointVis[] }) {
  const frames = part && part.recipeId === layout.id ? part.frames : null;
  return (
    <div className="shot-strip" style={{ gridTemplateColumns: `repeat(${layout.shots.length}, minmax(0, 1fr))` }}>
      {layout.shots.map((_, k) => {
        const f = frames?.[k];
        const [label, tone] = labels[f?.status ?? "waiting"];
        const bad = f && (f.status === "locateFailed" || f.status === "missing");
        return (
          <div key={k} className={`shot s-${f?.status ?? "waiting"}${f?.gapPoints ? " has-gap" : ""}`}>
            <div className="shot-head">
              <b className="mono">k={k}</b>
              <span className={f?.gapPoints ? "c-ng" : tone}>{f?.gapPoints ? `缺胶 ${f.gapPoints}` : label}</span>
            </div>
            <TrajectoryMap layout={layout} vis={vis} focus={k} compact className="shot-img" />
            <div className={`shot-foot mono${bad ? " c-err" : ""}`}>{f ? footer(f) : "—"}</div>
          </div>
        );
      })}
    </div>
  );
}
