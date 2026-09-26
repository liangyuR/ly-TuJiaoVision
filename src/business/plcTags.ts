import type { TagPreset } from "../features/plc";

export const inspectionTagPresets: TagPreset[] = [
  { value: "partStart", label: "工件开始（PLC→PC）" },
  { value: "partEnd", label: "运动结束（PLC→PC）" },
  { value: "resultAck", label: "结果确认（PLC→PC）" },
  { value: "faultReset", label: "故障复位（PLC→PC）" },
  { value: "partSn", label: "工件序列号（PLC→PC）" },
  { value: "productCode", label: "产品代码（PLC→PC）" },
  { value: "shotCount", label: "计划拍照点数（PLC→PC）" },
  { value: "visionReady", label: "视觉就绪（PC→PLC）" },
  { value: "armed", label: "已布防（PC→PLC）" },
  { value: "busy", label: "检测中（PC→PLC）" },
  { value: "done", label: "结果有效（PC→PLC）" },
  { value: "resultCode", label: "结果码（PC→PLC）" },
  { value: "faultCode", label: "异常码（PC→PLC）" },
  { value: "resultSn", label: "结果 SN（PC→PLC）" },
];
