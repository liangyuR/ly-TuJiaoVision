import { useEffect, useState } from "react";
import { Play, Repeat, Square } from "lucide-react";
import { plcApi, usePlcStatus } from "../../plc";
import { cycleApi, useRecipes, useSimStatus } from "../api";
import type { Scenario } from "../types";

const scenarios: [Scenario, string][] = [
  ["normal", "正常件"],
  ["excursion", "局部超差（允许）"],
  ["gap", "跨帧断胶"],
  ["lostFrame", "传输丢帧"],
  ["locateFail", "定位失败"],
  ["countMismatch", "拍照点数不一致"],
  ["random", "随机（连续运行用）"],
];

export default function SimControls({ compact = false }: { compact?: boolean }) {
  const recipes = useRecipes();
  const status = useSimStatus();
  const plc = usePlcStatus();
  const [isSim, setIsSim] = useState(false);
  const [recipeId, setRecipeId] = useState("");
  const [scenario, setScenario] = useState<Scenario>("normal");
  const [error, setError] = useState("");

  useEffect(() => {
    plcApi.getConfig().then((c) => setIsSim(c.connection.protocol === "simulator"));
  }, [plc?.state]);
  useEffect(() => {
    if (!recipeId && recipes.length) setRecipeId(recipes[0].id);
  }, [recipes, recipeId]);

  if (!isSim) {
    return compact ? null : <p className="muted">模拟节拍需要把 PLC 协议设为“模拟器”并连接。</p>;
  }

  const start = (continuous: boolean) => {
    setError("");
    cycleApi.simStart(recipeId, scenario, continuous).catch((e) => setError(String(e)));
  };
  const running = !!status?.running;

  return (
    <div className={`sim-controls${compact ? " compact" : ""}`}>
      {compact && <span className="sim-label">模拟节拍</span>}
      <select id="sim-recipe" className="input" value={recipeId} onChange={(e) => setRecipeId(e.target.value)} disabled={running}>
        {recipes.map((r) => (
          <option key={r.id} value={r.id}>
            {r.id} · 代码 {r.productCode} · N={r.shotCount}
          </option>
        ))}
      </select>
      <select id="sim-scenario" className="input" value={scenario} onChange={(e) => setScenario(e.target.value as Scenario)} disabled={running}>
        {scenarios.map(([v, label]) => (
          <option key={v} value={v}>
            {label}
          </option>
        ))}
      </select>
      <button className="btn primary" onClick={() => start(false)} disabled={running || !recipeId}>
        <Play size={15} />
        运行一件
      </button>
      <button className="btn" onClick={() => start(true)} disabled={running || !recipeId}>
        <Repeat size={15} />
        连续运行
      </button>
      <button className="btn" onClick={() => cycleApi.simStop()} disabled={!status?.continuous}>
        <Square size={15} />
        本件后停止
      </button>
      <span className={`sim-msg${error ? " c-ng" : ""}`}>{error || status?.message}</span>
    </div>
  );
}
