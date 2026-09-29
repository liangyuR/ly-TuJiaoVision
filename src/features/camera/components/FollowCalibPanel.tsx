import { invoke, isTauri } from "@tauri-apps/api/core";
import { useEffect, useRef, useState, type MouseEvent } from "react";
import { Crosshair, MoveUpRight, Ruler, ScanLine } from "lucide-react";
import { cameraApi, usePreviewCanvas } from "../api";
import type { CameraConfig, FollowCalib, Frame } from "../types";

type Tool = "nozzle" | "direction" | "scale";
type Polarity = "dark" | "light" | "any";

interface ProbePoint {
  l: number;
  offset: number | null;
  width: number | null;
  st: number;
  px: [number, number];
}

const rad = (d: number) => (d * Math.PI) / 180;
const deg = (r: number) => (r * 180) / Math.PI;

/** 与后端 FollowCalib::dir_to_img 一致：先镜像，再按 angleDeg 顺时针转。 */
function dirToImg(c: FollowCalib, [x, y]: [number, number]): [number, number] {
  const xm = c.mirror ? -x : x;
  const [s, co] = [Math.sin(rad(c.angleDeg)), Math.cos(rad(c.angleDeg))];
  return [xm * co - y * s, xm * s + y * co];
}

/** 三目演示的默认标定：胶嘴在图像下方正中，三台相机方位差 120°。 */
function defaultCalib(cam: number, size: [number, number]): FollowCalib {
  return { nozzle: [size[0] / 2, size[1] * 0.88], angleDeg: 120 * cam, mirror: false, mmPerPx: 0.05, maskPx: 60, imageSize: size };
}

interface Props {
  cam: number;
  config: CameraConfig;
  frame: Frame | undefined;
  onSaved: (c: CameraConfig) => void;
}

export default function FollowCalibPanel({ cam, config, frame, onSaved }: Props) {
  const { img, canvas } = usePreviewCanvas(cam, frame?.frameCounter);
  const svg = useRef<SVGSVGElement>(null);
  const size: [number, number] = img ? [img.fullWidth, img.fullHeight] : (config.follow?.imageSize ?? [1280, 1024]);
  const [calib, setCalib] = useState<FollowCalib>(config.follow ?? defaultCalib(cam, size));
  const [tool, setTool] = useState<Tool>("nozzle");
  const [travelDeg, setTravelDeg] = useState(0);
  const [scalePts, setScalePts] = useState<[number, number][]>([]);
  const [scaleMm, setScaleMm] = useState(10);
  const [probe, setProbe] = useState({ polarity: "dark" as Polarity, beadWidth: 2, searchMm: 4, nearMm: 3, farMm: 18 });
  const [points, setPoints] = useState<ProbePoint[]>([]);
  const [notice, setNotice] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    if (img && (calib.imageSize[0] !== img.fullWidth || calib.imageSize[1] !== img.fullHeight)) {
      setCalib((c) => ({ ...c, imageSize: [img.fullWidth, img.fullHeight] }));
    }
  }, [img, calib.imageSize]);

  // 标定时机器人沿 travelDeg 方向走，胶条在胶嘴身后，即工件坐标里的 −travel 方向
  const behind: [number, number] = [-Math.cos(rad(travelDeg)), -Math.sin(rad(travelDeg))];
  const beadImg = dirToImg(calib, behind);
  const beadDeg = deg(Math.atan2(beadImg[1], beadImg[0]));

  const toImage = (e: MouseEvent<SVGSVGElement>): [number, number] | null => {
    const el = svg.current;
    const m = el?.getScreenCTM();
    if (!el || !m) return null;
    const p = new DOMPoint(e.clientX, e.clientY).matrixTransform(m.inverse());
    return [p.x, p.y];
  };

  const click = (e: MouseEvent<SVGSVGElement>) => {
    const p = toImage(e);
    if (!p) return;
    if (tool === "nozzle") setCalib({ ...calib, nozzle: [Math.round(p[0]), Math.round(p[1])] });
    else if (tool === "direction") {
      setCalib({ ...calib, angleDeg: angleFor(deg(Math.atan2(p[1] - calib.nozzle[1], p[0] - calib.nozzle[0]))) });
    } else {
      const next = scalePts.length >= 2 ? [p] : [...scalePts, p];
      setScalePts(next);
      if (next.length === 2) {
        const px = Math.hypot(next[1][0] - next[0][0], next[1][1] - next[0][1]);
        if (px > 3) setCalib({ ...calib, mmPerPx: Number((scaleMm / px).toFixed(5)) });
      }
    }
  };

  /** 胶条在图像里的方向 theta（度）→ 图像方位 angleDeg，按标定时的走向换算。 */
  const angleFor = (thetaDeg: number) => {
    const m: [number, number] = [calib.mirror ? -behind[0] : behind[0], behind[1]];
    return Number(deg(rad(thetaDeg) - Math.atan2(m[1], m[0])).toFixed(2));
  };

  const runProbe = async (auto = false) => {
    setNotice(null);
    if (!isTauri()) return;
    try {
      const r = await invoke<{ points: ProbePoint[]; directionDeg: number }>("teach_follow_probe", {
        request: { cam, calib, directionDeg: auto ? null : beadDeg, ...probe },
      });
      setPoints(r.points);
      if (auto) setCalib((c) => ({ ...c, angleDeg: angleFor(r.directionDeg) }));
      const ok = r.points.filter((p) => p.st === 0);
      const w = ok.map((p) => p.width ?? 0);
      const o = ok.map((p) => p.offset ?? 0);
      const mean = (a: number[]) => a.reduce((x, y) => x + y, 0) / Math.max(a.length, 1);
      const sd = (a: number[]) => Math.sqrt(mean(a.map((v) => (v - mean(a)) ** 2)));
      setNotice({
        ok: ok.length >= r.points.length * 0.8,
        text: `测到 ${ok.length}/${r.points.length} 点 · 胶宽 ${mean(w).toFixed(2)} ± ${sd(w).toFixed(2)} mm · 偏移 ${mean(o).toFixed(2)} ± ${sd(o).toFixed(2)} mm`,
      });
    } catch (e) {
      setNotice({ ok: false, text: String(e) });
    }
  };

  const save = async () => {
    try {
      const next = { ...config, follow: calib };
      await cameraApi.saveConfig(cam, next);
      onSaved(next);
      setNotice({ ok: true, text: "随动标定已保存" });
    } catch (e) {
      setNotice({ ok: false, text: String(e) });
    }
  };

  const num = (label: string, value: number, onChange: (v: number) => void, step = 1) => (
    <label className="field">
      <span>{label}</span>
      <input className="input mono" type="number" step={step} value={value} onChange={(e) => onChange(Number(e.target.value))} />
    </label>
  );
  const arrowLen = Math.min(size[0], size[1]) * 0.35;
  const [nx, ny] = calib.nozzle;

  return (
    <div className="panel">
      <div className="panel-head">
        <h3 className="panel-title">随动标定</h3>
        <span className="muted">相机相对胶嘴：胶嘴位置、图像方位、像素当量（属于相机工位，换型不重标）</span>
        <span className="spacer" />
        <button className="btn" onClick={() => setCalib(defaultCalib(cam, size))} title="三目演示：胶嘴在下方正中，相机方位 120° 均布">
          默认三目
        </button>
        <button className="btn primary" onClick={save}>
          保存标定
        </button>
      </div>
      <div className="segmented">
        <button className={tool === "nozzle" ? "active" : ""} onClick={() => setTool("nozzle")}>
          <Crosshair size={14} /> 点胶嘴
        </button>
        <button className={tool === "direction" ? "active" : ""} onClick={() => setTool("direction")}>
          <MoveUpRight size={14} /> 点胶条方向
        </button>
        <button className={tool === "scale" ? "active" : ""} onClick={() => setTool("scale")}>
          <Ruler size={14} /> 量比例
        </button>
      </div>
      <p className="muted hint">
        {tool === "nozzle" && "在图上点胶嘴中心。"}
        {tool === "direction" && "机器人沿下面填的方向涂一段直线胶，在图上点胶条远离胶嘴的方向上任一点。"}
        {tool === "scale" && "在图上点两个相距已知长度的点（如胶嘴外径两侧、标尺刻度），填入实际距离。"}
      </p>
      <div className="calib-view">
        <canvas ref={canvas} style={{ display: img ? "block" : "none" }} />
        {!img && <span className="muted">这台相机还没有图像：模拟 / 回放相机在节拍中出图，回放相机也可在上方“下一张”</span>}
        <svg ref={svg} viewBox={`0 0 ${size[0]} ${size[1]}`} onClick={click}>
          <circle cx={nx} cy={ny} r={calib.maskPx} fill="rgba(56,189,248,0.08)" stroke="#38bdf8" strokeDasharray="6 5" vectorEffect="non-scaling-stroke" />
          <line x1={nx - 12} x2={nx + 12} y1={ny} y2={ny} stroke="#38bdf8" vectorEffect="non-scaling-stroke" />
          <line x1={nx} x2={nx} y1={ny - 12} y2={ny + 12} stroke="#38bdf8" vectorEffect="non-scaling-stroke" />
          <line
            x1={nx}
            y1={ny}
            x2={nx + beadImg[0] * arrowLen}
            y2={ny + beadImg[1] * arrowLen}
            stroke="#facc15"
            strokeWidth={2}
            strokeDasharray="10 6"
            vectorEffect="non-scaling-stroke"
          />
          {[probe.nearMm, probe.farMm].map((l) => (
            <circle key={l} cx={nx} cy={ny} r={l / calib.mmPerPx} fill="none" stroke="rgba(250,204,21,0.35)" vectorEffect="non-scaling-stroke" />
          ))}
          {scalePts.map(([x, y], i) => (
            <circle key={i} cx={x} cy={y} r={5} fill="#f472b6" vectorEffect="non-scaling-stroke" />
          ))}
          {scalePts.length === 2 && <line x1={scalePts[0][0]} y1={scalePts[0][1]} x2={scalePts[1][0]} y2={scalePts[1][1]} stroke="#f472b6" vectorEffect="non-scaling-stroke" />}
          {points.map((p) => (
            <circle key={p.l} cx={p.px[0]} cy={p.px[1]} r={3} fill={p.st === 0 ? "#22c55e" : "#ef4444"} vectorEffect="non-scaling-stroke" />
          ))}
        </svg>
      </div>
      <div className="calib-row four">
        {num("胶嘴 x（px）", nx, (v) => setCalib({ ...calib, nozzle: [v, ny] }))}
        {num("胶嘴 y（px）", ny, (v) => setCalib({ ...calib, nozzle: [nx, v] }))}
        {num("遮挡半径（px）", calib.maskPx, (v) => setCalib({ ...calib, maskPx: v }))}
        {num("像素当量（mm/px）", calib.mmPerPx, (v) => setCalib({ ...calib, mmPerPx: v }), 0.001)}
        {num("图像方位（°）", calib.angleDeg, (v) => setCalib({ ...calib, angleDeg: v }), 0.5)}
        {num("标定时走向（°）", travelDeg, setTravelDeg, 5)}
        {num("量比例：实际距离（mm）", scaleMm, setScaleMm, 0.5)}
        <label className="check">
          <input type="checkbox" checked={calib.mirror} onChange={(e) => setCalib({ ...calib, mirror: e.target.checked })} />
          图像相对工件坐标镜像
        </label>
      </div>
      <div className="panel-head">
        <h4 className="sub-title">试测</h4>
        <span className="muted">假设胶嘴身后是一条沿黄色虚线的直胶条，逐 0.5 mm 跑卡尺</span>
        <span className="spacer" />
        <button className="btn" onClick={() => runProbe(true)} disabled={!img} title="在当前帧上转一圈找胶条方向，按标定时走向换算图像方位">
          自动找方向
        </button>
        <button className="btn" onClick={() => runProbe()} disabled={!img}>
          <ScanLine size={15} />
          在当前帧试测
        </button>
      </div>
      <div className="calib-row four">
        <label className="field">
          <span>胶条极性</span>
          <select className="input" value={probe.polarity} onChange={(e) => setProbe({ ...probe, polarity: e.target.value as Polarity })}>
            <option value="dark">比背景暗</option>
            <option value="light">比背景亮</option>
            <option value="any">不限</option>
          </select>
        </label>
        {num("名义胶宽（mm）", probe.beadWidth, (v) => setProbe({ ...probe, beadWidth: v }), 0.1)}
        {num("搜索半宽（mm）", probe.searchMm, (v) => setProbe({ ...probe, searchMm: v }), 0.5)}
        {num("窗口 近 / 远（mm）", probe.nearMm, (v) => setProbe({ ...probe, nearMm: v }), 0.5)}
        {num("", probe.farMm, (v) => setProbe({ ...probe, farMm: v }), 0.5)}
      </div>
      {notice && <div className={`notice ${notice.ok ? "ok" : "error"}`}>{notice.text}</div>}
    </div>
  );
}
