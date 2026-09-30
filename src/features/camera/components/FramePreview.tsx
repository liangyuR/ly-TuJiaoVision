import { useState } from "react";
import { Zap } from "lucide-react";
import { cameraApi, usePreviewCanvas } from "../api";
import type { CameraConfig, CameraStatus, Frame } from "../types";

export default function FramePreview({ cam, status, lastFrame, config }: { cam: number; status: CameraStatus | null; lastFrame: Frame | undefined; config: CameraConfig | null }) {
  const { img, canvas } = usePreviewCanvas(cam, lastFrame?.frameCounter);
  const [error, setError] = useState("");

  const soft = () => {
    setError("");
    cameraApi.softTrigger(cam).catch((e) => setError(String(e)));
  };
  const canTrigger = config?.source === "replay" || (config?.acquisition === "triggered" && config.source === "mvs" && config.triggerSource === "Software");

  return (
    <div className="panel">
      <div className="panel-head">
        <h3 className="panel-title">最近一帧</h3>
        {lastFrame && (
          <span className="muted mono">
            帧 {lastFrame.frameCounter} · 触发 {lastFrame.triggerCounter}
            {lastFrame.lostPackets ? ` · 丢包 ${lastFrame.lostPackets}` : ""}
            {img && ` · ${img.fullWidth}×${img.fullHeight}`}
          </span>
        )}
        <span className="spacer" />
        <button className="btn" onClick={soft} disabled={!canTrigger} title="回放相机，或触发源为 Software 的触发采集海康相机">
          <Zap size={15} />
          {config?.source === "replay" ? "下一张" : "软触发一次"}
        </button>
      </div>
      <div className="preview-box">
        <canvas ref={canvas} style={{ display: img ? "block" : "none" }} />
        {!img && <span className="muted">{status?.ready ? "暂无图像（仅显示 Mono8）" : status?.message}</span>}
      </div>
      {error && <span className="c-ng" style={{ fontSize: 12 }}>{error}</span>}
    </div>
  );
}
