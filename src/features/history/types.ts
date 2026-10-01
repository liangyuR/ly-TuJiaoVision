import type { FrameView, Judgement, Verdict } from "../cycle/types";

export interface HistoryQuery {
  from?: number | null;
  to?: number | null;
  verdicts?: Verdict[];
  sn?: string;
  recipeId?: string | null;
  offset?: number;
  limit?: number;
}

export interface PartSummary {
  id: number;
  ts: number;
  sn: number;
  recipeId: string | null;
  recipeVersion: number | null;
  recipeHash: string | null;
  triggerMode: string | null;
  verdict: Verdict;
  plcCode: number;
  faultCode: number;
  reason: string;
  drainMs: number | null;
  framesExpected: number;
  framesReceived: number;
  retestOf: number | null;
}

export interface VerdictCounts {
  ok: number;
  excursion: number;
  ng: number;
  err: number;
}

export interface HistoryPage {
  total: number;
  counts: VerdictCounts;
  items: PartSummary[];
}

export interface PartDetail {
  summary: PartSummary;
  judgement: Judgement;
  frames: FrameView[];
  triggers: number;
  softwareVersion: string;
  points: { d: number[]; w?: (number | null)[]; st: number[] } | null;
  retests: number[];
}

export interface KindOverride {
  tolUpper?: number;
  tolLower?: number;
  absMin?: number;
  absMax?: number;
  maxExcursionLen?: number;
}

export interface Overrides {
  maxGapLen?: number;
  filterWindow?: number;
  line: KindOverride;
  corner: KindOverride;
  /** 胶宽限值（只作用于配置了胶宽的段） */
  width?: KindOverride;
}

export interface RejudgeRequest {
  query: HistoryQuery;
  ids: number[];
  useCurrentRecipe: boolean;
  overrides: Overrides;
}

export interface RejudgeResult {
  total: number;
  skipped: number;
  limitHit: boolean;
  matrix: { from: Verdict; to: Verdict; count: number }[];
  changes: { id: number; sn: number; ts: number; from: Verdict; to: Verdict; reason: string }[];
}
