import { useEffect, useState } from "react";
import { getAppInfo, getEngineStatus, type AppInfo, type EngineStatus } from "../lib/api";
import { cycleApi, useRecipes, type CycleSettings, type Timeouts } from "../features/cycle";

const timeoutFields: [keyof Timeouts, string, string][] = [
  ["armMs", "布防目标 T_arm（ms）", "partStart↑ → armed↑，超出仅报警"],
  ["motionMs", "运动超时 T_motion（ms）", "armed↑ 后等待 partEnd↑"],
  ["drainMs", "收尾等待 T_drain（ms）", "partEnd↑ 后等待剩余帧"],
  ["procMs", "单帧处理 T_proc（ms）", "单帧入队到测量完成"],
  ["ackMs", "结果确认 T_ack（ms）", "done↑ 后等待 resultAck↑"],
];

function CycleSettingsPanel() {
  const recipes = useRecipes();
  const [settings, setSettings] = useState<CycleSettings | null>(null);
  const [notice, setNotice] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    cycleApi.getSettings().then(setSettings);
  }, []);
  if (!settings) return null;

  const save = () =>
    cycleApi
      .saveSettings(settings)
      .then(() => setNotice({ ok: true, text: "已保存，从下一个工件开始生效" }))
      .catch((e) => setNotice({ ok: false, text: String(e) }));

  return (
    <div className="panel">
      <div className="panel-toolbar">
        <h3 className="panel-title">检测节拍</h3>
        <button className="btn primary" onClick={save}>保存</button>
      </div>
      <div className="form-grid">
        <label className="field">
          <span>型号来源</span>
          <select
            id="product-source"
            className="input"
            value={settings.productSource}
            onChange={(e) => setSettings({ ...settings, productSource: e.target.value as CycleSettings["productSource"] })}
          >
            <option value="plc">PLC 下发产品代码</option>
            <option value="manual">人工选择配方</option>
          </select>
        </label>
        {settings.productSource === "manual" && (
          <label className="field">
            <span>当前配方</span>
            <select
              id="manual-recipe-setting"
              className="input"
              value={settings.manualRecipeId ?? ""}
              onChange={(e) => setSettings({ ...settings, manualRecipeId: e.target.value || null })}
            >
              <option value="">未选择</option>
              {recipes.map((r) => (
                <option key={r.id} value={r.id}>
                  {r.id} · {r.name}
                </option>
              ))}
            </select>
          </label>
        )}
        <label className="field" title="超过天数的检测记录每天自动删除">
          <span>记录保留（天）</span>
          <input
            id="history-days"
            className="input mono"
            type="number"
            min={1}
            value={settings.historyDays}
            onChange={(e) => setSettings({ ...settings, historyDays: Number(e.target.value) })}
          />
        </label>
        {timeoutFields.map(([key, label, hint]) => (
          <label key={key} className="field" title={hint}>
            <span>{label}</span>
            <input
              id={`timeout-${key}`}
              className="input mono"
              type="number"
              min={0}
              step={100}
              value={settings.timeouts[key]}
              onChange={(e) => setSettings({ ...settings, timeouts: { ...settings.timeouts, [key]: Number(e.target.value) } })}
            />
          </label>
        ))}
      </div>
      <p className="muted hint">
        PLC 下发：按产品代码匹配配方，匹配不到判 ERR 95。人工选择：操作员在实时检测页切换配方，仅空闲时可切换，不读取产品代码。
      </p>
      {notice && <div className={`notice ${notice.ok ? "ok" : "error"}`}>{notice.text}</div>}
    </div>
  );
}

function VisionPanel() {
  const [settings, setSettings] = useState<CycleSettings | null>(null);
  const [engine, setEngine] = useState<EngineStatus | null>(null);
  const [notice, setNotice] = useState<{ ok: boolean; text: string } | null>(null);
  const refresh = () => getEngineStatus().then(setEngine).catch(() => setEngine(null));

  useEffect(() => {
    cycleApi.getSettings().then(setSettings);
    refresh();
  }, []);
  if (!settings) return null;

  const save = async () => {
    try {
      await cycleApi.saveSettings(settings);
      setNotice({ ok: true, text: "已保存" });
    } catch (e) {
      setNotice({ ok: false, text: String(e) });
    }
    refresh();
  };

  return (
    <div className="panel">
      <div className="panel-toolbar">
        <h3 className="panel-title">视觉测量（lyFlow）</h3>
        <button className="btn primary" onClick={save}>保存</button>
      </div>
      <div className="form-grid">
        <label className="field span-2">
          <span>核心库路径（lyflow_core.dll）</span>
          <input
            id="lyflow-core"
            className="input mono"
            placeholder="例如 D:\project\LyFlow-glueuild\corein\lyflow_core.dll"
            value={settings.lyflowCore ?? ""}
            onChange={(e) => setSettings({ ...settings, lyflowCore: e.target.value || null })}
          />
        </label>
        <label className="check" style={{ alignSelf: "end", height: 32 }}>
          <input id="vision-on" type="checkbox" checked={settings.vision} onChange={(e) => setSettings({ ...settings, vision: e.target.checked })} />
          用 lyFlow 测量（定位 + 逐点卡尺）
        </label>
      </div>
      <dl className="kv" style={{ marginTop: 12 }}>
        <dt>状态</dt>
        <dd className={engine?.ready ? "ok" : "muted"}>
          {engine ? `${engine.ready ? "已加载" : "未加载"}${engine.version ? ` · ${engine.version}` : ""}` : "--"}
        </dd>
        <dt>说明</dt>
        <dd>{engine?.message ?? "--"}</dd>
      </dl>
      <p className="muted hint">
        关闭时用模拟测量。打开后，模拟相机按配方几何合成图像并自动生成示教资料；海康相机需要先示教配方、再在图像源页做工位标定。
      </p>
      {notice && <div className={`notice ${notice.ok ? "ok" : "error"}`}>{notice.text}</div>}
    </div>
  );
}

export default function SettingsPage() {
  const [info, setInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    getAppInfo().then(setInfo).catch(() => setInfo(null));
  }, []);

  return (
    <div className="stack">
      <CycleSettingsPanel />
      <VisionPanel />
      <div className="panel">
        <h3 className="panel-title">关于</h3>
        <dl className="kv">
          <dt>应用</dt>
          <dd>{info?.name ?? "--"}</dd>
          <dt>版本</dt>
          <dd>{info?.version ?? "--"}</dd>
        </dl>
      </div>
    </div>
  );
}
