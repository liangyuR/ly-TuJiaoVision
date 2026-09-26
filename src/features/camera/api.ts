import { invoke, isTauri } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { subscribe } from "../plc";
import type { CameraConfig, CameraStatus, DeviceSummary, DryFrame, Frame } from "./types";

function call<T>(cmd: string, args: Record<string, unknown> | undefined, fallback: () => T): Promise<T> {
  if (!isTauri()) return Promise.resolve(fallback());
  return invoke<T>(cmd, args);
}

const defaultConfig: CameraConfig = {
  source: "sim",
  serial: "",
  triggerSource: "Line0",
  triggerActivation: "RisingEdge",
  triggerDelayUs: 0,
  debouncerUs: 5,
  exposureUs: 60,
  gainDb: 6,
  strobe: true,
  chunk: true,
};

export const cameraApi = {
  status: () => call<CameraStatus | null>("camera_status", undefined, () => null),
  getConfig: () => call<CameraConfig>("camera_get_config", undefined, () => structuredClone(defaultConfig)),
  saveConfig: (config: CameraConfig) => call<string[]>("camera_save_config", { config }, () => []),
  listDevices: () => call<DeviceSummary[]>("camera_list_devices", undefined, () => []),
  preview: () => call<ArrayBuffer>("camera_preview", undefined, () => new ArrayBuffer(0)),
  softTrigger: () => call<void>("camera_soft_trigger", undefined, () => undefined),
  dryRunStart: () => call<void>("camera_dry_run_start", undefined, () => undefined),
  dryRunGet: () => call<DryFrame[] | null>("camera_dry_run_get", undefined, () => null),
  dryRunStop: () => call<DryFrame[]>("camera_dry_run_stop", undefined, () => []),
};

/** 相机状态：每帧与每秒刷新一次。 */
export function useCameraStatus() {
  const [status, setStatus] = useState<CameraStatus | null>(null);
  const [lastFrame, setLastFrame] = useState<Frame | null>(null);
  useEffect(() => {
    const refresh = () => cameraApi.status().then(setStatus).catch(() => setStatus(null));
    refresh();
    const timer = setInterval(refresh, 1000);
    const off = subscribe<Frame>("camera://frame", (f) => {
      setLastFrame(f);
      refresh();
    });
    return () => {
      clearInterval(timer);
      off();
    };
  }, []);
  return { status, lastFrame };
}
