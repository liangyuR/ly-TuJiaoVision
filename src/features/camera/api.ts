import { invoke, isTauri } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { subscribe } from "../plc";
import type { CameraConfig, CameraStatus, DeviceSummary, DryFrame, Frame, PreviewImage, RecordEntry } from "./types";

function call<T>(cmd: string, args: Record<string, unknown> | undefined, fallback: () => T): Promise<T> {
  if (!isTauri()) return Promise.resolve(fallback());
  return invoke<T>(cmd, args);
}

export const defaultCameraConfig: CameraConfig = {
  id: "",
  name: "相机",
  source: "sim",
  serial: "",
  acquisition: "triggered",
  fps: 20,
  triggerSource: "Line0",
  triggerActivation: "RisingEdge",
  triggerDelayUs: 0,
  debouncerUs: 5,
  exposureUs: 60,
  gainDb: 6,
  strobe: true,
  chunk: true,
  replayDir: "",
  replayChannel: 0,
  follow: null,
};

/** 解析 camera_preview 的二进制：16 字节头（缩略图宽高、原图宽高）+ 灰度像素。 */
function decodePreview(buf: ArrayBuffer): PreviewImage | null {
  if (buf.byteLength < 16) return null;
  const v = new DataView(buf);
  const [width, height, fullWidth, fullHeight] = [0, 4, 8, 12].map((o) => v.getUint32(o, true));
  const px = new Uint8Array(buf, 16);
  const data = new ImageData(width, height);
  for (let i = 0; i < width * height; i++) {
    const g = px[i];
    data.data[i * 4] = data.data[i * 4 + 1] = data.data[i * 4 + 2] = g;
    data.data[i * 4 + 3] = 255;
  }
  return { width, height, fullWidth, fullHeight, data };
}

export const cameraApi = {
  rigStatus: () => call<CameraStatus[]>("camera_rig_status", undefined, () => []),
  rigConfig: () => call<CameraConfig[]>("camera_rig_config", undefined, () => [structuredClone(defaultCameraConfig)]),
  saveConfig: (cam: number, config: CameraConfig) => call<string[]>("camera_save_config", { cam, config }, () => []),
  add: (config: CameraConfig) => call<number>("camera_add", { config }, () => 0),
  remove: (cam: number) => call<void>("camera_remove", { cam }, () => undefined),
  listDevices: () => call<DeviceSummary[]>("camera_list_devices", undefined, () => []),
  preview: (cam: number) => call<ArrayBuffer>("camera_preview", { cam }, () => new ArrayBuffer(0)).then(decodePreview),
  softTrigger: (cam: number) => call<void>("camera_soft_trigger", { cam }, () => undefined),
  dryRunStart: () => call<void>("camera_dry_run_start", undefined, () => undefined),
  dryRunGet: () => call<DryFrame[] | null>("camera_dry_run_get", undefined, () => null),
  dryRunStop: () => call<DryFrame[]>("camera_dry_run_stop", undefined, () => []),
  records: () => call<{ root: string; items: RecordEntry[] }>("records_list", undefined, () => ({ root: "", items: [] })),
};

/** 相机组状态：每秒刷新，有帧到达时也刷新。 */
export function useRigStatus() {
  const [statuses, setStatuses] = useState<CameraStatus[]>([]);
  const [lastFrame, setLastFrame] = useState<Record<number, Frame>>({});
  useEffect(() => {
    let pending = false;
    const refresh = () => {
      if (pending) return;
      pending = true;
      cameraApi
        .rigStatus()
        .then(setStatuses)
        .catch(() => setStatuses([]))
        .finally(() => (pending = false));
    };
    refresh();
    const timer = setInterval(refresh, 1000);
    const off = subscribe<Frame>("camera://frame", (f) => {
      setLastFrame((prev) => ({ ...prev, [f.cam]: f }));
      refresh();
    });
    return () => {
      clearInterval(timer);
      off();
    };
  }, []);
  return { statuses, lastFrame };
}

/**
 * 某台相机的最近一帧缩略图：有新帧（frameKey 变了）才取，两次之间至少隔 intervalMs。
 * 已经发出去的请求不因为又来了新帧而作废，否则帧来得比取图快时画面永远不更新。
 */
export function usePreview(cam: number, frameKey: unknown, intervalMs = 250) {
  const [img, setImg] = useState<PreviewImage | null>(null);
  const lastAt = useRef(0);
  const pending = useRef(false);
  const current = useRef({ cam, mounted: true });
  current.current.cam = cam;
  useEffect(() => {
    current.current.mounted = true;
    return () => {
      current.current.mounted = false;
    };
  }, []);
  useEffect(() => {
    if (pending.current) return;
    const t = setTimeout(
      () => {
        pending.current = true;
        lastAt.current = Date.now();
        cameraApi
          .preview(cam)
          .then((p) => p && current.current.mounted && current.current.cam === cam && setImg(p))
          .catch(() => undefined)
          .finally(() => (pending.current = false));
      },
      Math.max(0, intervalMs - (Date.now() - lastAt.current)),
    );
    return () => clearTimeout(t);
  }, [cam, frameKey, intervalMs]);
  return img;
}

/** 缩略图画到 canvas 上：返回图像（含原图尺寸）与要挂到 canvas 上的 ref。 */
export function usePreviewCanvas(cam: number, frameKey: unknown, intervalMs = 250) {
  const img = usePreview(cam, frameKey, intervalMs);
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const el = canvas.current;
    if (!el || !img) return;
    el.width = img.width;
    el.height = img.height;
    el.getContext("2d")?.putImageData(img.data, 0, 0);
  }, [img]);
  return { img, canvas };
}
