import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate, Link } from "react-router-dom";
import { fetchPatches, type PatchItem } from "../api";
import { DateBlock } from "../components/ds/patch";

/** Patch notes HotS (proxy HotsPatchNotes) : liste datée, filtre par type côté client. */
export function Patches() {
  const nav = useNavigate();
  const [type, setType] = useState("");
  const { data, isLoading } = useQuery({ queryKey: ["patches"], queryFn: fetchPatches });
  const items = data?.items ?? [];
  const types = [...new Set(items.map((p) => p.patchType).filter(Boolean))];
  const rows = type ? items.filter((p) => p.patchType === type) : items;

  return (
    <div className="ds-page">
      <header className="ds-cover" style={{ marginTop: 18 }}>
        <div className="ds-kicker">Patch notes</div>
        <h1 className="ds-title" style={{ cursor: "default" }}>Patch notes</h1>
        <div className="ds-subtitle" style={{ cursor: "default" }}>Official HotS patch notes — {items.length} patches. Open one for details, or browse the changes hero by hero.</div>
        <div className="ds-filterbar">
          <span className={type === "" ? "ds-pill on" : "ds-pill"} onClick={() => setType("")}>All</span>
          {types.map((t) => <span key={t} className={type === t ? "ds-pill on" : "ds-pill"} onClick={() => setType(t)}>{t}</span>)}
          <Link to="/hero-changes" className="ds-btn primary" style={{ marginLeft: "auto" }}>Changes by hero ›</Link>
        </div>
      </header>

      <section className="ds-sec" style={{ marginTop: 26 }}>
        {isLoading && <div className="ds-empty">loading…</div>}
        {!isLoading && rows.length === 0 && <div className="ds-empty">No patch notes (referential unavailable?).</div>}
        <div className="ds-plist">
          {rows.map((p: PatchItem) => (
            <div key={p.internalId} className="ds-prow" role="link" tabIndex={0} onClick={() => nav(`/patch/${encodeURIComponent(p.internalId)}`)}
              onKeyDown={(e) => { if (e.key === "Enter") nav(`/patch/${encodeURIComponent(p.internalId)}`); }}>
              <DateBlock iso={p.liveDate} />
              <span className="t">{p.patchName}</span>
              <span className="ds-tag award">{p.patchType}</span>
              <span className="c">{p.heroCount ? `${p.heroCount} heroes` : ""}{p.heroCount && p.mapCount ? " · " : ""}{p.mapCount ? `${p.mapCount} maps` : ""} ›</span>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
