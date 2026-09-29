import { useEffect, useMemo, useState } from "react";
import { RotateCcw } from "lucide-react";
import Modal from "../features/plc/components/Modal";
import { cameraApi, useRigStatus, type CameraConfig } from "../features/camera";
import {
  CameraTile,
  computeVis,
  currentFrame,
  cycleApi,
  CycleStepper,
  ShotStrip,
  SignalLamps,
  SimControls,
  TrajectoryMap,
  UnrolledCurve,
  useCycle,
  useLayout,
  useRecipes,
  type LogLine,
  type Measured,
  type Recipe,
  type ResultView,
  type Snapshot,
  type Verdict,
} from "../features/cycle";

const verdictText: Record<Verdict, string> = {
  OK: "OK",
  OK_WITH_EXCURSION: "OK · 局部超差",
  NG_POSITION: "NG · 位置超差",
  NG_WIDTH: "NG · 胶宽超差",
  NG_ABSOLUTE: "NG · 超绝对限",
  NG_GAP: "NG · 断胶",
  ERR_INSPECT: "ERR · 未测成",
};

function verdictTone(v: Verdict) {
  return v.startsWith("OK") ? "v-ok" : v === "ERR_INSPECT" ? "v-err" : "v-ng";
}

function clock(ts: number) {
  const d = new Date(ts);
  return `${d.toLocaleTimeString("zh-CN", { hour12: false })}.${String(d.getMilliseconds()).padStart(3, "0")}`;
}

export default function InspectPage() {
  const { snapshot, logs, measured } = useCycle();
  const recipes = useRecipes();
  const { statuses, lastFrame } = useRigStatus();
  const [configs, setConfigs] = useState<CameraConfig[]>([]);
  const [view, setView] = useState<"part" | "frame">("part");
  const [pendingRecipe, setPendingRecipe] = useState<string | null>(null);
  const [switchError, setSwitchError] = useState("");

  const part = snapshot?.part ?? null;
  const layoutId = part?.recipeId ?? snapshot?.activeRecipeId ?? recipes[0]?.id;
  const summary = recipes.find((r) => r.id === layoutId);
  const layout = useLayout(layoutId, summary?.version);
  const phase = snapshot?.phase ?? "IDLE";
  const settled = phase === "REPORT" || phase === "RELEASE" || phase === "IDLE" || phase === "FAULT";
  const result = settled ? (snapshot?.result ?? null) : null;
  const partResult = result && part && result.sn === part.sn ? result : null;
  const follow = layout?.mode === "follow";

  useEffect(() => {
    cameraApi.rigConfig().then(setConfigs).catch(() => setConfigs([]));
  }, [statuses.length]);

  const vis = useMemo(() => (layout ? computeVis(layout, part, measured, partResult) : []), [layout, part, measured, partResult]);
  const cur = !follow && (phase === "ACQUIRE" || phase === "DRAIN") ? currentFrame(part) : -1;
  const focus = !follow && view === "frame" ? currentFrame(part) : null;
  const trigger = snapshot?.triggerMode ?? layout?.triggerMode;
  const cams = layout ? (follow ? (layout.follow?.cameras ?? []) : [layout.camera]) : [];
  const lastByCam = useMemo(() => {
    const out: Record<number, Measured> = {};
    measured.forEach((m) => (out[m.cam ?? 0] = m));
    return out;
  }, [measured]);
  const nozzle = follow && part?.nozzleS != null && part.recipeId === layout?.id ? { s: part.nozzleS, cam: part.activeCam } : null;

  const confirmSwitch = async () => {
    if (!pendingRecipe) return;
    try {
      await cycleApi.selectRecipe(pendingRecipe);
      setPendingRecipe(null);
      setSwitchError("");
    } catch (e) {
      setSwitchError(String(e));
    }
  };

  return (
    <div className="fly">
      <div className="fly-bar">
        <span className="fly-chip">{follow ? "三目随动" : trigger === "stop" ? "停稳拍" : "飞拍"}</span>
        {snapshot?.productSource === "manual" ? (
          <span className="fly-chip">
            配方
            <select
              id="manual-recipe"
              className="input"
              value={snapshot.activeRecipeId ?? ""}
              onChange={(e) => setPendingRecipe(e.target.value)}
              disabled={!(phase === "IDLE" || phase === "FAULT")}
            >
              {!snapshot.activeRecipeId && <option value="">请选择</option>}
              {recipes.map((r) => (
                <option key={r.id} value={r.id}>
                  {r.id} · {r.name}
                </option>
              ))}
            </select>
          </span>
        ) : (
          <span className="fly-chip" title="型号由 PLC 下发的产品代码匹配">
            {layout ? `${layout.id} · v${layout.version}` : "等待 PLC 下发型号"}
            {layout && <span className="muted mono">#{layout.hash.slice(0, 6)}</span>}
          </span>
        )}
        <span className="fly-chip">
          SN <b className="mono">{part?.sn ?? result?.sn ?? "—"}</b>
        </span>
        <span className="spacer" />
        <CycleStepper snapshot={snapshot} />
      </div>

      <div className="fly-bar">
        <SignalLamps />
        <span className="spacer" />
        <SimControls compact />
        {phase === "FAULT" && (
          <button className="btn" onClick={() => cycleApi.reset()}>
            <RotateCcw size={15} />
            复位故障
          </button>
        )}
      </div>

      {snapshot?.alarms.map((a) => (
        <div key={a} className="alarm-bar">
          {a}
        </div>
      ))}

      <div className="fly-body">
        <div className={`fly-main${follow ? " follow" : ""}`}>
          <div className={`insp-top${cams.length > 1 ? " grid" : ""}`}>
            <div className="panel">
              <div className="panel-head">
                <h3 className="panel-title">主视图</h3>
                <div className="legend">
                  <span><i style={{ background: "#22c55e" }} />合格</span>
                  <span><i style={{ background: "#f59e0b" }} />超公差（允许内）</span>
                  <span><i style={{ background: "#ef4444" }} />NG / 断胶</span>
                  <span><i style={{ background: "#a78bfa" }} />未测成</span>
                  <span><i style={{ background: "#3b4a66" }} />待测</span>
                </div>
                <span className="spacer" />
                {follow ? (
                  <span className="muted mono">
                    胶嘴 s={part?.nozzleS != null ? part.nozzleS.toFixed(1) : "—"} / {part?.endS != null ? part.endS.toFixed(0) : layout?.segments.at(-1)?.s1.toFixed(0) ?? "—"} mm
                  </span>
                ) : (
                  <div className="segmented">
                    <button className={view === "part" ? "active" : ""} onClick={() => setView("part")}>整件</button>
                    <button className={view === "frame" ? "active" : ""} onClick={() => setView("frame")}>当前帧</button>
                  </div>
                )}
              </div>
              {layout ? <TrajectoryMap layout={layout} vis={vis} current={cur} focus={focus} nozzle={nozzle} className="traj" /> : <div className="empty">等待配方</div>}
            </div>
            <div className={`cam-tiles n${cams.length}`}>
              {cams.map((c) => {
                const st = statuses.find((s) => s.cam === c);
                return st ? (
                  <CameraTile
                    key={c}
                    status={st}
                    calib={follow ? (configs[c]?.follow ?? null) : null}
                    last={lastByCam[c] ?? null}
                    active={follow ? part?.activeCam === c && phase === "ACQUIRE" : cur >= 0}
                    frame={lastFrame[c]}
                  />
                ) : (
                  <div key={c} className="cam-tile down">
                    <div className="cam-tile-head">
                      <b>相机 {c + 1}</b>
                    </div>
                    <div className="empty small">不在相机组里</div>
                  </div>
                );
              })}
            </div>
          </div>

          {!follow && (
            <div className="panel">
              <div className="panel-head">
                <h3 className="panel-title">拍照点</h3>
                <span className="muted">k 来源：{trigger === "stop" ? "停稳点触发计数" : "位置触发计数"}</span>
                <span className="spacer" />
                <span className="muted mono">
                  Chunk 触发 {part?.triggers ?? 0} · 收到 {part?.received ?? 0}
                </span>
              </div>
              {layout && <ShotStrip layout={layout} part={part} vis={vis} />}
            </div>
          )}

          <div className="panel">
            <div className="panel-head">
              <h3 className="panel-title">展开曲线</h3>
              <span className="muted">
                {follow ? "胶条横向偏移（mm）" : "距内边距离 d（mm）"}，横轴弧长；绿色带为公差，红虚线为绝对限，底部色条为{follow ? "测到该点的相机" : "各帧负责区间"}
              </span>
            </div>
            {layout && <UnrolledCurve layout={layout} measured={measured} vis={vis} />}
          </div>
          {follow && (
            <div className="panel">
              <div className="panel-head">
                <h3 className="panel-title">胶宽</h3>
                <span className="muted">mm，按段的胶宽限值判定</span>
              </div>
              {layout && <UnrolledCurve layout={layout} measured={measured} vis={vis} quantity="w" />}
            </div>
          )}
        </div>

        <div className="fly-side">
          <VerdictCard phase={phase} result={result} sn={part?.sn} />
          <Progress snapshot={snapshot} result={partResult} follow={follow} />
          <SegmentPanel layout={layout} measured={measured} result={partResult} />
          <div className="panel">
            <h3 className="panel-title" style={{ marginBottom: 0 }}>事件</h3>
            <EventLog logs={logs} />
          </div>
        </div>
      </div>

      {pendingRecipe && (
        <Modal
          title="切换配方"
          onClose={() => setPendingRecipe(null)}
          footer={
            <>
              {switchError && <span className="form-error">{switchError}</span>}
              <button className="btn" onClick={() => setPendingRecipe(null)}>取消</button>
              <button className="btn primary" onClick={confirmSwitch}>确认切换</button>
            </>
          }
        >
          <p>
            切换到 <b>{pendingRecipe}</b>，从下一个工件开始生效。请确认现场上料的型号与之一致。
          </p>
        </Modal>
      )}
    </div>
  );
}

function VerdictCard({ phase, result, sn }: { phase: string; result: ResultView | null; sn?: number }) {
  if (!result) {
    const idle = phase === "IDLE" || phase === "FAULT";
    return (
      <div className="panel fly-verdict v-run">
        <div className="vl"><span>判定结果</span><span className="mono">PLC —</span></div>
        <strong>{idle ? "等待工件" : "检测中…"}</strong>
        <div className="vr">{idle ? "" : `SN ${sn ?? "—"}`}</div>
      </div>
    );
  }
  return (
    <div className={`panel fly-verdict ${verdictTone(result.verdict)}`}>
      <div className="vl">
        <span>判定结果 · SN {result.sn}</span>
        <span className="mono">PLC {result.plcCode}{result.faultCode ? ` / ${result.faultCode}` : ""}</span>
      </div>
      <strong>{verdictText[result.verdict]}</strong>
      <div className="vr">{result.reason}</div>
    </div>
  );
}

function Progress({ snapshot, result, follow }: { snapshot: Snapshot | null; result: ResultView | null; follow: boolean }) {
  const part = snapshot?.part;
  const pct = part && part.total ? (part.filled / part.total) * 100 : 0;
  const travel = follow && part?.nozzleS != null && part.endS ? Math.min(100, Math.max(0, (part.nozzleS / part.endS) * 100)) : null;
  return (
    <div className="panel">
      <h3 className="panel-title" style={{ marginBottom: 0 }}>本件进度</h3>
      <div className="kv2">
        {follow ? (
          <>
            <span>胶嘴行程</span>
            <b>{travel != null ? `${travel.toFixed(0)}%` : "—"}</b>
            <div className="bar"><i style={{ width: `${travel ?? 0}%` }} /></div>
            <span>收到帧 / 测量帧</span>
            <b>{part ? `${part.received} / ${part.frames.length}` : "—"}</b>
          </>
        ) : (
          <>
            <span>帧</span>
            <b>{part ? `${part.received} / ${part.n}` : "—"}</b>
          </>
        )}
        <span>测量点</span>
        <b>{part ? `${part.filled} / ${part.total}` : "—"}</b>
        <div className="bar"><i style={{ width: `${pct}%` }} /></div>
        <span>处理队列</span>
        <b>{part?.queue ?? 0}</b>
        <span>收尾耗时（partEnd→done）</span>
        <b>{result?.drainMs != null ? `${result.drainMs} ms` : "—"}</b>
        <span>游离帧</span>
        <b className={snapshot?.strayFrames ? "c-warn" : ""}>{snapshot?.strayFrames ?? 0}</b>
      </div>
      <Stats snapshot={snapshot} />
    </div>
  );
}

function SegmentPanel({ layout, measured, result }: { layout: Recipe | null; measured: Measured[]; result: ResultView | null }) {
  const ranges = useMemo(() => {
    if (!layout) return [];
    const r = layout.segments.map(() => ({ min: Infinity, max: -Infinity, wmin: Infinity, wmax: -Infinity }));
    measured.forEach((m) =>
      m.idx.forEach((j, i) => {
        if (m.st[i] !== 0) return;
        const g = r[layout.points.seg[j]];
        g.min = Math.min(g.min, m.d[i]);
        g.max = Math.max(g.max, m.d[i]);
        const w = m.w?.[i];
        if (w != null) {
          g.wmin = Math.min(g.wmin, w);
          g.wmax = Math.max(g.wmax, w);
        }
      }),
    );
    return r;
  }, [layout, measured]);

  const errored = result?.verdict === "ERR_INSPECT";
  const width = layout?.segments.some((g) => g.width);
  return (
    <div className="panel">
      <div className="panel-head">
        <h3 className="panel-title">分段结果</h3>
        <span className="spacer" />
        <span className="muted" style={{ fontSize: 11 }}>{width ? "偏移 · 胶宽" : "d 最小–最大"}</span>
      </div>
      <div className="seg-list">
        {layout?.segments.map((g, i) => {
          const s = result?.segments[i];
          const tone = s ? (s.verdict === "OK" ? "c-ok" : s.verdict === "OK_WITH_EXCURSION" ? "c-warn" : "c-ng") : errored ? "c-err" : "c-mut";
          const rg = ranges[i];
          const d = rg && rg.min <= rg.max ? `${rg.min.toFixed(2)}–${rg.max.toFixed(2)}` : "—";
          const w = rg && rg.wmin <= rg.wmax ? ` · ${rg.wmin.toFixed(2)}–${rg.wmax.toFixed(2)}` : "";
          return (
            <div key={g.name}>
              <span className={`dot ${tone}`} style={{ background: "currentColor" }} />
              <span>{g.name}</span>
              <span className="rng">{d}{w}</span>
              <span className={`ss ${tone}`}>
                {s ? (s.verdict === "OK_WITH_EXCURSION" ? `局部超差 ${Math.max(s.excursionLen, s.wExcursionLen ?? 0).toFixed(1)}` : s.verdict) : errored ? "不可判" : "—"}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function EventLog({ logs }: { logs: LogLine[] }) {
  return (
    <div className="event-log">
      {logs
        .slice(-80)
        .reverse()
        .map((l, i) => (
          <div key={`${l.ts}-${i}`} className={`l-${l.level}`}>
            <span className="tm">{clock(l.ts)}</span>
            <span className="ev">{l.ev}</span>
            {l.msg}
          </div>
        ))}
    </div>
  );
}

function Stats({ snapshot }: { snapshot: Snapshot | null }) {
  const s = snapshot?.stats ?? { total: 0, ok: 0, ng: 0, err: 0 };
  const items: [string, string | number, string][] = [
    ["今日", s.total, ""],
    ["OK", s.ok, "c-ok"],
    ["NG", s.ng, "c-ng"],
    ["ERR", s.err, "c-err"],
    ["良率", s.total ? `${((s.ok / s.total) * 100).toFixed(1)}%` : "--", ""],
  ];
  return (
    <div className="stats5">
      {items.map(([label, value, tone]) => (
        <div key={label}>
          <span>{label}</span>
          <b className={tone}>{value}</b>
        </div>
      ))}
    </div>
  );
}
