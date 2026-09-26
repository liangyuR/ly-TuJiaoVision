import { useEffect, useState } from "react";
import { RefreshCw } from "lucide-react";
import { cameraApi } from "../api";
import type { CameraConfig, CameraStatus, DeviceSummary } from "../types";

export default function CameraConfigPanel({ status, onSaved }: { status: CameraStatus | null; onSaved?: (c: CameraConfig) => void }) {
  const [config, setConfig] = useState<CameraConfig | null>(null);
  const [devices, setDevices] = useState<DeviceSummary[]>([]);
  const [deviceError, setDeviceError] = useState("");
  const [notice, setNotice] = useState<{ ok: boolean; text: string } | null>(null);
  const [saving, setSaving] = useState(false);

  const refreshDevices = () => {
    setDeviceError("");
    cameraApi
      .listDevices()
      .then(setDevices)
      .catch((e) => setDeviceError(String(e)));
  };

  useEffect(() => {
    cameraApi.getConfig().then(setConfig);
    refreshDevices();
  }, []);
  if (!config) return null;

  const set = <K extends keyof CameraConfig>(key: K, value: CameraConfig[K]) => setConfig({ ...config, [key]: value });
  const num = (key: "triggerDelayUs" | "debouncerUs" | "exposureUs" | "gainDb") => (
    <input id={`cam-${key}`} className="input mono" type="number" value={config[key]} onChange={(e) => set(key, Number(e.target.value))} />
  );
  const mvs = config.source === "mvs";

  const save = async () => {
    setSaving(true);
    try {
      const warnings = await cameraApi.saveConfig(config);
      setNotice({ ok: warnings.length === 0, text: warnings.length ? `已应用，${warnings.length} 项参数相机未接受` : "已保存并写入相机" });
      onSaved?.(config);
    } catch (e) {
      setNotice({ ok: false, text: String(e) });
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="panel cam-config">
      <div className="panel-toolbar">
        <h3 className="panel-title">触发与 I/O</h3>
        <button className="btn primary" onClick={save} disabled={saving}>
          {saving ? "写入中…" : "保存并应用"}
        </button>
      </div>
      <div className="cfg-grid">
        <span>图像源</span>
        <div className="segmented">
          <button className={mvs ? "active" : ""} onClick={() => set("source", "mvs")}>海康 MVS</button>
          <button className={!mvs ? "active" : ""} onClick={() => set("source", "sim")}>模拟相机</button>
        </div>
        {mvs && (
          <>
            <span>相机</span>
            <div className="row">
              <select id="cam-serial" className="input grow" value={config.serial} onChange={(e) => set("serial", e.target.value)}>
                <option value="">第一台可用相机</option>
                {config.serial && !devices.some((d) => d.serial === config.serial) && <option value={config.serial}>{config.serial}（未发现）</option>}
                {devices.map((d) => (
                  <option key={d.serial} value={d.serial}>
                    {d.model} · {d.serial} · {d.ip ?? d.transport}
                  </option>
                ))}
              </select>
              <button className="icon-btn" onClick={refreshDevices} title="重新枚举">
                <RefreshCw size={16} />
              </button>
            </div>
            {deviceError && <span className="hint-cell c-ng">{deviceError}</span>}
            <span>触发源</span>
            <select id="cam-trigger" className="input" value={config.triggerSource} onChange={(e) => set("triggerSource", e.target.value as CameraConfig["triggerSource"])}>
              <option value="Line0">Line0（机器人位置比较输出）</option>
              <option value="Software">Software（台架调试、模拟节拍）</option>
            </select>
            <span>触发沿</span>
            <select
              id="cam-activation"
              className="input"
              value={config.triggerActivation}
              onChange={(e) => set("triggerActivation", e.target.value as CameraConfig["triggerActivation"])}
              disabled={config.triggerSource !== "Line0"}
            >
              <option value="RisingEdge">上升沿</option>
              <option value="FallingEdge">下降沿</option>
            </select>
            <span>触发延时（µs）</span>
            {num("triggerDelayUs")}
            <span>输入滤波（µs）</span>
            {num("debouncerUs")}
            <span className="hint-cell">滤掉 Line0 毛刺，避免多帧</span>
            <span>曝光时间（µs）</span>
            {num("exposureUs")}
            <span className="hint-cell">飞拍需要微秒级曝光配合频闪，上限见右侧核算</span>
            <span>增益（dB）</span>
            {num("gainDb")}
            <span>Line1 输出</span>
            <label className="check">
              <input type="checkbox" checked={config.strobe} onChange={(e) => set("strobe", e.target.checked)} />
              曝光信号（ExposureStartActive）驱动频闪
            </label>
            <span>Chunk 数据</span>
            <label className="check">
              <input type="checkbox" checked={config.chunk} onChange={(e) => set("chunk", e.target.checked)} />
              帧计数、Line0 触发计数、时间戳
            </label>
          </>
        )}
      </div>
      {mvs && (
        <p className="muted hint">
          固定写入：TriggerMode=On、ExposureAuto/GainAuto=Off、PixelFormat=Mono8；GigE 相机自动设置最佳包长。{status?.sdkVersion && ` SDK ${status.sdkVersion}`}
        </p>
      )}
      {!mvs && <p className="muted hint">模拟相机收到触发后约 180 ms 交付一帧，用于没有硬件时跑通节拍。</p>}
      {notice && <div className={`notice ${notice.ok ? "ok" : "error"}`}>{notice.text}</div>}
      {status?.warnings.length ? (
        <ul className="warn-list">
          {status.warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
