import { useState, type CSSProperties } from "react";
import { useQuery } from "@tanstack/react-query";
import { useParams, useNavigate, Link } from "react-router-dom";
import { fetchPlayer, fmtTime, awardLabel } from "../api";
import { Portrait, SecHead, heroRing } from "../components/ds";
import { ModeTag, hasMapArt, mapArt } from "../components/ds/match";

const pct = (w: number, g: number) => (g ? Math.round((100 * w) / g) : 0);

/** Player — couverture au portrait de son héros le plus joué, réserve de héros, parties récentes. */
export function Player() {
  const { toon } = useParams();
  const nav = useNavigate();
  const { data, isLoading } = useQuery({ queryKey: ["player", toon], queryFn: () => fetchPlayer(toon!) });
  const [allHeroes, setAllHeroes] = useState(false);
  if (isLoading) return <div className="ds-page"><div className="ds-empty" style={{ marginTop: 24 }}>loading…</div></div>;
  if (!data) return <div className="ds-page"><div className="ds-empty" style={{ marginTop: 24 }}>Player not found. <Link to="/matches">Back to matches</Link></div></div>;
  const main = data.heroes[0]?.hero ?? null;
  const wr = pct(data.wins, data.matches);
  const kdaRatio = ((data.avg_takedowns ?? 0) / Math.max(1, data.avg_deaths ?? 0)).toFixed(1);
  const maxG = data.heroes[0]?.games ?? 1;
  const others = data.names.filter((n) => n !== data.name);

  return (
    <div className="ds-page">
      <header className="ds-hhead" style={{ "--ring": heroRing(main) } as CSSProperties}>
        {main && <Portrait hero={main} size={160} className="xl" />}
        <div className="ds-hhead-txt">
          <div className="ds-kicker">Player · {data.toon}</div>
          <h1 className="ds-title" style={{ cursor: "default" }}>{data.name ?? data.toon}</h1>
          {others.length > 0 && <div className="ds-subtitle" style={{ cursor: "default" }}>also seen as {others.join(", ")}</div>}
          <div className="ds-statline" style={{ marginTop: 16 }}>
            <div className="ds-stat"><span className="ds-label">Win rate</span><strong>{wr}<span>%</span></strong><small>{data.wins}–{data.matches - data.wins} · {data.matches} games</small></div>
            <div className="ds-stat"><span className="ds-label">Avg K / D / T</span><strong>{data.avg_kills ?? "—"}<span>/</span>{data.avg_deaths ?? "—"}<span>/</span>{data.avg_takedowns ?? "—"}</strong><small>KDA ratio {kdaRatio}</small></div>
            <div className="ds-stat"><span className="ds-label">Heroes</span><strong>{data.heroes.length}</strong><small>{main ? `main: ${main}` : ""}</small></div>
          </div>
        </div>
      </header>

      <section className="ds-sec">
        <SecHead num={1} title="Hero pool" sub={data.heroes.length > 18
          ? <span className="ds-pill" onClick={() => setAllHeroes(!allHeroes)}>{allHeroes ? "top 18" : `show all ${data.heroes.length}`}</span>
          : "games · win rate · avg K/D/T"} />
        <div className="ds-panel">
          <div className="ds-pool">
            {(allHeroes ? data.heroes : data.heroes.slice(0, 18)).map((h) => {
              const p = pct(h.wins, h.games);
              return (
                <Link key={h.hero} to={`/hero/${encodeURIComponent(h.hero)}`} className="ds-poolrow">
                  <Portrait hero={h.hero} size={34} />
                  <span>{h.hero}</span>
                  <i className="ds-bar"><b style={{ width: `${Math.round((h.games * 100) / maxG)}%` }} /></i>
                  <span className={`wr ${p >= 55 ? "hot" : p < 45 ? "cold" : ""}`}>{p}% · {h.games}g</span>
                  <em>{h.avg_kills}/{h.avg_deaths}/{h.avg_takedowns}</em>
                </Link>
              );
            })}
          </div>
        </div>
      </section>

      <section className="ds-sec">
        <SecHead num={2} title="Recent games" />
        <div className="ds-mlist">
          {data.recent.map((g) => {
            const aw = awardLabel(g.award);
            const a = Math.max(0, (g.takedowns ?? 0) - (g.kills ?? 0));
            return (
              <div key={g.match_id} className={`ds-mrow ${g.win ? "w" : "l"}`} style={hasMapArt(g.map) ? mapArt(g.map) : undefined}
                onClick={() => nav(`/match/${g.match_id}`)} role="link" tabIndex={0} onKeyDown={(e) => { if (e.key === "Enter") nav(`/match/${g.match_id}`); }}>
                <span className="when">{fmtTime(g.played_at)}</span>
                <span><ModeTag mode={g.mode} short /></span>
                {g.hero ? <Portrait hero={g.hero} size={40} tone={g.win ? "pick" : "loss"} /> : <span />}
                <span className="what"><span className="map">{g.map ?? "—"}</span><span className="hero">{g.hero} · <span className="ds-kda">{g.kills}<i>/</i>{g.deaths}<i>/</i>{a}</span></span></span>
                <span style={{ display: "flex", gap: 6 }}>
                  <span className={`ds-tag ${g.win ? "win" : "loss"}`}>{g.win ? "WIN" : "LOSS"}</span>
                  {aw?.mvp && <span className="ds-tag mvp">👑 MVP</span>}
                </span>
                <span className="len">›</span>
              </div>
            );
          })}
        </div>
      </section>
    </div>
  );
}
