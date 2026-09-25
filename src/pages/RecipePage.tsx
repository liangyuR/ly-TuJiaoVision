const fields = [
  { key: "minWidth", label: "最小胶宽 (mm)", value: 1.2 },
  { key: "maxWidth", label: "最大胶宽 (mm)", value: 2.5 },
  { key: "breakLength", label: "断胶判定长度 (mm)", value: 0.5 },
  { key: "overflow", label: "溢胶容差 (mm)", value: 0.3 },
];

export default function RecipePage() {
  return (
    <div className="stack">
      <div className="panel">
        <div className="panel-toolbar">
          <h3 className="panel-title">当前配方</h3>
          <select className="input" defaultValue="default">
            <option value="default">默认配方</option>
          </select>
        </div>
        <div className="form-grid">
          {fields.map((f) => (
            <label key={f.key} className="field">
              <span>{f.label}</span>
              <input className="input" type="number" step="0.1" defaultValue={f.value} />
            </label>
          ))}
        </div>
      </div>
      <div className="panel">
        <h3 className="panel-title">算子流程</h3>
        <div className="empty small">
          <p>LyFlow 算子接入后在此配置检测流程</p>
        </div>
      </div>
    </div>
  );
}
