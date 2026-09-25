import { protocols, s7CpuPresets } from "../meta";
import type { ConnectionConfig, McOptions, ModbusOptions, ProtocolKind, S7ConnectionType, S7Options } from "../types";

interface ConnectionFormProps {
  value: ConnectionConfig;
  onChange: (value: ConnectionConfig) => void;
}

const hex = (n: number | null, width = 4) => (n === null ? "" : `0x${n.toString(16).toUpperCase().padStart(width, "0")}`);

function parseHex(text: string): number | null {
  const t = text.trim();
  if (!t) return null;
  const n = parseInt(t.replace(/^0x/i, ""), 16);
  return Number.isNaN(n) ? null : n;
}

export default function ConnectionForm({ value: c, onChange }: ConnectionFormProps) {
  const set = (patch: Partial<ConnectionConfig>) => onChange({ ...c, ...patch });
  const setModbus = (patch: Partial<ModbusOptions>) => set({ modbus: { ...c.modbus, ...patch } });
  const setS7 = (patch: Partial<S7Options>) => set({ s7: { ...c.s7, ...patch } });
  const setMc = (patch: Partial<McOptions>) => set({ mc: { ...c.mc, ...patch } });
  const isSim = c.protocol === "simulator";
  const num = (v: string) => Number(v);

  const changeProtocol = (protocol: ProtocolKind) =>
    set({ protocol, port: protocols[protocol].defaultPort || c.port });

  const cpu = s7CpuPresets.find(
    (p) => p.rack === c.s7.rack && p.slot === c.s7.slot && p.localTsap === c.s7.localTsap && p.remoteTsap === c.s7.remoteTsap,
  );

  return (
    <div className="form-grid">
      <label className="field">
        <span>协议</span>
        <select className="input" value={c.protocol} onChange={(e) => changeProtocol(e.target.value as ProtocolKind)}>
          {Object.entries(protocols).map(([k, v]) => (
            <option key={k} value={k}>
              {v.label}
            </option>
          ))}
        </select>
      </label>
      <label className="field">
        <span>IP / 主机名</span>
        <input className="input mono" disabled={isSim} value={c.host} onChange={(e) => set({ host: e.target.value })} />
      </label>
      <label className="field">
        <span>端口</span>
        <input className="input mono" type="number" disabled={isSim} value={c.port} onChange={(e) => set({ port: num(e.target.value) })} />
      </label>

      {c.protocol === "modbusTcp" && (
        <label className="field">
          <span>站号 (Unit ID)</span>
          <input className="input mono" type="number" min={0} max={255} value={c.modbus.unitId} onChange={(e) => setModbus({ unitId: num(e.target.value) })} />
        </label>
      )}

      {c.protocol === "s7" && (
        <>
          <label className="field">
            <span>CPU 型号</span>
            <select
              className="input"
              value={cpu?.key ?? "custom"}
              onChange={(e) => {
                const p = s7CpuPresets.find((x) => x.key === e.target.value);
                if (p) setS7({ rack: p.rack, slot: p.slot, localTsap: p.localTsap, remoteTsap: p.remoteTsap });
              }}
            >
              {s7CpuPresets.map((p) => (
                <option key={p.key} value={p.key}>
                  {p.label}
                </option>
              ))}
              <option value="custom" disabled>
                自定义
              </option>
            </select>
          </label>
          <label className="field">
            <span>机架 (Rack)</span>
            <input className="input mono" type="number" min={0} max={7} value={c.s7.rack} onChange={(e) => setS7({ rack: num(e.target.value) })} />
          </label>
          <label className="field">
            <span>插槽 (Slot)</span>
            <input className="input mono" type="number" min={0} max={31} value={c.s7.slot} onChange={(e) => setS7({ slot: num(e.target.value) })} />
          </label>
          <label className="field">
            <span>连接类型</span>
            <select className="input" value={c.s7.connectionType} onChange={(e) => setS7({ connectionType: e.target.value as S7ConnectionType })}>
              <option value="pg">PG</option>
              <option value="op">OP</option>
              <option value="basic">S7 Basic</option>
            </select>
          </label>
          <label className="field">
            <span>PDU 长度</span>
            <select className="input" value={c.s7.pduSize} onChange={(e) => setS7({ pduSize: num(e.target.value) })}>
              {[240, 480, 960].map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>本地 TSAP（留空自动）</span>
            <input className="input mono" placeholder="0x0100" defaultValue={hex(c.s7.localTsap)} key={`l${c.s7.localTsap}`} onBlur={(e) => setS7({ localTsap: parseHex(e.target.value) })} />
          </label>
          <label className="field">
            <span>远端 TSAP（留空按机架/插槽）</span>
            <input className="input mono" placeholder="自动" defaultValue={hex(c.s7.remoteTsap)} key={`r${c.s7.remoteTsap}`} onBlur={(e) => setS7({ remoteTsap: parseHex(e.target.value) })} />
          </label>
        </>
      )}

      {c.protocol === "mc" && (
        <>
          <label className="field">
            <span>网络号</span>
            <input className="input mono" type="number" min={0} max={255} value={c.mc.networkNo} onChange={(e) => setMc({ networkNo: num(e.target.value) })} />
          </label>
          <label className="field">
            <span>PC 号</span>
            <input className="input mono" type="number" min={0} max={255} value={c.mc.pcNo} onChange={(e) => setMc({ pcNo: num(e.target.value) })} />
          </label>
          <label className="field">
            <span>目标模块 IO 号</span>
            <input className="input mono" defaultValue={hex(c.mc.moduleIo)} key={`io${c.mc.moduleIo}`} onBlur={(e) => setMc({ moduleIo: parseHex(e.target.value) ?? 0x03ff })} />
          </label>
          <label className="field">
            <span>目标模块站号</span>
            <input className="input mono" type="number" min={0} max={255} value={c.mc.moduleStation} onChange={(e) => setMc({ moduleStation: num(e.target.value) })} />
          </label>
        </>
      )}

      <label className="field">
        <span>通讯超时 (ms)</span>
        <input className="input mono" type="number" value={c.timeoutMs} onChange={(e) => set({ timeoutMs: num(e.target.value) })} />
      </label>
      <label className="field">
        <span>轮询周期 (ms)</span>
        <input className="input mono" type="number" value={c.pollIntervalMs} onChange={(e) => set({ pollIntervalMs: num(e.target.value) })} />
      </label>
      <label className="field">
        <span>重连间隔 (ms)</span>
        <input className="input mono" type="number" value={c.reconnectIntervalMs} onChange={(e) => set({ reconnectIntervalMs: num(e.target.value) })} />
      </label>

      {c.protocol === "mc" && (
        <label className="check field-check">
          <input type="checkbox" checked={c.mc.xyOctal} onChange={(e) => setMc({ xyOctal: e.target.checked })} />
          <span>X/Y 使用八进制编号（iQ-F / FX5）</span>
        </label>
      )}
    </div>
  );
}
