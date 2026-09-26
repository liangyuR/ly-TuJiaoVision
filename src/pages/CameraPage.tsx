import { useEffect, useState } from "react";
import { cameraApi, CameraConfigPanel, DryRunPanel, FeasibilityCalc, FramePreview, useCameraStatus, type CameraConfig } from "../features/camera";
import { SimControls } from "../features/cycle";

export default function CameraPage() {
  const { status, lastFrame } = useCameraStatus();
  const [config, setConfig] = useState<CameraConfig | null>(null);
  useEffect(() => {
    cameraApi.getConfig().then(setConfig);
  }, []);

  const mvs = status?.source === "mvs";
  const frameMs = status?.maxFps ? 1000 / status.maxFps : null;

  return (
    <div className="cam-page">
      <div className="dev-bar">
        <span className={`badge ${status?.ready ? "link-connected" : "link-error"}`}>{status?.ready ? "就绪" : "未就绪"}</span>
        <span className="chip-static">{mvs ? "海康 MVS" : "模拟相机"}</span>
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
        {mvs && <span className={`chip-static${status?.lostPackets ? " c-warn" : ""}`}>丢包 {status?.lostPackets ?? 0}</span>}
        <span className="muted" style={{ fontSize: 12.5 }}>{status?.message}</span>
      </div>
      <div className="col">
        <CameraConfigPanel status={status} onSaved={setConfig} />
      </div>
      <div className="col">
        <FeasibilityCalc exposure={config?.exposureUs} fps={status?.maxFps} />
        <DryRunPanel frameMs={frameMs} />
        <FramePreview status={status} lastFrame={lastFrame} config={config} />
        <div className="panel">
          <h3 className="panel-title" style={{ marginBottom: 0 }}>模拟节拍</h3>
          <p className="muted">
            软件代替 PLC 与机器人跑完整节拍。模拟相机下直接产生帧；海康相机需把触发源设为 Software，由软件发软触发。
          </p>
          <SimControls />
        </div>
      </div>
    </div>
  );
}
