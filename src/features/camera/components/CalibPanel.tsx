import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { Crosshair } from "lucide-react";

interface CalibInfo {
  path: string;
  rms: number | null;
  mmPerPx: number | null;
  maxError: number | null;
  pattern: number[] | null;
  square: number | null;
  ts: number | null;
}

/** 工位标定：标定板放在内边所在高度，软触发一帧，用 lyFlow 的 image.board_calib 求单应，存为工位标定文件。 */
export default function CalibPanel({ isSim }: { isSim: boolean }) {
  const [info, setInfo] = useState<CalibInfo | null>(null);
  const [cols, setCols] = useState(11);
  const [rows, setRows] = useState(8);
  const [square, setSquare] = useState(5);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    if (isTauri()) invoke<CalibInfo | null>("vision_calib_info").then(setInfo).catch(() => setInfo(null));
  }, []);

  const run = async () => {
    setBusy(true);
    setNotice(null);
    try {
      const r = await invoke<CalibInfo>("vision_calibrate", { pattern: [cols, rows], square });
      setInfo(r);
      setNotice({ ok: true, text: `标定完成：残差 RMS ${r.rms?.toFixed(4)} mm，约 ${r.mmPerPx?.toFixed(4)} mm/px` });
    } catch (e) {
      setNotice({ ok: false, text: String(e) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="panel">
      <div className="panel-head">
        <h3 className="panel-title">工位标定</h3>
        <span className="muted">标定属于相机工位，换型不重标；标定板放在内边所在高度的平面上</span>
        <span className="spacer" />
        <button className="btn primary" onClick={run} disabled={busy || isSim}>
          <Crosshair size={15} />
          {busy ? "标定中…" : "用最近一帧标定"}
        </button>
      </div>
      {isSim ? (
        <p className="muted">模拟相机的像素当量已知（0.08 mm/px），自动使用内置标定。</p>
      ) : (
        <div className="calib-row">
          <label className="field">
            <span>内角点（列）</span>
            <input id="calib-cols" className="input mono" type="number" min={2} value={cols} onChange={(e) => setCols(Number(e.target.value))} />
          </label>
          <label className="field">
            <span>内角点（行）</span>
            <input id="calib-rows" className="input mono" type="number" min={2} value={rows} onChange={(e) => setRows(Number(e.target.value))} />
          </label>
          <label className="field">
            <span>格长（mm）</span>
            <input id="calib-square" className="input mono" type="number" step={0.5} value={square} onChange={(e) => setSquare(Number(e.target.value))} />
          </label>
        </div>
      )}
      <dl className="kv">
        <dt>当前标定</dt>
        <dd>
          {info
            ? `RMS ${info.rms?.toFixed(4) ?? "—"} mm · ${info.mmPerPx?.toFixed(4) ?? "—"} mm/px${info.pattern ? ` · ${info.pattern.join("×")} @ ${info.square} mm` : ""}${info.ts ? ` · ${new Date(info.ts).toLocaleString("zh-CN", { hour12: false })}` : ""}`
            : "未标定"}
        </dd>
      </dl>
      {notice && <div className={`notice ${notice.ok ? "ok" : "error"}`}>{notice.text}</div>}
    </div>
  );
}
