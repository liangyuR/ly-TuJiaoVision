import { linkStateLabels } from "../meta";
import type { LinkState } from "../types";

export default function PlcStatusBadge({ state }: { state: LinkState | undefined }) {
  const s = state ?? "disconnected";
  return <span className={`badge link-${s}`}>{linkStateLabels[s]}</span>;
}
