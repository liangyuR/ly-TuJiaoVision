import type { TagPreset } from "../features/plc";

export const inspectionTagPresets: TagPreset[] = [
  { value: "trigger", label: "检测触发" },
  { value: "busy", label: "检测中" },
  { value: "done", label: "检测完成" },
  { value: "resultOk", label: "结果 OK" },
  { value: "resultNg", label: "结果 NG" },
  { value: "resultCode", label: "结果代码" },
  { value: "glueWidth", label: "胶宽" },
  { value: "workpieceId", label: "工件编号" },
];
