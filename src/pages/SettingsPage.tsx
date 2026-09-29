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

/** 两个面板各管一部分字段：保存时读回最新设置、只改自己的字段，另一个面板刚保存的内容不被旧值覆盖。 */
async function savePart(part: Partial<CycleSettings>) {
  const cur = await cycleApi.getSettings();
  await cycleApi.saveSettings({ ...cur, ...part });
}

function CycleSettingsPanel() {
  const recipes = useRecipes();
  const [settings, setSettings] = useState<CycleSettings | null>(null);
  const [notice, setNotice] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    cycleApi.getSettings().then(setSettings);
  }, []);
  if (!settings) return null;

  const save = async () => {
    try {
      const { productSource, manualRecipeId, historyDays, timeouts } = settings;
      await savePart({ productSource, manualRecipeId, historyDays, timeouts });
      setNotice({ ok: true, text: "已保存，从下一个工件开始生效" });
    } catch (e) {
      setNotice({ ok: false, text: String(e) });
    }
  };

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

function MeasurePanel() {
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
      const { lyflowCore, vision, followVision, record, recordKeep, recordMaxGb } = settings;
      await savePart({ lyflowCore, vision, followVision, record, recordKeep, recordMaxGb });
      setNotice({ ok: true, text: "已保存" });
    } catch (e) {
      setNotice({ ok: false, text: String(e) });
    }
    refresh();
  };

  return (
    <div className="panel">
      <div className="panel-toolbar">
        <h3 className="panel-title">测量与帧录制</h3>
        <button className="btn primary" onClick={save}>保存</button>
      </div>
      <div className="form-grid">
        <label className="field">
          <span>飞拍配方</span>
          <select id="measure-fly" className="input" value={settings.vision ? "lyFlow" : "sim"} onChange={(e) => setSettings({ ...settings, vision: e.target.value === "lyFlow" })}>
            <option value="sim">模拟测量（不看图像）</option>
            <option value="lyFlow">lyFlow 流程（模板定位 + 逐点卡尺）</option>
          </select>
        </label>
        <label className="field">
          <span>随动配方</span>
          <select id="measure-follow" className="input" value={settings.followVision ? "native" : "sim"} onChange={(e) => setSettings({ ...settings, followVision: e.target.value === "native" })}>
            <option value="native">本程序卡尺</option>
            <option value="sim">模拟测量（不看图像）</option>
          </select>
        </label>
        {settings.vision && (
          <label className="field span-2">
            <span>核心库路径（lyflow_core.dll）</span>
            <input
              id="lyflow-core"
              className="input mono"
              placeholder="例如 D:\project\LyFlow\build\core\bin\lyflow_core.dll"
              value={settings.lyflowCore ?? ""}
              onChange={(e) => setSettings({ ...settings, lyflowCore: e.target.value || null })}
            />
          </label>
        )}
        <label className="field">
          <span>帧录制</span>
          <select id="record-mode" className="input" value={settings.record} onChange={(e) => setSettings({ ...settings, record: e.target.value as CycleSettings["record"] })}>
            <option value="off">关闭</option>
            <option value="failed">只留 NG / ERR 件</option>
            <option value="all">全部</option>
          </select>
        </label>
        <label className="field">
          <span>录制最多保留（件）</span>
          <input
            id="record-keep"
            className="input mono"
            type="number"
            min={1}
            value={settings.recordKeep}
            onChange={(e) => setSettings({ ...settings, recordKeep: Number(e.target.value) })}
          />
        </label>
        <label className="field" title="随动一件三路约 0.5 GB">
          <span>录制总大小上限（GB）</span>
          <input
            id="record-max-gb"
            className="input mono"
            type="number"
            min={0.5}
            step={1}
            value={settings.recordMaxGb}
            onChange={(e) => setSettings({ ...settings, recordMaxGb: Number(e.target.value) })}
          />
        </label>
      </div>
      <dl className="kv" style={{ marginTop: 12 }}>
        <dt>引擎</dt>
        <dd className={engine?.ready ? "ok" : "muted"}>
          {engine ? `${engine.backend} · ${engine.ready ? "就绪" : "未就绪"}${engine.version ? ` · ${engine.version}` : ""}` : "--"}
        </dd>
        <dt>说明</dt>
        <dd>{engine?.message ?? "--"}</dd>
      </dl>
      <p className="muted hint">
        本程序卡尺用于随动配方：按随动标定把胶路投到图像上，沿法向找胶条两侧边缘，得出偏移与胶宽。lyFlow 流程用于飞拍配方（模板定位 +
        逐点卡尺），需要带图像域的 lyFlow 版本。帧录制把整帧图像写到数据目录的 records 下，可在图像源页选作回放目录。
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
      <MeasurePanel />
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
