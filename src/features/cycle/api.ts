import { invoke, isTauri } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { subscribe } from "../plc";
import type { CycleSettings, ImportedPath, InspectMode, LogLine, Measured, Recipe, RecipeDoc, RecipeSummary, Scenario, SimStatus, Snapshot } from "./types";

function call<T>(cmd: string, args: Record<string, unknown> | undefined, fallback: () => T): Promise<T> {
  if (!isTauri()) return Promise.resolve(fallback());
  return invoke<T>(cmd, args);
}

const defaultSettings: CycleSettings = {
  productSource: "plc",
  manualRecipeId: null,
  timeouts: { armMs: 200, motionMs: 30000, drainMs: 1000, procMs: 3000, ackMs: 5000 },
  historyDays: 180,
  lyflowCore: null,
  vision: false,
  followVision: true,
  record: "off",
  recordKeep: 100,
  recordMaxGb: 20,
};

export const cycleApi = {
  snapshot: () => call<Snapshot | null>("cycle_snapshot", undefined, () => null),
  logs: () => call<LogLine[]>("cycle_logs", undefined, () => []),
  partData: () => call<Measured[]>("cycle_part_data", undefined, () => []),
  recipes: () => call<RecipeSummary[]>("cycle_recipes", undefined, () => []),
  layout: (recipeId: string) => call<Recipe | null>("cycle_layout", { recipeId }, () => null),
  getSettings: () => call<CycleSettings>("cycle_get_settings", undefined, () => structuredClone(defaultSettings)),
  saveSettings: (settings: CycleSettings) => call<void>("cycle_save_settings", { settings }, () => undefined),
  selectRecipe: (recipeId: string) => call<void>("cycle_select_recipe", { recipeId }, () => undefined),
  reset: () => call<void>("cycle_reset", undefined, () => undefined),
  simStatus: () => call<SimStatus>("sim_status", undefined, () => ({ running: false, continuous: false, parts: 0, message: "" })),
  simStart: (recipeId: string, scenario: Scenario, continuous: boolean) =>
    call<void>("sim_start", { recipeId, scenario, continuous }, () => undefined),
  simStop: () => call<void>("sim_stop", undefined, () => undefined),
};

export const recipeApi = {
  list: () => call<{ recipes: RecipeSummary[]; errors: string[] }>("recipe_list", undefined, () => ({ recipes: [], errors: [] })),
  doc: (id: string) => call<RecipeDoc>("recipe_doc", { id }, () => Promise.reject("非桌面环境") as never),
  preview: (doc: RecipeDoc) => call<Recipe>("recipe_preview", { doc }, () => Promise.reject("非桌面环境") as never),
  template: (mode: InspectMode) => call<RecipeDoc>("recipe_template", { mode }, () => Promise.reject("非桌面环境") as never),
  save: (doc: RecipeDoc, originalId: string | null) =>
    call<RecipeSummary>("recipe_save", { doc, originalId }, () => Promise.reject("非桌面环境") as never).then((r) => {
      layoutCache.clear();
      notifyRecipes();
      return r;
    }),
  remove: (id: string) =>
    call<void>("recipe_delete", { id }, () => undefined).then(() => {
      layoutCache.clear();
      notifyRecipes();
    }),
  parsePath: (text: string, fileName: string) =>
    call<ImportedPath>("recipe_parse_path", { text, fileName }, () => ({ points: [], bulges: [], closed: false, note: null })),
};

const layoutCache = new Map<string, Promise<Recipe | null>>();
const recipeListeners = new Set<() => void>();
function notifyRecipes() {
  recipeListeners.forEach((f) => f());
}

export function useLayout(recipeId: string | null | undefined, version?: number) {
  const [layout, setLayout] = useState<Recipe | null>(null);
  const [gen, setGen] = useState(0);
  useEffect(() => {
    const f = () => setGen((g) => g + 1);
    recipeListeners.add(f);
    return () => void recipeListeners.delete(f);
  }, []);
  useEffect(() => {
    if (!recipeId) return setLayout(null);
    if (!layoutCache.has(recipeId)) layoutCache.set(recipeId, cycleApi.layout(recipeId).catch(() => null));
    let alive = true;
    layoutCache.get(recipeId)!.then((l) => alive && setLayout(l));
    return () => {
      alive = false;
    };
  }, [recipeId, version, gen]);
  return layout;
}

export function useRecipes() {
  const [recipes, setRecipes] = useState<RecipeSummary[]>([]);
  useEffect(() => {
    const load = () => cycleApi.recipes().then(setRecipes).catch(() => setRecipes([]));
    load();
    recipeListeners.add(load);
    return () => void recipeListeners.delete(load);
  }, []);
  return recipes;
}

export function useCycle() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [logs, setLogs] = useState<LogLine[]>([]);
  const [measured, setMeasured] = useState<Measured[]>([]);

  useEffect(() => {
    cycleApi.snapshot().then((s) => s && setSnapshot(s));
    cycleApi.logs().then(setLogs);
    cycleApi.partData().then(setMeasured);
    const offs = [
      subscribe<Snapshot>("cycle://snapshot", setSnapshot),
      subscribe<LogLine>("cycle://log", (line) => setLogs((prev) => [...prev.slice(-199), line])),
      subscribe<Measured>("cycle://frame", (m) =>
        setMeasured((prev) => (prev.length && prev[0].sn !== m.sn ? [m] : [...prev, m])),
      ),
    ];
    return () => offs.forEach((off) => off());
  }, []);

  const sn = snapshot?.part?.sn;
  const current = sn === undefined ? [] : measured.filter((m) => m.sn === sn);
  return { snapshot, logs, measured: current };
}

export function useSimStatus() {
  const [status, setStatus] = useState<SimStatus | null>(null);
  useEffect(() => {
    cycleApi.simStatus().then(setStatus);
    return subscribe<SimStatus>("sim://status", setStatus);
  }, []);
  return status;
}
