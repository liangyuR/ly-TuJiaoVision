import { useEffect, useState } from "react";
import { Play, Square } from "lucide-react";
import { cameraApi } from "../api";
import type { DryFrame } from "../types";

const H = 130;

export default function DryRunPanel({ frameMs }: { frameMs: number | null }) {
  const [running, setRunning] = useState(false);
  const [frames, setFrames] = useState<DryFrame[]>([]);
  const [plan, setPlan] = useState(6);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!running) return;
    const timer = setInterval(() => cameraApi.dryRunGet().then((f) => f && setFrames(f)), 300);
    return () => clearInterval(timer);
  }, [running]);

  const start = async () => {
    setError("");
    try {
      await cameraApi.dryRunStart();
      setFrames([]);
      setRunning(true);
    } catch (e) {
      setError(String(e));
    }
  };
  const stop = async () => {
    setFrames(await cameraApi.dryRunStop());
    setRunning(false);
  };

  const intervals = frames.slice(1).map((f, i) => f.tMs - frames[i].tMs);
  const triggers = frames.length ? frames[frames.length - 1].triggerCounter - frames[0].triggerCounter + 1 : 0;
  const jumps = frames.slice(1).filter((f, i) => f.frameCounter !== frames[i].frameCounter + 1).length;
  const lost = frames.reduce((a, f) => a + f.lostPackets, 0);
  const minGap = intervals.length ? Math.min(...intervals) : null;
  const top = Math.max(700, ...intervals, frameMs ?? 0) * 1.1;
  const Y = (ms: number) => H - 16 - (ms / top) * (H - 24);
  const barW = intervals.length ? Math.min(46, 460 / intervals.length - 6) : 0;
  const ok = !running && frames.length > 0 && frames.length === plan && triggers === plan && jumps === 0;

  return (
    <div className="panel">
      <div className="panel-head">
        <h3 className="panel-title">触发空跑测试</h3>
        <span className="muted">机器人不带工件走一遍路径，只计数不检测</span>
        <span className="spacer" />
        {running ? (
          <button className="btn" onClick={stop}>
            <Square size={15} />
            结束
          </button>
        ) : (
          <button className="btn primary" onClick={start}>
            <Play size={15} />
            开始空跑
          </button>
        )}
      </div>
      <div className="dry-stats">
        <label>
          计划 N <input id="dry-plan" className="input mono" type="number" min={1} value={plan} onChange={(e) => setPlan(Number(e.target.value))} />
        </label>
        <span>收到帧 <b className={frames.length === plan ? "c-ok" : ""}>{frames.length}</b></span>
        <span>触发计数 <b className={triggers === plan ? "c-ok" : ""}>{triggers}</b></span>
        <span>帧计数跳号 <b className={jumps ? "c-ng" : ""}>{jumps}</b></span>
        <span>丢包 <b className={lost ? "c-warn" : ""}>{lost}</b></span>
        <span>最短间隔 <b>{minGap !== null ? `${minGap.toFixed(0)} ms` : "—"}</b></span>
        {!running && frames.length > 0 && <span className={ok ? "c-ok" : "c-ng"}>{ok ? "通过" : "不通过"}</span>}
      </div>
      <svg className="dry-chart" viewBox={`0 0 500 ${H}`} preserveAspectRatio="none">
        {intervals.map((ms, i) => {
          const x = 30 + i * (barW + 6);
          return (
            <g key={i}>
              <rect x={x} y={Y(ms)} width={barW} height={H - 16 - Y(ms)} fill={frameMs && ms < frameMs ? "rgba(239,68,68,0.7)" : "rgba(56,189,248,0.55)"} />
              {barW > 22 && (
                <text x={x + barW / 2} y={Y(ms) - 3} textAnchor="middle" fontSize={9} fill="#cbd5e1">
                  {ms.toFixed(0)}
                </text>
              )}
            </g>
          );
        })}
        {frameMs && (
          <>
            <line x1={24} x2={496} y1={Y(frameMs)} y2={Y(frameMs)} stroke="#f59e0b" strokeDasharray="5 4" />
            <text x={494} y={Y(frameMs) - 4} textAnchor="end" fontSize={9.5} fill="#fbbf24">
              单帧时间 {frameMs.toFixed(0)} ms
            </text>
          </>
        )}
        {!intervals.length && (
          <text x={250} y={H / 2} textAnchor="middle" fontSize={11} fill="#6b7b97">
            {running ? "等待触发…" : "相邻帧的时间间隔将显示在这里"}
          </text>
        )}
      </svg>
      {error && <span className="c-ng" style={{ fontSize: 12 }}>{error}</span>}
    </div>
  );
}
