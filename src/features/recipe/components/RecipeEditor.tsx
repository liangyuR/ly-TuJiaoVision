import { useEffect, useMemo, useState, type ReactNode } from "react";
import { Save, Upload } from "lucide-react";
import { recipeApi } from "../../cycle/api";
import TrajectoryMap from "../../cycle/components/TrajectoryMap";
import type { FollowSpec, JudgeParams, PathSpec, Recipe, RecipeDoc, SegmentLimits } from "../../cycle/types";

interface Props {
  initial: RecipeDoc;
  originalId: string | null;
  /** 相机组里的相机：配方按编号引用 */
  cameras: { id: string; name: string }[];
  onSaved: (id: string) => void;
}

const paramFields: [keyof JudgeParams, string][] = [
  ["nominal", "名义"],
  ["tolUpper", "上公差"],
  ["tolLower", "下公差"],
  ["absMin", "绝对下限"],
  ["absMax", "绝对上限"],
  ["maxExcursionLen", "允许超差长度"],
];

const pointsText = (pts: [number, number][]) => pts.map(([x, y]) => `${x}, ${y}`).join("\n");

function parseRows(text: string): number[][] {
  return text
    .split(/\r?\n/)
    .map((l) => l.split(/[,;\s\t]+/).filter(Boolean).map(Number))
    .filter((v) => v.length >= 2 && v.every(Number.isFinite));
}

function parseText(text: string): [number, number][] {
  return parseRows(text).map((v) => [v[0], v[1]] as [number, number]);
}

/** 胶路文本：每行 x, y，第 3 列可选，是从这一点出发那条边的 bulge（圆弧）。 */
const pathText = (pts: [number, number][], bulges: number[] = []) =>
  pts.map(([x, y], i) => (bulges[i] ? `${x}, ${y}, ${Number(bulges[i].toFixed(6))}` : `${x}, ${y}`)).join("\n");

function parsePathText(text: string): { points: [number, number][]; bulges: number[] } {
  const rows = parseRows(text);
  const bulges = rows.map((v) => v[2] ?? 0);
  return { points: rows.map((v) => [v[0], v[1]] as [number, number]), bulges: bulges.some((b) => b !== 0) ? bulges : [] };
}

function Num({ label, value, onChange, step = 0.1, hint }: { label: string; value: number; onChange: (v: number) => void; step?: number; hint?: string }) {
  return (
    <label className="field" title={hint}>
      <span>{label}</span>
      <input className="input mono" type="number" step={step} value={Number.isFinite(value) ? value : ""} onChange={(e) => onChange(Number(e.target.value))} />
    </label>
  );
}

function Section({ title, children, extra }: { title: string; children: ReactNode; extra?: ReactNode }) {
  return (
    <section className="rcp-section">
      <div className="panel-head">
        <h4 className="sub-title">{title}</h4>
        <span className="spacer" />
        {extra}
      </div>
      {children}
    </section>
  );
}

function LimitsTable({ label, value, onChange, widthDefault }: { label: string; value: SegmentLimits; onChange: (v: SegmentLimits) => void; widthDefault: JudgeParams }) {
  const row = (name: string, p: JudgeParams, set: (p: JudgeParams) => void) => (
    <tr>
      <td>{name}</td>
      {paramFields.map(([k]) => (
        <td key={k}>
          <input className="input mono" type="number" step={0.05} value={p[k]} onChange={(e) => set({ ...p, [k]: Number(e.target.value) })} />
        </td>
      ))}
    </tr>
  );
  return (
    <>
      {row(`${label} · 位置`, value.position, (position) => onChange({ ...value, position }))}
      {value.width ? (
        row(`${label} · 胶宽`, value.width, (width) => onChange({ ...value, width }))
      ) : (
        <tr>
          <td>{label} · 胶宽</td>
          <td colSpan={paramFields.length}>
            <button className="btn small" onClick={() => onChange({ ...value, width: widthDefault })}>
              启用胶宽判定
            </button>
          </td>
        </tr>
      )}
    </>
  );
}

export default function RecipeEditor({ initial, originalId, cameras, onSaved }: Props) {
  const [doc, setDoc] = useState<RecipeDoc>(initial);
  const [preview, setPreview] = useState<Recipe | null>(null);
  const [previewError, setPreviewError] = useState("");
  const [notice, setNotice] = useState<{ ok: boolean; text: string } | null>(null);
  const [polyText, setPolyText] = useState(initial.path.kind === "polyline" ? pathText(initial.path.points, initial.path.bulges) : "");
  const [shotsText, setShotsText] = useState(pointsText(initial.shots));

  useEffect(() => {
    const t = setTimeout(() => {
      recipeApi
        .preview(doc)
        .then((r) => {
          setPreview(r);
          setPreviewError("");
        })
        .catch((e) => setPreviewError(String(e)));
    }, 350);
    return () => clearTimeout(t);
  }, [doc]);

  const set = <K extends keyof RecipeDoc>(k: K, v: RecipeDoc[K]) => setDoc({ ...doc, [k]: v });
  const setPath = (p: PathSpec) => set("path", p);
  const follow = doc.mode === "follow";
  const f = doc.follow;
  const setFollow = (patch: Partial<FollowSpec>) => f && set("follow", { ...f, ...patch });
  const bead = f?.beadWidth ?? 2;
  const widthDefault: JudgeParams = { nominal: bead, tolUpper: 0.35 * bead, tolLower: 0.3 * bead, absMin: 0.4 * bead, absMax: 1.9 * bead, maxExcursionLen: 3 };

  const importFile = async (file: File) => {
    try {
      const r = await recipeApi.parsePath(await file.text(), file.name);
      setPolyText(pathText(r.points, r.bulges));
      setPath({ kind: "polyline", points: r.points, bulges: r.bulges, closed: r.closed, radius: doc.path.kind === "polyline" ? doc.path.radius : 0 });
      const arcs = r.bulges.filter((b) => b !== 0).length;
      setNotice({
        ok: true,
        text: `从 ${file.name} 读到 ${r.points.length} 个点${arcs ? `、${arcs} 段圆弧` : ""}${r.closed ? "，闭合" : ""}${r.note ? `。${r.note}` : ""}`,
      });
    } catch (e) {
      setNotice({ ok: false, text: String(e) });
    }
  };

  const save = async () => {
    try {
      const r = await recipeApi.save(doc, originalId);
      setNotice({ ok: true, text: `已保存 ${r.id} v${r.version}` });
      onSaved(r.id);
    } catch (e) {
      setNotice({ ok: false, text: String(e) });
    }
  };

  const summary = useMemo(() => {
    if (!preview) return "";
    const len = preview.segments.at(-1)?.s1 ?? 0;
    return `${preview.segments.length} 段 · 全长 ${len.toFixed(1)} mm · ${preview.points.x.length} 个测量点 · ${preview.closed ? "闭合" : "开放"}`;
  }, [preview]);

  return (
    <div className="rcp-editor">
      <div className="rcp-form">
        <div className="panel-toolbar">
          <h3 className="panel-title">
            {originalId ? `编辑 ${originalId}` : "新配方"} · {follow ? "随动" : "飞拍"}
            {originalId && <span className="muted mono"> v{doc.version}</span>}
          </h3>
          <button className="btn primary" onClick={save} disabled={!!previewError}>
            <Save size={15} />
            保存
          </button>
        </div>
        {notice && <div className={`notice ${notice.ok ? "ok" : "error"}`}>{notice.text}</div>}

        <Section title="基本">
          <div className="form-grid">
            <label className="field">
              <span>配方编号</span>
              <input className="input mono" value={doc.id} onChange={(e) => set("id", e.target.value.trim())} />
            </label>
            <label className="field">
              <span>名称</span>
              <input className="input" value={doc.name} onChange={(e) => set("name", e.target.value)} />
            </label>
            <Num label="产品代码（PLC 下发）" value={doc.productCode} step={1} onChange={(v) => set("productCode", v)} />
            <Num label="测量点间距（mm）" value={doc.spacing} onChange={(v) => set("spacing", v)} />
            <Num label="中值滤波窗口（点，奇数）" value={doc.filterWindow} step={2} onChange={(v) => set("filterWindow", v)} />
            <Num label="允许断胶长度（mm）" value={doc.maxGapLen} onChange={(v) => set("maxGapLen", v)} />
          </div>
        </Section>

        <Section
          title="胶路"
          extra={
            <div className="segmented">
              <button className={doc.path.kind === "roundedRect" ? "active" : ""} onClick={() => setPath({ kind: "roundedRect", width: 240, height: 140, radius: 20 })}>
                圆角矩形
              </button>
              <button
                className={doc.path.kind === "polyline" ? "active" : ""}
                onClick={() => {
                  const p = parsePathText(polyText);
                  setPath({ kind: "polyline", points: p.points.length >= 2 ? p.points : [[0, 0], [100, 0]], bulges: p.points.length >= 2 ? p.bulges : [], closed: false, radius: 0 });
                }}
              >
                折线 / 导入
              </button>
            </div>
          }
        >
          {doc.path.kind === "roundedRect" ? (
            <div className="form-grid">
              <Num label="宽（mm）" value={doc.path.width} step={1} onChange={(v) => doc.path.kind === "roundedRect" && setPath({ ...doc.path, width: v })} />
              <Num label="高（mm）" value={doc.path.height} step={1} onChange={(v) => doc.path.kind === "roundedRect" && setPath({ ...doc.path, height: v })} />
              <Num label="圆角半径（mm）" value={doc.path.radius} step={1} onChange={(v) => doc.path.kind === "roundedRect" && setPath({ ...doc.path, radius: v })} />
            </div>
          ) : (
            <div className="rcp-poly">
              <textarea
                className="input mono"
                rows={7}
                value={polyText}
                placeholder={"每行一个点：x, y（mm），可选第 3 列 bulge"}
                onChange={(e) => {
                  setPolyText(e.target.value);
                  if (doc.path.kind === "polyline") setPath({ ...doc.path, ...parsePathText(e.target.value) });
                }}
              />
              <div className="rcp-poly-side">
                <label className="btn">
                  <Upload size={15} />
                  导入 CSV / DXF
                  <input type="file" accept=".csv,.txt,.dxf" hidden onChange={(e) => e.target.files?.[0] && importFile(e.target.files[0])} />
                </label>
                <label className="check">
                  <input type="checkbox" checked={doc.path.closed} onChange={(e) => doc.path.kind === "polyline" && setPath({ ...doc.path, closed: e.target.checked })} />
                  闭合胶路
                </label>
                <Num label="拐角倒圆半径（mm）" value={doc.path.radius} onChange={(v) => doc.path.kind === "polyline" && setPath({ ...doc.path, radius: v })} />
                <p className="muted hint">
                  DXF 读 ENTITIES 段里的 LWPOLYLINE / POLYLINE（含圆弧与闭合）、CIRCLE，以及首尾相连的 LINE / ARC；有多条路径时取最长的。第 3 列 bulge = tan(圆心角/4)，正值逆时针；首尾重合的点自动当作闭合。
                </p>
              </div>
            </div>
          )}
        </Section>

        <Section title="判定限值（mm）">
          <div className="table-wrap">
            <table className="table rcp-limits">
              <thead>
                <tr>
                  <th />
                  {paramFields.map(([k, l]) => (
                    <th key={k}>{l}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                <LimitsTable label="直线段" value={doc.line} onChange={(v) => set("line", v)} widthDefault={widthDefault} />
                <LimitsTable label="拐角" value={doc.corner} onChange={(v) => set("corner", v)} widthDefault={widthDefault} />
              </tbody>
            </table>
          </div>
          <p className="muted hint">
            位置：{follow ? "胶条中线相对名义胶路的横向偏移（名义通常为 0）" : "内边到胶中线的距离"}。连续超出公差带超过"允许超差长度"判 NG，超出绝对限直接 NG。
          </p>
        </Section>

        {!follow && (
          <Section title="飞拍拍照点">
            <div className="form-grid">
              <label className="field">
                <span>触发方式</span>
                <select className="input" value={doc.triggerMode} onChange={(e) => set("triggerMode", e.target.value as RecipeDoc["triggerMode"])}>
                  <option value="fly">飞拍（位置比较触发）</option>
                  <option value="stop">停稳拍</option>
                </select>
              </label>
              <label className="field">
                <span>相机</span>
                <select className="input" value={doc.camera} onChange={(e) => set("camera", e.target.value)}>
                  {!cameras.some((c) => c.id === doc.camera) && <option value={doc.camera}>{doc.camera}（不在相机组里）</option>}
                  {cameras.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.name} · {c.id}
                    </option>
                  ))}
                </select>
              </label>
              <Num label="视野宽（mm）" value={doc.fov[0]} step={1} onChange={(v) => set("fov", [v, doc.fov[1]])} />
              <Num label="视野高（mm）" value={doc.fov[1]} step={1} onChange={(v) => set("fov", [doc.fov[0], v])} />
            </div>
            <label className="field">
              <span>拍照点中心（每行 x, y，按拍照顺序）</span>
              <textarea
                className="input mono"
                rows={5}
                value={shotsText}
                onChange={(e) => {
                  setShotsText(e.target.value);
                  set("shots", parseText(e.target.value));
                }}
              />
            </label>
          </Section>
        )}

        {follow && f && (
          <Section title="随动参数">
            <div className="rcp-cams">
              <span>参与相机</span>
              {[...cameras, ...f.cameras.filter((id) => !cameras.some((c) => c.id === id)).map((id) => ({ id, name: `${id}（不在相机组里）` }))].map((c) => (
                <label key={c.id} className="check">
                  <input
                    type="checkbox"
                    checked={f.cameras.includes(c.id)}
                    onChange={(e) =>
                      setFollow({
                        // 按相机组里的顺序排，与图像源页一致
                        cameras: e.target.checked
                          ? cameras.map((x) => x.id).filter((id) => id === c.id || f.cameras.includes(id))
                          : f.cameras.filter((id) => id !== c.id),
                      })
                    }
                  />
                  {c.name}
                </label>
              ))}
            </div>
            <div className="form-grid">
              <label className="field">
                <span>胶嘴定位</span>
                <select
                  className="input"
                  value={f.timing.kind}
                  onChange={(e) => setFollow({ timing: e.target.value === "plc" ? { kind: "plc", scale: 0.1 } : { kind: "timed", speedMmS: 80, delayMs: 300 } })}
                >
                  <option value="timed">按名义速度推算</option>
                  <option value="plc">PLC 进度寄存器 pathProgress</option>
                </select>
              </label>
              {f.timing.kind === "timed" ? (
                <>
                  <Num label="名义速度（mm/s）" value={f.timing.speedMmS} step={5} onChange={(v) => f.timing.kind === "timed" && setFollow({ timing: { ...f.timing, speedMmS: v } })} />
                  <Num label="布防后起步延时（ms）" value={f.timing.delayMs} step={50} onChange={(v) => f.timing.kind === "timed" && setFollow({ timing: { ...f.timing, delayMs: v } })} />
                </>
              ) : (
                <Num label="进度换算（mm / 单位）" value={f.timing.scale} step={0.01} onChange={(v) => f.timing.kind === "plc" && setFollow({ timing: { kind: "plc", scale: v } })} />
              )}
              <Num label="可测窗口 近端（mm）" value={f.nearMm} step={0.5} onChange={(v) => setFollow({ nearMm: v })} hint="离胶嘴这么近的胶条还被胶嘴挡着或未成型" />
              <Num label="可测窗口 远端（mm）" value={f.farMm} step={0.5} onChange={(v) => setFollow({ farMm: v })} />
              <Num label="测量步长（mm）" value={f.stepMm} step={0.5} onChange={(v) => setFollow({ stepMm: v })} hint="新进入窗口的胶条攒够这么长才测一帧" />
              <Num label="超行程（mm）" value={f.overrunMm} step={1} onChange={(v) => setFollow({ overrunMm: v })} hint="走完胶路后关胶再走的距离，让最后一段也进窗口" />
              <Num label="搜索半宽（mm）" value={f.searchMm} step={0.5} onChange={(v) => setFollow({ searchMm: v })} />
              <Num label="名义胶宽（mm）" value={f.beadWidth} step={0.1} onChange={(v) => setFollow({ beadWidth: v })} />
              <label className="field">
                <span>胶条极性</span>
                <select className="input" value={f.polarity} onChange={(e) => setFollow({ polarity: e.target.value as FollowSpec["polarity"] })}>
                  <option value="dark">比背景暗</option>
                  <option value="light">比背景亮</option>
                  <option value="any">不限</option>
                </select>
              </label>
              <Num label="最小边缘灰度差" value={f.minContrast} step={1} onChange={(v) => setFollow({ minContrast: v })} hint="低于它记为缺胶" />
              <Num label="起点区（mm）" value={f.startZoneMm} step={0.5} onChange={(v) => setFollow({ startZoneMm: v })} hint="起胶处（开放胶路还有收胶处）这么长内不判断胶" />
              <label className="check" title="起点处找胶条起点、拐角处用横向偏移反推沿程误差，校正按时间推算的胶嘴位置">
                <input type="checkbox" checked={f.autoSync} onChange={(e) => setFollow({ autoSync: e.target.checked })} />
                从图像同步胶嘴位置
              </label>
            </div>
          </Section>
        )}
      </div>

      <div className="rcp-preview">
        <div className="panel-head">
          <h4 className="sub-title">预览</h4>
          <span className="muted">{previewError ? "" : summary}</span>
        </div>
        {previewError && <div className="notice error">{previewError}</div>}
        {preview && <TrajectoryMap layout={preview} vis={new Array(preview.points.x.length).fill("none")} className="traj rcp-traj" />}
        {preview && (
          <div className="rcp-segs">
            {preview.segments.map((g) => (
              <span key={g.name} className={g.kind === "corner" ? "corner" : ""}>
                {g.name} <b className="mono">{(g.s1 - g.s0).toFixed(1)}</b>
              </span>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
