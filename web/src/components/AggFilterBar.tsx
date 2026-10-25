import { useSettings, operatorNames, type AggFilter } from "../api";

const MODES: [string, number | undefined][] = [
  ["All", undefined],
  ["Storm League", 50091],
  ["ARAM", 50101],
  ["Custom", -1],
  ["Hero League", 50061],
  ["QM", 50001],
];

/** Barre de filtres partagée des agrégats (Heroes, Hero, Maps…) : mode · mes parties · compte ·
 *  dates. `mineLabel` adapte le libellé (« My heroes » vs « My games »). Style design system. */
export function AggFilterBar({ value, onChange, mineLabel = "Mine only" }: { value: AggFilter; onChange: (f: AggFilter) => void; mineLabel?: string }) {
  useSettings();
  const accounts = operatorNames();
  const set = (patch: Partial<AggFilter>) => onChange({ ...value, ...patch });
  const active = value.mode != null || value.mine || value.account || value.from || value.to;
  return (
    <div className="ds-filterbar">
      {MODES.map(([label, m]) => (
        <span key={label} className={value.mode === m ? "ds-pill on" : "ds-pill"} onClick={() => set({ mode: m })}>{label}</span>
      ))}
      <span className="ds-sep" />
      <span className={value.mine ? "ds-pill on" : "ds-pill"} onClick={() => set({ mine: !value.mine })}>{mineLabel}</span>
      {accounts.length > 1 && (
        <select className="ds-input" value={value.account ?? ""} onChange={(e) => set({ account: e.target.value || undefined })}>
          <option value="">All my accounts</option>
          {accounts.map((a) => <option key={a} value={a}>{a}</option>)}
        </select>
      )}
      <label>from <input type="date" className="ds-input" value={value.from ?? ""} onChange={(e) => set({ from: e.target.value || undefined })} /></label>
      <label>to <input type="date" className="ds-input" value={value.to ?? ""} onChange={(e) => set({ to: e.target.value || undefined })} /></label>
      {active && <span className="ds-pill" onClick={() => onChange({})}>✕ reset</span>}
    </div>
  );
}
