import { useCallback, useEffect, useRef, useState } from "react";
import { ChevronLeft, ChevronRight, Download, Radio, RefreshCw, Search, X } from "lucide-react";
import StepChart from "../components/StepChart";
import { plcApi, subscribe } from "../api";
import { categoryLabels, levelLabels } from "../meta";
import type { HistorySample, LogCategory, LogLevel, LogPage, LogQuery, PlcPoint } from "../types";
import { formatTs, fromLocalInput, toLocalInput } from "../time";

const PAGE_SIZE = 100;

const ranges = [
  { key: "15m", label: "15 分钟", ms: 15 * 60_000 },
  { key: "1h", label: "1 小时", ms: 3_600_000 },
  { key: "24h", label: "24 小时", ms: 86_400_000 },
  { key: "7d", label: "7 天", ms: 7 * 86_400_000 },
  { key: "custom", label: "自定义", ms: 0 },
] as const;

type RangeKey = (typeof ranges)[number]["key"];

function toggle<T>(list: T[], v: T) {
  return list.includes(v) ? list.filter((x) => x !== v) : [...list, v];
}

function csvCell(v: string | null | undefined) {
  const s = v ?? "";
  return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}

export default function PlcLogPage() {
  const [points, setPoints] = useState<PlcPoint[]>([]);
  const [rangeKey, setRangeKey] = useState<RangeKey>("1h");
  const [customStart, setCustomStart] = useState(() => toLocalInput(Date.now() - 3_600_000));
  const [customEnd, setCustomEnd] = useState(() => toLocalInput(Date.now()));
  const [levels, setLevels] = useState<LogLevel[]>([]);
  const [categories, setCategories] = useState<LogCategory[]>([]);
  const [pointId, setPointId] = useState("");
  const [keywordInput, setKeywordInput] = useState("");
  const [keyword, setKeyword] = useState("");
  const [page, setPage] = useState(0);
  const [live, setLive] = useState(true);
  const [data, setData] = useState<LogPage>({ total: 0, items: [] });
  const [history, setHistory] = useState<{ samples: HistorySample[]; start: number; end: number } | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    plcApi.getConfig().then((c) => setPoints(c.points));
  }, []);

  const buildQuery = useCallback((): { query: LogQuery; start: number; end: number } => {
    const now = Date.now();
    if (rangeKey === "custom") {
      const start = fromLocalInput(customStart) ?? now - 3_600_000;
      const end = fromLocalInput(customEnd) ?? now;
      return { query: { start, end, levels, categories, pointId: pointId || null, keyword: keyword || null }, start, end };
    }
    const start = now - ranges.find((r) => r.key === rangeKey)!.ms;
    return { query: { start, end: null, levels, categories, pointId: pointId || null, keyword: keyword || null }, start, end: now };
  }, [rangeKey, customStart, customEnd, levels, categories, pointId, keyword]);

  const run = useCallback(async () => {
    const { query, start, end } = buildQuery();
    setLoading(true);
    setError("");
    try {
      const [result, samples] = await Promise.all([
        plcApi.queryLogs({ ...query, limit: PAGE_SIZE, offset: page * PAGE_SIZE }),
        pointId ? plcApi.pointHistory(pointId, start, end) : Promise.resolve(null),
      ]);
      setData(result);
      setHistory(samples ? { samples, start, end } : null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [buildQuery, page, pointId]);

  const runRef = useRef(run);
  runRef.current = run;

  useEffect(() => {
    run();
  }, [run]);

  const following = live && rangeKey !== "custom" && page === 0;

  useEffect(() => {
    if (!following) return;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const unsubscribe = subscribe("plc://log", () => {
      if (timer) return;
      timer = setTimeout(() => {
        timer = null;
        runRef.current();
      }, 800);
    });
    return () => {
      unsubscribe();
      if (timer) clearTimeout(timer);
    };
  }, [following]);

  const resetPage = () => setPage(0);

  const exportCsv = async () => {
    const { query } = buildQuery();
    const rows: string[] = ["时间,级别,类别,点位,内容,旧值,新值"];
    for (let offset = 0; offset < 50_000; offset += 1000) {
      const batch = await plcApi.queryLogs({ ...query, limit: 1000, offset });
      for (const e of batch.items) {
        rows.push(
          [formatTs(e.ts), levelLabels[e.level], categoryLabels[e.category], e.pointName, e.message, e.oldValue, e.newValue]
            .map(csvCell)
            .join(","),
        );
      }
      if (batch.items.length < 1000) break;
    }
    const blob = new Blob(["﻿" + rows.join("\n")], { type: "text/csv;charset=utf-8" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `plc-log-${toLocalInput(Date.now()).replace(/[:T]/g, "")}.csv`;
    a.click();
    URL.revokeObjectURL(a.href);
  };

  const pageCount = Math.max(1, Math.ceil(data.total / PAGE_SIZE));
  const selectedPoint = points.find((p) => p.id === pointId);

  return (
    <div className="stack">
      <div className="panel filters">
        <div className="filter-row">
          <div className="segmented">
            {ranges.map((r) => (
              <button
                key={r.key}
                className={rangeKey === r.key ? "active" : ""}
                onClick={() => {
                  setRangeKey(r.key);
                  resetPage();
                }}
              >
                {r.label}
              </button>
            ))}
          </div>
          {rangeKey === "custom" && (
            <div className="row">
              <input className="input mono" type="datetime-local" step={1} value={customStart} onChange={(e) => { setCustomStart(e.target.value); resetPage(); }} />
              <span className="muted">至</span>
              <input className="input mono" type="datetime-local" step={1} value={customEnd} onChange={(e) => { setCustomEnd(e.target.value); resetPage(); }} />
            </div>
          )}
          <div className="spacer" />
          <button
            className={`btn ${following ? "live" : ""}`}
            disabled={rangeKey === "custom"}
            title={rangeKey === "custom" ? "自定义时间段不支持实时跟随" : ""}
            onClick={() => {
              setLive((v) => !v);
              resetPage();
            }}
          >
            <Radio size={16} />
            {following ? "实时跟随中" : "实时跟随"}
          </button>
          <button className="btn" onClick={run}>
            <RefreshCw size={16} className={loading ? "spin" : ""} />
            刷新
          </button>
          <button className="btn" onClick={exportCsv}>
            <Download size={16} />
            导出 CSV
          </button>
        </div>

        <div className="filter-row">
          <span className="filter-label">级别</span>
          {(Object.keys(levelLabels) as LogLevel[]).map((l) => (
            <button key={l} className={`chip level-${l} ${levels.includes(l) ? "on" : ""}`} onClick={() => { setLevels(toggle(levels, l)); resetPage(); }}>
              {levelLabels[l]}
            </button>
          ))}
          <span className="filter-label">类别</span>
          {(Object.keys(categoryLabels) as LogCategory[]).map((c) => (
            <button key={c} className={`chip ${categories.includes(c) ? "on" : ""}`} onClick={() => { setCategories(toggle(categories, c)); resetPage(); }}>
              {categoryLabels[c]}
            </button>
          ))}
        </div>

        <div className="filter-row">
          <span className="filter-label">点位</span>
          <select className="input" value={pointId} onChange={(e) => { setPointId(e.target.value); resetPage(); }}>
            <option value="">全部点位</option>
            {points.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
          <div className="search">
            <Search size={16} />
            <input
              className="input"
              placeholder="搜索内容或点位名称，回车确认"
              value={keywordInput}
              onChange={(e) => setKeywordInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  setKeyword(keywordInput.trim());
                  resetPage();
                }
              }}
            />
            {keywordInput && (
              <button className="icon-btn" onClick={() => { setKeywordInput(""); setKeyword(""); resetPage(); }}>
                <X size={14} />
              </button>
            )}
          </div>
        </div>
      </div>

      {history && (
        <div className="panel">
          <div className="panel-toolbar">
            <h3 className="panel-title">点位趋势 · {selectedPoint?.name ?? pointId}</h3>
            <span className="muted">{history.samples.length} 个采样点 · 基于值变化记录</span>
          </div>
          <StepChart samples={history.samples} start={history.start} end={history.end} />
        </div>
      )}

      <div className="panel">
        <div className="panel-toolbar">
          <div className="row">
            <h3 className="panel-title">日志</h3>
            <span className="muted">共 {data.total} 条</span>
            {error && <span className="ng">{error}</span>}
          </div>
          <div className="row">
            <button className="icon-btn" disabled={page === 0} onClick={() => setPage(page - 1)}>
              <ChevronLeft size={18} />
            </button>
            <span className="mono muted">
              {page + 1} / {pageCount}
            </span>
            <button className="icon-btn" disabled={page + 1 >= pageCount} onClick={() => setPage(page + 1)}>
              <ChevronRight size={18} />
            </button>
          </div>
        </div>
        <div className="table-wrap">
          <table className="table log-table">
            <thead>
              <tr>
                <th>时间</th>
                <th>级别</th>
                <th>类别</th>
                <th>点位</th>
                <th>内容</th>
              </tr>
            </thead>
            <tbody>
              {data.items.length === 0 && (
                <tr>
                  <td colSpan={5} className="muted center">
                    {loading ? "加载中…" : "所选条件下暂无日志"}
                  </td>
                </tr>
              )}
              {data.items.map((e) => (
                <tr key={e.id}>
                  <td className="mono nowrap">{formatTs(e.ts)}</td>
                  <td>
                    <span className={`badge level-${e.level}`}>{levelLabels[e.level]}</span>
                  </td>
                  <td className="nowrap">{categoryLabels[e.category] ?? e.category}</td>
                  <td className="nowrap">
                    {e.pointId ? (
                      <button className="link" onClick={() => { setPointId(e.pointId!); resetPage(); }}>
                        {e.pointName ?? e.pointId}
                      </button>
                    ) : (
                      <span className="muted">—</span>
                    )}
                  </td>
                  <td className="msg">{e.message}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
