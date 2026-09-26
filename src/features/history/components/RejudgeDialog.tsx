import { useState } from "react";
import { useNavigate } from "react-router-dom";
import Modal from "../../plc/components/Modal";
import type { Verdict } from "../../cycle/types";
import { historyApi } from "../api";
import { formatTime, verdictClass, verdictLabel } from "../meta";
import type { HistoryQuery, KindOverride, Overrides, RejudgeResult } from "../types";

const order: Verdict[] = ["OK", "OK_WITH_EXCURSION", "NG_POSITION", "NG_ABSOLUTE", "NG_GAP"];
const kindFields: [keyof KindOverride, string][] = [
  ["tolUpper", "上公差"],
  ["tolLower", "下公差"],
  ["maxExcursionLen", "连续超差允许长度"],
  ["absMin", "绝对限下"],
  ["absMax", "绝对限上"],
];

const isNg = (v: Verdict) => v.startsWith("NG");
const isOk = (v: Verdict) => v.startsWith("OK");

function numOrUndef(s: string) {
  return s.trim() === "" ? undefined : Number(s);
}

/** 用存储的测量表批量重判：可选当前配方与试算参数，输出判定变化矩阵。 */
export default function RejudgeDialog({ query, total, onClose }: { query: HistoryQuery; total: number; onClose: () => void }) {
  const navigate = useNavigate();
  const [useCurrent, setUseCurrent] = useState(false);
  const [overrides, setOverrides] = useState<Overrides>({ line: {}, corner: {} });
  const [result, setResult] = useState<RejudgeResult | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState("");

  const run = async () => {
    setRunning(true);
    setError("");
    try {
      setResult(await historyApi.rejudge({ query, ids: [], useCurrentRecipe: useCurrent, overrides }));
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  };

  const kind = (k: "line" | "corner", f: keyof KindOverride) => (
    <input
      id={`rj-${k}-${f}`}
      className="input mono"
      type="number"
      step={0.05}
      placeholder="不变"
      value={overrides[k][f] ?? ""}
      onChange={(e) => setOverrides({ ...overrides, [k]: { ...overrides[k], [f]: numOrUndef(e.target.value) } })}
    />
  );

  const cell = (from: Verdict, to: Verdict) => result?.matrix.find((m) => m.from === from && m.to === to)?.count ?? 0;
  const released = result ? result.matrix.filter((m) => isNg(m.from) && isOk(m.to)).reduce((a, m) => a + m.count, 0) : 0;

  return (
    <Modal
      title="批量重判"
      width={820}
      onClose={onClose}
      footer={
        <>
          {error && <span className="form-error">{error}</span>}
          <button className="btn" onClick={onClose}>关闭</button>
          <button className="btn primary" onClick={run} disabled={running}>{running ? "重判中…" : `重判当前筛选（${total} 件）`}</button>
        </>
      }
    >
      <div className="rj">
        <p className="muted">用已存储的测量表重新判定，不需要重新拍照；ERR 件和缺少测量数据的记录会跳过。试算参数只用于本次重判，不会保存到配方。</p>
        <div className="segmented">
          <button className={!useCurrent ? "active" : ""} onClick={() => setUseCurrent(false)}>记录当时的配方版本</button>
          <button className={useCurrent ? "active" : ""} onClick={() => setUseCurrent(true)}>当前配方</button>
        </div>
        <div className="rj-params">
          <span />
          {kindFields.map(([, label]) => (
            <span key={label} className="muted">{label}（mm）</span>
          ))}
          <span>直边</span>
          {kindFields.map(([f]) => <div key={f}>{kind("line", f)}</div>)}
          <span>R 角</span>
          {kindFields.map(([f]) => <div key={f}>{kind("corner", f)}</div>)}
        </div>
        <div className="row">
          <label className="field">
            <span>断胶允许长度（mm）</span>
            <input id="rj-gap" className="input mono" type="number" step={0.1} placeholder="不变" value={overrides.maxGapLen ?? ""} onChange={(e) => setOverrides({ ...overrides, maxGapLen: numOrUndef(e.target.value) })} />
          </label>
          <label className="field">
            <span>滤波窗口（奇数）</span>
            <input id="rj-filter" className="input mono" type="number" step={2} min={1} placeholder="不变" value={overrides.filterWindow ?? ""} onChange={(e) => setOverrides({ ...overrides, filterWindow: numOrUndef(e.target.value) })} />
          </label>
        </div>

        {result && (
          <>
            <div className="rj-summary">
              重判 <b>{result.total}</b> 件，跳过 <b>{result.skipped}</b> 件{result.limitHit && "（超过 5000 件，只取最近 5000 件）"}
              {released > 0 && <span className="rj-alert">NG→OK {released} 件：放宽后可能放过真实缺陷，请逐件复核</span>}
            </div>
            <div className="table-wrap">
              <table className="table rj-matrix">
                <thead>
                  <tr>
                    <th>原结果 ＼ 新结果</th>
                    {order.map((v) => <th key={v}>{verdictLabel[v]}</th>)}
                  </tr>
                </thead>
                <tbody>
                  {order.map((from) => (
                    <tr key={from}>
                      <td><span className={`vt ${verdictClass(from)}`}>{verdictLabel[from]}</span></td>
                      {order.map((to) => {
                        const n = cell(from, to);
                        const cls = n === 0 ? "zero" : from === to ? "same" : isNg(from) && isOk(to) ? "release" : "changed";
                        return <td key={to} className={`mono ${cls}`}>{n}</td>;
                      })}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            {result.changes.length > 0 && (
              <div className="rj-changes">
                {result.changes.map((c) => (
                  <button key={c.id} className="rj-change" onClick={() => navigate(`/history/${c.id}`)}>
                    <span className="mono">{formatTime(c.ts)}</span>
                    <span className="mono">SN {c.sn}</span>
                    <span className={`vt ${verdictClass(c.from)}`}>{verdictLabel[c.from]}</span>→
                    <span className={`vt ${verdictClass(c.to)}`}>{verdictLabel[c.to]}</span>
                    <span className="muted ellipsis">{c.reason}</span>
                  </button>
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </Modal>
  );
}
