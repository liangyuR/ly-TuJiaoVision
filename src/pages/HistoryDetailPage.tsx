import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { ArrowLeft, Scale } from "lucide-react";
import { computeVis, TrajectoryMap, UnrolledCurve, type Measured, type PartView, type Recipe } from "../features/cycle";
import { formatTime, historyApi, verdictClass, verdictLabel, type PartDetail, type RejudgeResult } from "../features/history";

const frameStatus: Record<string, [string, string]> = {
  waiting: ["未到达", "c-err"],
  measuring: ["未完成", "c-err"],
  done: ["正常", "c-ok"],
  locateFailed: ["定位失败", "c-err"],
  error: ["测量出错", "c-err"],
  missing: ["未收到", "c-err"],
};

export default function HistoryDetailPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const [detail, setDetail] = useState<PartDetail | null>(null);
  const [layout, setLayout] = useState<Recipe | null>(null);
  const [error, setError] = useState("");
  const [rejudge, setRejudge] = useState<RejudgeResult | null>(null);

  useEffect(() => {
    setDetail(null);
    setRejudge(null);
    historyApi
      .detail(Number(id))
      .then((d) => {
        setDetail(d);
        return historyApi.recipe(d.summary.recipeHash, d.summary.recipeId);
      })
      .then(setLayout)
      .catch((e) => setError(String(e)));
  }, [id]);

  const view = useMemo(() => {
    if (!detail || !layout) return null;
    const pts = detail.points;
    const idx: number[] = [];
    pts?.st.forEach((st, j) => st !== 3 && idx.push(j));
    const measured: Measured[] = pts
      ? [
          {
            sn: detail.summary.sn,
            k: -1,
            cam: 0,
            s: null,
            located: true,
            score: 0,
            ms: 0,
            error: null,
            idx,
            d: idx.map((j) => pts.d[j]),
            w: idx.map((j) => pts.w?.[j] ?? null),
            st: idx.map((j) => pts.st[j]),
            px: [],
          },
        ]
      : [];
    const missing = () => ({ status: "missing" as const, cam: layout.camera, s: null, arrivedMs: null, frameCounter: null, triggerCounter: null, counterJump: false, score: null, points: 0, gapPoints: 0, ms: null });
    const part: PartView = {
      sn: detail.summary.sn,
      recipeId: layout.id,
      mode: layout.mode,
      n: layout.shots.length,
      received: detail.summary.framesReceived,
      triggers: detail.triggers,
      queue: 0,
      filled: idx.length,
      total: layout.points.k.length,
      frames: detail.frames.length || layout.mode === "follow" ? detail.frames : layout.shots.map(missing),
      nozzleS: null,
      endS: null,
      activeCam: null,
    };
    return { measured, vis: computeVis(layout, part, measured, detail.judgement) };
  }, [detail, layout]);

  const runRejudge = async () => {
    if (!detail) return;
    try {
      setRejudge(await historyApi.rejudge({ query: {}, ids: [detail.summary.id], useCurrentRecipe: true, overrides: { line: {}, corner: {}, width: {} } }));
    } catch (e) {
      setError(String(e));
    }
  };

  if (error) return <div className="notice error">{error}</div>;
  if (!detail) return <div className="muted">加载中…</div>;
  const s = detail.summary;
  const j = detail.judgement;
  const ngSegments = j.segments.map((r, i) => ({ r, i })).filter(({ r }) => r.verdict !== "OK");
  const rj = rejudge?.matrix[0];

  return (
    <div className="stack hist-detail">
      <div className="panel detail-head">
        <button className="icon-btn" onClick={() => navigate("/history")} title="返回列表"><ArrowLeft size={18} /></button>
        <b className="mono">SN {s.sn}</b>
        <span className="muted">
          {formatTime(s.ts)} · {s.recipeId ?? "无配方"}
          {s.recipeVersion != null && ` v${s.recipeVersion}`}
          {s.recipeHash && ` #${s.recipeHash.slice(0, 6)}`}
          {s.triggerMode && ` · ${s.triggerMode === "stop" ? "停稳拍" : s.triggerMode === "follow" ? "随动" : "飞拍"}`}
          {s.framesExpected > 0 && ` · 帧 ${s.framesReceived}/${s.framesExpected}`}
        </span>
        <span className="spacer" />
        <span className={`vt big ${verdictClass(s.verdict)}`}>{verdictLabel[s.verdict]} · {s.plcCode}{s.faultCode ? ` / ${s.faultCode}` : ""}</span>
        <button className="btn" onClick={runRejudge} disabled={!detail.points || s.verdict === "ERR_INSPECT"} title="ERR 件没有完整测量数据，不能重判">
          <Scale size={15} />
          按当前配方重判
        </button>
      </div>

      {rejudge && (
        <div className={`notice ${rj && rj.from !== rj.to ? "error" : "ok"}`}>
          {rejudge.total === 0
            ? "该记录无法重判（缺少测量数据或配方快照）"
            : rj && rj.from === rj.to
              ? `按当前配方重判：${verdictLabel[rj.to]}，与原结果一致`
              : `按当前配方重判：${verdictLabel[rejudge.changes[0].from]} → ${verdictLabel[rejudge.changes[0].to]} · ${rejudge.changes[0].reason}`}
        </div>
      )}

      <div className={`detail-grid${layout?.mode === "follow" ? " follow" : ""}`}>
        <div className="panel">
          <div className="panel-head">
            <h3 className="panel-title">轨迹图</h3>
            <span className="muted">{layout ? "" : "配方快照缺失，无法绘制"}</span>
          </div>
          {layout && view && <TrajectoryMap layout={layout} vis={view.vis} className="traj detail-traj" />}
        </div>

        <div className="panel side">
          <h3 className="panel-title" style={{ marginBottom: 0 }}>判定明细</h3>
          <p className="reason-box">{j.reason}</p>
          {j.gaps.map((g, i) => (
            <div key={i} className="ng-item">
              <div className="ng-title c-ng">断胶 · {layout?.segments[g.segment]?.name ?? `段 ${g.segment}`}</div>
              <div className="kv2">
                <span>弧长区间 s</span><b>{g.s0.toFixed(1)} – {g.s1.toFixed(1)} mm</b>
                <span>断胶长度</span><b>{g.len.toFixed(1)} mm（允许 ≤ {layout?.maxGapLen ?? "—"}）</b>
                {g.frames.length > 0 && (
                  <>
                    <span>来源帧</span>
                    <b>{g.frames.map((k) => `k${k}`).join(" + ")}{g.frames.length > 1 ? "，跨帧合并" : ""}</b>
                  </>
                )}
              </div>
            </div>
          ))}
          {ngSegments.filter(({ r }) => r.verdict !== "NG_GAP").map(({ r, i }) => (
            <div key={i} className="ng-item">
              <div className={`ng-title ${r.verdict === "OK_WITH_EXCURSION" ? "c-warn" : "c-ng"}`}>{verdictLabel[r.verdict]} · {layout?.segments[i]?.name ?? `段 ${i}`}</div>
              <div className="kv2">
                <span>{layout?.mode === "follow" ? "偏移范围" : "d 范围"}</span><b>{r.min?.toFixed(2) ?? "—"} – {r.max?.toFixed(2) ?? "—"} mm</b>
                <span>连续超差</span><b>{r.excursionLen.toFixed(1)} mm（允许 ≤ {layout?.segments[i]?.params.maxExcursionLen ?? "—"}）</b>
                {r.wMin != null && (
                  <>
                    <span>胶宽范围</span><b>{r.wMin.toFixed(2)} – {r.wMax?.toFixed(2) ?? "—"} mm</b>
                    <span>胶宽连续超差</span><b>{(r.wExcursionLen ?? 0).toFixed(1)} mm</b>
                  </>
                )}
              </div>
            </div>
          ))}
          <h3 className="panel-title" style={{ margin: "8px 0 0" }}>结果记录</h3>
          <div className="kv2">
            <span>记录号</span><b>#{s.id}</b>
            <span>配置哈希</span><b>{s.recipeHash?.slice(0, 12) ?? "—"}</b>
            <span>软件版本</span><b>{detail.softwareVersion}</b>
            <span>收尾耗时</span><b>{s.drainMs != null ? `${s.drainMs} ms` : "—"}</b>
            <span>触发计数</span><b>{detail.triggers}</b>
            <span>复检自</span><b>{s.retestOf ? <Link to={`/history/${s.retestOf}`}>#{s.retestOf}</Link> : "—"}</b>
            <span>后续复检</span>
            <b>{detail.retests.length ? detail.retests.map((r) => <Link key={r} to={`/history/${r}`} style={{ marginLeft: 6 }}>#{r}</Link>) : "—"}</b>
          </div>
          <p className="muted hint">原图在帧录制打开时写入数据目录的 records 下（按时间与 SN 命名）。</p>
        </div>

        <div className="panel curve-panel">
          <div className="panel-head">
            <h3 className="panel-title">展开曲线</h3>
            <span className="muted">{layout?.mode === "follow" ? "胶条横向偏移" : "距内边距离 d"}（mm），绿色带为公差，红虚线为绝对限</span>
          </div>
          {layout && view && <UnrolledCurve layout={layout} measured={view.measured} vis={view.vis} />}
          {layout?.mode === "follow" && view && (
            <>
              <div className="panel-head">
                <h3 className="panel-title">胶宽</h3>
              </div>
              <UnrolledCurve layout={layout} measured={view.measured} vis={view.vis} quantity="w" />
            </>
          )}
        </div>

        <div className="panel frames-panel">
          <h3 className="panel-title" style={{ marginBottom: 0 }}>逐帧记录</h3>
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr><th>k</th><th>相机 · s</th><th>到达（相对 armed）</th><th>间隔</th><th>Chunk 帧 / 触发</th><th>分数</th><th>负责点</th><th>缺胶点</th><th>处理耗时</th><th>状态</th></tr>
              </thead>
              <tbody>
                {detail.frames.map((f, k) => {
                  const prev = k > 0 ? detail.frames[k - 1].arrivedMs : null;
                  const [label, tone] = frameStatus[f.status] ?? ["—", ""];
                  return (
                    <tr key={k}>
                      <td className="mono">k{k}</td>
                      <td className="mono">#{(f.cam ?? 0) + 1}{f.s != null ? ` · ${f.s.toFixed(1)}` : ""}</td>
                      <td className="mono">{f.arrivedMs != null ? `${f.arrivedMs} ms` : "—"}</td>
                      <td className="mono">{f.arrivedMs != null && prev != null ? `${f.arrivedMs - prev} ms` : "—"}</td>
                      <td className={`mono${f.counterJump ? " c-err" : ""}`}>{f.frameCounter ?? "—"} / {f.triggerCounter ?? "—"}{f.counterJump ? " 跳号" : ""}</td>
                      <td className="mono">{f.score?.toFixed(2) ?? "—"}</td>
                      <td className="mono">{f.points || "—"}</td>
                      <td className={`mono${f.gapPoints ? " c-ng" : ""}`}>{f.gapPoints}</td>
                      <td className="mono">{f.ms != null ? `${f.ms} ms` : "—"}</td>
                      <td className={tone}>{f.gapPoints ? <span className="c-ng">含缺胶</span> : label}</td>
                    </tr>
                  );
                })}
                {detail.frames.length === 0 && (
                  <tr><td colSpan={10} className="muted center">本件未布防（校验阶段即判 ERR），或随动期间没有帧落在可测窗口里</td></tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </div>
  );
}
