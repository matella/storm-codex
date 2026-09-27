import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import { fmtDur, aggParams, type AggFilter } from "../api";
import { AggFilterBar } from "../components/AggFilterBar";
import { hasMapArt, mapArt } from "../components/ds/match";

interface MapStat { map: string; games: number; blue_wins: number; avg_length: number; my_games: number; my_wins: number }
const pct = (w: number, g: number) => (g ? Math.round((100 * w) / g) : 0);

/** Maps — une tuile par carte (art de la carte) : ton bilan, winrate, côté bleu, durée moyenne. */
export function Maps() {
  const [filter, setFilter] = useState<AggFilter>({});
  const { data, isLoading } = useQuery({
    queryKey: ["maps", filter],
    queryFn: async () => (await fetch(`/api/maps?${aggParams(filter)}`)).json() as Promise<MapStat[]>,
  });
  const my = (data ?? []).reduce((s, m) => ({ g: s.g + m.my_games, w: s.w + m.my_wins }), { g: 0, w: 0 });
  return (
    <div className="ds-page">
      <header className="ds-cover" style={{ marginTop: 18 }}>
        <div className="ds-kicker">Maps</div>
        <h1 className="ds-title" style={{ cursor: "default" }}>Battlegrounds</h1>
        <div className="ds-subtitle" style={{ cursor: "default" }}>
          {data ? `${data.length} maps` : "loading…"}{my.g > 0 && <> · you <b style={{ color: "var(--win)" }}>{my.w}</b>–<b style={{ color: "var(--loss-soft)" }}>{my.g - my.w}</b> ({pct(my.w, my.g)}%)</>} — click a map to see its games.
        </div>
        <AggFilterBar value={filter} onChange={setFilter} mineLabel="My games" />
      </header>

      <section className="ds-sec" style={{ marginTop: 26 }}>
        {isLoading && <div className="ds-empty">loading…</div>}
        {data?.length === 0 && <div className="ds-empty">No map for this filter.</div>}
        <div className="ds-mapts">
          {data?.map((m) => {
            const mine = m.my_games > 0;
            const p = mine ? pct(m.my_wins, m.my_games) : pct(m.blue_wins, m.games);
            const t = !mine ? "" : p >= 55 ? "hot" : p < 45 ? "cold" : "";
            return (
              <Link key={m.map} to={`/matches?map=${encodeURIComponent(m.map)}`}
                className={hasMapArt(m.map) ? "ds-mapt" : "ds-mapt noart"} style={hasMapArt(m.map) ? mapArt(m.map) : undefined}>
                <strong>{m.map}</strong>
                <div className="ds-mapt-f">
                  <span className={`wr ${t}`}>{mine ? p : "—"}{mine && <i>%</i>}</span>
                  <em>{mine ? `you ${m.my_wins}–${m.my_games - m.my_wins}` : "you haven't played it"}</em>
                </div>
                {mine && <i className={`ds-bar ${t === "hot" ? "win" : t === "cold" ? "loss" : ""}`}><b style={{ width: `${p}%` }} /></i>}
                <div className="meta">
                  <span><b>{m.games}</b> games</span>
                  <span>blue wins <b>{pct(m.blue_wins, m.games)}%</b></span>
                  <span>avg <b>{fmtDur(m.avg_length)}</b></span>
                </div>
              </Link>
            );
          })}
        </div>
      </section>
    </div>
  );
}
