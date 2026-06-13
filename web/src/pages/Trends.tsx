import { useQuery } from "@tanstack/react-query";
import { fmtDur } from "../api";

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

export function Trends() {
  const { data, isLoading } = useQuery({
    queryKey: ["trends"],
    queryFn: async () => (await fetch("/api/trends")).json() as Promise<Trend[]>,
  });
  return (
    <>
      <h1>Trends by patch</h1>
      <p className="note">
        Each row is a HotS game build (a patch). It shows when you played on that patch, how YOU did
        (your games &amp; win rate), plus overall sample size, blue-side win rate and average game length.
      </p>
      <div className="card">
        {isLoading && <div className="empty">loading…</div>}
        {data && data.length === 0 && <div className="empty">no data</div>}
        {data && data.length > 0 && (
          <table>
            <thead>
              <tr>
                <th>Patch (build)</th>
                <th>Played</th>
                <th>My W-L</th>
                <th>My win %</th>
                <th>Games</th>
                <th>Blue win</th>
                <th>Avg. length</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {data.map((t) => {
                const myWr = t.my_games ? (100 * t.my_wins) / t.my_games : 0;
                const blueWr = t.games ? (100 * t.blue_wins) / t.games : 0;
                const sameDay = fmtDate(t.first_seen) === fmtDate(t.last_seen);
                return (
                  <tr key={t.build}>
                    <td className="mono">{t.build}</td>
                    <td className="mono muted" style={{ fontSize: 11 }}>
                      {sameDay ? fmtDate(t.last_seen) : `${fmtDate(t.first_seen)} – ${fmtDate(t.last_seen)}`}
                    </td>
                    <td className="mono">
                      {t.my_games ? (
                        <><span style={{ color: "var(--win)" }}>{t.my_wins}</span>-<span style={{ color: "var(--loss)" }}>{t.my_games - t.my_wins}</span></>
                      ) : <span className="muted">—</span>}
                    </td>
                    <td className="mono" style={{ color: t.my_games ? (myWr >= 50 ? "var(--win)" : "var(--loss)") : "var(--text-2)" }}>
                      {t.my_games ? `${myWr.toFixed(0)}%` : "—"}
                    </td>
                    <td className="mono muted">{t.games}</td>
                    <td className="mono tm-blue">{blueWr.toFixed(0)}%</td>
                    <td className="mono">{fmtDur(t.avg_length)}</td>
                    <td style={{ width: 110 }}>
                      {t.my_games ? <div className="gauge"><div style={{ width: `${myWr}%` }} /></div> : null}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
    </>
  );
}
