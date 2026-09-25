import { useEffect, useMemo, useState } from "react";
import { FileJson, Pencil, PenLine, Plug, PlugZap, Plus, Save, Trash2, Undo2 } from "lucide-react";
import ConnectionForm from "../components/ConnectionForm";
import PlcStatusBadge from "../components/PlcStatusBadge";
import PointEditor from "../components/PointEditor";
import PointsJsonDialog from "../components/PointsJsonDialog";
import WriteDialog from "../components/WriteDialog";
import { plcApi, usePlcStatus, usePlcValues } from "../api";
import { dataTypeLabels, edgeLabels, effectiveOrder, formatValue, isMultiWord, newPoint, protocols } from "../meta";
import { formatClock } from "../time";
import type { PlcConfig, PlcPoint, TagPreset } from "../types";

type Editing = { point: PlcPoint; isNew: boolean } | null;

interface PlcPageProps {
  tagPresets?: TagPreset[];
}

export default function PlcPage({ tagPresets = [] }: PlcPageProps) {
  const status = usePlcStatus();
  const values = usePlcValues();
  const [saved, setSaved] = useState<PlcConfig | null>(null);
  const [draft, setDraft] = useState<PlcConfig | null>(null);
  const [editing, setEditing] = useState<Editing>(null);
  const [writing, setWriting] = useState<PlcPoint | null>(null);
  const [jsonOpen, setJsonOpen] = useState(false);
  const [notice, setNotice] = useState<{ kind: "ok" | "error"; text: string } | null>(null);

  useEffect(() => {
    plcApi.getConfig().then((c) => {
      setSaved(c);
      setDraft(structuredClone(c));
    });
  }, []);

  useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => setNotice(null), 4000);
    return () => clearTimeout(t);
  }, [notice]);

  const dirty = useMemo(() => JSON.stringify(saved) !== JSON.stringify(draft), [saved, draft]);
  const running = status?.state === "connected" || status?.state === "connecting" || status?.state === "error";
  const tagLabel = useMemo(() => new Map(tagPresets.map((t) => [t.value, t.label])), [tagPresets]);

  if (!draft) return null;

  const save = async () => {
    try {
      await plcApi.saveConfig(draft);
      setSaved(structuredClone(draft));
      setNotice({ kind: "ok", text: "配置已保存" });
      return true;
    } catch (e) {
      setNotice({ kind: "error", text: String(e) });
      return false;
    }
  };

  const toggleConnection = async () => {
    try {
      if (running) await plcApi.disconnect();
      else if (!dirty || (await save())) await plcApi.connect();
    } catch (e) {
      setNotice({ kind: "error", text: String(e) });
    }
  };

  const upsertPoint = (point: PlcPoint) => {
    const points = editing?.isNew
      ? [...draft.points, point]
      : draft.points.map((p) => (p.id === editing?.point.id ? point : p));
    const heartbeat =
      !editing?.isNew && draft.heartbeat.pointId === editing?.point.id
        ? { ...draft.heartbeat, pointId: point.id }
        : draft.heartbeat;
    setDraft({ ...draft, points, heartbeat });
    setEditing(null);
  };

  const removePoint = (id: string) =>
    setDraft({
      ...draft,
      points: draft.points.filter((p) => p.id !== id),
      heartbeat: draft.heartbeat.pointId === id ? { ...draft.heartbeat, pointId: null } : draft.heartbeat,
    });

  const savedPoints = new Map(saved?.points.map((p) => [p.id, JSON.stringify(p)]));
  const meta = protocols[draft.connection.protocol];
  const writablePoints = draft.points.filter((p) => p.access === "readWrite");

  return (
    <div className="stack plc">
      <div className="panel">
        <div className="panel-toolbar">
          <div className="row">
            <h3 className="panel-title">连接</h3>
            <PlcStatusBadge state={status?.state} />
            <span className="muted ellipsis" title={status?.message}>
              {status?.message}
            </span>
          </div>
          <div className="row">
            {dirty && <span className="badge warn">未保存</span>}
            <button className="btn" disabled={!dirty} onClick={() => saved && setDraft(structuredClone(saved))}>
              <Undo2 size={16} />
              还原
            </button>
            <button className="btn" disabled={!dirty} onClick={save}>
              <Save size={16} />
              保存配置
            </button>
            <button className={`btn ${running ? "danger" : "primary"}`} onClick={toggleConnection}>
              {running ? <PlugZap size={16} /> : <Plug size={16} />}
              {running ? "断开" : "连接"}
            </button>
          </div>
        </div>

        <ConnectionForm value={draft.connection} onChange={(connection) => setDraft({ ...draft, connection })} />

        <div className="form-grid section">
          <label className="field">
            <span>心跳点位</span>
            <select
              className="input"
              value={draft.heartbeat.pointId ?? ""}
              onChange={(e) => setDraft({ ...draft, heartbeat: { ...draft.heartbeat, pointId: e.target.value || null } })}
            >
              <option value="">不启用</option>
              {writablePoints.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}（{p.address}）
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>心跳周期 (ms)</span>
            <input
              className="input mono"
              type="number"
              disabled={!draft.heartbeat.pointId}
              value={draft.heartbeat.intervalMs}
              onChange={(e) => setDraft({ ...draft, heartbeat: { ...draft.heartbeat, intervalMs: Number(e.target.value) } })}
            />
          </label>
          <label className="field">
            <span>日志保留天数（0 为永久）</span>
            <input
              className="input mono"
              type="number"
              min={0}
              value={draft.logRetentionDays}
              onChange={(e) => setDraft({ ...draft, logRetentionDays: Number(e.target.value) })}
            />
          </label>
        </div>

        <div className="conn-footer">
          <label className="check">
            <input type="checkbox" checked={draft.autoConnect} onChange={(e) => setDraft({ ...draft, autoConnect: e.target.checked })} />
            <span>启动时自动连接</span>
          </label>
          <div className="stats-inline">
            <span>周期 <b className="mono">{status?.cycleMs ?? "--"} ms</b></span>
            <span>轮询 <b className="mono">{status?.pollCount ?? 0}</b></span>
            <span>错误 <b className="mono">{status?.errorCount ?? 0}</b></span>
            <span>最近 <b className="mono">{status?.lastPoll ? formatClock(status.lastPoll) : "--"}</b></span>
          </div>
        </div>
        {notice && <div className={`notice ${notice.kind}`}>{notice.text}</div>}
      </div>

      <div className="panel">
        <div className="panel-toolbar">
          <div className="row">
            <h3 className="panel-title">地址表</h3>
            <span className="muted">{draft.points.length} 个点位</span>
          </div>
          <div className="row">
            <button className="btn" onClick={() => setJsonOpen(true)}>
              <FileJson size={16} />
              导入/导出
            </button>
            <button className="btn primary" onClick={() => setEditing({ point: newPoint(), isNew: true })}>
              <Plus size={16} />
              新增点位
            </button>
          </div>
        </div>
        <p className="muted hint top">
          {meta.label} 地址示例：<span className="mono">{meta.examples.join("  ")}</span>
        </p>
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>名称</th>
                <th>地址</th>
                <th>类型</th>
                <th>读写</th>
                <th>边沿</th>
                <th>标签</th>
                <th>记录</th>
                <th>当前值</th>
                <th className="right">操作</th>
              </tr>
            </thead>
            <tbody>
              {draft.points.length === 0 && (
                <tr>
                  <td colSpan={9} className="muted center">
                    暂无点位，点击「新增点位」开始配置
                  </td>
                </tr>
              )}
              {draft.points.map((p) => {
                const v = values[p.id];
                const live = savedPoints.get(p.id) === JSON.stringify(p) && !dirty;
                return (
                  <tr key={p.id}>
                    <td>
                      <div className="cell-title">
                        {p.name}
                        {draft.heartbeat.pointId === p.id && <span className="tag subtle">心跳</span>}
                      </div>
                      {p.description && <div className="cell-sub">{p.description}</div>}
                    </td>
                    <td className="mono">{p.address}</td>
                    <td className="nowrap">
                      {dataTypeLabels[p.dataType].split(" ")[0]}
                      {isMultiWord(p.dataType) && <span className="cell-sub"> {effectiveOrder(p, draft.connection)}</span>}
                    </td>
                    <td>{p.access === "readWrite" ? "读写" : "只读"}</td>
                    <td>{p.edge === "none" ? <span className="muted">—</span> : edgeLabels[p.edge]}</td>
                    <td>
                      <div className="tags">
                        {p.tags.length === 0 && <span className="muted">—</span>}
                        {p.tags.map((t) => (
                          <span key={t} className="tag">
                            {tagLabel.get(t) ?? t}
                          </span>
                        ))}
                      </div>
                    </td>
                    <td>{p.logChanges ? "是" : <span className="muted">否</span>}</td>
                    <td className={`mono value ${v?.error ? "ng" : ""}`}>
                      {live || v ? formatValue(v) : <span className="muted">待保存</span>}
                    </td>
                    <td className="right nowrap">
                      <button
                        className="icon-btn"
                        title="写入"
                        disabled={p.access !== "readWrite" || status?.state !== "connected" || !live}
                        onClick={() => setWriting(p)}
                      >
                        <PenLine size={16} />
                      </button>
                      <button className="icon-btn" title="编辑" onClick={() => setEditing({ point: p, isNew: false })}>
                        <Pencil size={16} />
                      </button>
                      <button className="icon-btn danger" title="删除" onClick={() => removePoint(p.id)}>
                        <Trash2 size={16} />
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </div>

      {editing && (
        <PointEditor
          initial={editing.point}
          isNew={editing.isNew}
          existingIds={draft.points.map((p) => p.id)}
          connection={draft.connection}
          tagPresets={tagPresets}
          onSave={upsertPoint}
          onClose={() => setEditing(null)}
        />
      )}
      {writing && (
        <WriteDialog
          point={writing}
          current={values[writing.id]}
          onWrite={(value) => plcApi.writePoint(writing.id, value)}
          onClose={() => setWriting(null)}
        />
      )}
      {jsonOpen && (
        <PointsJsonDialog points={draft.points} onApply={(points) => setDraft({ ...draft, points })} onClose={() => setJsonOpen(false)} />
      )}
    </div>
  );
}
