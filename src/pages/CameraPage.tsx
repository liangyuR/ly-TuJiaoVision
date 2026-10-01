import { useEffect, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import {
  CalibPanel,
  cameraApi,
  CameraConfigPanel,
  defaultCameraConfig,
  DryRunPanel,
  FeasibilityCalc,
  FollowCalibPanel,
  FramePreview,
  useRigStatus,
  type CameraConfig,
} from "../features/camera";
import { SimControls } from "../features/cycle";

const sourceText = { mvs: "海康 MVS", sim: "模拟相机", replay: "回放目录" } as const;

export default function CameraPage() {
  const { statuses, lastFrame } = useRigStatus();
  const [configs, setConfigs] = useState<CameraConfig[]>([]);
  const [cam, setCam] = useState(0);
  const [error, setError] = useState("");

  const reload = () => cameraApi.rigConfig().then(setConfigs);
  useEffect(() => {
    reload();
  }, []);

  // 序列号留空的相机连上后后台固定了序列号：重新取，别拿旧的空值保存回去
  const unpinned = statuses.some((s) => s.device && configs[s.cam]?.source === "mvs" && !configs[s.cam]?.serial);
  useEffect(() => {
    if (unpinned) reload();
  }, [unpinned]);

  const config = configs[cam] ?? null;
  const status = statuses.find((s) => s.cam === cam) ?? null;
  const frameMs = status?.maxFps ? 1000 / status.maxFps : null;
  const saved = (c: CameraConfig) => setConfigs((prev) => prev.map((p, i) => (i === cam ? c : p)));

  const add = async () => {
    setError("");
    try {
      const base = configs[configs.length - 1] ?? defaultCameraConfig;
      let n = configs.length + 1;
      while (configs.some((c) => c.name === `相机 ${n}`)) n++;
      const i = await cameraApi.add({ ...base, name: `相机 ${n}`, serial: "", follow: null });
      await reload();
      setCam(i);
    } catch (e) {
      setError(String(e));
    }
  };
  const remove = async () => {
    if (!config || !window.confirm(`从相机组里移除「${config.name}」（${config.id}）？用到它的配方要改用别的相机才能开工，这个编号以后也不会再分给别的相机。`)) return;
    setError("");
    try {
      await cameraApi.remove(cam);
      await reload();
      setCam(Math.max(0, cam - 1));
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="cam-page">
      <div className="cam-tabs">
        {configs.map((c, i) => {
          const st = statuses.find((s) => s.cam === i);
          return (
            <button key={c.id} className={`tab${i === cam ? " active" : ""}`} onClick={() => setCam(i)}>
              <i className={st?.ready ? "ok" : ""} />
              {c.name || `相机 ${i + 1}`}
              <span className="muted mono">{c.id}</span>
            </button>
          );
        })}
        <button className="btn" onClick={add} title="相机组最多 8 台">
          <Plus size={15} />
          添加相机
        </button>
        {configs.length > 1 && (
          <button className="btn" onClick={remove}>
            <Trash2 size={15} />
            移除当前
          </button>
        )}
        {error && <span className="c-ng" style={{ fontSize: 12.5 }}>{error}</span>}
      </div>
      <div className="dev-bar">
        <span className={`badge ${status?.ready ? "link-connected" : "link-error"}`}>{status?.ready ? "就绪" : "未就绪"}</span>
        {status && <span className="chip-static">{sourceText[status.source]}</span>}
        {status && <span className="chip-static">{status.acquisition === "freeRun" ? "连续采集" : "触发采集"}</span>}
        {status?.device && (
          <>
            <span className="chip-static">{status.device.model}</span>
            <span className="chip-static mono">{status.device.serial}</span>
            {status.device.ip && <span className="chip-static mono">{status.device.ip}</span>}
          </>
        )}
        <span className="chip-static">
          帧 {status?.frames ?? 0} · {status?.fps ? `${status.fps.toFixed(1)} fps` : "—"}
          {status?.maxFps ? ` · 相机上限 ${status.maxFps.toFixed(1)} fps` : ""}
        </span>
        {status?.source === "mvs" && <span className={`chip-static${status.lostPackets ? " c-warn" : ""}`}>丢包 {status.lostPackets}</span>}
        {status?.droppedFrames ? <span className="chip-static c-warn">节拍丢帧 {status.droppedFrames}</span> : null}
        <span className="muted" style={{ fontSize: 12.5 }}>{status?.message}</span>
      </div>
      {/* 面板按相机编号挂载：移除前面的相机后序号会变，按序号挂载会留着被移除相机的设置 */}
      <div className="col">
        {config && <CameraConfigPanel key={config.id} cam={cam} initial={config} follow={config.follow} status={status} onSaved={saved} />}
      </div>
      <div className="col">
        <FramePreview key={config?.id} cam={cam} status={status} lastFrame={lastFrame[cam]} config={config} />
        {config?.acquisition === "freeRun" && <FollowCalibPanel key={`f${config.id}`} cam={cam} config={config} frame={lastFrame[cam]} onSaved={saved} />}
        {config?.acquisition === "triggered" && (
          <>
            <FeasibilityCalc exposure={config.exposureUs} fps={status?.maxFps} />
            <DryRunPanel cam={cam} frameMs={frameMs} />
            <CalibPanel key={config.id} cam={cam} isSim={config.source === "sim"} />
          </>
        )}
        <div className="panel">
          <h3 className="panel-title" style={{ marginBottom: 0 }}>模拟节拍</h3>
          <p className="muted">
            软件代替 PLC 与机器人跑完整节拍。模拟相机直接产生帧；回放相机按节拍出图；海康相机触发采集时需把触发源设为 Software。
          </p>
          <SimControls />
        </div>
      </div>
    </div>
  );
}
