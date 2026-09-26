import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useQueryClient } from "@tanstack/react-query";
import { scoutingWrite, useScoutingList } from "../api";
import { day, fmtRate, statusMeta } from "../scouting";

const inp = { background: "var(--surface-2)", border: "1px solid var(--hairline-strong)", color: "var(--text)", borderRadius: 6, padding: "5px 9px", fontSize: 12 } as const;

/** Scouting — liste des rapports (plus récent d'abord) + création. */
export function Scouting() {
  const { data: reports, isLoading } = useScoutingList();
  const qc = useQueryClient();
  const nav = useNavigate();
  const [title, setTitle] = useState("");
  const [target, setTarget] = useState("");
  const [msg, setMsg] = useState<string | null>(null);

  const create = async () => {
    if (!title.trim()) { setMsg("give the report a title"); return; }
    const r = await scoutingWrite("/api/scouting", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title, target_name: target }),
    });
    if (r.status === 401) { setMsg("✗ unauthorized — set the admin token in Admin"); return; }
    if (!r.ok) { setMsg(`✗ HTTP ${r.status}`); return; }
    const { id } = await r.json();
    qc.invalidateQueries({ queryKey: ["scouting"] });
    nav(`/scouting/${id}`);
  };

  return (
    <>
      <h1>Scouting</h1>
      <p className="note">Drop an opposing team's replays, get their tendencies as numbers, hand the pack to an LLM, import its analysis back.</p>

      <div className="card">
        <div className="card-hd"><h2>New report</h2></div>
        <div className="row" style={{ flexWrap: "wrap" }}>
          <input style={{ ...inp, flex: 2, minWidth: 200 }} placeholder="title (e.g. Week 4 — vs Team X)" value={title}
            onChange={(e) => setTitle(e.target.value)} onKeyDown={(e) => e.key === "Enter" && create()} />
          <input style={{ ...inp, flex: 1, minWidth: 140 }} placeholder="team name (optional)" value={target}
            onChange={(e) => setTarget(e.target.value)} onKeyDown={(e) => e.key === "Enter" && create()} />
          <span className="pill on" onClick={create}>Create</span>
          {msg && <span className="note">{msg}</span>}
        </div>
      </div>

      <div className="card">
        <div className="card-hd"><h2>Reports</h2><span className="muted mono" style={{ marginLeft: "auto", fontSize: 11 }}>{reports?.length ?? 0}</span></div>
        {isLoading && <div className="empty">loading…</div>}
        {reports && reports.length === 0 && <div className="empty">No report yet — create one above.</div>}
        {reports?.map((r) => {
          const st = statusMeta(r.status);
          return (
            <div key={r.id} className="row link" onClick={() => nav(`/scouting/${r.id}`)}>
              <div style={{ display: "flex", flexDirection: "column", gap: 2, minWidth: 0 }}>
                <span style={{ fontSize: 13, fontWeight: 500 }}>{r.title}</span>
                <span className="muted" style={{ fontSize: 11 }}>
                  {r.target_name ?? "unnamed team"} · created {day(r.created_at)}
                  {r.first_date && ` · games ${day(r.first_date)} → ${day(r.last_date)}`}
                </span>
              </div>
              <span style={{ marginLeft: "auto" }} className="mono muted">{r.games} replay{r.games === 1 ? "" : "s"}</span>
              <span className="mono" style={{ width: 90, textAlign: "right", fontSize: 11 }}>{fmtRate(r.record)}</span>
              <span className={`bdg ${st.cls}`} style={{ width: 110, textAlign: "center" }}>{st.label}</span>
            </div>
          );
        })}
      </div>
    </>
  );
}
