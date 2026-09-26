export type CameraSource = "sim" | "mvs";

export interface CameraConfig {
  source: CameraSource;
  serial: string;
  triggerSource: "Line0" | "Software";
  triggerActivation: "RisingEdge" | "FallingEdge";
  triggerDelayUs: number;
  debouncerUs: number;
  exposureUs: number;
  gainDb: number;
  strobe: boolean;
  chunk: boolean;
}

export interface DeviceSummary {
  serial: string;
  model: string;
  userName: string;
  transport: "GigE" | "USB3";
  ip: string | null;
}

export interface CameraStatus {
  source: CameraSource;
  ready: boolean;
  message: string;
  device: DeviceSummary | null;
  sdkVersion: string | null;
  frames: number;
  fps: number;
  maxFps: number | null;
  lostPackets: number;
  warnings: string[];
}

export interface Frame {
  frameCounter: number;
  triggerCounter: number;
  lostPackets: number;
  ts: number;
}

export interface DryFrame {
  tMs: number;
  frameCounter: number;
  triggerCounter: number;
  lostPackets: number;
}
