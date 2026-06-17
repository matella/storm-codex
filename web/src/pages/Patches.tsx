import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate, Link } from "react-router-dom";
import { fetchPatches, fmtTime, type PatchItem } from "../api";

/** Liste des patch notes HotS (proxy HotsPatchNotes). Filtre par type côté client. */
export function Patches() {
  const nav = useNavigate();
  const [type, setType] = useState("");
  const { data, isLoading } = useQuery({ queryKey: ["patches"], queryFn: fetchPatches });
  const items = data?.items ?? [];
  const types = [...new Set(items.map((p) => p.patchType).filter(Boolean))];
  const rows = type ? items.filter((p) => p.patchType === type) : items;

  return (
    <>
      <h1 style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
        Patch Notes
        <Link to="/hero-changes" className="pill" style={{ fontSize: 11, marginLeft: "auto" }}>By hero ›</Link>
      </h1>
      <p className="note">Official HotS patch notes — {items.length} patches. Click one for details, or browse <Link to="/hero-changes" style={{ color: "var(--accent)" }}>changes by hero</Link>.</p>
      <div className="card">
        <div className="card-hd" style={{ flexWrap: "wrap", gap: 6 }}>
          <span className={type === "" ? "pill on" : "pill"} onClick={() => setType("")}>All</span>
          {types.map((t) => (
            <span key={t} className={type === t ? "pill on" : "pill"} onClick={() => setType(t)}>{t}</span>
          ))}
        </div>
        {isLoading && <div className="empty">loading…</div>}
        {!isLoading && rows.length === 0 && <div className="empty">no patch notes (referential unavailable?)</div>}
        <table>
          <thead><tr><th>Patch</th><th>Type</th><th>Date</th><th>Heroes</th><th>Maps</th></tr></thead>
          <tbody>
            {rows.map((p: PatchItem) => (
              <tr key={p.internalId} className="link" onClick={() => nav(`/patch/${encodeURIComponent(p.internalId)}`)}>
                <td>{p.patchName}</td>
                <td><span className="bdg b-qm">{p.patchType}</span></td>
                <td className="mono muted" style={{ fontSize: 11 }}>{fmtTime(p.liveDate)}</td>
                <td className="mono">{p.heroCount || "—"}</td>
                <td className="mono">{p.mapCount || "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}
