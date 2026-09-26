import { useCycle, SimControls } from "../features/cycle";

export default function CameraPage() {
  const { snapshot } = useCycle();
  const cam = snapshot?.camera;
  return (
    <div className="stack">
      <div className="panel">
        <div className="panel-toolbar">
          <h3 className="panel-title">图像源</h3>
          <span className={`badge ${cam?.ready ? "link-connected" : ""}`}>{cam ? `${cam.source} · ${cam.ready ? "就绪" : "未就绪"}` : "未连接后端"}</span>
        </div>
        <dl className="kv">
          <dt>触发计数</dt>
          <dd className="mono">{cam?.triggers ?? "—"}</dd>
          <dt>帧计数</dt>
          <dd className="mono">{cam?.frames ?? "—"}</dd>
          <dt>说明</dt>
          <dd className="muted">海康 MVS 相机（外触发、曝光、频闪输出、Chunk 计数）在 P2 接入。当前为模拟相机：收到触发后约 180 ms 交付一帧。</dd>
        </dl>
      </div>
      <div className="panel">
        <h3 className="panel-title">模拟节拍</h3>
        <p className="muted" style={{ marginBottom: 12 }}>
          软件代替 PLC 与机器人：写入 SN、产品代码、拍照点数，置 partStart，等待 armed 后按拍照点发触发，最后给 partEnd 并确认结果。可选异常场景用来检查报警与 ERR 处理。
        </p>
        <SimControls />
      </div>
    </div>
  );
}
