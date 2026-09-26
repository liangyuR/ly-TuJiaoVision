import { useEffect, useRef, useState } from "react";
import { Zap } from "lucide-react";
import { cameraApi } from "../api";
import type { CameraConfig, CameraStatus, Frame } from "../types";

export default function FramePreview({ status, lastFrame, config }: { status: CameraStatus | null; lastFrame: Frame | null; config: CameraConfig | null }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState<[number, number] | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!lastFrame || status?.source !== "mvs") return;
    cameraApi.preview().then((buf) => {
      if (buf.byteLength < 8 || !canvas.current) return;
      const view = new DataView(buf);
      const w = view.getUint32(0, true), h = view.getUint32(4, true);
      const px = new Uint8Array(buf, 8);
      const el = canvas.current;
      el.width = w;
      el.height = h;
      const img = new ImageData(w, h);
      for (let i = 0; i < w * h; i++) {
        const g = px[i];
        img.data[i * 4] = img.data[i * 4 + 1] = img.data[i * 4 + 2] = g;
        img.data[i * 4 + 3] = 255;
      }
      el.getContext("2d")?.putImageData(img, 0, 0);
      setSize([w, h]);
    });
  }, [lastFrame, status?.source]);

  const soft = () => {
    setError("");
    cameraApi.softTrigger().catch((e) => setError(String(e)));
  };

  return (
    <div className="panel">
      <div className="panel-head">
        <h3 className="panel-title">最近一帧</h3>
        {lastFrame && (
          <span className="muted mono">
            帧 {lastFrame.frameCounter} · 触发 {lastFrame.triggerCounter}
            {lastFrame.lostPackets ? ` · 丢包 ${lastFrame.lostPackets}` : ""}
          </span>
        )}
        <span className="spacer" />
        <button className="btn" onClick={soft} disabled={status?.source !== "mvs" || config?.triggerSource !== "Software"} title="触发源为 Software 时可用">
          <Zap size={15} />
          软触发一次
        </button>
      </div>
      <div className="preview-box">
        <canvas ref={canvas} style={{ display: size ? "block" : "none" }} />
        {!size && <span className="muted">{status?.source === "mvs" ? "暂无图像（仅显示 Mono8）" : "模拟相机不产生图像"}</span>}
      </div>
      {error && <span className="c-ng" style={{ fontSize: 12 }}>{error}</span>}
    </div>
  );
}
