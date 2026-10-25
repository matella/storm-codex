import { useQuery } from "@tanstack/react-query";
import { fmtDur } from "../api";
import { SecHead } from "../components/ds";

interface Trend {
  build: number;
  games: number;
  blue_wins: number;
  avg_length: number;
  first_seen: string | null;
  last_seen: string | null;
  my_games: number;
  my_wins: number;
}

const fmtDate = (iso: string | null) =>
  iso ? new Date(iso).toLocaleDateString("en-GB", { day: "2-digit", month: "short", year: "2-digit" }) : "—";
const pct = (w: number, g: number) => (g ? Math.round((100 * w) / g) : 0);
const tone = (p: number) => (p >= 55 ? "hot" : p < 45 ? "cold" : "");

/** Trends — un build HotS = un patch : ton winrate patch par patch (graphique), puis le détail. */
export function Trends() {
  const { data, isLoading } = useQuery({
    queryKey: ["trends"],
    queryFn: async () => (await fetch("/api/trends")).json() as Promise<Trend[]>,
  });
  // chronologique pour le graphique (l'API renvoie du plus récent au plus ancien)
  const chrono = [...(data ?? [])].sort((a, b) => a.build - b.build).slice(-24);
  const mine = (data ?? []).filter((t) => t.my_games > 0);
  const best = [...mine].filter((t) => t.my_games >= 5).sort((a, b) => b.my_wins / b.my_games - a.my_wins / a.my_games)[0];

  return (
    <div className="ds-page">
      <header className="ds-cover" style={{ marginTop: 18 }}>
        <div className="ds-kicker">Trends</div>
        <h1 className="ds-title" style={{ cursor: "default" }}>Patch by patch</h1>
        <div className="ds-subtitle" style={{ cursor: "default" }}>Each HotS game build is a patch: how you did on it, how much was played, blue-side win rate and game length.</div>
        {data && (
          <div className="ds-statline">
            <div className="ds-stat"><span className="ds-label">Patches</span><strong>{data.length}</strong><small>{mine.length} with your games</small></div>
            {best && <div className="ds-stat"><span className="ds-label">Your best patch · 5+ games</span><strong>{best.build}</strong><small>{best.my_wins}/{best.my_games} · {pct(best.my_wins, best.my_games)}%</small></div>}
          </div>
        )}
      </header>

      {isLoading && <div className="ds-empty" style={{ marginTop: 20 }}>loading…</div>}
      {data && data.length === 0 && <div className="ds-empty" style={{ marginTop: 20 }}>No data.</div>}
      {data && data.length > 0 && (
        <>
          <section className="ds-sec">
            <SecHead num={1} title="Your win rate" sub={`last ${chrono.length} patches · dashed line = 50%`} />
            <div className="ds-panel">
              <div className="ds-trend">
                <div className="mid" />
                {chrono.map((t) => {
                  const p = pct(t.my_wins, t.my_games);
                  return (
                    <div key={t.build} className="col" title={`${t.build} · ${t.my_games ? `${t.my_wins}/${t.my_games} (${p}%)` : "no game of yours"}`}>
                      <span className="v">{t.my_games ? `${p}%` : ""}</span>
                      <div className={`bar ${t.my_games ? tone(p) : "none"}`} style={{ height: t.my_games ? `${Math.max(4, p) * 0.9}%` : "4%" }} />
                    </div>
                  );
                })}
              </div>
              <div className="ds-trend-x">{chrono.map((t) => <span key={t.build} title={fmtDate(t.first_seen)}>{t.build}</span>)}</div>
            </div>
          </section>

          <section className="ds-sec">
            <SecHead num={2} title="Detail" />
            <div className="ds-panel flush scroll">
              <table className="ds-table">
                <thead><tr><th>Patch (build)</th><th>Played</th><th>You</th><th>Your win %</th><th>Games</th><th>Blue win</th><th>Avg length</th></tr></thead>
                <tbody>
                  {data.map((t) => {
                    const p = pct(t.my_wins, t.my_games);
                    const same = fmtDate(t.first_seen) === fmtDate(t.last_seen);
                    return (
                      <tr key={t.build}>
                        <td className="ds-num" style={{ color: "var(--text)" }}>{t.build}</td>
                        <td className="ds-num">{same ? fmtDate(t.last_seen) : `${fmtDate(t.first_seen)} – ${fmtDate(t.last_seen)}`}</td>
                        <td className="ds-num">{t.my_games ? <><span style={{ color: "var(--win)" }}>{t.my_wins}</span>–<span style={{ color: "var(--loss-soft)" }}>{t.my_games - t.my_wins}</span></> : "—"}</td>
                        <td>{t.my_games ? <div className="ds-cell"><span>{p}%</span><i className={`ds-bar ${tone(p) === "hot" ? "win" : tone(p) === "cold" ? "loss" : ""}`}><b style={{ width: `${p}%` }} /></i></div> : <span className="ds-num">—</span>}</td>
                        <td className="ds-num">{t.games}</td>
                        <td className="ds-num ds-us">{pct(t.blue_wins, t.games)}%</td>
                        <td className="ds-num">{fmtDur(t.avg_length)}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          </section>
        </>
      )}
    </div>
  );
}
