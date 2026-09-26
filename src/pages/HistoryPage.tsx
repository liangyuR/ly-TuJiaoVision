import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { ChevronLeft, ChevronRight, Download, RefreshCw, Scale } from "lucide-react";
import { subscribe } from "../features/plc";
import { useRecipes } from "../features/cycle";
import { formatTime, historyApi, RejudgeDialog, verdictClass, verdictGroups, verdictLabel, type HistoryPage as Page, type HistoryQuery } from "../features/history";

const PAGE = 50;
const ranges: [string, string, number | null][] = [
  ["today", "今天", 0],
  ["7d", "7 天", 7],
  ["30d", "30 天", 30],
  ["all", "全部", null],
];

function rangeStart(days: number | null) {
  if (days === null) return null;
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return d.getTime() - days * 86_400_000;
}

export default function HistoryPage() {
  const navigate = useNavigate();
  const recipes = useRecipes();
  const [range, setRange] = useState("today");
  const [groups, setGroups] = useState<string[]>([]);
  const [sn, setSn] = useState("");
  const [recipeId, setRecipeId] = useState("");
  const [offset, setOffset] = useState(0);
  const [page, setPage] = useState<Page | null>(null);
  const [error, setError] = useState("");
  const [exported, setExported] = useState<string | null>(null);
  const [rejudge, setRejudge] = useState(false);

  const query = useMemo<HistoryQuery>(
    () => ({
      from: rangeStart(ranges.find((r) => r[0] === range)![2]),
      verdicts: verdictGroups.filter((g) => groups.includes(g.key)).flatMap((g) => g.verdicts),
      sn,
      recipeId: recipeId || null,
    }),
    [range, groups, sn, recipeId],
  );

  const load = useCallback(() => {
    historyApi
      .query({ ...query, offset, limit: PAGE })
      .then((p) => {
        setPage(p);
        setError("");
      })
      .catch((e) => setError(String(e)));
  }, [query, offset]);

  useEffect(load, [load]);
  useEffect(() => setOffset(0), [query]);
  useEffect(() => subscribe<number>("history://inserted", () => offset === 0 && load()), [load, offset]);

  const exportCsv = async () => {
    try {
      setExported(await historyApi.exportCsv(query));
    } catch (e) {
      setError(String(e));
    }
  };

  const c = page?.counts;
  return (
    <div className="stack">
      <div className="panel">
        <div className="filter-row">
          <div className="segmented">
            {ranges.map(([key, label]) => (
              <button key={key} className={range === key ? "active" : ""} onClick={() => setRange(key)}>
                {label}
              </button>
            ))}
          </div>
          <span className="filter-label">结果</span>
          {verdictGroups.map((g) => (
            <button
              key={g.key}
              className={`chip${groups.includes(g.key) ? " on" : ""}`}
              onClick={() => setGroups(groups.includes(g.key) ? groups.filter((x) => x !== g.key) : [...groups, g.key])}
            >
              {g.label}
            </button>
          ))}
          <input id="history-sn" className="input" placeholder="SN" value={sn} onChange={(e) => setSn(e.target.value)} style={{ width: 140 }} />
          <select id="history-recipe" className="input" value={recipeId} onChange={(e) => setRecipeId(e.target.value)}>
            <option value="">全部配方</option>
            {recipes.map((r) => (
              <option key={r.id} value={r.id}>{r.id}</option>
            ))}
          </select>
          <span className="spacer" />
          <button className="icon-btn" onClick={load} title="刷新"><RefreshCw size={16} /></button>
          <button className="btn" onClick={() => setRejudge(true)} disabled={!page?.total}>
            <Scale size={15} />
            批量重判
          </button>
          <button className="btn" onClick={exportCsv} disabled={!page?.total}>
            <Download size={15} />
            导出 CSV
          </button>
        </div>
        <div className="hist-counts">
          <span>共 <b>{page?.total ?? 0}</b> 件</span>
          <span>OK <b className="c-ok">{c?.ok ?? 0}</b></span>
          <span>局部超差 <b className="c-warn">{c?.excursion ?? 0}</b></span>
          <span>NG <b className="c-ng">{c?.ng ?? 0}</b></span>
          <span>ERR <b className="c-err">{c?.err ?? 0}</b></span>
          {page && page.total > 0 && <span>良率 <b>{(((c!.ok + c!.excursion) / page.total) * 100).toFixed(1)}%</b></span>}
        </div>
        {exported && (
          <div className="notice ok">
            已导出：<span className="mono">{exported}</span>{" "}
            <button className="link" onClick={() => historyApi.reveal(exported)}>打开所在文件夹</button>
          </div>
        )}
        {error && <div className="notice error">{error}</div>}
      </div>

      <div className="panel">
        <div className="table-wrap">
          <table className="table hist-table">
            <thead>
              <tr>
                <th>时间</th>
                <th>SN</th>
                <th>配方</th>
                <th>结果</th>
                <th>PLC 码</th>
                <th>原因</th>
                <th>帧</th>
                <th>收尾</th>
              </tr>
            </thead>
            <tbody>
              {page?.items.map((p) => (
                <tr key={p.id} onClick={() => navigate(`/history/${p.id}`)}>
                  <td className="mono nowrap">{formatTime(p.ts)}</td>
                  <td className="mono nowrap">
                    {p.sn}
                    {p.retestOf && <span className="tag retest" title={`复检，上一次记录 #${p.retestOf}`}>复检</span>}
                  </td>
                  <td className="nowrap">
                    {p.recipeId ?? "—"}
                    {p.recipeVersion != null && <span className="muted"> v{p.recipeVersion}</span>}
                    {p.triggerMode && <span className="muted"> · {p.triggerMode === "stop" ? "停稳拍" : "飞拍"}</span>}
                  </td>
                  <td className="nowrap"><span className={`vt ${verdictClass(p.verdict)}`}>{verdictLabel[p.verdict]}</span></td>
                  <td className="mono">{p.plcCode}{p.faultCode ? ` / ${p.faultCode}` : ""}</td>
                  <td className="reason">{p.reason}</td>
                  <td className="mono nowrap">{p.framesExpected ? `${p.framesReceived}/${p.framesExpected}` : "—"}</td>
                  <td className="mono nowrap">{p.drainMs != null ? `${p.drainMs} ms` : "—"}</td>
                </tr>
              ))}
              {page && page.items.length === 0 && (
                <tr>
                  <td colSpan={8} className="muted center">没有符合条件的记录</td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
        {page && page.total > PAGE && (
          <div className="pager">
            <button className="icon-btn" disabled={offset === 0} onClick={() => setOffset(Math.max(0, offset - PAGE))}><ChevronLeft size={16} /></button>
            <span className="muted">{offset + 1}–{Math.min(offset + PAGE, page.total)} / {page.total}</span>
            <button className="icon-btn" disabled={offset + PAGE >= page.total} onClick={() => setOffset(offset + PAGE)}><ChevronRight size={16} /></button>
          </div>
        )}
      </div>
      {rejudge && page && <RejudgeDialog query={query} total={page.total} onClose={() => setRejudge(false)} />}
    </div>
  );
}
