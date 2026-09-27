import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import { fetchSynergies } from "../api";
import { Portrait, SecHead } from "../components/ds";

type Sort = "winrate" | "games";
const pct = (w: number, g: number) => (g ? Math.round((100 * w) / g) : 0);
const tone = (p: number) => (p >= 55 ? "hot" : p < 45 ? "cold" : "");

/** Tri d'une liste {games,wins} par winrate ou par volume. */
function sorted<T extends { games: number; wins: number }>(rows: T[], by: Sort): T[] {
  const wr = (r: T) => (r.games ? r.wins / r.games : 0);
  return [...rows].sort((a, b) => (by === "winrate" ? wr(b) - wr(a) || b.games - a.games : b.games - a.games));
}

/** Synergies — du point de vue de l'opérateur : coéquipiers récurrents et héros affrontés. */
export function Synergies() {
  const { data, isLoading } = useQuery({ queryKey: ["synergies"], queryFn: fetchSynergies });
  const [sort, setSort] = useState<Sort>("winrate");
  const maxT = Math.max(1, ...(data?.teammates ?? []).map((t) => t.games));
  const maxE = Math.max(1, ...(data?.enemies ?? []).map((e) => e.games));
  const initials = (n: string) => n.replace(/[^\p{L}\p{N}]/gu, "").slice(0, 2).toUpperCase() || "?";

  return (
    <div className="ds-page">
      <header className="ds-cover" style={{ marginTop: 18 }}>
        <div className="ds-kicker">Synergies</div>
        <h1 className="ds-title" style={{ cursor: "default" }}>With &amp; against</h1>
        <div className="ds-subtitle" style={{ cursor: "default" }}>From your perspective, across all your accounts — allies you played 3+ games with, enemy heroes you faced 3+ times.</div>
        <div className="ds-filterbar">
          <span className="ds-label" style={{ margin: 0 }}>Sort</span>
          <span className={sort === "winrate" ? "ds-pill on" : "ds-pill"} onClick={() => setSort("winrate")}>Win rate</span>
          <span className={sort === "games" ? "ds-pill on" : "ds-pill"} onClick={() => setSort("games")}>Games</span>
        </div>
      </header>
      {isLoading && <div className="ds-empty" style={{ marginTop: 20 }}>loading…</div>}
      {data && (
        <div className="ds-grid2" style={{ marginTop: 10 }}>
          <section className="ds-sec" style={{ marginTop: 30 }}>
            <SecHead num={1} title="Teammates" sub={`${data.teammates.length} recurring`} />
            <div className="ds-panel">
              <div className="ds-rows">
                {sorted(data.teammates, sort).map((t) => {
                  const p = pct(t.wins, t.games);
                  return (
                    <div key={t.name} className="ds-srow">
                      <span className="ds-initial">{initials(t.name)}</span>
                      <span className="nm">{t.name}</span>
                      <i className="ds-bar"><b style={{ width: `${Math.round((t.games * 100) / maxT)}%` }} /></i>
                      <span className={`wr ${tone(p)}`}>{p}%</span>
                      <em>{t.wins}–{t.games - t.wins}</em>
                    </div>
                  );
                })}
                {data.teammates.length === 0 && <div className="ds-empty-row">no recurring teammates</div>}
              </div>
            </div>
          </section>
          <section className="ds-sec" style={{ marginTop: 30 }}>
            <SecHead num={2} title="Against heroes" sub="your win rate when they are in front" />
            <div className="ds-panel">
              <div className="ds-rows">
                {sorted(data.enemies, sort).map((e) => {
                  const p = pct(e.wins, e.games);
                  return (
                    <Link key={e.hero} to={`/hero/${encodeURIComponent(e.hero)}`} className="ds-srow">
                      <Portrait hero={e.hero} size={34} tone={p < 45 ? "red" : "plain"} />
                      <span className="nm">{e.hero}</span>
                      <i className="ds-bar"><b style={{ width: `${Math.round((e.games * 100) / maxE)}%` }} /></i>
                      <span className={`wr ${tone(p)}`}>{p}%</span>
                      <em>{e.wins}–{e.games - e.wins}</em>
                    </Link>
                  );
                })}
                {data.enemies.length === 0 && <div className="ds-empty-row">not enough data</div>}
              </div>
            </div>
          </section>
        </div>
      )}
    </div>
  );
}
