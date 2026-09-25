import { useState } from "react";
import Modal from "./Modal";
import { newPoint } from "../meta";
import type { PlcPoint } from "../types";

interface PointsJsonDialogProps {
  points: PlcPoint[];
  onApply: (points: PlcPoint[]) => void;
  onClose: () => void;
}

export default function PointsJsonDialog({ points, onApply, onClose }: PointsJsonDialogProps) {
  const [text, setText] = useState(() => JSON.stringify(points, null, 2));
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);

  const apply = () => {
    try {
      const parsed = JSON.parse(text);
      if (!Array.isArray(parsed)) throw new Error("需要点位数组");
      onApply(parsed.map((p) => ({ ...newPoint(), ...p })));
      onClose();
    } catch (e) {
      setError(`解析失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const copy = async () => {
    await navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <Modal
      title="导入 / 导出地址表"
      onClose={onClose}
      width={720}
      footer={
        <>
          {error && <span className="form-error">{error}</span>}
          <button className="btn" onClick={copy}>
            {copied ? "已复制" : "复制"}
          </button>
          <button className="btn" onClick={onClose}>
            取消
          </button>
          <button className="btn primary" onClick={apply}>
            应用到地址表
          </button>
        </>
      }
    >
      <p className="muted hint">复制下方 JSON 可备份或在设备间共享；粘贴 JSON 后点击应用将替换当前地址表（需再保存配置）。</p>
      <textarea className="input code" spellCheck={false} value={text} onChange={(e) => setText(e.target.value)} />
    </Modal>
  );
}
