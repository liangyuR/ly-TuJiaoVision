import { useState } from "react";
import { ImageOff, Play, Square } from "lucide-react";

const metrics = [
  { label: "胶宽 (mm)", value: "--" },
  { label: "断胶数", value: "--" },
  { label: "溢胶数", value: "--" },
  { label: "耗时 (ms)", value: "--" },
];

const stats = [
  { label: "总数", value: "0", tone: "" },
  { label: "OK", value: "0", tone: "ok" },
  { label: "NG", value: "0", tone: "ng" },
  { label: "良率", value: "--", tone: "" },
];

export default function InspectPage() {
  const [running, setRunning] = useState(false);

  return (
    <div className="inspect">
      <div className="panel viewer">
        <div className="panel-toolbar">
          <span className="muted">相机 · 未选择</span>
          <button className={`btn ${running ? "danger" : "primary"}`} onClick={() => setRunning((v) => !v)}>
            {running ? <Square size={16} /> : <Play size={16} />}
            {running ? "停止" : "开始检测"}
          </button>
        </div>
        <div className="viewer-canvas">
          <div className="empty">
            <ImageOff size={40} />
            <p>暂无图像</p>
          </div>
        </div>
      </div>

      <div className="inspect-side">
        <div className="panel verdict idle">
          <span className="verdict-label">判定结果</span>
          <strong>--</strong>
        </div>
        <div className="panel">
          <h3 className="panel-title">检测指标</h3>
          <div className="metrics">
            {metrics.map((m) => (
              <div key={m.label} className="metric">
                <span className="muted">{m.label}</span>
                <strong>{m.value}</strong>
              </div>
            ))}
          </div>
        </div>
        <div className="panel grow">
          <h3 className="panel-title">统计</h3>
          <div className="metrics">
            {stats.map((s) => (
              <div key={s.label} className="metric">
                <span className="muted">{s.label}</span>
                <strong className={s.tone}>{s.value}</strong>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
