import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { fetchSynergies } from "../api";
import { Avatar } from "../components/Avatar";

type Sort = "winrate" | "games";

/** Tri d'une liste {games,wins} par WR (min 1 partie) ou volume. */
function sorted<T extends { games: number; wins: number }>(rows: T[], by: Sort): T[] {
  const wr = (r: T) => (r.games ? r.wins / r.games : 0);
  return [...rows].sort((a, b) => (by === "winrate" ? wr(b) - wr(a) : b.games - a.games));
}

export function Synergies() {
  const { data, isLoading } = useQuery({ queryKey: ["synergies"], queryFn: fetchSynergies });
  const nav = useNavigate();

  return (
    <>
      <h1>Synergies</h1>
      <p className="note">From your perspective, across all your accounts. Allies you played ≥3 games with, and enemy heroes you faced ≥3 times.</p>

      {isLoading && <div className="empty">loading…</div>}
      {data && (
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16 }}>
          <div>
            <p className="cap">Best teammates (by win rate)</p>
            <div className="card">
              <table>
                <thead><tr><th>Ally</th><th>Games</th><th>W–L</th><th>Win rate</th></tr></thead>
                <tbody>
                  {sorted(data.teammates, "winrate").map((t) => {
                    const w = (100 * t.wins) / t.games;
                    return (
                      <tr key={t.name}>
                        <td>{t.name}</td>
                        <td className="mono">{t.games}</td>
                        <td className="mono"><span style={{ color: "var(--win)" }}>{t.wins}</span>-<span style={{ color: "var(--loss)" }}>{t.games - t.wins}</span></td>
                        <td className="mono" style={{ color: w >= 50 ? "var(--win)" : "var(--loss)" }}>{w.toFixed(0)}%</td>
                      </tr>
                    );
                  })}
                  {data.teammates.length === 0 && <tr><td colSpan={4} className="empty">no recurring teammates</td></tr>}
                </tbody>
              </table>
            </div>
          </div>

          <div>
            <p className="cap">Vs enemy heroes (your win rate)</p>
            <div className="card">
              <table>
                <thead><tr><th>Enemy hero</th><th>Faced</th><th>W–L</th><th>Win rate</th></tr></thead>
                <tbody>
                  {sorted(data.enemies, "winrate").map((e) => {
                    const w = (100 * e.wins) / e.games;
                    return (
                      <tr key={e.hero} className="link" onClick={() => nav(`/hero/${encodeURIComponent(e.hero)}`)}>
                        <td><span style={{ display: "flex", alignItems: "center", gap: 7 }}><Avatar hero={e.hero} size={20} /> {e.hero}</span></td>
                        <td className="mono">{e.games}</td>
                        <td className="mono"><span style={{ color: "var(--win)" }}>{e.wins}</span>-<span style={{ color: "var(--loss)" }}>{e.games - e.wins}</span></td>
                        <td className="mono" style={{ color: w >= 50 ? "var(--win)" : "var(--loss)" }}>{w.toFixed(0)}%</td>
                      </tr>
                    );
                  })}
                  {data.enemies.length === 0 && <tr><td colSpan={4} className="empty">not enough data</td></tr>}
                </tbody>
              </table>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
