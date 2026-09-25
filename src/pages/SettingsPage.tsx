import { useEffect, useState } from "react";
import { getAppInfo, getEngineStatus, type AppInfo, type EngineStatus } from "../lib/api";

export default function SettingsPage() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [engine, setEngine] = useState<EngineStatus | null>(null);

  useEffect(() => {
    getAppInfo().then(setInfo).catch(() => setInfo(null));
    getEngineStatus().then(setEngine).catch(() => setEngine(null));
  }, []);

  return (
    <div className="stack">
      <div className="panel">
        <h3 className="panel-title">关于</h3>
        <dl className="kv">
          <dt>应用</dt>
          <dd>{info?.name ?? "--"}</dd>
          <dt>版本</dt>
          <dd>{info?.version ?? "--"}</dd>
        </dl>
      </div>
      <div className="panel">
        <h3 className="panel-title">检测引擎</h3>
        <dl className="kv">
          <dt>后端</dt>
          <dd>{engine?.backend ?? "--"}</dd>
          <dt>状态</dt>
          <dd>{engine ? (engine.ready ? "就绪" : "未就绪") : "--"}</dd>
          <dt>说明</dt>
          <dd>{engine?.message ?? "--"}</dd>
        </dl>
      </div>
    </div>
  );
}
