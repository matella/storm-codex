import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { fetchMatches, modeBadge, fmtTime, fmtDur, type MatchSummary } from "../api";
import { Avatar } from "../components/Avatar";

const MODE_FILTERS: [string, number | undefined][] = [
  ["Tous", undefined],
  ["Storm League", 50111],
  ["Hero League", 50091],
  ["QM", 50051],
  ["ARAM", 50101],
];

function ownHero(m: MatchSummary): { hero: string | null; win: boolean | null; kda: string } {
  // heuristique : 1er joueur (l'archive est nominative — affinable via le toon propriétaire)
  const p = m.players?.[0];
  return { hero: p?.hero ?? null, win: p?.win ?? null, kda: "" };
}

export function Matches() {
  const [mode, setMode] = useState<number | undefined>(undefined);
  const nav = useNavigate();
  const { data, isLoading } = useQuery({
    queryKey: ["matches", { mode }],
    queryFn: () => fetchMatches({ mode, limit: 100 }),
  });

  return (
    <>
      <h1>Matchs</h1>
      <div className="card">
        <div className="card-hd" style={{ flexWrap: "wrap", gap: 6 }}>
          {MODE_FILTERS.map(([label, m]) => (
            <span key={label} className={mode === m ? "pill on" : "pill"} onClick={() => setMode(m)}>
              {label}
            </span>
          ))}
          <a
            href={`/api/matches.csv${mode != null ? `?mode=${mode}` : ""}`}
            className="pill"
            style={{ marginLeft: "auto" }}
          >
            export CSV ↓
          </a>
          <span style={{ fontSize: 10, color: "var(--kicker)" }}>{data?.length ?? 0} matchs</span>
        </div>
        {isLoading && <div className="empty">chargement…</div>}
        {data?.length === 0 && <div className="empty">aucun match</div>}
        {data?.map((m) => {
          const mb = modeBadge(m.mode);
          const o = ownHero(m);
          const winner = m.winner;
          return (
            <div key={m.id} className="row link" onClick={() => nav(`/match/${m.id}`)}>
              <span className="mono muted" style={{ minWidth: 92, fontSize: 11 }}>{fmtTime(m.played_at)}</span>
              <span className={`bdg ${mb.cls}`}>{mb.short}</span>
              <Avatar hero={o.hero} />
              <span style={{ fontSize: 12 }}>{m.map ?? "—"}</span>
              {o.win != null && <span className={`bdg ${o.win ? "b-win" : "b-loss"}`}>{o.win ? "V" : "D"}</span>}
              <span style={{ marginLeft: "auto", color: "var(--kicker)", fontSize: 10 }}>
                {fmtDur(m.length)} · équipe {winner === 0 ? "bleue" : winner === 1 ? "rouge" : "?"} gagne ›
              </span>
            </div>
          );
        })}
      </div>
    </>
  );
}
