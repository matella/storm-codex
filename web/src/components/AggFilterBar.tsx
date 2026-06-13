import { useSettings, operatorNames, type AggFilter } from "../api";

const MODES: [string, number | undefined][] = [
  ["All", undefined],
  ["Storm League", 50091],
  ["ARAM", 50101],
  ["Custom", -1],
  ["Hero League", 50061],
  ["QM", 50001],
];
const inp = { background: "var(--surface-2)", border: "1px solid var(--hairline-strong)", color: "var(--text)", borderRadius: 6, padding: "4px 8px", fontSize: 12 } as const;

/** Barre de filtres partagée des agrégats (Heroes, Maps…) : mode · mes parties · compte · dates.
 *  `mineLabel` adapte le libellé (« My heroes » vs « My games »). */
export function AggFilterBar({ value, onChange, mineLabel = "Mine only" }: { value: AggFilter; onChange: (f: AggFilter) => void; mineLabel?: string }) {
  useSettings();
  const accounts = operatorNames();
  const set = (patch: Partial<AggFilter>) => onChange({ ...value, ...patch });
  const active = value.mode != null || value.mine || value.account || value.from || value.to;
  return (
    <div className="card-hd" style={{ flexWrap: "wrap", gap: 6, alignItems: "center" }}>
      {MODES.map(([label, m]) => (
        <span key={label} className={value.mode === m ? "pill on" : "pill"} onClick={() => set({ mode: m })}>{label}</span>
      ))}
      <span style={{ width: 1, alignSelf: "stretch", background: "var(--hairline)", margin: "0 4px" }} />
      <span className={value.mine ? "pill on" : "pill"} onClick={() => set({ mine: !value.mine })}>{mineLabel}</span>
      {accounts.length > 1 && (
        <select style={inp} value={value.account ?? ""} onChange={(e) => set({ account: e.target.value || undefined })}>
          <option value="">All my accounts</option>
          {accounts.map((a) => <option key={a} value={a}>{a}</option>)}
        </select>
      )}
      <label style={{ fontSize: 10, color: "var(--text-2)" }}>from <input type="date" style={inp} value={value.from ?? ""} onChange={(e) => set({ from: e.target.value || undefined })} /></label>
      <label style={{ fontSize: 10, color: "var(--text-2)" }}>to <input type="date" style={inp} value={value.to ?? ""} onChange={(e) => set({ to: e.target.value || undefined })} /></label>
      {active && <span className="pill" onClick={() => onChange({})}>✕ reset</span>}
    </div>
  );
}
