import { PlcLogPage, PlcPage } from "../features/plc";
import { inspectionTagPresets } from "../business/plcTags";

export function PlcSettingsPage() {
  return <PlcPage tagPresets={inspectionTagPresets} />;
}

export function PlcLogsPage() {
  return <PlcLogPage />;
}
