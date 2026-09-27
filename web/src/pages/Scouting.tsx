import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useQueryClient } from "@tanstack/react-query";
import { scoutingWrite, useScoutingList } from "../api";
import { useDossierFonts } from "../components/Dossier";
import { day } from "../scouting";
import type { ReportStatus } from "../scouting";
import "./scouting-dossier.css";

const STATUS: Record<ReportStatus, string> = {
  empty: "no replays", ready: "pack ready", analyzed: "draft plan ready", stale: "plan outdated",
};

/** Scouting — liste des rapports (plus récent d'abord) + création, dans le style « dossier ». */
export function Scouting() {
  const { data: reports, isLoading } = useScoutingList();
  useDossierFonts();
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
  const enter = (e: React.KeyboardEvent) => { if (e.key === "Enter") create(); };

  return (
    <div className="sd">
      <header className="sd-cover" style={{ marginTop: 18 }}>
        <div className="sd-kicker">Storm Codex · scouting</div>
        <h1 className="sd-title" style={{ cursor: "default" }}>Scouting</h1>
        <div className="sd-subtitle" style={{ cursor: "default" }}>
          Drop an opposing team's replays, get their draft habits as numbers, hand the pack to an LLM, bring back a map-by-map draft plan.
        </div>
        <div className="sd-edit" style={{ marginTop: 22 }}>
          <input className="sd-input big" placeholder="team name" value={target} onChange={(e) => setTarget(e.target.value)} onKeyDown={enter} />
          <input className="sd-input" style={{ flex: 2, minWidth: 220 }} placeholder="report title (e.g. Week 4 — playoffs)" value={title}
            onChange={(e) => setTitle(e.target.value)} onKeyDown={enter} />
          <span className="sd-btn primary" style={{ alignSelf: "center" }} onClick={create}>New report</span>
        </div>
        {msg && <div className="sd-meta" style={{ marginTop: 8, color: "var(--sd-loss2)" }}>{msg}</div>}
      </header>

      <section className="sd-sec">
        <div className="sd-sec-hd">
          <span className="num">{String(reports?.length ?? 0).padStart(2, "0")}</span>
          <h2>Reports</h2>
          <small>newest first</small>
        </div>
        {isLoading && <div className="sd-empty">loading…</div>}
        {reports && reports.length === 0 && <div className="sd-empty">No report yet — name the team above and create one.</div>}
        <div className="sd-rlist">
          {reports?.map((r) => {
            const rec = r.record;
            return (
              <a key={r.id} className="sd-rcard" href={`/scouting/${r.id}`} onClick={(e) => { e.preventDefault(); nav(`/scouting/${r.id}`); }}>
                <div className="sd-rcard-main">
                  <span className="idx">REPORT #{r.id} · CREATED {day(r.created_at)}</span>
                  <strong>{r.target_name ?? "Unnamed team"}</strong>
                  <span className="sd-rcard-title">{r.title}</span>
                </div>
                <div className="sd-rcard-side">
                  <span className="sd-rcard-rec">{rec && rec.n ? <>{rec.k}<i>–</i>{rec.n - rec.k}</> : "—"}</span>
                  <span className="sd-meta">{r.games} replay{r.games === 1 ? "" : "s"}{r.first_date ? ` · ${day(r.first_date)} → ${day(r.last_date)}` : ""}</span>
                  <span className={`sd-status ${r.status}`}>{STATUS[r.status]}</span>
                </div>
              </a>
            );
          })}
        </div>
      </section>
    </div>
  );
}
