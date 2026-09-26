import type { CameraStatus } from "../camera/types";

export type Phase = "IDLE" | "VALIDATE" | "ACQUIRE" | "DRAIN" | "JUDGE" | "REPORT" | "RELEASE" | "FAULT";
export type FrameStatus = "waiting" | "measuring" | "done" | "locateFailed" | "error" | "missing";
export type Verdict = "OK" | "OK_WITH_EXCURSION" | "NG_POSITION" | "NG_ABSOLUTE" | "NG_GAP" | "ERR_INSPECT";
export type TriggerMode = "fly" | "stop";
export type ProductSource = "plc" | "manual";
export type Scenario = "normal" | "excursion" | "gap" | "lostFrame" | "locateFail" | "countMismatch" | "random";

export interface JudgeParams {
  nominal: number;
  tolUpper: number;
  tolLower: number;
  absMin: number;
  absMax: number;
  maxExcursionLen: number;
}

export interface Segment {
  name: string;
  kind: "line" | "corner";
  s0: number;
  s1: number;
  params: JudgeParams;
}

export interface Recipe {
  id: string;
  name: string;
  version: number;
  hash: string;
  productCode: number;
  triggerMode: TriggerMode;
  part: [number, number, number];
  fov: [number, number];
  shots: [number, number][];
  spacing: number;
  filterWindow: number;
  maxGapLen: number;
  segments: Segment[];
  points: { x: number[]; y: number[]; seg: number[]; k: number[] };
}

export interface RecipeSummary {
  id: string;
  name: string;
  version: number;
  hash: string;
  productCode: number;
  shotCount: number;
  triggerMode: TriggerMode;
}

export interface FrameView {
  status: FrameStatus;
  arrivedMs: number | null;
  frameCounter: number | null;
  triggerCounter: number | null;
  counterJump: boolean;
  score: number | null;
  points: number;
  gapPoints: number;
  ms: number | null;
}

export interface PartView {
  sn: number;
  recipeId: string;
  n: number;
  received: number;
  triggers: number;
  queue: number;
  filled: number;
  total: number;
  frames: FrameView[];
}

export interface SegmentResult {
  verdict: Verdict;
  min: number | null;
  max: number | null;
  excursionLen: number;
}

export interface GapRun {
  segment: number;
  s0: number;
  s1: number;
  len: number;
  frames: number[];
}

export interface Judgement {
  verdict: Verdict;
  plcCode: number;
  faultCode: number;
  reason: string;
  segments: SegmentResult[];
  gaps: GapRun[];
}

export interface ResultView extends Judgement {
  sn: number;
  recipeId: string | null;
  ts: number;
  drainMs: number | null;
}

export interface Snapshot {
  phase: Phase;
  since: number;
  fault: string | null;
  productSource: ProductSource;
  activeRecipeId: string | null;
  triggerMode: TriggerMode | null;
  part: PartView | null;
  result: ResultView | null;
  stats: { total: number; ok: number; ng: number; err: number };
  strayFrames: number;
  alarms: string[];
  camera: CameraStatus;
}

export interface LogLine {
  ts: number;
  level: "info" | "warn" | "err" | "ok" | "ng";
  ev: string;
  msg: string;
}

export interface Measured {
  sn: number;
  k: number;
  located: boolean;
  score: number;
  ms: number;
  idx: number[];
  d: number[];
  st: number[];
}

export interface Timeouts {
  armMs: number;
  motionMs: number;
  drainMs: number;
  procMs: number;
  ackMs: number;
}

export interface CycleSettings {
  productSource: ProductSource;
  manualRecipeId: string | null;
  timeouts: Timeouts;
  historyDays: number;
  lyflowCore: string | null;
  vision: boolean;
}

export interface SimStatus {
  running: boolean;
  continuous: boolean;
  parts: number;
  message: string;
}

/** 测量点显示状态 */
export type PointVis = "none" | "ok" | "exc" | "ng" | "gap" | "inv" | "miss";
