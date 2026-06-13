import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { fetchMatches, fetchHeroes, modeBadge, fmtTime, fmtDur, mapImage, pickOperator, type MatchSummary } from "../api";
import { Avatar } from "../components/Avatar";

// Codes officiels (storm-stats GameMode). Brawls/IA sont rejetés au parse → on liste les modes réels.
// Modes réellement présents dans l'archive en tête (SL 50091, ARAM 50101, Custom -1).
const MODE_FILTERS: [string, number | undefined][] = [
  ["All", undefined],
  ["Storm League", 50091],
  ["ARAM", 50101],
  ["Custom", -1],
  ["Hero League", 50061],
  ["QM", 50001],
];

interface MapStat { map: string; games: number }

function ownHero(m: MatchSummary): { hero: string | null; win: boolean | null } {
  // perspective opérateur (operator_names) ; fallback 1er joueur si aucun match
  const p = pickOperator(m.players ?? []);
  return { hero: p?.hero ?? null, win: p?.win ?? null };
}

export function Matches() {
  const [mode, setMode] = useState<number | undefined>(undefined);
  const [map, setMap] = useState<string>("");
  const [hero, setHero] = useState<string>("");
  const nav = useNavigate();

  const { data, isLoading } = useQuery({
    queryKey: ["matches", { mode, map, hero }],
    queryFn: () => fetchMatches({ mode, map: map || undefined, hero: hero || undefined, limit: 200 }),
  });
  // dropdowns peuplés depuis les agrégats (triés par fréquence)
  const { data: maps } = useQuery({
    queryKey: ["maps-filter"],
    queryFn: async () => (await fetch("/api/maps")).json() as Promise<MapStat[]>,
    staleTime: Infinity,
  });
  const { data: heroes } = useQuery({ queryKey: ["heroes-filter"], queryFn: fetchHeroes, staleTime: Infinity });

  const csv = `/api/matches.csv?${new URLSearchParams({
    ...(mode != null ? { mode: String(mode) } : {}),
    ...(map ? { map } : {}),
    ...(hero ? { hero } : {}),
  })}`;

  return (
    <>
      <h1>Matches</h1>
      <div className="card">
        <div className="card-hd" style={{ flexWrap: "wrap", gap: 6 }}>
          {MODE_FILTERS.map(([label, m]) => (
            <span key={label} className={mode === m ? "pill on" : "pill"} onClick={() => setMode(m)}>
              {label}
            </span>
          ))}
          <select className="filter-select" value={map} onChange={(e) => setMap(e.target.value)}>
            <option value="">All maps</option>
            {maps?.slice().sort((a, b) => a.map.localeCompare(b.map)).map((mp) => (
              <option key={mp.map} value={mp.map}>{mp.map}</option>
            ))}
          </select>
          <select className="filter-select" value={hero} onChange={(e) => setHero(e.target.value)}>
            <option value="">All heroes</option>
            {heroes?.slice().sort((a, b) => a.hero.localeCompare(b.hero)).map((h) => (
              <option key={h.hero} value={h.hero}>{h.hero}</option>
            ))}
          </select>
          <a href={csv} className="pill" style={{ marginLeft: "auto" }}>export CSV ↓</a>
          <a
            href={`/api/matches?${new URLSearchParams({
              ...(mode != null ? { mode: String(mode) } : {}),
              ...(map ? { map } : {}),
              ...(hero ? { hero } : {}),
              limit: "1000",
            })}`}
            className="pill"
            target="_blank"
            rel="noreferrer"
          >
            export JSON ↓
          </a>
          <span style={{ fontSize: 10, color: "var(--kicker)" }}>{data?.length ?? 0} matches</span>
        </div>
        {isLoading && <div className="empty">loading…</div>}
        {data?.length === 0 && <div className="empty">no matches for this filter</div>}
        {data?.map((m) => {
          const mb = modeBadge(m.mode);
          const o = ownHero(m);
          const winner = m.winner;
          const bg = mapImage(m.map);
          return (
            <div
              key={m.id}
              className="row link"
              onClick={() => nav(`/match/${m.id}`)}
              style={
                bg
                  ? {
                      // image de carte en fond, voilée pour garder le texte lisible (fallback
                      // silencieux si la carte n'a pas d'image, ex. ARAM)
                      backgroundImage: `linear-gradient(90deg, var(--surface) 0%, rgba(14,16,22,.82) 45%, rgba(14,16,22,.62) 100%), url(${bg})`,
                      backgroundSize: "cover",
                      backgroundPosition: "center 30%",
                    }
                  : undefined
              }
            >
              <span className="mono muted" style={{ minWidth: 92, fontSize: 11 }}>{fmtTime(m.played_at)}</span>
              <span className={`bdg ${mb.cls}`}>{mb.short}</span>
              <Avatar hero={o.hero} />
              <span style={{ fontSize: 12 }}>{m.map ?? "—"}</span>
              {o.win != null && <span className={`bdg ${o.win ? "b-win" : "b-loss"}`}>{o.win ? "W" : "L"}</span>}
              <span style={{ marginLeft: "auto", color: "var(--kicker)", fontSize: 10 }}>
                {fmtDur(m.length)} · {winner === 0 ? "blue" : winner === 1 ? "red" : "?"} team wins ›
              </span>
            </div>
          );
        })}
      </div>
    </>
  );
}
