import { useEffect, useState } from "react";
import { RefreshCw } from "lucide-react";
import { cameraApi } from "../api";
import type { CameraConfig, CameraSource, CameraStatus, DeviceSummary, FollowCalib, RecordEntry } from "../types";

interface Props {
  cam: number;
  initial: CameraConfig;
  /** 随动标定由标定面板维护，这里保存时带上它的最新值 */
  follow: FollowCalib | null;
  status: CameraStatus | null;
  onSaved?: (c: CameraConfig) => void;
}

const sources: [CameraSource, string][] = [
  ["mvs", "海康 MVS"],
  ["sim", "模拟相机"],
  ["replay", "回放目录"],
];

export default function CameraConfigPanel({ cam, initial, follow, status, onSaved }: Props) {
  const [config, setConfig] = useState<CameraConfig>(initial);
  const [devices, setDevices] = useState<DeviceSummary[]>([]);
  const [records, setRecords] = useState<RecordEntry[]>([]);
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
    if (config.source === "mvs") refreshDevices();
    if (config.source === "replay") cameraApi.records().then((r) => setRecords(r.items));
  }, [config.source]);

  const set = <K extends keyof CameraConfig>(key: K, value: CameraConfig[K]) => setConfig({ ...config, [key]: value });
  const num = (key: "triggerDelayUs" | "debouncerUs" | "exposureUs" | "gainDb" | "fps" | "replayChannel", step = 1) => (
    <input id={`cam-${key}`} className="input mono" type="number" step={step} value={config[key]} onChange={(e) => set(key, Number(e.target.value))} />
  );
  const mvs = config.source === "mvs";
  const replay = config.source === "replay";
  const triggered = config.acquisition === "triggered";

  const save = async () => {
    setSaving(true);
    try {
      const next = { ...config, follow };
      const warnings = await cameraApi.saveConfig(cam, next);
      setNotice({ ok: warnings.length === 0, text: warnings.length ? `已应用，${warnings.length} 项参数相机未接受` : "已保存并应用" });
      onSaved?.(next);
    } catch (e) {
      setNotice({ ok: false, text: String(e) });
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="panel cam-config">
      <div className="panel-toolbar">
        <h3 className="panel-title">相机参数</h3>
        <button className="btn primary" onClick={save} disabled={saving}>
          {saving ? "写入中…" : "保存并应用"}
        </button>
      </div>
      <div className="cfg-grid">
        <span>名称</span>
        <input id="cam-name" className="input" value={config.name} onChange={(e) => set("name", e.target.value)} />
        <span>图像源</span>
        <div className="segmented">
          {sources.map(([v, label]) => (
            <button key={v} className={config.source === v ? "active" : ""} onClick={() => set("source", v)}>
              {label}
            </button>
          ))}
        </div>
        <span>采集方式</span>
        <div className="segmented">
          <button className={triggered ? "active" : ""} onClick={() => set("acquisition", "triggered")}>触发（飞拍）</button>
          <button className={!triggered ? "active" : ""} onClick={() => set("acquisition", "freeRun")}>连续（随动）</button>
        </div>
        {!triggered && (
          <>
            <span>帧率（fps）</span>
            {num("fps")}
            <span className="hint-cell">布防期间按此帧率出帧；胶嘴速度 ÷ 帧率 就是相邻两帧间胶嘴走过的距离</span>
          </>
        )}
        {replay && (
          <>
            <span>图片目录</span>
            <input id="cam-replayDir" className="input mono" value={config.replayDir} placeholder="D:\现场图\Glue1" onChange={(e) => set("replayDir", e.target.value)} />
            {records.length > 0 && (
              <>
                <span>帧录制</span>
                <select className="input" value="" onChange={(e) => e.target.value && set("replayDir", e.target.value)}>
                  <option value="">从录制目录里选…</option>
                  {records.map((r) => (
                    <option key={r.path} value={r.path}>
                      {r.name} · {r.frames} 帧
                    </option>
                  ))}
                </select>
              </>
            )}
            <span>通道</span>
            {num("replayChannel")}
            <span className="hint-cell">认 cam{"{通道}"}_{"{序号}"}.pgm（帧录制）与 Frame{"{序号}"}_{"{通道}"}.jpg（海康演示图）；0 取目录里的第一个通道</span>
          </>
        )}
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
            {triggered && (
              <>
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
              </>
            )}
            <span>曝光时间（µs）</span>
            {num("exposureUs")}
            <span className="hint-cell">运动中取图需要微秒级曝光配合频闪</span>
            <span>增益（dB）</span>
            {num("gainDb", 0.5)}
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
          固定写入：ExposureAuto/GainAuto=Off、PixelFormat=Mono8；{triggered ? "TriggerMode=On" : "TriggerMode=Off + AcquisitionFrameRate"}；GigE 相机自动设置最佳包长。
          {status?.sdkVersion && ` SDK ${status.sdkVersion}`}
        </p>
      )}
      {config.source === "sim" && (
        <p className="muted hint">
          {triggered ? "模拟相机收到触发后约 180 ms 交付一帧，用于没有硬件时跑通节拍。" : "模拟随动相机按胶嘴此刻的位置合成画面，需要先做随动标定（可用下方的默认三目标定）。"}
        </p>
      )}
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
